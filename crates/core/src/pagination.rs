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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_pagination_variant() {
        let p = Pagination::Pages {
            page: 2,
            page_size: 50,
        };
        assert_eq!(
            p,
            Pagination::Pages {
                page: 2,
                page_size: 50
            }
        );
    }

    #[test]
    fn cursor_pagination_variant() {
        let p = Pagination::Cursor {
            last_id: Some("abc".into()),
            limit: 100,
        };
        assert_eq!(p, Pagination::Cursor {
            last_id: Some("abc".into()),
            limit: 100,
        });
    }

    #[test]
    fn offset_pagination_variant() {
        let p = Pagination::Offset {
            limit: 10,
            offset: 20,
        };
        assert_eq!(
            p,
            Pagination::Offset {
                limit: 10,
                offset: 20
            }
        );
    }

    #[test]
    fn page_pagination_default() {
        let p = PagePagination::default();
        assert_eq!(p.page, 1);
        assert_eq!(p.page_size, 1000);
    }

    #[test]
    fn pagination_serde_roundtrip() {
        let p = Pagination::Cursor {
            last_id: Some("x".into()),
            limit: 5,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Pagination = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
