//! The standard response envelope (SDD §12.3, naming convention §5).
//!
//! Every endpoint answers with one of three shapes:
//!
//! - item:  `{success, data}`
//! - list:  `{success, data: [...], meta: {page, pageSize, total}}`
//! - error: `{success: false, error: {code, message, details}}`
//!
//! Handlers build these through the helpers here rather than assembling JSON by
//! hand, so the envelope stays uniform across modules.
//!
//! Phase 1 ships this surface with no caller: the operational endpoints answer
//! outside the envelope by design, and the first list endpoint arrives with the
//! identity module in Phase 2. The tests below are the only consumers today.
#![allow(
    dead_code,
    reason = "envelope and pagination helpers are consumed by the module routers from Phase 2"
)]

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::ValidationDetail;

/// Item response: `{success: true, data}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ItemEnvelope<T> {
    pub success: bool,
    pub data: T,
}

impl<T> ItemEnvelope<T> {
    pub fn new(data: T) -> Self {
        Self {
            success: true,
            data,
        }
    }
}

/// List response: `{success: true, data: [...], meta}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ListEnvelope<T> {
    pub success: bool,
    pub data: Vec<T>,
    pub meta: PageMeta,
}

impl<T> ListEnvelope<T> {
    pub fn new(data: Vec<T>, meta: PageMeta) -> Self {
        Self {
            success: true,
            data,
            meta,
        }
    }
}

/// Pagination metadata (FR-API-006). Field names are `camelCase` on the wire.
#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

impl PageMeta {
    pub fn new(page: u32, page_size: u32, total: u64) -> Self {
        Self {
            page,
            page_size,
            total,
        }
    }
}

/// Error response: `{success: false, error: {code, message, details}}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorEnvelope {
    pub success: bool,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    /// Always present, empty when the failure is not field-level, so clients can
    /// iterate it without a null check.
    pub details: Vec<ValidationDetail>,
}

impl ErrorEnvelope {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        details: Vec<ValidationDetail>,
    ) -> Self {
        Self {
            success: false,
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
                details,
            },
        }
    }
}

/// Default page size when the caller does not ask for one.
pub const DEFAULT_PAGE_SIZE: u32 = 20;

/// Upper bound on page size. Requests above it are clamped rather than
/// rejected, so a caller cannot force an unbounded scan (NFR-PERF-002).
pub const MAX_PAGE_SIZE: u32 = 100;

/// Pagination query parameters, accepted by every list endpoint.
#[derive(Debug, Clone, Copy, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct Pagination {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
}

impl Pagination {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(1).max(1)
    }

    pub fn page_size(&self) -> u32 {
        self.page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    /// Row offset for SQL. Returns `i64` because that is what SQLx binds to
    /// PostgreSQL `OFFSET`/`LIMIT`.
    pub fn offset(&self) -> i64 {
        i64::from(self.page() - 1) * i64::from(self.page_size())
    }

    pub fn limit(&self) -> i64 {
        i64::from(self.page_size())
    }

    pub fn meta(&self, total: u64) -> PageMeta {
        PageMeta::new(self.page(), self.page_size(), total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pagination(page: Option<u32>, page_size: Option<u32>) -> Pagination {
        Pagination { page, page_size }
    }

    #[test]
    fn defaults_to_the_first_page() {
        let p = Pagination::default();

        assert_eq!(p.page(), 1);
        assert_eq!(p.page_size(), DEFAULT_PAGE_SIZE);
        assert_eq!(p.offset(), 0);
    }

    #[test]
    fn clamps_page_size_to_the_maximum() {
        // An unbounded page size is the easy way to defeat pagination entirely.
        let p = pagination(None, Some(10_000));

        assert_eq!(p.page_size(), MAX_PAGE_SIZE);
        assert_eq!(p.limit(), i64::from(MAX_PAGE_SIZE));
    }

    #[test]
    fn treats_page_zero_as_the_first_page() {
        assert_eq!(pagination(Some(0), None).page(), 1);
        assert_eq!(pagination(Some(0), None).offset(), 0);
    }

    #[test]
    fn rejects_a_zero_page_size_by_clamping_upward() {
        assert_eq!(pagination(None, Some(0)).page_size(), 1);
    }

    #[test]
    fn computes_offset_from_page_and_size() {
        let p = pagination(Some(3), Some(25));

        assert_eq!(p.offset(), 50);
        assert_eq!(p.limit(), 25);
    }

    #[test]
    fn offset_does_not_overflow_on_a_large_page_number() {
        // u32::MAX pages at 100 per page overflows i32 but not i64; the cast
        // must not wrap into a negative OFFSET, which PostgreSQL rejects.
        let p = pagination(Some(u32::MAX), Some(MAX_PAGE_SIZE));

        assert!(p.offset() > 0, "offset stayed positive");
    }

    #[test]
    fn item_envelope_reports_success() {
        let json = serde_json::to_value(ItemEnvelope::new(serde_json::json!({"id": 1})))
            .expect("serializes");

        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["id"], 1);
    }

    #[test]
    fn list_envelope_uses_camel_case_meta() {
        let envelope = ListEnvelope::new(vec![1, 2, 3], PageMeta::new(2, 20, 45));
        let json = serde_json::to_value(envelope).expect("serializes");

        assert_eq!(json["success"], true);
        assert_eq!(json["meta"]["page"], 2);
        assert_eq!(json["meta"]["pageSize"], 20);
        assert_eq!(json["meta"]["total"], 45);
        assert!(
            json["meta"].get("page_size").is_none(),
            "no snake_case leak"
        );
    }
}
