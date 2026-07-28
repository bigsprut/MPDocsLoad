//! Progress callback для отслеживания долгих выгрузок (спец. §2.4: `progress.rs`).
//!
//! Используется GUI (`GtkProgressBar`) и CLI для отображения прогресса.

use std::sync::Arc;

/// Сообщение о прогрессе выгрузки.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Доля завершённого: 0.0 ..= 1.0 (или `None`, если суммарный объём неизвестен).
    pub fraction: Option<f64>,
    /// Человекочитаемое описание текущего шага.
    pub message: String,
    /// Сколько элементов обработано / сколько всего (если известно).
    pub current: Option<u64>,
    pub total: Option<u64>,
}

impl ProgressUpdate {
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            fraction: None,
            message: message.into(),
            current: None,
            total: None,
        }
    }

    #[must_use]
    pub fn fraction(fraction: f64, message: impl Into<String>) -> Self {
        Self {
            fraction: Some(fraction.clamp(0.0, 1.0)),
            message: message.into(),
            current: None,
            total: None,
        }
    }

    #[must_use]
    pub fn items(current: u64, total: u64, message: impl Into<String>) -> Self {
        // Для прогресс-фракции потеря точности u64->f64 на ~52-битной мантиссе
        // несущественна (прогресс отображается до 2 знаков).
        let fraction = if total == 0 {
            None
        } else {
            #[allow(clippy::cast_precision_loss)]
            Some(current as f64 / total as f64)
        };
        Self {
            fraction,
            message: message.into(),
            current: Some(current),
            total: Some(total),
        }
    }
}

/// Трейт обратного вызова прогресса. Реализуется UI-слоем (GUI/CLI).
pub trait ProgressCallback: Send + Sync {
    /// Вызывается провайдером при изменении прогресса.
    fn report(&self, update: ProgressUpdate);
}

/// Тип-псевдоним для arc-ссылки на колбэк.
pub type ProgressCallbackRef = Arc<dyn ProgressCallback>;

/// No-op реализация (для тестов / headless-выгрузки).
pub struct NoopProgress;

impl ProgressCallback for NoopProgress {
    fn report(&self, _update: ProgressUpdate) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct Recorder {
        last: Mutex<Option<ProgressUpdate>>,
    }
    impl ProgressCallback for Recorder {
        fn report(&self, update: ProgressUpdate) {
            *self.last.lock() = Some(update);
        }
    }

    #[test]
    fn items_compute_fraction() {
        let u = ProgressUpdate::items(3, 10, "downloading");
        assert_eq!(u.fraction, Some(0.3));
        assert_eq!(u.current, Some(3));
        assert_eq!(u.total, Some(10));
    }

    #[test]
    fn callback_invoked() {
        let r = Arc::new(Recorder::default());
        r.report(ProgressUpdate::fraction(0.5, "half"));
        assert_eq!(r.last.lock().as_ref().unwrap().fraction, Some(0.5));
    }
}
