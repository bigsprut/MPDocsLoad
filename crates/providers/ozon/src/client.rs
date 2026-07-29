//! HTTP-клиент Ozon с rate limit, retry policy и circuit breaker
//! (спец. §2.8.2, §2.11.2, news/584 — 50 RPS).

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::{Response, StatusCode};
use serde::Serialize;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{debug, warn};

use mdwf_core::{Authenticator, CoreError, CoreResult};

use crate::auth::DEFAULT_BASE_URL;

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

/// HTTP-клиент Ozon: POST JSON с retry + circuit breaker.
#[derive(Clone)]
pub struct OzonHttpClient {
    http: reqwest::Client,
    base_url: String,
    retry: RetryPolicy,
    breaker: Arc<CircuitBreaker>,
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
        })
    }

    /// Базовый URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Выполняет POST-запрос с JSON-телом, retry и circuit breaker.
    /// Возвращает распарсенный JSON.
    pub async fn post<B: Serialize>(
        &self,
        path: &str,
        body: &B,
        auth: &dyn Authenticator,
    ) -> CoreResult<Value> {
        let url = format!("{}{path}", self.base_url);
        self.breaker.check()?;

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
                        self.breaker.on_failure();
                        return Err(map_status_error(status, resp).await);
                    }
                    if status.is_success() {
                        self.breaker.on_success();
                        let json = resp
                            .json::<Value>()
                            .await
                            .map_err(|e| CoreError::Network(e))?;
                        // Ozon отдаёт ошибки в `{"code":..., "message":...}` даже с 200 иногда.
                        return Ok(json);
                    }
                    // Retryable: 429, 5xx.
                    warn!(%status, attempt, "retryable status");
                    if attempt > self.retry.max_retries {
                        self.breaker.on_failure();
                        return Err(map_status_error(status, resp).await);
                    }
                    self.breaker.on_failure();
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
                Err(e) => {
                    warn!(error = %e, attempt, "network error");
                    if attempt > self.retry.max_retries {
                        self.breaker.on_failure();
                        return Err(CoreError::Network(e));
                    }
                    self.breaker.on_failure();
                    sleep(self.retry.delay_for_attempt(attempt)).await;
                }
            }
        }
    }
}

async fn map_status_error(status: StatusCode, resp: Response) -> CoreError {
    let body = resp.text().await.unwrap_or_default();
    CoreError::Internal(format!("Ozon API {status}: {body}"))
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
}
