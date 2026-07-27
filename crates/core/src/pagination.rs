//! Пагинация (спец. §2.4: `pagination.rs`).
//!
//! Три схемы, перечисленные в спец. §2.9.3 для Ozon (Pages/Cursor/Offset),
//! и аналогичные для WB (RrdidCursor/DateCursor/OffsetLimit/TaskId — спец. §2.4).

use serde::{Deserialize, Serialize};

/// Схема пагинации, поддерживаемая эндпоинтом.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pagination {
    /// Страничная: `page` (1-based) + `page_size`.
    Pages { page: u32, page_size: u32 },
    /// Курсорная: `last_id` + `limit`.
    Cursor { last_id: Option<String>, limit: u32 },
    /// Сдвиг: `limit` + `offset`.
    Offset { limit: u32, offset: u32 },
}

/// Часть унифицированного представления для страничной пагинации.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagePagination {
    pub page: u32,
    pub page_size: u32,
}

impl Default for PagePagination {
    fn default() -> Self {
        // 1000 — максимум для Ozon v3 endpoints (спец. §2.9.3).
        Self {
            page: 1,
            page_size: 1000,
        }
    }
}
