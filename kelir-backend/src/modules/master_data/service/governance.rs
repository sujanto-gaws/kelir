//! Master-data changes routed through the document workflow (FR-MDM-010;
//! [#255], **D-55**, [ADR-0033]).
//!
//! # The dependency runs one way, and this module is the far end of it
//!
//! [#255] AC6: *nothing in the workflow engine learns about master data*. It
//! does not. The chain is **engine → document → here**:
//!
//! * the engine projects a state's `mapsToDocumentStatus` onto the document, as
//!   it does for every document;
//! * the document module — which has known about master data since FR-DOC-011's
//!   entity link — asks this module to settle whatever change that document
//!   was carrying;
//! * this module knows which table a `PARTY` is in, and nothing above it does.
//!
//! There is no `match` on entity type anywhere in `modules::workflow`, and the
//! two functions here are the only places one exists at all. That was the
//! failure [#178](https://github.com/sujanto-gaws/kelir/issues/178) AC4 named,
//! and it is avoided by direction rather than by discipline.
//!
//! # Both writes are in the transaction that caused them
//!
//! [`raise`] runs inside the submit's transaction: the document becomes
//! `PENDING` and the record becomes `PENDING_APPROVAL` together, or neither
//! does. [`settle`] runs inside the transaction that closes the process
//! ([#255] AC5): the document becomes `APPROVED` and the record takes the
//! change in the same statement batch.
//!
//! A record parked at `PENDING_APPROVAL` by a submit that then rolled back
//! would be a record nobody can edit and no approver can see a document for.
//!
//! [#255]: https://github.com/sujanto-gaws/kelir/issues/255
//! [ADR-0033]: ../../../../docs/architectures/adr/0033.%20A%20Governed%20Record%20Parks%20at%20Pending%20Approval.md

use serde_json::Value;
use uuid::Uuid;

use super::domain::{self, GovernedEntity, RecordStatus};
use super::repository as repo;
use crate::error::AppError;
use crate::state::AppState;

/// Whether this document type governs a master-data entity, and which.
///
/// **Configuration, not code** ([#255] AC3). `document_types.target_entity_type`
/// has carried this since `0015_document.sql` and was read by nothing until
/// now; a type that sets it and binds a workflow routes that entity's changes
/// through approval.
///
/// A value this build cannot place governs nothing — see
/// [`GovernedEntity::from_db`].
pub fn governed_entity(target_entity_type: Option<&str>) -> Option<GovernedEntity> {
    target_entity_type.and_then(GovernedEntity::from_db)
}

/// Parks a record while its change is approved ([#255] AC1), **in the submit's
/// own transaction**.
///
/// Called by `document::service::submit` for a document whose type governs an
/// entity. Everything it refuses, it refuses *before* the submit commits, so a
/// refusal leaves no document in flight and no record parked.
///
/// # Three refusals, and the order is deliberate
///
/// 1. **The change is unreadable** — `domain::read_change`, which also refuses a
///    change naming a field this process cannot apply. Checked first because it
///    is a property of the payload and needs no row.
/// 2. **The record cannot take a change** — archived, or already parked by a
///    change raised through some other route.
/// 3. **A change is already in flight** — the partial unique index, which is
///    what makes the answer true when two submissions arrive at once.
pub async fn raise(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
    document_id: Uuid,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<(), AppError> {
    // The change itself, checked at the moment it is proposed rather than at
    // the moment it is approved. See `domain::governance`'s header.
    let _change = domain::read_change(entity, form_data)?;

    let current = repo::lock_record_status(transaction, tenant_id, entity, entity_id)
        .await?
        .ok_or_else(|| AppError::not_found(entity.missing()))?;

    // **A record already parked is refused as what it is**, before the state
    // machine gets a chance to say something less useful. `check_move_to` would
    // refuse this too — `PENDING_APPROVAL` is not in its own legal set — with
    // *PENDING_APPROVAL cannot become PENDING_APPROVAL*, which is true and
    // tells the person nothing about the change that is already in flight.
    if current.is_parked() {
        return Err(domain::already_in_flight());
    }

    // `may_move_to` is the state machine, asked rather than re-implemented:
    // a change parks a record from `DRAFT` or `ACTIVE`, and an archived record
    // is refused here rather than by a rule this module keeps its own copy of.
    current.check_move_to(RecordStatus::PendingApproval)?;

    let id = Uuid::now_v7();

    repo::insert_change_request(
        transaction,
        &repo::NewChangeRequest {
            id,
            tenant_id,
            document_id,
            entity,
            entity_id,
            previous: current,
            actor,
        },
    )
    .await
    .map_err(in_flight_or_else)?;

    let moved = repo::move_record_status_in(
        transaction,
        tenant_id,
        entity,
        entity_id,
        current,
        RecordStatus::PendingApproval,
        actor,
    )
    .await?;

    if moved == 0 {
        // Unreachable: the row was read under `FOR UPDATE` three statements
        // above and this transaction still holds it.
        return Err(AppError::Internal {
            source: anyhow::anyhow!(
                "record {entity_id} was locked at {} and then not parked",
                current.as_db()
            ),
        });
    }

    Ok(())
}

/// Applies or refuses the change a document was carrying, **in the transaction
/// that closed the process** ([#255] AC5).
///
/// Called by the document module when a workflow projects a terminal status.
/// A document that carried no change settles nothing and says nothing — most
/// documents are not governed changes, and this is on the path of every one of
/// them.
///
/// # What each outcome does
///
/// * **`APPROVED`** — the change is written to the record and the record becomes
///   `ACTIVE`. Approving a change is what makes a record active, which is the
///   `DRAFT → PENDING_APPROVAL → ACTIVE` path `record_status` was drawn for.
/// * **anything else terminal** — `REJECTED` and `CANCELLED` — the record is put
///   back where it was and **not written** ([#255] AC4). `previous_record_status`
///   is why *back* is not guessed: an `ACTIVE` supplier whose change is refused
///   is still an active supplier.
///
/// A non-terminal status settles nothing: a document moving to `PENDING` or
/// `IN_REVIEW` is a change still being decided.
///
/// # The record of the attempt (AC4)
///
/// **The `mdm_change_requests` row is the record**, kept rather than deleted and
/// carrying its outcome: `GET /master-data/parties/{id}/change-requests` lists
/// what has been proposed for a record and how each attempt ended, which is the
/// record's own history rather than the document's.
///
/// **No audit row is written here, and that is a constraint rather than a
/// choice.** `audit::record` takes a `&PgPool` — the hash chain is written on
/// its own connection, deliberately, because an audit row is a control over an
/// action rather than part of it — and this function runs inside a transaction
/// the workflow engine owns, which holds no pool. Threading `AppState` through
/// `engine::enter` to reach one would put an application handle into the one
/// module in this codebase that is written entirely against a transaction.
/// The document's own approval is audited by the workflow, where the pool is.
pub async fn settle(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    approved: bool,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(change) =
        repo::lock_open_change_for_document(transaction, tenant_id, document_id).await?
    else {
        return Ok(());
    };

    let target = if approved {
        RecordStatus::Active
    } else {
        change.previous_record_status
    };

    if approved {
        apply(transaction, tenant_id, &change, form_data, actor).await?;
    }

    let moved = repo::move_record_status_in(
        transaction,
        tenant_id,
        change.entity,
        change.entity_id,
        RecordStatus::PendingApproval,
        target,
        actor,
    )
    .await?;

    if moved == 0 {
        // The record is not where this change parked it. Somebody moved it by
        // another route, and applying a change to a record in an unknown state
        // is worse than refusing: the approval stands, the change does not, and
        // the mismatch is loud rather than silent.
        return Err(AppError::Internal {
            source: anyhow::anyhow!(
                "record {} was not at PENDING_APPROVAL when its change was settled",
                change.entity_id
            ),
        });
    }

    repo::resolve_change_request(
        transaction,
        tenant_id,
        change.id,
        if approved { "APPLIED" } else { "REFUSED" },
        actor,
    )
    .await?;

    Ok(())
}

/// What has been proposed for a record, newest first ([#255] AC4).
///
/// **The record's own history of attempts**, including the refused ones: a
/// change that was raised and rejected is a thing that happened to this record,
/// and a list that showed only what was applied would answer *what is this
/// record* while hiding *what was asked of it*.
///
/// Behind the record's own read permission, checked by the caller: this is the
/// record's data, and whoever may read the record may read what has been
/// proposed for it. The document behind each attempt is still behind
/// `document:read` — this list names it and does not open it.
pub async fn list_changes(
    state: &AppState,
    caller: &crate::middleware::auth::Authenticated,
    entity: GovernedEntity,
    entity_id: Uuid,
) -> Result<Vec<domain::ChangeAttempt>, AppError> {
    caller.require(match entity {
        GovernedEntity::Party => super::PARTY_READ,
        GovernedEntity::Facility => super::FACILITY_READ,
    })?;

    Ok(repo::changes_for_record(&state.pool, caller.tenant_id(), entity, entity_id).await?)
}

/// Writes the approved change to the record.
///
/// **The record's own fields, in one statement**, which is what makes this
/// possible inside a transaction somebody else opened. `domain::read_change`
/// refused anything wider at raise, so what arrives here is what can be applied
/// here.
async fn apply(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    change: &super::domain::ChangeRequest,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<(), AppError> {
    // Re-read rather than carried from the raise: the document's payload is
    // what was approved, and reading it here means the value applied is the one
    // the approver saw rather than one this module remembered.
    //
    // A payload that no longer reads as a change is an internal error and not a
    // refusal: it was readable when it was raised, `mark_submitted` is the last
    // write to `form_data_json`, and an approval that cannot be applied is a
    // state this design says cannot happen. If it does, it says so.
    let proposed =
        domain::read_change(change.entity, form_data).map_err(|error| AppError::Internal {
            source: anyhow::anyhow!(
                "document {} was approved carrying a change that no longer reads as one: {error:?}",
                change.document_id
            ),
        })?;

    let written = match proposed {
        domain::ProposedChange::Party(party) => {
            repo::update_party_fields(
                &mut **transaction,
                tenant_id,
                change.entity_id,
                party.status_id.as_deref(),
                party.external_id.as_deref(),
                party.description.as_deref(),
                party.additional_attributes.as_ref(),
                actor,
            )
            .await?
        }
        domain::ProposedChange::Facility(facility) => {
            repo::update_facility_fields(
                &mut **transaction,
                tenant_id,
                change.entity_id,
                &repo::FacilityFields {
                    name: facility.name.as_deref(),
                    facility_type: facility.facility_type.as_deref(),
                    parent_facility_id: None,
                    owner_party_id: None,
                    address_json: None,
                    attributes_json: facility.additional_attributes.as_ref(),
                },
                actor,
            )
            .await?
        }
    };

    if written == 0 {
        return Err(AppError::Internal {
            source: anyhow::anyhow!(
                "record {} took no change from the approval of document {}",
                change.entity_id,
                change.document_id
            ),
        });
    }

    Ok(())
}

/// Refuses a direct edit of a record that has a change in flight ([#255] AC1).
///
/// **The record's own status is the answer**, which is why this takes a status
/// rather than looking anything up: `PUT /parties/{id}` already reads the row it
/// is about, and a record at `PENDING_APPROVAL` is one whose next state belongs
/// to an approval rather than to an editor.
///
/// That is also what makes the rule uniform: a record parked by any route
/// refuses direct edits, including one parked by a release that grows a second
/// way to park it.
pub fn refuse_if_awaiting_approval(status: RecordStatus) -> Result<(), AppError> {
    if status == RecordStatus::PendingApproval {
        return Err(domain::awaiting_approval());
    }

    Ok(())
}

/// Turns the unique-index violation into the refusal it means.
///
/// The index is `uq_mdm_change_requests_open_per_record`, and it fires when two
/// submissions race for one record — the case the service's own read cannot
/// refuse, because both reads happen before either write.
fn in_flight_or_else(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database) = &error {
        if database.constraint() == Some("uq_mdm_change_requests_open_per_record") {
            return domain::already_in_flight();
        }
    }

    AppError::from(error)
}
