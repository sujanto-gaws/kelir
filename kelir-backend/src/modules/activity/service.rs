//! Writing the timeline, and reading it (FR-ACT-001, FR-ACT-004; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

use serde_json::Value;
use uuid::Uuid;

use super::domain::{self, ActivityEvent, EventCategory};
use super::repository as repo;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::document::service::document as document_service;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What a module hands over when something happens to a document.
///
/// **Nothing here is looked up.** The caller has the document, the actor and the
/// transaction already; a service that went back to the database to enrich an
/// event would be a service that can fail *after* the action it describes has
/// been decided, which is the failure mode `record` exists to make impossible.
pub struct Happening<'a> {
    pub tenant_id: Uuid,
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub attachment_id: Option<Uuid>,
    pub comment_id: Option<Uuid>,
    /// Naming convention §7's dotted vocabulary — `Document.Submitted`.
    pub event_type: &'a str,
    pub category: EventCategory,
    pub actor_user_id: Option<Uuid>,
    /// The actor's name **now**, which is what the row keeps for ever (#247 AC5).
    pub actor_name: Option<&'a str>,
    pub action_summary: &'a str,
    pub details: Value,
}

/// Records one thing that happened, **in the caller's transaction**.
///
/// # This is the half of the four-record distinction that a signature can hold
///
/// `modules::audit::record` takes a `&PgPool` and writes on its own connection,
/// and `record_or_warn` swallows the failure — because an audit row is a
/// **control over** an action and must exist whether or not anybody wanted it
/// to. This takes `&mut sqlx::PgTransaction` and returns its error, because an
/// activity event is **part of what the action produced**: an approval that
/// rolled back did not happen, and a timeline saying it did would be worse than
/// a timeline that never mentioned it.
///
/// [#247] AC2 states that as a rule about transactions. It is *held* by there
/// being nowhere else to write one: no caller can reach
/// [`repo::insert_event`] with a pool, because the module exposes this.
///
/// **A failure here fails the action.** That is the point, and it is the
/// opposite of the audit path's deliberate tolerance. The two are one line apart
/// in this file so nobody has to hold both rules in their head at once.
pub async fn record(
    transaction: &mut sqlx::PgTransaction<'_>,
    happening: &Happening<'_>,
) -> Result<(), AppError> {
    repo::insert_event(
        &mut **transaction,
        &repo::NewActivityEvent {
            tenant_id: happening.tenant_id,
            document_id: happening.document_id,
            workflow_instance_id: happening.workflow_instance_id,
            task_id: happening.task_id,
            attachment_id: happening.attachment_id,
            comment_id: happening.comment_id,
            event_type: happening.event_type,
            event_category: happening.category,
            actor_user_id: happening.actor_user_id,
            actor_name: happening.actor_name,
            action_summary: happening.action_summary,
            details: happening.details.clone(),
        },
    )
    .await?;

    Ok(())
}

/// A document's timeline.
///
/// **One permission, and it is the document's own** ([#250] AC2, **D-47**).
/// Whether this caller may see what happened to a document is the same question
/// as whether they may see the document, asked once, through its module's
/// service — which is also what scopes the read to a tenant and to a row, so
/// there is nothing a second check could add.
///
/// # It asked for `activity:read` as well, and D-45 is why it no longer does
///
/// [#292] found the entries carrying an attachment's original file name, a
/// comment's length and the second party to a delegation — detail belonging to
/// three other surfaces, each with its own permission. `activity:read` was the
/// only thing standing between a document's reader and those, and it stood
/// there by accident: nothing said it was for that, and it guarded them all
/// equally badly, since anybody who held it saw everything.
///
/// **D-45 took the detail out.** What an entry says now is what happened *to
/// the document* — created, submitted, moved, a file was attached, somebody
/// commented, a decision was taken — and every one of those is the document's
/// own history, which `document:read` covers by definition. So the second
/// permission was left guarding nothing the first does not, and a reader who
/// may open a document had to be granted a separate permission to be told what
/// had happened to it.
///
/// **Whoever raised the document is the commonest reader of its timeline**, and
/// a deployment has no reason to have granted them anything named `activity`.
/// That is [#263]'s shape, which this project has now met three times:
/// #263 a screen showing too little, #292 a surface showing too much, and this.
///
/// [`domain::disclosable`] is what keeps the premise true, including for the
/// rows written before D-45 — `activity_events` is append-only, so the boundary
/// is the only place that can hold them.
///
/// [#250]: https://github.com/sujanto-gaws/kelir/issues/250
/// [#263]: https://github.com/sujanto-gaws/kelir/issues/263
/// [#292]: https://github.com/sujanto-gaws/kelir/issues/292
pub async fn list_activity(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<ActivityEvent>, PageMeta), AppError> {
    // **The document's read is the whole gate**, and it is not a missing check
    // (D-47). `get_document` requires `document:read`, scopes to the caller's
    // tenant and answers 404 for a row they may not see — so a caller who
    // reaches the line below has already been told they may read this document,
    // and what follows is that document's own history.
    let document = document_service::get_document(state, caller, document_id).await?;
    let tenant_id = caller.tenant_id();

    // **The count and the page are drawn under the same predicate** (#247 AC6),
    // and D-45 is what keeps that true: redacting a field changes what an entry
    // says and never whether it is there, so no entry is dropped and the total
    // still counts what the page is a page of. Filtering entries by the reader —
    // the other shape #292 offered — is what would have put these two statements
    // into disagreement.
    let total = repo::count_for_document(&state.pool, tenant_id, document.id).await?;
    let events = repo::list_for_document(
        &state.pool,
        tenant_id,
        document.id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    let events = events
        .into_iter()
        .map(|event| ActivityEvent {
            details: domain::disclosable(&event.event_type, event.details),
            ..event
        })
        .collect();

    Ok((events, pagination.meta(total.max(0) as u64)))
}
