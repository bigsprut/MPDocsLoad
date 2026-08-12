//! HTTP-клиент Ozon с rate limit, retry policy и circuit breaker
//! (спец. §2.8.2, §2.11.2, news/584 — 50 RPS).

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::{Response, StatusCode};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{debug, warn};

use mdwf_core::{Authenticator, CoreError, CoreResult};

use crate::auth::DEFAULT_BASE_URL;

/// Лимит запросов к Ozon Seller API: 50 RPS (спец. news/584).
pub const RATE_LIMIT_RPS: u32 = 50;

/// Минимальный интервал между запросами для соблюдения 50 RPS.
pub const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(20); // 1000ms / 50

/// Retry policy (спец. §2.8.2): не ретраятся 400/401/403/404/422;
/// ретраятся 429, 5xx и сетевые ошибки. База 500 мс, экспонента ×2, до 5 попыток.
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
    /// Задержка для N-й попытки (1-based): base × 2^(n-2) с cap (спец. §2.8.2).
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        // Попытка 2 -> base, 3 -> base*2, 4 -> base*4 ...
        if attempt <= 1 {
            return self.base_delay;
        }
        let mult = 1u64 << (attempt - 2).min(20);
        let delay = self.base_delay.as_millis() as u64 * mult;
        Duration::from_millis(delay.min(self.max_delay.as_millis() as u64))
    }

    /// True, если статус НЕ нужно ретраить (спец. §2.8.2).
    #[must_use]
    pub fn is_non_retryable(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::UNPROCESSABLE_ENTITY
        )
    }
}

/// Простой circuit breaker (спец. §2.11.2): 5 ошибок подряд -> открыть
/// на `cooldown` (по умолчанию 5 минут).
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    cooldown: Duration,
    state: Mutex<BreakerState>,
}

#[derive(Debug)]
struct BreakerState {
    failure_count: u32,
    is_open: bool,
    opened_at: Option<std::time::Instant>,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            failure_threshold,
            cooldown,
            state: Mutex::new(BreakerState {
                failure_count: 0,
                is_open: false,
                opened_at: None,
            }),
        }
    }

    /// Проверяет, можно ли выполнить запрос. Возвращает ошибку, если цепь разомкнута.
    pub fn check(&self) -> CoreResult<()> {
        let mut state = self.state.lock();
        if state.is_open {
            if let Some(opened) = state.opened_at {
                if opened.elapsed() < self.cooldown {
                    return Err(CoreError::Internal(format!(
                        "circuit breaker open (осталось {:?})",
                        self.cooldown - opened.elapsed()
                    )));
                }
            }
            // Half-open: разрешаем попытку.
            state.is_open = false;
            state.failure_count = 0;
            state.opened_at = None;
        }
        Ok(())
    }

    /// Отметить успех.
    pub fn on_success(&self) {
        let mut state = self.state.lock();
        state.failure_count = 0;
    }

    /// Отметить неудачу; при достижении порога размыкает цепь.
    pub fn on_failure(&self) {
        let mut state = self.state.lock();
        state.failure_count += 1;
        if state.failure_count >= self.failure_threshold {
            state.is_open = true;
            state.opened_at = Some(std::time::Instant::now());
            warn!(
                threshold = state.failure_count,
                "circuit breaker opened"
            );
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(300))
    }
}

/// Rate limiter: минимальный интервал между запросами (для соблюдения 50 RPS).
/// При 429 адаптивно увеличивает интервал через backoff().
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

    /// Увеличивает интервал после 429: следующий запрос не уйдёт раньше `penalty`.
    pub async fn backoff(&self, penalty: Duration) {
        let mut last = self.last_request.lock();
        let now = std::time::Instant::now();
        *last = Some(now + penalty);
    }
}

/// HTTP-клиент Ozon: POST JSON с retry, rate limit и circuit breaker.
#[derive(Clone)]
pub struct OzonHttpClient {
    http: reqwest::Client,
    base_url: String,
    retry: RetryPolicy,
    breaker: Arc<CircuitBreaker>,
    limiter: Arc<RateLimiter>,
}

impl OzonHttpClient {
    /// Создаёт клиент с базовым URL и retry policy.
    pub fn new(base_url: Option<&str>, retry: RetryPolicy) -> CoreResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(CoreError::Network)?;
        Ok(Self {
            http,
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL).to_string(),
            retry,
            breaker: Arc::new(CircuitBreaker::default()),
            limiter: Arc::new(RateLimiter::new(MIN_REQUEST_INTERVAL)),
        })
    }

    /// Базовый URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Выполняет POST-запрос с JSON-телом, retry, rate limit и circuit breaker.
    /// Возвращает распарсенный JSON.
    pub async fn post<B: Serialize>(
        &self,
        path: &str,
        body: &B,
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        let url = format!("{}{path}", self.base_url);
        self.post_url(&url, body, auth).await
    }

    /// Выполняет POST-запрос по полному URL (внутренний, для /v1/report/info).
    async fn post_url<B: Serialize>(
        &self,
        url: &str,
        body: &B,
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        self.limiter.acquire().await;
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            debug!(%url, attempt, "POST");
            let req = self.http.post(url).json(body);
            let req = auth.apply(req);
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        self.breaker.on_success();
                        // 2xx, но не JSON — это ошибка протокола, не сеть.
                        return resp.json::<Value>().await.map_err(|e| {
                            CoreError::Internal(format!(
                                "Ozon {url}: некорректный JSON-ответ ({e})"
                            ))
                        });
                    }
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = extract_retry_after(&resp)
                            .unwrap_or_else(|| self.retry.delay_for_attempt(attempt));
                        self.limiter.backoff(retry_after).await;
                        self.breaker.on_failure();
                        if attempt >= 3 {
                            // Читаем причину из body (Ozon: {message}).
                            let body = resp.text().await.unwrap_or_default();
                            let (_, message) = parse_ozon_error(429, &body);
                            return Err(CoreError::Api {
                                status: 429,
                                message: format!(
                                    "{message} Превышен лимит запросов после {attempt} попыток."
                                ),
                                retryable: true,
                            });
                        }
                        sleep(retry_after).await;
                        continue;
                    }
                    if !RetryPolicy::is_non_retryable(status) && attempt <= self.retry.max_retries {
                        self.breaker.on_failure();
                        sleep(self.retry.delay_for_attempt(attempt)).await;
                        continue;
                    }
                    self.breaker.on_failure();
                    return Err(map_status_error(status, resp).await);
                }
                Err(e) => {
                    self.breaker.on_failure();
                    if attempt > self.retry.max_retries {
                        return Err(CoreError::Network(e));
                    }
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
            }
        }
    }

    /// Скачивает файл по прямому URL (для ссылок из /v1/report/info).
    pub async fn download_file(&self, url: &str) -> CoreResult<Vec<u8>> {
        debug!(%url, "download_file");
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(CoreError::Network)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let (retryable, message) = parse_ozon_error(status.as_u16(), &body);
            return Err(CoreError::Api {
                status: status.as_u16(),
                message,
                retryable,
            });
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(CoreError::Network)
    }

    /// POST к эндпоинтам без авторизации — не нужен, используем post_url.
    /// Этот метод для /v1/report/info (с авторизацией, но по base_url).
    pub async fn post_report_info(
        &self,
        code: &str,
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        let url = format!("{}/v1/report/info", self.base_url);
        let body = json!({ "code": code });
        self.post_url(&url, &body, auth).await
    }

    /// Получает список ID всех складов продавца через /v2/warehouse/list
    /// (пагинация по cursor). Используется для ozon.warehouse_stock, когда ID
    /// складов не переданы явно (auto-fill). Дока: POST /v2/warehouse/list
    /// {limit<=200, cursor?} → {warehouses:[{warehouse_id,...}], has_next}.
    pub async fn fetch_warehouse_ids(
        &self,
        auth: &dyn Authenticator,
    ) -> CoreResult<Vec<String>> {
        let url = format!("{}/v2/warehouse/list", self.base_url);
        let mut ids = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut body = json!({ "limit": 200 });
            if let Some(c) = &cursor {
                body["cursor"] = json!(c);
            }
            let resp = self.post_url(&url, &body, auth).await?;
            if let Some(whs) = resp.get("warehouses").and_then(serde_json::Value::as_array) {
                for wh in whs {
                    // warehouse_id — int64. as_i64() покрывает; для >i64 редки.
                    if let Some(id) = wh.get("warehouse_id").and_then(serde_json::Value::as_i64) {
                        ids.push(id.to_string());
                    } else if let Some(id) = wh
                        .get("warehouse_id")
                        .and_then(serde_json::Value::as_str)
                    {
                        // На случай если сервер отдаёт строкой.
                        ids.push(id.to_string());
                    }
                }
            }
            let has_next = resp
                .get("has_next")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !has_next {
                break;
            }
            cursor = resp
                .get("cursor")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(ids)
    }

    /// Получает список SKU всех товаров продавца через /v3/product/list
    /// (пагинация по cursor last_id). Используется для ozon.analytics_stocks,
    /// когда SKU не переданы явно (auto-fill). Дока: POST /v3/product/list
    /// `{limit<=1000, last_id?, filter:{visibility:"ALL"}}` →
    /// `{result:{items:[{product_id, offer_id, sku, ...}], total, last_id}}`.
    ///
    /// Возвращает **числовые SKU Ozon** (int64, поле `sku`) как строки — именно
    /// их ждёт `/v1/analytics/stocks` в параметре `skus` (НЕ offer_id-артикул
    /// продавца). Защита от бесконечного цикла: ~200 страниц (~200k товаров).
    pub async fn fetch_skus(
        &self,
        auth: &dyn Authenticator,
    ) -> CoreResult<Vec<String>> {
        let url = format!("{}/v3/product/list", self.base_url);
        let mut skus = Vec::new();
        let mut last_id = String::new();
        let mut iter = 0u32;
        loop {
            let body = json!({
                "limit": 1000,
                "last_id": last_id,
                "filter": { "visibility": "ALL" }
            });
            let resp = self.post_url(&url, &body, auth).await?;
            let result = resp.get("result").cloned().unwrap_or(json!({}));
            if let Some(items) = result.get("items").and_then(serde_json::Value::as_array) {
                for item in items {
                    // sku — int64 (Ozon internal numeric SKU). На случай если сервер
                    // отдаёт строкой — пробуем и as_i64, и as_str.
                    if let Some(sku) = item.get("sku").and_then(serde_json::Value::as_i64) {
                        skus.push(sku.to_string());
                    } else if let Some(sku) =
                        item.get("sku").and_then(serde_json::Value::as_str)
                    {
                        skus.push(sku.to_string());
                    }
                }
            }
            let next_last_id = result
                .get("last_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            // Выход: пустой/неизменный last_id (конец списка) либо лимит итераций.
            if next_last_id.is_empty() || next_last_id == last_id {
                break;
            }
            last_id = next_last_id;
            iter += 1;
            if iter >= 200 {
                break;
            }
        }
        Ok(skus)
    }
}

/// Извлекает задержку из ответа Ozon 429. Ozon использует несколько заголовков:
/// - `Retry-After` (стандарт, секунды) — основные эндпоинты.
/// - `X-Ratelimit-Retry` (секунды) — резерв.
/// - `Item-Retry-After` (минуты!) — product-import эндпоинты. Переводим в секунды.
///
/// Возвращает None, если ни одного заголовка нет. Все значения ограничены 1 часом.
fn extract_retry_after(resp: &Response) -> Option<Duration> {
    if let Some(val) = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = val.trim().parse::<u64>() {
            return Some(Duration::from_secs(secs.min(3600)));
        }
    }
    if let Some(val) = resp.headers().get("x-ratelimit-retry").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = val.trim().parse::<u64>() {
            return Some(Duration::from_secs(secs.min(3600)));
        }
    }
    // Item-Retry-After у Ozon — в МИНУТАХ (product import). Переводим в секунды.
    if let Some(val) = resp.headers().get("item-retry-after").and_then(|v| v.to_str().ok()) {
        if let Ok(mins) = val.trim().parse::<u64>() {
            return Some(Duration::from_secs(mins.saturating_mul(60).min(3600)));
        }
    }
    None
}

/// Парсит тело ошибки Ozon в человекочитаемое сообщение.
///
/// Ozon возвращает gRPC-gateway формат: `{"code": <num>, "message": "<text>",
/// "details": [{"typeUrl","value"}]}`. Берём `message`; при наличии `details`
/// кратко дополняем. Fallback для не-JSON: первые 500 симваков body.
///
/// Возвращает (retryable, message). retryable = 429 || 5xx.
fn parse_ozon_error(status: u16, body: &str) -> (bool, String) {
    let retryable = status == 429 || (500..=599).contains(&status);
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            // message — основное описание.
            let msg = v.get("message").and_then(|m| m.as_str()).map(str::to_string);
            // details[] — доп. контекст (тип + значение).
            let details = v
                .get("details")
                .and_then(|d| d.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|d| d.get("value").and_then(|x| x.as_str()).map(str::to_string))
                        .collect::<Vec<_>>()
                        .join("; ")
                });
            match (msg, details) {
                (Some(m), Some(d)) if !d.is_empty() => Some(format!("{m} ({d})")),
                (Some(m), _) => Some(m),
                _ => None,
            }
        })
        .unwrap_or_else(|| {
            // Не JSON или нет message — первые 500 симваков, обрезанные.
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
    let (retryable, message) = parse_ozon_error(status.as_u16(), &body);
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
    fn retry_delays() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay_for_attempt(1), Duration::from_millis(500));
        assert_eq!(p.delay_for_attempt(2), Duration::from_millis(500));
        assert_eq!(p.delay_for_attempt(3), Duration::from_millis(1000));
        assert_eq!(p.delay_for_attempt(4), Duration::from_millis(2000));
        assert_eq!(p.delay_for_attempt(5), Duration::from_millis(4000));
        assert_eq!(p.delay_for_attempt(6), Duration::from_millis(8000));
    }

    #[test]
    fn non_retryable_statuses() {
        assert!(RetryPolicy::is_non_retryable(StatusCode::BAD_REQUEST));
        assert!(RetryPolicy::is_non_retryable(StatusCode::UNAUTHORIZED));
        assert!(RetryPolicy::is_non_retryable(StatusCode::FORBIDDEN));
        assert!(RetryPolicy::is_non_retryable(StatusCode::NOT_FOUND));
        assert!(RetryPolicy::is_non_retryable(StatusCode::UNPROCESSABLE_ENTITY));
        assert!(!RetryPolicy::is_non_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(!RetryPolicy::is_non_retryable(StatusCode::INTERNAL_SERVER_ERROR));
    }

    #[test]
    fn breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        assert!(cb.check().is_ok());
        cb.on_failure();
        cb.on_failure();
        assert!(cb.check().is_ok()); // ещё не открылся
        cb.on_failure();
        assert!(cb.check().is_err()); // открылся
    }

    #[test]
    fn breaker_resets_on_success() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(60));
        cb.on_failure();
        cb.on_success();
        assert!(cb.check().is_ok());
        cb.on_failure();
        assert!(cb.check().is_ok()); // счётчик сброшен успехом
    }

    #[test]
    fn parse_ozon_error_grpc_gateway_shape() {
        // gRPC-gateway: {code, message, details[]}.
        let body = r#"{"code":18,"message":"Api-key is invalid or expired","details":[]}"#;
        let (retryable, msg) = parse_ozon_error(401, body);
        assert!(!retryable);
        assert_eq!(msg, "Api-key is invalid or expired");
    }

    #[test]
    fn parse_ozon_error_with_details() {
        let body = r#"{"code":3,"message":"bad param","details":[{"typeUrl":"t","value":"price<=0"}]}"#;
        let (_, msg) = parse_ozon_error(400, body);
        assert!(msg.contains("bad param"));
        assert!(msg.contains("price<=0"));
    }

    #[test]
    fn parse_ozon_error_non_json_fallback() {
        let (retryable, msg) = parse_ozon_error(500, "Internal Server Error");
        assert!(retryable);
        assert_eq!(msg, "Internal Server Error");
    }

    #[test]
    fn parse_ozon_error_empty_body() {
        let (_, msg) = parse_ozon_error(404, "");
        assert!(msg.contains("404"), "empty body fallback: {msg}");
    }

    #[test]
    fn parse_ozon_error_truncates_long_body() {
        let long = "x".repeat(1000);
        let (_, msg) = parse_ozon_error(500, &long);
        assert_eq!(msg.chars().count(), 500, "should truncate to 500 chars");
    }

    #[test]
    fn parse_ozon_error_429_is_retryable() {
        let (retryable, _) = parse_ozon_error(429, r#"{"message":"too many requests"}"#);
        assert!(retryable);
    }
}
