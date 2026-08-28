//! Moving a document through its own lifecycle (FR-DOC-007, [#169]).
//!
//! **Not reachable from `PUT /documents/{id}`.** A transition is not a field
//! edit: it has a from-state, a legal set, its own permission and its own audit
//! action, and letting an ordinary update carry `status` would put all of that
//! behind `document:update` (#169 AC1, which is #99's AC1 one module over).
//! `UpdateDocumentRequest` has no such member and `deny_unknown_fields` is what
//! refuses one.
//!
//! **Not reachable for `DRAFT -> SUBMITTED` either.** That is [`super::submit`]'s
//! transaction, and [`DocumentStatus::check_move_to`] refuses it by name with a
//! message pointing at the endpoint that does it.
//!
//! # And from Sprint 10, not reachable at all while a workflow is deciding
//!
//! [#178] AC2 makes the synchronization **one-way**: a workflow transition sets
//! the document's status, and setting the document's status does not move the
//! workflow. This route is where that costs something, and the cost is paid
//! deliberately — a document with a live process instance is refused here,
//! naming the instance and the action that would move it.
//!
//! Letting it through was the alternative and it is worse: it produces a
//! document whose status disagrees with the process driving it, which is the
//! exact defect #178 exists to prevent, and it produces it silently on the
//! screen a person is most likely to trust. AC2 allows for a manual override as
//! *"a separate, audited action rather than a side effect"*; nothing has asked
//! for one, so none is built, and this refusal is what makes the need visible if
//! anything ever does.
//!
//! **A document whose process has finished is transitionable again**, which is
//! not a loophole: `COMPLETED` and `CANCELLED` are not live, nothing is deciding
//! the document, and the legality table is back to being the only rule.
//!
//! [#169]: https://github.com/sujanto-gaws/kelir/issues/169
//! [#178]: https://github.com/sujanto-gaws/kelir/issues/178

use serde_json::json;
use uuid::Uuid;

use super::super::domain::{DocumentStatus, TransitionRequest, TransitionResult};
use super::super::repository as repo;
use super::super::{DOCUMENT_TRANSITION, OBJECT_TYPE};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::state::AppState;

/// Moves one document to a new status, or refuses and says why.
///
/// **The legality check and the write are one transaction, and the write is
/// conditional on the status the check read.** Two callers transitioning the
/// same document at once cannot both succeed from the same starting state: the
/// loser's `UPDATE` matches no row and gets a 409 rather than silently
/// overwriting the winner's decision. That is [#169] AC3, and [record 03] found
/// exactly this guard untested in the facility arm of the shared master-data
/// transition service (#139) — so the test here is the one that was missing
/// there.
///
/// [#169]: https://github.com/sujanto-gaws/kelir/issues/169
/// [record 03]: ../../../../../projects/verifications/03.%20Sprint%206%20Surface%20Verification.md
pub async fn transition(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: TransitionRequest,
) -> Result<TransitionResult, AppError> {
    // Its own permission, not `document:update` (AC4). Someone who may correct a
    // requisition's line items is not thereby someone who may approve it.
    caller.require(DOCUMENT_TRANSITION)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    // **Read without a lock, on purpose.** Locking the row here would serialise
    // two simultaneous transitions into two sequential ones, and the second
    // would then see the state the first wrote and be refused as an *illegal
    // move* rather than as a lost race. That is a correct outcome reached by the
    // wrong mechanism: it makes the compare-and-swap below unreachable, so the
    // guard that would still hold if somebody wrote a second caller is a guard
    // nothing exercises — which is what record 03 found in the facility arm of
    // the master-data transition service (#139).
    let current = repo::find_status(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    // #178 AC2, and it is checked before the legality table on purpose: a
    // caller whose document is under a workflow needs to be told *that*, not
    // that SUBMITTED cannot become APPROVED. The two refusals send them to
    // different places.
    refuse_while_a_workflow_is_deciding(state, tenant_id, id).await?;

    // A 422 naming both ends and what was possible. Refused before the write
    // rather than by it, because "you cannot go there from here" and "somebody
    // else moved it" are different failures and a caller fixes them
    // differently.
    current.check_move_to(request.status)?;

    let mut transaction = state.pool.begin().await?;

    let moved = repo::move_status(
        &mut transaction,
        tenant_id,
        id,
        current,
        request.status,
        actor,
    )
    .await?;

    if moved == 0 {
        // The row was in this state a moment ago and is no longer. Answering
        // with the stale check would report a move that did not happen.
        return Err(AppError::conflict(
            "this document changed while the transition was being applied",
        ));
    }

    repo::record_transition(
        &mut transaction,
        tenant_id,
        id,
        Some(current),
        request.status,
        actor,
        request.reason.as_deref(),
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Document.StatusChanged",
            // Distinct from UPDATE and from SUBMIT. An auditor asking "who
            // rejected this" must not have to read a payload to find out which
            // kind of write happened.
            action: "STATUS_CHANGE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: request.reason.as_deref(),
            old_value: Some(json!({ "status": current })),
            new_value: Some(json!({ "status": request.status })),
        },
    )
    .await;

    Ok(TransitionResult {
        previous_status: current,
        status: request.status,
    })
}

/// Refuses a manual transition on a document a workflow is deciding
/// ([#178](https://github.com/sujanto-gaws/kelir/issues/178) AC2).
///
/// Read on the pool rather than under the transaction's lock, and that is
/// correct rather than a lapse: this is not a check that guards a write against
/// a concurrent change, it is a check that this **surface** does not apply. A
/// process that starts a microsecond after this read is a process whose own
/// transition will then set the status the manual one just wrote — an ordering
/// no lock here can improve, because the two writers are the whole point of the
/// rule rather than a race inside it. What makes the outcome consistent is that
/// the workflow's write is the projection and always lands last.
async fn refuse_while_a_workflow_is_deciding(
    state: &AppState,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<(), AppError> {
    let Some(instance_id) =
        crate::modules::workflow::repository::instance::live_instance_of_document(
            &state.pool,
            tenant_id,
            id,
        )
        .await?
    else {
        return Ok(());
    };

    Err(AppError::conflict(format!(
        "this document is being decided by workflow instance {instance_id}; its status          follows that process rather than being set directly. Act on the task instead —          a status written here would disagree with the process the moment it moved"
    )))
}

/// A document's status history, oldest first.
///
/// Behind the document's own read permission and nothing more: the history is a
/// property of the document, and a caller who may read the document may read how
/// it got where it is.
pub async fn status_history(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Vec<StatusHistoryEntry>, AppError> {
    caller.require(super::super::DOCUMENT_READ)?;

    let tenant_id = caller.tenant_id();

    // The document is read first so that a history over a document in another
    // tenant answers 404 about the document rather than an empty list — an
    // empty list would say "this document has no history", which is a false
    // statement about a document that is not theirs to know about.
    if repo::find_document(&state.pool, tenant_id, id)
        .await?
        .is_none()
    {
        return Err(AppError::not_found("Document"));
    }

    let rows = sqlx::query!(
        r#"
        SELECT old_status, new_status, changed_by, reason, created_at
        FROM document_status_history
        WHERE tenant_id = $1 AND document_id = $2
        ORDER BY created_at, id
        "#,
        tenant_id,
        id
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| StatusHistoryEntry {
            previous_status: row.old_status.as_deref().map(DocumentStatus::from_db),
            status: DocumentStatus::from_db(&row.new_status),
            changed_by: row.changed_by,
            reason: row.reason,
            changed_at: row.created_at,
        })
        .collect())
}

/// One row of a document's status history.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusHistoryEntry {
    /// `None` on the row that records the document's creation.
    pub previous_status: Option<DocumentStatus>,
    pub status: DocumentStatus,
    /// `None` for a transition an engine made rather than a person — which
    /// nothing produces until Phase 5, and §6.10 made the column nullable for.
    pub changed_by: Option<Uuid>,
    pub reason: Option<String>,
    pub changed_at: chrono::DateTime<chrono::Utc>,
}
