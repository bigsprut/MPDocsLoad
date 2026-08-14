//! HTTP-клиент WB с rate limits по доменам (спец. §2.10.2) и retry policy.
//!
//! Домены (спец. §2.10.2):
//! - finance-api.wildberries.ru: 1 RPM, burst 1
//! - documents-api.wildberries.ru: 1 req/10s, burst 5
//! - statistics-api.wildberries.ru: 1 RPM, burst 10
//! - seller-analytics-api.wildberries.ru: 1 RPM, burst 1
//! - returns-api.wildberries.ru: 1 RPM, burst 1

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::{Response, StatusCode};
use serde::Serialize;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{debug, warn};

use mdwf_core::{Authenticator, CoreError, CoreResult};

/// Retry policy для WB (аналогично Ozon, спец. §2.8.2).
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_millis(30_000),
        }
    }
}

impl RetryPolicy {
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return self.base_delay;
        }
        let mult = 1u64 << (attempt - 2).min(20);
        let delay = self.base_delay.as_millis() as u64 * mult;
        Duration::from_millis(delay.min(self.max_delay.as_millis() as u64))
    }

    #[must_use]
    pub fn is_non_retryable(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::CONFLICT
                | StatusCode::UNPROCESSABLE_ENTITY
        )
    }
}

/// Rate limiter: минимальный интервал между запросами + burst.
/// Простая реализация — sleep перед каждым запросом, если прошло меньше интервала.
#[derive(Debug)]
pub struct RateLimiter {
    min_interval: Duration,
    last_request: Mutex<Option<std::time::Instant>>,
}

impl RateLimiter {
    #[must_use]
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_request: Mutex::new(None),
        }
    }

    /// Ждёт, если запрос был слишком недавно.
    pub async fn acquire(&self) {
        let wait = {
            let mut last = self.last_request.lock();
            let now = std::time::Instant::now();
            let wait = match *last {
                Some(t) if now.duration_since(t) < self.min_interval => {
                    self.min_interval - now.duration_since(t)
                }
                _ => Duration::ZERO,
            };
            *last = Some(now + wait);
            wait
        };
        if !wait.is_zero() {
            sleep(wait).await;
        }
    }

    /// Увеличивает интервал после получения 429: следующий запрос не уйдёт
    /// раньше, чем через `penalty` (берётся из X-Ratelimit-Retry WB, если есть).
    /// Дополнительно к этому acquire() всё равно контролирует min_interval.
    pub async fn backoff(&self, penalty: Duration) {
        let mut last = self.last_request.lock();
        // Сдвигаем "последний запрос" в будущее на величину штрафа,
        // чтобы acquire() следующего запроса подождал.
        let now = std::time::Instant::now();
        *last = Some(now + penalty);
    }
}

/// Домен WB API (сверено с официальной документацией dev.wildberries.ru).
#[derive(Debug, Clone, Copy)]
pub enum WbDomain {
    /// finance-api: баланс + отчёты реализации + эквайринг (дока: "Финансы").
    Finance,
    /// documents-api: категории, список, скачивание документов (дока: "Документы").
    Documents,
    /// statistics-api: заказы, продажи (дока: "Отчёты").
    Statistics,
    /// seller-analytics-api: штрафы, аналитика.
    Analytics,
    /// returns-api: возвраты.
    Returns,
}

impl WbDomain {
    /// Базовый URL домена. При тестировании можно переопределить через
    /// переменные окружения `MDWF_WB_BASE_*` (например, на mock-сервер wiremock).
    #[must_use]
    pub fn base_url(self) -> String {
        let env_key = match self {
            Self::Finance => "MDWF_WB_BASE_FINANCE",
            Self::Documents => "MDWF_WB_BASE_DOCUMENTS",
            Self::Statistics => "MDWF_WB_BASE_STATISTICS",
            Self::Analytics => "MDWF_WB_BASE_ANALYTICS",
            Self::Returns => "MDWF_WB_BASE_RETURNS",
        };
        if let Ok(url) = std::env::var(env_key) {
            return url.trim_end_matches('/').to_string();
        }
        match self {
            // Баланс: GET /api/v1/account/balance — подтверждено на finance-api.
            Self::Finance => "https://finance-api.wildberries.ru",
            Self::Documents => "https://documents-api.wildberries.ru",
            Self::Statistics => "https://statistics-api.wildberries.ru",
            Self::Analytics => "https://seller-analytics-api.wildberries.ru",
            Self::Returns => "https://returns-api.wildberries.ru",
        }
        .to_string()
    }

    /// Минимальный интервал между запросами (по официальной документации WB).
    /// Лимиты Personal-токена (для продавцов). При тестировании можно обнулить
    /// через `MDWF_WB_NO_RATELIMIT=1`.
    #[must_use]
    pub fn min_interval(self) -> Duration {
        if std::env::var("MDWF_WB_NO_RATELIMIT").as_deref() == Ok("1") {
            return Duration::ZERO;
        }
        match self {
            // Дока "Документы" (categories/list/download): 1 req/10s, burst 5.
            // download/all: 1 req/5min, burst 5.
            Self::Documents => Duration::from_secs(10),
            // Дока "Финансы": 1 req/1min, burst 1.
            Self::Finance => Duration::from_secs(60),
            Self::Analytics | Self::Returns => Duration::from_secs(60), // 1 RPM, burst 1
            Self::Statistics => Duration::from_secs(6),                 // ~10 RPM
        }
    }

}

/// HTTP-клиент WB с rate limit по доменам и retry policy.
#[derive(Clone)]
pub struct WbHttpClient {
    http: reqwest::Client,
    retry: RetryPolicy,
    limiters: Arc<[RateLimiter; 5]>,
}

impl WbHttpClient {
    pub fn new(retry: RetryPolicy) -> CoreResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(CoreError::Network)?;
        Ok(Self {
            http,
            retry,
            limiters: Arc::new([
                RateLimiter::new(WbDomain::Finance.min_interval()),
                RateLimiter::new(WbDomain::Documents.min_interval()),
                RateLimiter::new(WbDomain::Statistics.min_interval()),
                RateLimiter::new(WbDomain::Analytics.min_interval()),
                RateLimiter::new(WbDomain::Returns.min_interval()),
            ]),
        })
    }

    fn limiter_for(&self, domain: WbDomain) -> &RateLimiter {
        let idx = match domain {
            WbDomain::Finance => 0,
            WbDomain::Documents => 1,
            WbDomain::Statistics => 2,
            WbDomain::Analytics => 3,
            WbDomain::Returns => 4,
        };
        &self.limiters[idx]
    }

    /// GET-запрос с query-параметрами, retry и rate limit.
    pub async fn get(
        &self,
        domain: WbDomain,
        path: &str,
        query: &[(&str, &str)],
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        let url = format!("{}{path}", domain.base_url());
        self.limiter_for(domain).acquire().await;

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            debug!(%url, attempt, "GET");
            let mut req = self.http.get(&url);
            for (k, v) in query {
                req = req.query(&[(k.to_string(), v.to_string())]);
            }
            let req = auth.apply(req);

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if RetryPolicy::is_non_retryable(status) {
                        return Err(map_status_error(status, resp).await);
                    }
                    if status.is_success() {
                        // 204 No Content — конец курсорной пагинации WB (detailed-
                        // методы финансов): пустое тело — НЕ ошибка протокола.
                        if status == StatusCode::NO_CONTENT {
                            return Ok(Value::Array(Vec::new()));
                        }
                        // 2xx, но не JSON — ошибка протокола, не сеть.
                        return resp.json::<Value>().await.map_err(|e| {
                            CoreError::Protocol(format!("WB: некорректный JSON-ответ ({e})"))
                        });
                    }
                    // 429 Too Many Requests: WB отдаёт X-Ratelimit-Retry (секунды).
                    // Это КРИТИЧНО: на 429 обычная экспоненциальная задержка недостаточна,
                    // WB реально требует ждать 600+ секунд (forum/2141).
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = extract_ratelimit_retry(&resp)
                            .unwrap_or_else(|| self.retry.delay_for_attempt(attempt));
                        warn!(%status, attempt, ?retry_after, "rate limited (429) — waiting");
                        // На 429 делаем не больше 3 попыток (чтобы не долбиться),
                        // и обновляем лимитер на больший интервал.
                        self.limiter_for(domain).backoff(retry_after).await;
                        if attempt >= 3 {
                            // Читаем причину из body (WB: {message/detail} и др.).
                            let body = resp.text().await.unwrap_or_default();
                            let (_, message) = parse_wb_error(429, &body);
                            return Err(CoreError::Api {
                                status: 429,
                                message: format!(
                                    "{message} Превышен лимит запросов после {attempt} \
                                     попыток. Подождите {} сек перед повтором.",
                                    retry_after.as_secs()
                                ),
                                retryable: true,
                            });
                        }
                        sleep(retry_after).await;
                        continue;
                    }
                    warn!(%status, attempt, "retryable status");
                    if attempt > self.retry.max_retries {
                        return Err(map_status_error(status, resp).await);
                    }
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
                Err(e) => {
                    warn!(error = %e, attempt, "network error");
                    if attempt > self.retry.max_retries {
                        return Err(CoreError::Network(e));
                    }
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
            }
        }
    }

    /// POST-запрос с JSON-телом, retry и rate limit.
    pub async fn post<B: Serialize>(
        &self,
        domain: WbDomain,
        path: &str,
        body: &B,
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        let url = format!("{}{path}", domain.base_url());
        self.limiter_for(domain).acquire().await;

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            debug!(%url, attempt, "POST");
            let req = self.http.post(&url).json(body);
            let req = auth.apply(req);

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if RetryPolicy::is_non_retryable(status) {
                        return Err(map_status_error(status, resp).await);
                    }
                    if status.is_success() {
                        // 204 No Content — конец курсорной пагинации WB (detailed-
                        // методы финансов): пустое тело — НЕ ошибка протокола.
                        if status == StatusCode::NO_CONTENT {
                            return Ok(Value::Array(Vec::new()));
                        }
                        // 2xx, но не JSON — ошибка протокола, не сеть.
                        return resp.json::<Value>().await.map_err(|e| {
                            CoreError::Protocol(format!("WB: некорректный JSON-ответ ({e})"))
                        });
                    }
                    // 429 Too Many Requests (см. комментарий в GET).
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = extract_ratelimit_retry(&resp)
                            .unwrap_or_else(|| self.retry.delay_for_attempt(attempt));
                        warn!(%status, attempt, ?retry_after, "rate limited (429) — waiting");
                        self.limiter_for(domain).backoff(retry_after).await;
                        if attempt >= 3 {
                            let body = resp.text().await.unwrap_or_default();
                            let (_, message) = parse_wb_error(429, &body);
                            return Err(CoreError::Api {
                                status: 429,
                                message: format!(
                                    "{message} Превышен лимит запросов после {attempt} \
                                     попыток. Подождите {} сек перед повтором.",
                                    retry_after.as_secs()
                                ),
                                retryable: true,
                            });
                        }
                        sleep(retry_after).await;
                        continue;
                    }
                    warn!(%status, attempt, "retryable status");
                    if attempt > self.retry.max_retries {
                        return Err(map_status_error(status, resp).await);
                    }
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
                Err(e) => {
                    warn!(error = %e, attempt, "network error");
                    if attempt > self.retry.max_retries {
                        return Err(CoreError::Network(e));
                    }
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
            }
        }
    }
}

/// Извлекает задержку из ответа WB 429. WB использует заголовки:
/// - `X-Ratelimit-Retry` — секунды до разрешения (спец. forum/2141: до 600+ сек)
/// - `Retry-After` — стандартный HTTP-заголовок (секунды)
/// Возвращает None, если заголовков нет.
fn extract_ratelimit_retry(resp: &Response) -> Option<Duration> {
    // Сначала пробуем X-Ratelimit-Retry (WB-специфичный).
    if let Some(val) = resp.headers().get("x-ratelimit-retry").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = val.trim().parse::<u64>() {
            // Ограничиваем разумным потолком (1 час), защита от мусора.
            return Some(Duration::from_secs(secs.min(3600)));
        }
    }
    // Затем стандартный Retry-After.
    if let Some(val) = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = val.trim().parse::<u64>() {
            return Some(Duration::from_secs(secs.min(3600)));
        }
    }
    None
}

/// Парсит тело ошибки WB в человекочитаемое сообщение.
///
/// WB нестандартен: несколько форматов ошибок в разных API-семействах (пробуем
/// по порядку полей):
/// 1. `{error, errorText}` — большинство эндпоинтов (OpenAPI).
/// 2. `{message, detail}` — некоторые эндпоинты.
/// 3. `{data: {errors: [...]}}` — аналитика/возвраты.
/// 4. `{"message": "..."}` — простой формат.
/// Fallback для не-JSON: первые 500 симваков body.
///
/// Возвращает (retryable, message). retryable = 429 || 5xx.
fn parse_wb_error(status: u16, body: &str) -> (bool, String) {
    let retryable = status == 429 || (500..=599).contains(&status);
    let v = serde_json::from_str::<Value>(body).ok();
    let message = v
        .as_ref()
        .and_then(|v| {
            // {error, errorText} — самый частый WB-формат.
            if let (Some(code), Some(text)) = (
                v.get("error").and_then(|x| x.as_str()),
                v.get("errorText").and_then(|x| x.as_str()),
            ) {
                if !text.is_empty() {
                    return Some(format!("{code}: {text}"));
                }
                return Some(code.to_string());
            }
            // {message, detail}.
            if let Some(msg) = v.get("message").and_then(|x| x.as_str()) {
                let detail = v.get("detail").and_then(|x| x.as_str()).unwrap_or("");
                if !detail.is_empty() {
                    return Some(format!("{msg}: {detail}"));
                }
                return Some(msg.to_string());
            }
            // {data: {errors: [...]}}.
            if let Some(errs) = v
                .get("data")
                .and_then(|d| d.get("errors"))
                .and_then(|e| e.as_array())
            {
                let joined = errs
                    .iter()
                    .filter_map(|e| {
                        e.as_str()
                            .map(str::to_string)
                            .or_else(|| e.get("message").and_then(|m| m.as_str()).map(str::to_string))
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        })
        .unwrap_or_else(|| {
            let truncated: String = body.chars().take(500).collect();
            if truncated.is_empty() {
                format!("HTTP {status} (пустое тело ответа)")
            } else {
                truncated
            }
        });
    (retryable, message)
}

async fn map_status_error(status: StatusCode, resp: Response) -> CoreError {
    let body = resp.text().await.unwrap_or_default();
    let (retryable, message) = parse_wb_error(status.as_u16(), &body);
    CoreError::Api {
        status: status.as_u16(),
        message,
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delays_exponential() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay_for_attempt(3), Duration::from_millis(1000));
        assert_eq!(p.delay_for_attempt(4), Duration::from_millis(2000));
    }

    #[test]
    fn domain_intervals() {
        assert_eq!(WbDomain::Documents.min_interval(), Duration::from_secs(10));
        assert_eq!(WbDomain::Finance.min_interval(), Duration::from_secs(60));
    }

    #[test]
    fn domain_base_urls() {
        // Убираем возможные env-override.
        std::env::remove_var("MDWF_WB_BASE_FINANCE");
        std::env::remove_var("MDWF_WB_BASE_DOCUMENTS");
        assert_eq!(WbDomain::Finance.base_url(), "https://finance-api.wildberries.ru");
        assert_eq!(WbDomain::Documents.base_url(), "https://documents-api.wildberries.ru");
    }

    #[test]
    fn conflict_is_non_retryable() {
        // 409 — логический конфликт, не transient: не ретраим.
        assert!(RetryPolicy::is_non_retryable(StatusCode::CONFLICT));
        assert!(!RetryPolicy::is_non_retryable(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn parse_wb_error_error_errortext() {
        // Самый частый WB-формат: {error, errorText}.
        let body = r#"{"error":"invalid token","errorText":"token expired"}"#;
        let (retryable, msg) = parse_wb_error(401, body);
        assert!(!retryable);
        assert_eq!(msg, "invalid token: token expired");
    }

    #[test]
    fn parse_wb_error_message_detail() {
        let body = r#"{"message":"bad request","detail":"field sku required"}"#;
        let (_, msg) = parse_wb_error(400, body);
        assert_eq!(msg, "bad request: field sku required");
    }

    #[test]
    fn parse_wb_error_data_errors_array() {
        let body = r#"{"data":{"errors":["sku not found","price invalid"]}}"#;
        let (_, msg) = parse_wb_error(422, body);
        assert!(msg.contains("sku not found"));
        assert!(msg.contains("price invalid"));
    }

    #[test]
    fn parse_wb_error_non_json_fallback() {
        let (retryable, msg) = parse_wb_error(500, "Internal Server Error");
        assert!(retryable);
        assert_eq!(msg, "Internal Server Error");
    }

    #[test]
    fn parse_wb_error_empty_body() {
        let (_, msg) = parse_wb_error(404, "");
        assert!(msg.contains("404"));
    }
}
