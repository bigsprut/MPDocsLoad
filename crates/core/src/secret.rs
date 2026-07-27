//! Обёртка для хранения секретов (API-ключей, токенов).
//!
//! Гарантирует, что секреты не попадают в логи случайно (спец. §1.3 п.6, §2.13.2 п.9).

use std::fmt;

/// Строка, содержащая секретное значение (API-ключ, токен).
///
/// `Debug`/`Display` маскируют содержимое, чтобы секрет не утёк в логи.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    /// Создаёт секрет из строки.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Создаёт пустой секрет.
    #[must_use]
    pub fn empty() -> Self {
        Self(String::new())
    }

    /// Возвращает ссылку на открытое значение.
    /// Вызывающий отвечает за то, чтобы оно не попало в логи/вывод.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// Проверяет, пуст ли секрет.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_masks_value() {
        let s = SecretString::new("super-secret-token");
        assert_eq!(format!("{s:?}"), "SecretString(***)");
    }

    #[test]
    fn display_masks_value() {
        let s = SecretString::new("super-secret-token");
        assert_eq!(format!("{s}"), "***");
    }

    #[test]
    fn expose_returns_real_value() {
        let s = SecretString::new("super-secret-token");
        assert_eq!(s.expose_secret(), "super-secret-token");
    }
}
