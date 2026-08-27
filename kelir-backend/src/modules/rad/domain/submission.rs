//! A filled-in form, as the server re-evaluated it (FR-RAD-010, [#164]).
//!
//! **`payload` is the server's answer and never the client's.** JFSS S8.1 makes
//! the backend re-evaluate every `calculate` expression and overwrite the
//! submitted value before persistence, and S10.2 does the same for every
//! `conditional`. Whatever the browser computed reaches
//! [`crate::modules::rad::service::evaluation`] and stops there; what a
//! [`Submission`] carries is what came out.
//!
//! **This is not a document, and the distinction is the sprint boundary.**
//! FR-DOC-001..007 — creating a document from a type, its number, its status,
//! its versions — are Sprint 9's under decision **D-16**, and a form that
//! submits is not yet a document that exists. What is here is the smallest row
//! that can prove a re-evaluation happened and be read back
//! (construction plan §6.1).
//!
//! [#164]: https://github.com/sujanto-gaws/kelir/issues/164

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

/// The largest payload this API accepts.
///
/// Bounded for the reason `MAX_DEFINITION_BYTES` is, and at the same size: the
/// column is JSONB and PostgreSQL would take a hundred megabytes of it, at
/// which point every read of the submission carries that. A form whose
/// definition is capped at a megabyte cannot legitimately collect much more
/// than one — a datagrid is the only shape that grows without bound, and a
/// requisition with ten thousand lines is a spreadsheet somebody attached to
/// the wrong surface.
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

/// Submitting a filled-in form.
///
/// **One property, and it carries every data key** — JFSS S10.1 requires the
/// client to submit the `key` of every `role: "data"` component, *visible or
/// not*. S10.1.1 is why a hidden key being present is expected rather than
/// suspicious: a field whose `conditional` depends on a hidden field would
/// otherwise be decided from different inputs on the two sides, which is the
/// Polyglot Parity failure the errata was written to close. The server
/// discards the values of the components it computes as hidden; it does not
/// refuse the submission for carrying them.
///
/// There is deliberately no `formRevision` here. The revision is the one the
/// path names — a published revision is immutable and a new revision is a new
/// row — so a caller that could state one could state a different one.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitFormRequest {
    /// Every data key the definition declares, carrying its current value.
    pub payload: Value,
}

/// A stored submission.
///
/// `payload` is read back from the row rather than returned from memory, for
/// the reason every write in this module is read back before it is audited
/// (#135): the response then says what was stored rather than what the service
/// believed it stored, and those are different claims — which is exactly the
/// claim a Tamper-Proof Pattern is asked to make good on.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub id: Uuid,
    pub form_id: Uuid,
    /// The revision of the form this was filled in against.
    pub form_revision: i32,
    /// The server's re-evaluated payload.
    pub payload: Value,
    pub submitted_at: DateTime<Utc>,
    pub submitted_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
