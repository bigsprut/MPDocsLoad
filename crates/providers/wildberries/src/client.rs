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
}

/// Домен WB API (спец. §2.10.2).
#[derive(Debug, Clone, Copy)]
pub enum WbDomain {
    Finance,
    Documents,
    Statistics,
    Analytics,
    Returns,
    OpenApi, // api.wildberries.ru для баланса
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
            Self::OpenApi => "MDWF_WB_BASE_OPENAPI",
        };
        if let Ok(url) = std::env::var(env_key) {
            return url.trim_end_matches('/').to_string();
        }
        match self {
            Self::Finance => "https://finance-api.wildberries.ru",
            Self::Documents => "https://documents-api.wildberries.ru",
            Self::Statistics => "https://statistics-api.wildberries.ru",
            Self::Analytics => "https://seller-analytics-api.wildberries.ru",
            Self::Returns => "https://returns-api.wildberries.ru",
            Self::OpenApi => "https://api.wildberries.ru",
        }
        .to_string()
    }

    /// Минимальный интервал между запросами (спец. §2.10.2).
    /// При тестировании можно обнулить через `MDWF_WB_NO_RATELIMIT=1`.
    #[must_use]
    pub fn min_interval(self) -> Duration {
        if std::env::var("MDWF_WB_NO_RATELIMIT").as_deref() == Ok("1") {
            return Duration::ZERO;
        }
        match self {
            Self::Documents => Duration::from_secs(10), // 1 req/10s
            Self::Finance | Self::Analytics | Self::Returns => Duration::from_secs(60), // 1 RPM
            Self::Statistics => Duration::from_secs(6), // ~10 RPM
            Self::OpenApi => Duration::from_secs(1),
        }
    }
}

/// HTTP-клиент WB с rate limit по доменам и retry policy.
#[derive(Clone)]
pub struct WbHttpClient {
    http: reqwest::Client,
    retry: RetryPolicy,
    limiters: Arc<[RateLimiter; 6]>,
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
                RateLimiter::new(WbDomain::OpenApi.min_interval()),
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
            WbDomain::OpenApi => 0,
            WbDomain::Finance => 1,
            WbDomain::Documents => 2,
            WbDomain::Statistics => 3,
            WbDomain::Analytics => 4,
            WbDomain::Returns => 5,
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
                        return Ok(resp.json::<Value>().await.map_err(CoreError::Network)?);
                    }
                    warn!(%status, attempt, "retryable status");
                    if attempt > self.retry.max_retries {
                        return Err(map_status_error(status, resp).await);
                    }
                    // Для 429 WB отдаёт заголовок X-Ratelimit-Retry (секунды).
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
                        return Ok(resp.json::<Value>().await.map_err(CoreError::Network)?);
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

async fn map_status_error(status: StatusCode, resp: Response) -> CoreError {
    let body = resp.text().await.unwrap_or_default();
    CoreError::Internal(format!("WB API {status}: {body}"))
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
}
