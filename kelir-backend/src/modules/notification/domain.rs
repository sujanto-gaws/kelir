//! What a notification is (FR-NTF-001; [#251]).
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// What a notification is about (Database Schema §11.1's vocabulary).
///
/// **A closed set here and an open `TEXT` in the column**, which is the
/// opposite way round from `EventCategory` and its `CHECK`. §11.3 declares
/// `notification_type` with no constraint because
/// [#257](https://github.com/sujanto-gaws/kelir/issues/257)'s templates key off
/// it and a plugin may add one; this enum is the set *this release writes*.
/// [`Self::from_db`] is what keeps a row somebody else wrote readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationType {
    /// A task is waiting for you.
    TaskAssigned,
    /// A document you raised was decided.
    DocumentDecided,
    /// A type this build does not know, read from a row a later release wrote.
    Other,
}

impl NotificationType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::TaskAssigned => "TASK_ASSIGNED",
            Self::DocumentDecided => "DOCUMENT_DECIDED",
            Self::Other => "OTHER",
        }
    }

    /// **Unknown reads as [`Self::Other`] rather than refusing**, which is
    /// `EventCategory::from_db`'s reasoning and the same trade: nothing
    /// branches on this value, so a wrong guess costs a label and not a
    /// decision. The `title` and `body` are the row's own words and carry the
    /// meaning; the type is how a client groups them.
    ///
    /// The contrast is `VirusScanStatus::from_db`, which refuses — because
    /// there a wrong guess costs the bytes.
    pub fn from_db(value: &str) -> Self {
        match value {
            "TASK_ASSIGNED" => Self::TaskAssigned,
            "DOCUMENT_DECIDED" => Self::DocumentDecided,
            _ => Self::Other,
        }
    }
}

/// One notification, as its recipient reads it.
///
/// **No `recipientUserId`.** Every row this type can reach is the caller's own —
/// the statement is what guarantees it (#251 AC7) — so serializing the
/// recipient would be telling somebody their own id back. Its absence is also
/// what makes an accidentally unscoped read visible in a test: there is no
/// field to assert the wrong value in, so the assertion has to be about which
/// rows came back.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: Uuid,

    /// Where it points. A client offers the document, the task, or neither.
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,

    pub notification_type: NotificationType,
    pub title: String,
    pub body: String,

    /// **Null while unread**, and the only place readness lives — `status` has
    /// a `READ` value this release never writes, because two columns saying one
    /// thing invite them to disagree.
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// What the centre reports alongside the page.
///
/// **The unread count is not `meta.total`**, and the difference is the point:
/// the page is *everything addressed to me*, and the badge is *how many of them
/// I have not read*. A client computing the badge from the page would count one
/// page of it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    pub unread: i64,
}
