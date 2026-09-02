//! Moving a master-data record through its governance lifecycle (FR-MDM-007).
//!
//! One use case, two entities. The transition is the same for a party and a
//! facility — read where the record is, check the move is legal, write it,
//! audit it — and the only thing that differs is which table the row is in, so
//! [`TransitionTarget`] carries that and the rest is shared. Two copies would
//! be two state machines.
//!
//! **Not reachable from `PUT /parties/{id}`.** A transition is not a field
//! edit: it has a from-state, a legal set, its own permission and its own audit
//! action, and letting an ordinary update carry `recordStatusId` would put all
//! of that behind `master-data:party:update` (#99 AC1). Neither update request
//! type has the field, and `deny_unknown_fields` is what refuses it.

use serde_json::json;
use uuid::Uuid;

use super::domain::{RecordStatus, TransitionRequest, TransitionResult, TransitionTarget};
use super::repository as repo;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::state::AppState;

/// The permission a transition needs.
///
/// One permission over a governance action rather than one per entity. The
/// alternative — `master-data:party:approve`, `master-data:facility:approve` —
/// would grow a row per entity for a control that does not vary by entity, and
/// **D-6** rejected exactly that shape for the catalogue. It is deliberately
/// not `master-data:party:update`: someone who may correct a supplier's address
/// is not thereby someone who may take the supplier out of service.
pub const RECORD_STATUS_TRANSITION: &str = "master-data:record-status:transition";

/// Moves one record to a new lifecycle status, or refuses and says why.
///
/// **Read and write in one statement, not two.** The status is checked against
/// the row and written back conditionally on it still holding that value, so
/// two callers transitioning the same record at once cannot both succeed from
/// the same starting state — the check-then-act failure #105 was about, and the
/// reason this does not read the row first and update it after.
pub async fn transition(
    state: &AppState,
    caller: &Authenticated,
    target: TransitionTarget,
    id: Uuid,
    request: TransitionRequest,
) -> Result<TransitionResult, AppError> {
    caller.require(RECORD_STATUS_TRANSITION)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let current = repo::find_record_status(&state.pool, tenant_id, target, id)
        .await?
        .ok_or_else(|| AppError::not_found(target.missing()))?;

    // **The parked state belongs to the process, not to a caller** (FR-MDM-010,
    // [#255](https://github.com/sujanto-gaws/kelir/issues/255), **D-55**).
    //
    // `may_move_to` permits `DRAFT -> PENDING_APPROVAL` because a governed
    // change makes that move; this surface refuses it because a person asking
    // for it would park a record with no document behind it — the record
    // awaiting an approver that does not exist, which is what this module's
    // documentation warned about before the workflow existed. And a person
    // asking to move a record *out* of it would strand the change document that
    // put it there.
    //
    // The state machine stays in one place and this is a rule about a surface,
    // which is the split that keeps `may_move_to` the only copy of the table.
    if current.is_parked() || request.record_status_id.is_parked() {
        return Err(parked_belongs_to_the_process(current));
    }

    current.check_move_to(request.record_status_id)?;

    let moved = repo::move_record_status(
        &state.pool,
        tenant_id,
        target,
        id,
        current,
        request.record_status_id,
        actor,
    )
    .await?;

    if moved == 0 {
        // The row was there a moment ago and is no longer in the state this
        // transition was checked against. Answering with the stale check would
        // report a move that did not happen.
        return Err(AppError::conflict(
            "The record changed while this transition was being applied",
        ));
    }

    let event_type = format!("{}.RecordStatusChanged", target.entity());

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: &event_type,
            // Distinct from STATUS_CHANGE, which #80 uses for
            // `mdm_parties.status`. Two columns, two meanings, two actions — an
            // auditor asking "who took this supplier out of service" must not
            // have to read the payload to tell which one happened.
            action: "RECORD_STATUS_CHANGE",
            object_type: target.object_type(),
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: request.reason.as_deref(),
            old_value: Some(json!({ "recordStatusId": current.as_db() })),
            new_value: Some(json!({ "recordStatusId": request.record_status_id.as_db() })),
        },
    )
    .await;

    Ok(TransitionResult {
        previous_record_status_id: current,
        record_status_id: request.record_status_id,
    })
}

/// The lifecycle status of one record, for the aggregate to read back.
pub async fn record_status_of(
    state: &AppState,
    tenant_id: Uuid,
    target: TransitionTarget,
    id: Uuid,
) -> Result<Option<RecordStatus>, AppError> {
    Ok(repo::find_record_status(&state.pool, tenant_id, target, id).await?)
}

/// The refusal for a transition into or out of the parked state.
///
/// A **409** rather than a 422: the transition asked for is legal in the state
/// machine, and what refuses it is where the record is and who is asking — a
/// property of the resource's condition rather than of the request.
fn parked_belongs_to_the_process(current: RecordStatus) -> AppError {
    if current.is_parked() {
        return AppError::conflict(
            "this record has a change awaiting approval; it leaves PENDING_APPROVAL when that \
             change is approved or refused, not by a transition",
        );
    }

    AppError::conflict(
        "PENDING_APPROVAL is entered by raising a change document against this record, not by a \
         transition: a record parked by hand would be waiting for an approval nobody can give",
    )
}
