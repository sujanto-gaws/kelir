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
        "Attachment.Added" | "Attachment.Downloaded" | "Attachment.Deleted" => &[],
        // A reference's label and URL are the two most quotable things this
        // module stores — a URL names a host, a path and often a customer — and
        // both are behind `attachment:read`
        // ([#254](https://github.com/sujanto-gaws/kelir/issues/254)).
        "Reference.Added" | "Reference.Deleted" => &[],
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

/// The event types that mean **this person touched this document** (FR-RPT-003,
/// [#433]).
///
/// # The definition, in one sentence, before anything queries it
///
/// > **You touched a document when you raised or changed it, said something on
/// > it, attached something to it, or moved its workflow.**
///
/// [#433] AC2 asks for that sentence to exist *before* the query rather than
/// after it, and the reason is in the requirement's own wording: **`recent` is
/// exactly the word that hides an unstated join.** A widget called *what you
/// touched last* whose ordering nobody can state is a widget nobody can test,
/// because every observed row is consistent with some definition.
///
/// So the four clauses below are the definition, and the list is exhaustive —
/// the groups are the sentence's four verbs in the same order.
///
/// # Why this list lives here and not in `modules::reporting`
///
/// **`activity_events.event_type` is this module's vocabulary** (naming
/// convention §7), and [`disclosable`] one screen up is already this module
/// classifying it. A reader asking *what does `Workflow.TaskClaimed` count as*
/// should find both answers in one file, and a reporting module holding its own
/// copy of the vocabulary would be a second place to update when a verb is
/// added.
///
/// # Why an allow-list — the same argument [`disclosable`] makes, pointing the
/// other way
///
/// [`disclosable`] is an allow-list because forgetting to extend a deny-list is
/// silent, and a silent omission there serves a file name to somebody who may
/// not read it. **Here a silent omission under-reports a card**, which is the
/// mild direction — but the allow-list earns its place for a different reason:
/// **one excluded event type is a read.**
///
/// `Attachment.Downloaded` is written when somebody *opens* a file
/// (`attachment::service`), and it is the one row in the table that records
/// looking rather than doing. Under a deny-list it counts by default, and the
/// widget quietly stops being *what you touched* and becomes *what you looked
/// at* — a different feature, with a different privacy question, arrived at by
/// nobody deciding anything. **An event type nobody has classified should not
/// change what a screen means**, and that is what makes the list closed rather
/// than open.
///
/// **`Document.Deleted` is excluded for a different reason**: it is not a
/// judgement about whether deleting is touching, but that the statement joins
/// `documents` with `deleted_at IS NULL` ([#433] AC6), so the row it names
/// cannot come back anyway. A predicate that can never match belongs out of the
/// list rather than in it looking load-bearing.
///
/// The workflow *definition* events — `Workflow.Created`, `Workflow.Updated`,
/// `Workflow.Published`, `Workflow.RevisionCreated`, `Workflow.Deleted` — are
/// absent because they carry no `document_id` at all. They are things that
/// happened to a workflow, and this list is read through a join on the document.
///
/// # When a verb is added
///
/// A module adding an event type with a `document_id` has to decide whether it
/// is a touch, and `a_new_event_type_is_classified_deliberately` is what makes
/// that a decision rather than an omission: it pins the set, so the test fails
/// and whoever added the verb writes down which of the four clauses it belongs
/// to.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
pub const TOUCH_EVENT_TYPES: &[&str] = &[
    // **You raised or changed the document itself.** `Document.Deleted` is the
    // one absence, and the paragraph above says why it is not a judgement.
    "Document.Created",
    "Document.Updated",
    "Document.Submitted",
    "Document.StatusChanged",
    // **You said something on it.** An edit and a deletion are in because the
    // question is *did this person act on this document*, and tidying your own
    // wording is acting on it — the alternative would date the document from a
    // comment you have since rewritten.
    "Comment.Added",
    "Comment.Replied",
    "Comment.Edited",
    "Comment.Deleted",
    // **You attached something to it, or took something off.** Adding and
    // removing only — `Attachment.Downloaded` is the read this list exists to
    // keep out.
    "Attachment.Added",
    "Attachment.Deleted",
    "Reference.Added",
    "Reference.Deleted",
    // **You moved its workflow.** Deciding is the obvious one; claiming and
    // delegating are here because both are a person taking a position on this
    // document's progress, which is what somebody scanning *what did I work on*
    // is looking for.
    "Workflow.Decided",
    "Workflow.TaskClaimed",
    "Workflow.TaskCompleted",
    "Workflow.TaskDelegated",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_event_type_is_classified_deliberately() {
        // **This test exists to fail.** Adding an event type that names a
        // document is a decision about whether it is a touch (FR-RPT-003), and
        // the failure is what turns that into a decision instead of a default.
        // If you are here because you added a verb: put it in one of
        // `TOUCH_EVENT_TYPES`' four groups, or add it below with the reason it
        // is not a touch.
        assert_eq!(
            TOUCH_EVENT_TYPES,
            &[
                "Document.Created",
                "Document.Updated",
                "Document.Submitted",
                "Document.StatusChanged",
                "Comment.Added",
                "Comment.Replied",
                "Comment.Edited",
                "Comment.Deleted",
                "Attachment.Added",
                "Attachment.Deleted",
                "Reference.Added",
                "Reference.Deleted",
                "Workflow.Decided",
                "Workflow.TaskClaimed",
                "Workflow.TaskCompleted",
                "Workflow.TaskDelegated",
            ],
        );
    }

    #[test]
    fn opening_a_file_is_not_touching_the_document() {
        // The one excluded event type that is a **read**, and the reason
        // `TOUCH_EVENT_TYPES` is closed rather than "everything but". Under a
        // deny-list this counts by default and the widget silently becomes
        // *what you looked at* — a different feature nobody chose.
        assert!(!TOUCH_EVENT_TYPES.contains(&"Attachment.Downloaded"));
    }

    #[test]
    fn deleting_a_document_names_a_row_the_statement_cannot_return() {
        // Not a judgement about whether deleting is touching: the recent-documents
        // statement joins `documents` with `deleted_at IS NULL` (#433 AC6), so
        // this event type could only ever match a row already excluded. A
        // predicate that can never match belongs out of the list rather than in
        // it looking load-bearing.
        assert!(!TOUCH_EVENT_TYPES.contains(&"Document.Deleted"));
    }

    #[test]
    fn a_workflow_definition_event_is_not_a_document_event() {
        // These carry no `document_id`, so the join drops them whatever this
        // list says. They are listed here so the absence reads as known.
        for event_type in [
            "Workflow.Created",
            "Workflow.Updated",
            "Workflow.Published",
            "Workflow.RevisionCreated",
            "Workflow.Deleted",
        ] {
            assert!(
                !TOUCH_EVENT_TYPES.contains(&event_type),
                "{event_type} names a workflow rather than a document",
            );
        }
    }

    #[test]
    fn the_touch_list_has_no_duplicates() {
        // A duplicate would be harmless in `= ANY($n)` and is exactly the kind
        // of thing a hand-maintained list grows, so it is asserted rather than
        // trusted.
        let mut sorted = TOUCH_EVENT_TYPES.to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();

        assert_eq!(sorted.len(), count);
    }
}
