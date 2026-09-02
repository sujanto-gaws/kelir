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
///
/// **The subject travels as an id and not as a description**
/// ([#292](https://github.com/sujanto-gaws/kelir/issues/292), **D-45**). The
/// four link columns are here so a reader can go and ask for the file, the
/// comment or the task — through the surface that checks its own permission —
/// and `details` is what is left once nothing in it belongs to another surface.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: Uuid,
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub attachment_id: Option<Uuid>,
    pub comment_id: Option<Uuid>,
    /// The dotted vocabulary of naming convention §7 — `Document.Submitted`,
    /// `Workflow.TaskCompleted`.
    pub event_type: String,
    pub event_category: EventCategory,
    pub actor_user_id: Option<Uuid>,
    /// **The actor's name when this happened**, not now (#247 AC5).
    pub actor_name: Option<String>,
    pub action_summary: String,
    /// What happened **to the document**, and nothing about the subject — see
    /// [`disclosable`], which is what the read passes it through.
    pub details: Value,
    pub occurred_at: DateTime<Utc>,
}

/// What each event type's `details` may say — **by name, closed, and empty by
/// default** ([#292](https://github.com/sujanto-gaws/kelir/issues/292) AC1–AC2,
/// **D-45**).
///
/// # Why this exists when the write path no longer produces the keys
///
/// D-45 takes the disclosure out at the source: `Attachment.Added` stopped
/// carrying the file's name, `Comment.Added` its length, `Workflow.Decided` the
/// second party to a delegation. That fixes every row written from this release
/// on and **nothing already in the table** — the names an earlier release wrote
/// are still in `details_json`, and `activity_events` is append-only, so there
/// is no version of this fix that reaches them by rewriting.
///
/// So the read side names what it will serve, and the rows the rule did not
/// govern are governed at the boundary instead. That the write path now agrees
/// with it is what makes this list short, not what makes it unnecessary.
///
/// # Why an allow-list, and why the unknown event serves nothing
///
/// A deny-list has to be extended by every module that adds an event type, and
/// forgetting is silent — which is the *whole* of what #292 is: a second
/// permission nobody remembered to ask for. An allow-list forgets in the safe
/// direction. An event type this release does not know is one written by a
/// release that did not consult this table, so it serves `{}` and the entry
/// still renders: the event type, the summary, the actor and the link are
/// enough for a timeline, which is D-45's argument in one line.
///
/// **[`EventCategory::from_db`] guesses and this refuses**, one screen apart,
/// for the reason stated there: a wrong category costs a label, and a wrong key
/// here costs the file name.
pub fn disclosable(event_type: &str, details: Value) -> Value {
    // Keyed by event type rather than by category, because `Workflow.Decided`
    // needs both answers: the action and the states are the document's own
    // story and stay, and `onBehalfOfUserId` is the workflow's and goes.
    let permitted: &[&str] = match event_type {
        // The document's own lifecycle, behind the document's own read — which
        // the timeline has already required by the time this is called.
        "Document.Created" => &["documentTypeId"],
        "Document.StatusChanged" => &["from", "to"],
        "Document.Submitted" => &["documentNumber"],
        // *What* was decided moved this document, so it is the document's.
        // *On whose behalf* is the delegation's, and `workflow_history` keeps
        // it behind the workflow's read.
        "Workflow.Decided" => &["action", "from", "to"],
        // Everything an attachment, a comment or a hand-off could say about
        // itself is behind `attachment:read`, `comment:read` or the workflow's
        // read. The timeline says that it happened, and links.
        "Attachment.Added" | "Attachment.Downloaded" => &[],
        // The comment epic's four, and the tail's three say no more than the
        // first did ([#253](https://github.com/sujanto-gaws/kelir/issues/253)).
        // *Somebody replied*, *somebody edited*, *somebody deleted* — the words
        // before and after an edit are the comment's, behind `comment:read`,
        // and an edit is the one event where carrying them would be most
        // tempting and most wrong: it would put a copy of the old text where
        // deleting the comment cannot reach it.
        "Comment.Added" | "Comment.Replied" | "Comment.Edited" | "Comment.Deleted" => &[],
        "Workflow.TaskDelegated" => &[],
        _ => &[],
    };

    match details {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .filter(|(key, _)| permitted.contains(&key.as_str()))
                .collect(),
        ),
        // `details_json` is `NOT NULL DEFAULT '{}'` and every writer passes an
        // object. A scalar is a row this codebase did not write, and there is
        // no key in it to check.
        _ => Value::Object(serde_json::Map::new()),
    }
}
