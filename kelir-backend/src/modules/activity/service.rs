//! Writing the timeline, and reading it (FR-ACT-001, FR-ACT-004; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

use serde_json::Value;
use uuid::Uuid;

use super::domain::{self, ActivityEvent, EventCategory};
use super::repository as repo;
use super::ACTIVITY_READ;
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
/// **Two permissions, and the document's own read is what scopes it.**
/// `activity:read` says whether this account reads timelines at all; which
/// document's is the document's question, asked through its module's service —
/// the rule `modules::attachment` and `modules::comment` both state, for the
/// same reason: a thing that hangs on a document cannot be more visible than the
/// document.
///
/// # And a third rule, one line below the other two
///
/// **Two permissions are all this asks for, and that is why the entries carry
/// nothing a third would have guarded** ([#292], **D-45**). An attachment's
/// name, a comment's length and the second party to a delegation are behind
/// `attachment:read`, `comment:read` and the workflow's own read; a timeline
/// repeating them would be a fourth surface answering three other modules'
/// questions without asking their permissions.
///
/// The write paths no longer produce those keys. [`domain::disclosable`] is
/// what makes that true of the **rows already in the table**, which no fix to a
/// writer can reach — `activity_events` is append-only, so the boundary is the
/// only place left that can hold them.
///
/// [#292]: https://github.com/sujanto-gaws/kelir/issues/292
pub async fn list_activity(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<ActivityEvent>, PageMeta), AppError> {
    caller.require(ACTIVITY_READ)?;

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
