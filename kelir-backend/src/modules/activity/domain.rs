//! What a timeline entry is (FR-ACT-001, FR-ACT-004; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

/// Which part of the product an event came from (Database Schema §10.1).
///
/// **A closed set, matched by the column's `CHECK`.** The three this release
/// writes are `DOCUMENT`, `WORKFLOW` and — from
/// [#248](https://github.com/sujanto-gaws/kelir/issues/248) — `ATTACHMENT` and
/// `COMMENT`. The rest exist because §10.1 declares them, and a type that could
/// not represent a row the database permits would panic on a row somebody else
/// wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventCategory {
    Document,
    Attachment,
    Comment,
    Workflow,
    Security,
    MasterData,
    Notification,
}

impl EventCategory {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Document => "DOCUMENT",
            Self::Attachment => "ATTACHMENT",
            Self::Comment => "COMMENT",
            Self::Workflow => "WORKFLOW",
            Self::Security => "SECURITY",
            Self::MasterData => "MASTER_DATA",
            Self::Notification => "NOTIFICATION",
        }
    }

    /// **Unknown reads as `Document`**, which is the least wrong answer for a
    /// timeline: a row a later release wrote is still a thing that happened to
    /// this document, and refusing to render it would hide the event rather
    /// than the category. Nothing branches on this value, so a wrong guess
    /// costs a label and not a decision — which is why it is a fallback here
    /// and a refusal in `VirusScanStatus::from_db`, where it costs the bytes.
    pub fn from_db(value: &str) -> Self {
        match value {
            "ATTACHMENT" => Self::Attachment,
            "COMMENT" => Self::Comment,
            "WORKFLOW" => Self::Workflow,
            "SECURITY" => Self::Security,
            "MASTER_DATA" => Self::MasterData,
            "NOTIFICATION" => Self::Notification,
            _ => Self::Document,
        }
    }
}

/// One entry, as the timeline reports it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: Uuid,
    pub document_id: Option<Uuid>,
    /// The dotted vocabulary of naming convention §7 — `Document.Submitted`,
    /// `Workflow.TaskCompleted`.
    pub event_type: String,
    pub event_category: EventCategory,
    pub actor_user_id: Option<Uuid>,
    /// **The actor's name when this happened**, not now (#247 AC5).
    pub actor_name: Option<String>,
    pub action_summary: String,
    pub details: Value,
    pub occurred_at: DateTime<Utc>,
}
