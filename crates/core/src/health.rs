//! Health check — статус работоспособности провайдера (спец. §2.9.4).

use serde::{Deserialize, Serialize};

/// Уровень здоровья.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthLevel {
    /// Полностью работоспособен.
    Ok,
    /// Работает, но есть предупреждения (например, ключ истекает скоро).
    Degraded,
    /// Неработоспособен (auth failed, network down).
    Down,
}

/// Результат health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub level: HealthLevel,
    pub message: String,
}

impl HealthStatus {
    #[must_use]
    pub fn ok() -> Self {
        Self {
            level: HealthLevel::Ok,
            message: String::new(),
        }
    }

    #[must_use]
    pub fn degraded(message: impl Into<String>) -> Self {
        Self {
            level: HealthLevel::Degraded,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn down(message: impl Into<String>) -> Self {
        Self {
            level: HealthLevel::Down,
            message: message.into(),
        }
    }

    /// True, если провайдер полностью неработоспособен.
    #[must_use]
    pub fn is_down(&self) -> bool {
        matches!(self.level, HealthLevel::Down)
    }
}
