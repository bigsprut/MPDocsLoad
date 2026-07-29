//! Пагинация WB API (спец. §2.4 — RrdidCursor/DateCursor/OffsetLimit/TaskId).

/// Курсор по `rrdid` (для documents API).
#[derive(Debug, Clone, Default)]
pub struct RrdidCursor {
    pub last_rrdid: Option<i64>,
    pub limit: u32,
}

impl RrdidCursor {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_rrdid: None,
            limit: 1000,
        }
    }
}

/// Курсор по дате.
#[derive(Debug, Clone)]
pub struct DateCursor {
    pub date_from: chrono::NaiveDate,
    pub date_to: chrono::NaiveDate,
}

/// Offset+limit.
#[derive(Debug, Clone, Copy)]
pub struct OffsetLimit {
    pub limit: u32,
    pub offset: u32,
}

impl Default for OffsetLimit {
    fn default() -> Self {
        Self {
            limit: 1000,
            offset: 0,
        }
    }
}

/// Идентификатор асинхронной задачи (для acceptance_report).
#[derive(Debug, Clone)]
pub struct TaskId(pub String);

impl TaskId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_limit_default() {
        let ol = OffsetLimit::default();
        assert_eq!(ol.limit, 1000);
        assert_eq!(ol.offset, 0);
    }
}
