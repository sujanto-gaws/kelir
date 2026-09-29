//! The integration log as a reader sees it (FR-INT-006, #548): one row of
//! `integration_logs` (Database Schema §12.5) in a list, and whole in a detail.
//!
//! **Nothing here resolves or unmasks anything.** The payloads were masked
//! before they were stored (`service::test_call`), and they are returned
//! exactly as stored. There is no field for an unmasked payload, and nothing
//! in this module could fill one (#548 AC3).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::response::Pagination;

/// `integration_logs.status` — §12.5's `CHECK` vocabulary, whole.
///
/// A test call writes only `SUCCESS` and `FAILED`; the other three belong to
/// retry and delivery (FR-INT-007, FR-INT-005), which are not built. They are
/// here because the column allows them and a filter should accept what a row
/// may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationLogStatus {
    Success,
    Failed,
    Pending,
    Retrying,
    DeadLetter,
}

impl IntegrationLogStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Failed => "FAILED",
            Self::Pending => "PENDING",
            Self::Retrying => "RETRYING",
            Self::DeadLetter => "DEAD_LETTER",
        }
    }

    /// The column's `CHECK` admits nothing else; an unknown value, which only
    /// a schema change could produce, reads as `FAILED` — a row nobody
    /// recognises is not reported as a success.
    pub fn from_db(value: &str) -> Self {
        match value {
            "SUCCESS" => Self::Success,
            "PENDING" => Self::Pending,
            "RETRYING" => Self::Retrying,
            "DEAD_LETTER" => Self::DeadLetter,
            _ => Self::Failed,
        }
    }
}

/// `integration_logs.direction` — §12.5's `CHECK` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationDirection {
    Inbound,
    Outbound,
}

impl IntegrationDirection {
    /// The `CHECK` admits only the two; anything else reads as `OUTBOUND`,
    /// the only direction anything writes today.
    pub fn from_db(value: &str) -> Self {
        match value {
            "INBOUND" => Self::Inbound,
            _ => Self::Outbound,
        }
    }
}

/// One log row in the list — everything but the payloads.
///
/// `integrationType` and `method` are returned as stored: the column has no
/// `CHECK`, and a reader should see what was written rather than a value
/// narrowed to today's vocabulary.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationLogSummary {
    pub id: Uuid,
    pub external_system_id: Option<Uuid>,
    /// The system's code, joined — `null` when the row names no system.
    pub external_system_code: Option<String>,
    pub external_system_name: Option<String>,
    pub direction: IntegrationDirection,
    /// `REST` for a test call.
    pub integration_type: Option<String>,
    pub method: Option<String>,
    /// The URL that was, or would have been, called.
    pub endpoint: Option<String>,
    /// `IntegrationEndpoint` for a test call.
    pub entity_type: Option<String>,
    /// The endpoint's id, for a test call.
    pub entity_id: Option<Uuid>,
    pub status: IntegrationLogStatus,
    /// The system's HTTP status; `null` when nothing answered.
    pub status_code: Option<i32>,
    /// `CODE: message` for a call that got no answer.
    pub error_message: Option<String>,
    pub correlation_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i32>,
}

/// One log row whole: the summary and its payloads, **as stored** — masked
/// when they were written and never resolved again.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationLog {
    #[serde(flatten)]
    pub summary: IntegrationLogSummary,
    /// The document the call was made for; `null` for a test call.
    pub document_id: Option<Uuid>,
    /// What was sent, masked before it was stored.
    #[schema(value_type = Option<Object>)]
    pub request_payload: Option<Value>,
    /// What came back, masked before it was stored.
    #[schema(value_type = Option<Object>)]
    pub response_payload: Option<Value>,
}

/// The list's query string: paging and four filters.
///
/// Unknown parameters are ignored, as [`Pagination`] ignores them everywhere.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct IntegrationLogQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Only the calls to this external system.
    pub external_system_id: Option<Uuid>,
    /// Only the calls to this endpoint — rows whose `entityType` is
    /// `IntegrationEndpoint` and whose `entityId` is this id.
    pub endpoint_id: Option<Uuid>,
    pub status: Option<IntegrationLogStatus>,
    /// Inclusive lower bound on `startedAt`.
    pub from: Option<DateTime<Utc>>,
    /// **Exclusive** upper bound on `startedAt`, so consecutive ranges do not
    /// share a row.
    pub to: Option<DateTime<Utc>>,
}

impl IntegrationLogQuery {
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }
}

/// Refuses a range whose `to` is before its `from`. An empty range — `to`
/// equal to `from` — is allowed and selects nothing.
pub fn validate_query(query: &IntegrationLogQuery) -> Result<(), AppError> {
    match (query.from, query.to) {
        (Some(from), Some(to)) if to < from => {
            Err(AppError::validation(vec![ValidationDetail::new(
                "to",
                "range",
                "RANGE_INVERTED",
                "`to` is before `from`, so this range selects nothing",
            )]))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::integration::domain::details;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn every_status_the_column_allows_round_trips() {
        for status in [
            IntegrationLogStatus::Success,
            IntegrationLogStatus::Failed,
            IntegrationLogStatus::Pending,
            IntegrationLogStatus::Retrying,
            IntegrationLogStatus::DeadLetter,
        ] {
            assert_eq!(IntegrationLogStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn an_unknown_status_is_not_read_as_success() {
        assert_eq!(
            IntegrationLogStatus::from_db("SOMETHING_NEW"),
            IntegrationLogStatus::Failed
        );
    }

    #[test]
    fn a_direction_is_read_from_the_column() {
        assert_eq!(
            IntegrationDirection::from_db("INBOUND"),
            IntegrationDirection::Inbound
        );
        assert_eq!(
            IntegrationDirection::from_db("OUTBOUND"),
            IntegrationDirection::Outbound
        );
    }

    #[test]
    fn an_inverted_range_is_refused_and_an_empty_one_is_not() {
        let from = at("2026-09-29T10:00:00Z");
        let earlier = at("2026-09-29T09:00:00Z");

        let inverted = IntegrationLogQuery {
            from: Some(from),
            to: Some(earlier),
            ..Default::default()
        };
        let details = details(validate_query(&inverted).expect_err("refused"));
        assert_eq!(details[0].code, "RANGE_INVERTED");

        let empty = IntegrationLogQuery {
            from: Some(from),
            to: Some(from),
            ..Default::default()
        };
        assert!(validate_query(&empty).is_ok());
        assert!(validate_query(&IntegrationLogQuery::default()).is_ok());
    }

    #[test]
    fn the_detail_is_the_summary_flattened_with_its_payloads() {
        let log = IntegrationLog {
            summary: IntegrationLogSummary {
                id: Uuid::nil(),
                external_system_id: None,
                external_system_code: None,
                external_system_name: None,
                direction: IntegrationDirection::Outbound,
                integration_type: Some("REST".into()),
                method: Some("GET".into()),
                endpoint: None,
                entity_type: None,
                entity_id: None,
                status: IntegrationLogStatus::DeadLetter,
                status_code: None,
                error_message: None,
                correlation_id: "c".into(),
                started_at: at("2026-09-29T10:00:00Z"),
                completed_at: None,
                duration_ms: None,
            },
            document_id: None,
            request_payload: Some(serde_json::json!({ "a": 1 })),
            response_payload: None,
        };

        let json = serde_json::to_value(&log).expect("serialises");
        assert_eq!(json["status"], "DEAD_LETTER");
        assert_eq!(json["direction"], "OUTBOUND");
        assert_eq!(json["correlationId"], "c");
        assert_eq!(json["requestPayload"]["a"], 1);
        assert!(json["responsePayload"].is_null());
        assert!(json.get("summary").is_none(), "flattened: {json}");
    }
}
