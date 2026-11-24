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
//! Each returns the [`GovernedAudit`] its record is owed ([#1002]), for the
//! caller to write after the commit.
//!
//! [#255]: https://github.com/sujanto-gaws/kelir/issues/255
//! [#322]: https://github.com/sujanto-gaws/kelir/issues/322
//! [#1002]: https://github.com/sujanto-gaws/kelir/issues/1002
//! [ADR-0033]: ../../../../docs/architectures/adr/0033.%20A%20Governed%20Record%20Parks%20at%20Pending%20Approval.md

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::domain::{self, GovernedEntity, RecordStatus};
use super::repository as repo;
use crate::error::AppError;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry};
use crate::state::AppState;

/// The audit row a governed change owes its record ([#1002]), built in the
/// transaction that made the change and written after it commits.
///
/// **Written by the caller, not here**: the audit chain has one writer,
/// `audit::record` on the pool ([#995]), and a row inserted in the engine's
/// transaction would be a second. A rollback then leaves no row either.
///
/// [#995]: https://github.com/sujanto-gaws/kelir/issues/995
/// [#1002]: https://github.com/sujanto-gaws/kelir/issues/1002
#[derive(Debug)]
#[must_use = "a governed change is audited only when this is recorded after the commit"]
pub struct GovernedAudit {
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
    actor: Option<Uuid>,
    step: Step,
}

/// What happened to the change.
#[derive(Debug)]
enum Step {
    /// Raised: the record parked, from the status it held.
    Requested { from: RecordStatus },
    /// Approved and applied: the fields that moved, as stored before and after.
    Approved { old_value: Value, new_value: Value },
    /// Refused: nothing was written to the record, so there are no values.
    Rejected { reason: String },
    /// Withdrawn: the document ended `CANCELLED`. No values, as a refusal.
    Withdrawn { reason: String },
}

impl Step {
    /// A change not applied: withdrawn if its document ended `CANCELLED`,
    /// rejected otherwise (the product owner, 2026-11-24).
    fn refused(ended_as: &str, reason: String) -> Self {
        if ended_as == "CANCELLED" {
            Self::Withdrawn { reason }
        } else {
            Self::Rejected { reason }
        }
    }
}

impl GovernedAudit {
    /// Writes the row, logging rather than propagating a failure: the change
    /// has committed, and the caller's answer stands whether or not it lands.
    pub async fn record(self, pool: &PgPool, ip_address: Option<&str>) {
        audit::record_or_warn(pool, self.entry(ip_address)).await;
    }

    fn entry<'a>(&'a self, ip_address: Option<&'a str>) -> AuditEntry<'a> {
        let (old_value, new_value, reason) = match &self.step {
            Step::Requested { from } => (
                Some(json!({ "recordStatusId": from.as_db() })),
                Some(json!({ "recordStatusId": RecordStatus::PendingApproval.as_db() })),
                None,
            ),
            Step::Approved {
                old_value,
                new_value,
            } => (Some(old_value.clone()), Some(new_value.clone()), None),
            Step::Rejected { reason } | Step::Withdrawn { reason } => {
                (None, None, Some(reason.as_str()))
            }
        };

        AuditEntry {
            tenant_id: self.tenant_id,
            event_type: self.event_type(),
            action: match self.step {
                Step::Requested { .. } => "CHANGE_REQUEST",
                Step::Approved { .. } => "APPROVE",
                Step::Rejected { .. } => "REJECT",
                Step::Withdrawn { .. } => "CANCEL",
            },
            object_type: match self.entity {
                GovernedEntity::Party => ObjectType::Party,
                GovernedEntity::Facility => ObjectType::Facility,
            },
            object_id: self.entity_id,
            actor_user_id: self.actor,
            ip_address,
            reason,
            old_value,
            new_value,
        }
    }

    /// The product owner's names, 2026-11-16 (#1002).
    fn event_type(&self) -> &'static str {
        match (self.entity, &self.step) {
            (GovernedEntity::Party, Step::Requested { .. }) => "Party.ChangeRequested",
            (GovernedEntity::Party, Step::Approved { .. }) => "Party.ChangeApproved",
            (GovernedEntity::Party, Step::Rejected { .. }) => "Party.ChangeRejected",
            (GovernedEntity::Party, Step::Withdrawn { .. }) => "Party.ChangeWithdrawn",
            (GovernedEntity::Facility, Step::Requested { .. }) => "Facility.ChangeRequested",
            (GovernedEntity::Facility, Step::Approved { .. }) => "Facility.ChangeApproved",
            (GovernedEntity::Facility, Step::Rejected { .. }) => "Facility.ChangeRejected",
            (GovernedEntity::Facility, Step::Withdrawn { .. }) => "Facility.ChangeWithdrawn",
        }
    }
}

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
///
/// Returns the `CHANGE_REQUEST` row the caller writes after the commit.
pub async fn raise(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
    document_id: Uuid,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<GovernedAudit, AppError> {
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

    Ok(GovernedAudit {
        tenant_id,
        entity,
        entity_id,
        actor,
        step: Step::Requested { from: current },
    })
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
/// # When the record is not where this change parked it ([#322])
///
/// An out-of-band write, a later release or a plugin can move a record while
/// its change is in flight — **D-60** closed the one route the product had into
/// that state and did not close the class. This used to answer
/// `AppError::Internal`, so the approver saw `INTERNAL_ERROR`: loud to a log,
/// opaque to the person holding the task.
///
/// **The two decisions are answered differently, and the asymmetry is the
/// point.** [`domain::moved_since_parked`] refuses an *approval* with a 409
/// naming the state, because approving a change that cannot be applied would
/// leave the document `APPROVED` and the record untouched — an approval that
/// says something happened when nothing did. A *rejection* completes: there is
/// nothing to write and nothing to put back, so the change request resolves
/// `REFUSED` and the process ends.
///
/// **That asymmetry is what keeps the task closable**, which is the half a
/// clean refusal alone would fail. If both decisions refused, every decision on
/// that task would refuse and the task would be one no decision can close —
/// the 500's own failure with a better status code on it. Rejecting is the way
/// out, and the refusal's message says so.
///
/// # The record of the attempt (AC4)
///
/// **The `mdm_change_requests` row is the record**, kept rather than deleted and
/// carrying its outcome: `GET /master-data/parties/{id}/change-requests` lists
/// what has been proposed for a record and how each attempt ended, which is the
/// record's own history rather than the document's.
///
/// **The audit row is returned, not written** ([#1002]); see
/// [`GovernedAudit`]. An approval's carries the fields that moved, read under
/// this transaction's lock before and after; a refusal's carries the document
/// and `ended_as` in `reason`, and no values, and is a withdrawal when
/// `ended_as` is `CANCELLED`. A document whose change is
/// already settled, or that carried none, returns `None`, so a second settle
/// writes no second row.
///
/// [#1002]: https://github.com/sujanto-gaws/kelir/issues/1002
pub async fn settle(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    approved: bool,
    ended_as: &str,
    form_data: &Value,
    actor: Option<Uuid>,
) -> Result<Option<GovernedAudit>, AppError> {
    let Some(change) =
        repo::lock_open_change_for_document(transaction, tenant_id, document_id).await?
    else {
        return Ok(None);
    };

    // **Where the record actually is, read under `FOR UPDATE` before anything
    // is written** ([#322]). `raise` reads it the same way and for the same
    // reason: the status is what every decision below turns on, and asking for
    // it is how the refusal can name it.
    let current =
        repo::lock_record_status(transaction, tenant_id, change.entity, change.entity_id).await?;

    if !current.is_some_and(RecordStatus::is_parked) {
        return moved_on(
            transaction,
            tenant_id,
            &change,
            approved,
            ended_as,
            current,
            actor,
        )
        .await
        .map(Some);
    }

    let target = if approved {
        RecordStatus::Active
    } else {
        change.previous_record_status
    };

    // The record as stored before the approval writes it, read under the lock
    // taken above.
    let before = if approved {
        let before = stored(transaction, tenant_id, &change).await?;
        apply(transaction, tenant_id, &change, form_data, actor).await?;
        Some(before)
    } else {
        None
    };

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
        // Unreachable: the row was read at `PENDING_APPROVAL` under `FOR UPDATE`
        // above and this transaction still holds it. `raise` states the same
        // thing at the same place, and both are `Internal` because a lock that
        // did not hold is a broken premise rather than a refusal anybody can act
        // on.
        return Err(AppError::Internal {
            source: anyhow::anyhow!(
                "record {} was locked at PENDING_APPROVAL and then not moved",
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

    let step = match before {
        Some(before) => {
            let (old_value, new_value) =
                approved_values(transaction, tenant_id, &change, before).await?;
            Step::Approved {
                old_value,
                new_value,
            }
        }
        None => Step::refused(
            ended_as,
            format!(
                "document {document_id} ended {ended_as}; the change it carried was not applied"
            ),
        ),
    };

    Ok(Some(GovernedAudit {
        tenant_id,
        entity: change.entity,
        entity_id: change.entity_id,
        actor,
        step,
    }))
}

/// A governed record as stored, for an approval's two halves.
enum Stored {
    Party(repo::PartyRow),
    Facility(Box<domain::Facility>),
}

/// Reads the record this transaction holds locked.
async fn stored(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    change: &domain::ChangeRequest,
) -> Result<Stored, AppError> {
    let read = match change.entity {
        GovernedEntity::Party => repo::find_party(&mut **transaction, tenant_id, change.entity_id)
            .await?
            .map(Stored::Party),
        GovernedEntity::Facility => {
            repo::find_facility(&mut **transaction, tenant_id, change.entity_id)
                .await?
                .map(|facility| Stored::Facility(Box::new(facility)))
        }
    };

    // Unreachable: the row is locked by this transaction, read at
    // `PENDING_APPROVAL`, and nothing here deletes it.
    read.ok_or_else(|| AppError::Internal {
        source: anyhow::anyhow!(
            "record {} was locked for its approval and then not read",
            change.entity_id
        ),
    })
}

/// The fields an approval moved, as stored before it and after, in the same
/// vocabulary as the record's own `*.Updated` rows, with `recordStatusId`.
async fn approved_values(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    change: &domain::ChangeRequest,
    before: Stored,
) -> Result<(Value, Value), AppError> {
    let after = stored(transaction, tenant_id, change).await?;

    let (mut changes, statuses) = match (before, after) {
        (Stored::Party(before), Stored::Party(after)) => (
            super::party::party_changes(&before, &after),
            (before.record_status, after.record_status),
        ),
        (Stored::Facility(before), Stored::Facility(after)) => (
            super::facility::facility_changes(&before, &after),
            (before.record_status_id, after.record_status_id),
        ),
        // Unreachable: both reads are of `change.entity`.
        _ => {
            return Err(AppError::Internal {
                source: anyhow::anyhow!("record {} read as two kinds of record", change.entity_id),
            })
        }
    };
    changes.field("recordStatusId", &statuses.0, &statuses.1);

    Ok(changes.halves())
}

/// Settles a change whose record is no longer parked where it left it
/// ([#322]).
///
/// **Refuses the approval, completes the rejection**, for the reason
/// [`settle`]'s header gives: an approval that cannot be applied must not say
/// it was, and a task no decision can close is what the 500 this replaces
/// actually produced.
///
/// **Nothing is written on either path** — no field is applied, and no status is
/// moved. The record has been moved by somebody else, and putting it back would
/// be this function inventing an intent from a `previous_record_status` that is
/// now describing a record two states ago.
///
/// **`REFUSED` rather than a new outcome.** `mdm_change_requests.outcome`'s own
/// column comment reads *`REFUSED` when the process ended any other way*, and
/// that is what happened: the change was not applied. A third value would need a
/// migration to widen the `CHECK` and would tell the record's history something
/// its reader — `GET /master-data/parties/{id}/change-requests` — has no column
/// to show. What the document's own status says, and this row does not, is
/// *why*, and the rejection's audit row names it.
async fn moved_on(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    change: &super::domain::ChangeRequest,
    approved: bool,
    ended_as: &str,
    current: Option<RecordStatus>,
    actor: Option<Uuid>,
) -> Result<GovernedAudit, AppError> {
    if approved {
        return Err(domain::moved_since_parked(current));
    }

    repo::resolve_change_request(transaction, tenant_id, change.id, "REFUSED", actor).await?;

    Ok(GovernedAudit {
        tenant_id,
        entity: change.entity,
        entity_id: change.entity_id,
        actor,
        step: Step::refused(
            ended_as,
            format!(
                "document {} ended {ended_as} while the record was {}; the change it carried \
                 was not applied",
                change.document_id,
                current.map_or("gone", RecordStatus::as_db)
            ),
        ),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The product owner's names (#1002, and `CANCELLED` 2026-11-24), for both
    /// entities. Each step's
    /// values are asserted through the API in `master_data_governance.rs`.
    #[test]
    fn each_step_is_named_as_ruled_for_both_entities() {
        for (entity, object_type) in [
            (GovernedEntity::Party, ObjectType::Party),
            (GovernedEntity::Facility, ObjectType::Facility),
        ] {
            let steps = [
                Step::Requested {
                    from: RecordStatus::Draft,
                },
                Step::Approved {
                    old_value: json!({}),
                    new_value: json!({}),
                },
                Step::refused("REJECTED", String::new()),
                Step::refused("CANCELLED", String::new()),
            ];
            let names: Vec<String> = steps
                .into_iter()
                .map(|step| {
                    let audit = GovernedAudit {
                        tenant_id: Uuid::nil(),
                        entity,
                        entity_id: Uuid::nil(),
                        actor: None,
                        step,
                    };
                    let entry = audit.entry(None);
                    assert_eq!(entry.object_type, object_type);
                    format!("{} {}", entry.event_type, entry.action)
                })
                .collect();
            let prefix = entity.missing();

            assert_eq!(
                names,
                [
                    format!("{prefix}.ChangeRequested CHANGE_REQUEST"),
                    format!("{prefix}.ChangeApproved APPROVE"),
                    format!("{prefix}.ChangeRejected REJECT"),
                    format!("{prefix}.ChangeWithdrawn CANCEL"),
                ]
            );
        }
    }
}
