//! Пагинация Ozon API (спец. §2.9.3).
//!
//! Три схемы:
//! - Pages: `page` (1-based) + `page_size` (max 1000).
//! - Cursor: `last_id` + `limit` (max 1000).
//! - Offset: `limit` + `offset`.

/// Максимальный размер страницы/лимита для Ozon (спец. §2.9.3).
pub const MAX_PAGE_SIZE: u32 = 1000;

/// Страничная пагинация: `page` + `page_size`.
#[derive(Debug, Clone, Copy)]
pub struct PagesPagination {
    pub page: u32,
    pub page_size: u32,
}

impl Default for PagesPagination {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: MAX_PAGE_SIZE,
        }
    }
}

impl PagesPagination {
    /// True, если страница последняя (вернулось меньше элементов, чем page_size).
    #[must_use]
    pub fn is_last_page(self, returned_count: usize) -> bool {
        (returned_count as u32) < self.page_size
    }

    /// Следующая страница.
    #[must_use]
    pub const fn next(self) -> Self {
        Self {
            page: self.page + 1,
            page_size: self.page_size,
        }
    }
}

/// Курсорная пагинация: `last_id` + `limit`.
#[derive(Debug, Clone)]
pub struct CursorPagination {
    pub last_id: Option<String>,
    pub limit: u32,
}

impl Default for CursorPagination {
    fn default() -> Self {
        Self {
            last_id: None,
            limit: MAX_PAGE_SIZE,
        }
    }
}

impl CursorPagination {
    /// Обновляет курсор после запроса.
    pub fn advance(&mut self, last_id: impl Into<String>) {
        self.last_id = Some(last_id.into());
    }
}

/// Offset-пагинация: `limit` + `offset`.
#[derive(Debug, Clone, Copy)]
pub struct OffsetPagination {
    pub limit: u32,
    pub offset: u32,
}

impl Default for OffsetPagination {
    fn default() -> Self {
        Self {
            limit: MAX_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl OffsetPagination {
    /// True, если достигнут конец (вернулось меньше limit).
    #[must_use]
    pub fn is_last_page(self, returned_count: usize) -> bool {
        (returned_count as u32) < self.limit
    }

    /// Следующая страница.
    #[must_use]
    pub const fn next(self) -> Self {
        Self {
            limit: self.limit,
            offset: self.offset + self.limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_last_page_detection() {
        let p = PagesPagination {
            page: 1,
            page_size: 1000,
        };
        assert!(!p.is_last_page(1000));
        assert!(p.is_last_page(500));
    }

    #[test]
    fn pages_next_increments() {
        let p = PagesPagination {
            page: 3,
            page_size: 50,
        };
        assert_eq!(p.next().page, 4);
    }

    #[test]
    fn offset_next_advances_by_limit() {
        let p = OffsetPagination {
            limit: 100,
            offset: 200,
        };
        assert_eq!(p.next().offset, 300);
    }
}
