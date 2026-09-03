//! Facility use cases (FR-MDM-004).
//!
//! The one entity in this module that is not a party. What it adds over the
//! party's create/read/update/delete is the hierarchy: `parentFacilityId` is a
//! self-reference, so a facility can be put under another, and nothing in the
//! database stops it being put under itself at one remove. [`refuse_cycle`] is
//! that stop.

use serde_json::{json, Value};
use uuid::Uuid;

use super::domain::{
    validate_create_facility, validate_update_facility, CreateFacilityRequest, Facility,
    FacilitySummary, FacilityType, GovernedEntity, MasterDataOption, PostalAddress,
    UpdateFacilityRequest, MAX_FACILITY_DEPTH,
};
use super::governance;
use super::repository::{self as repo, FacilityFields, NewFacility};
use super::FACILITY_READ;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a facility (naming convention §7).
const OBJECT_TYPE: &str = "FACILITY";

pub async fn list_facilities(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<FacilitySummary>, PageMeta), AppError> {
    caller.require(FACILITY_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_facilities(&state.pool, tenant_id).await?;
    let facilities = repo::list_facilities(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((facilities, pagination.meta(total.max(0) as u64)))
}

/// One page of the facilities a form may offer for selection (FR-RAD-007, #161).
///
/// **The same permission as reading the facility list, and deliberately not a
/// new one.** A lookup is a narrower view of data `master-data:facility:read`
/// already opens, so it can grant nothing the caller could not get from
/// `GET /master-data/facilities` — which is the whole answer to the question
/// [#161] asks, and it holds by construction rather than by two checks agreeing.
/// Minting a `rad:lookup:read` beside it would create the gap it was meant to
/// close: a caller could then be given the lookup without the list.
///
/// The refusal comes before anything is read, as everywhere else in this module.
///
/// [#161]: https://github.com/sujanto-gaws/kelir/issues/161
pub async fn list_facility_options(
    state: &AppState,
    caller: &Authenticated,
    search: Option<&str>,
    pagination: &Pagination,
) -> Result<(Vec<MasterDataOption>, PageMeta), AppError> {
    caller.require(FACILITY_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_facility_options(&state.pool, tenant_id, search).await?;
    let options = repo::list_facility_options(
        &state.pool,
        tenant_id,
        search,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((options, pagination.meta(total.max(0) as u64)))
}

pub async fn get_facility(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Facility, AppError> {
    caller.require(FACILITY_READ)?;

    repo::find_facility(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Facility"))
}

pub async fn create_facility(
    state: &AppState,
    caller: &Authenticated,
    request: CreateFacilityRequest,
) -> Result<Facility, AppError> {
    caller.require("master-data:facility:create")?;

    validate_create_facility(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    // Both references are resolved before anything is written, so a parent or
    // an owner that does not exist is a 422 naming the path rather than a
    // foreign-key violation surfacing as a 500 — and before the transaction
    // opens, so this call holds one connection at a time (coding standard
    // §2.5, #118).
    let parent = resolve_parent(state, tenant_id, request.parent_facility_id.as_deref()).await?;
    let owner = resolve_owner(state, tenant_id, request.owner_party_id.as_deref()).await?;

    let id = Uuid::now_v7();
    let facility_code = request.facility_id.trim();
    let name = request.name.as_deref().unwrap_or_default().trim();
    let address = address_json(request.address.as_ref())?;
    let attributes = request
        .additional_attributes
        .clone()
        .unwrap_or_else(|| json!({}));

    // The parent is re-read under the lock before the child is written (#137).
    // Resolving it above answered "is there such a facility" a moment ago, and
    // a delete landing in between would leave this row under a facility that no
    // longer exists — the state `delete_facility`'s refusal is there to prevent,
    // arrived at without anybody deciding it.
    let mut transaction = state.pool.begin().await?;

    if let Some(parent_id) = parent {
        repo::lock_facility_hierarchy(&mut transaction, tenant_id).await?;

        if !repo::facility_is_live(&mut *transaction, tenant_id, parent_id).await? {
            return Err(parent_no_longer_there());
        }
    }

    repo::insert_facility(
        &mut *transaction,
        &NewFacility {
            id,
            tenant_id,
            facility_code,
            name,
            facility_type: request.facility_type_id.map(FacilityType::as_db),
            parent_facility_id: parent,
            owner_party_id: owner,
            address_json: &address,
            attributes_json: &attributes,
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_facility_to_conflict)?;

    transaction.commit().await?;

    // Read back before the record is written, so the record says what the row
    // holds rather than what the request asked for (#135). The two differ even
    // on a create: a name is trimmed on the way in, and a reference is stored
    // against the code the resolver found.
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Facility.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "facilityId": created.facility_id,
                "name": created.name,
                "facilityTypeId": created.facility_type_id,
                "parentFacilityId": created.parent_facility_id,
                "ownerPartyId": created.owner_party_id,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_facility(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateFacilityRequest,
) -> Result<Facility, AppError> {
    caller.require("master-data:facility:update")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_facility(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Facility"))?;

    validate_update_facility(&request, &before.facility_id)?;

    // The same refusal `update_party` makes, for the same reason (#255 AC1).
    governance::refuse_if_awaiting_approval(before.record_status_id)?;

    // Both references resolve on the pool, before the transaction opens, so
    // this call holds one connection at a time (coding standard §2.5, #118).
    let parent = match &request.parent_facility_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(code)) => Some(resolve_parent(state, tenant_id, Some(code)).await?),
    };

    let owner = match &request.owner_party_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(code)) => Some(resolve_owner(state, tenant_id, Some(code)).await?),
    };

    let address = match request.address.as_ref() {
        Some(address) => Some(address_json(Some(address))?),
        None => None,
    };

    // The cycle check and the write it guards, in one transaction (#133). Read
    // on the pool and written after, they were check-then-act: two callers each
    // walked a path the other was about to change, both were told the move was
    // legal, and the pair closed a loop neither could see on its own.
    let mut transaction = state.pool.begin().await?;

    if let Some(Some(parent_id)) = parent {
        repo::lock_facility_hierarchy(&mut transaction, tenant_id).await?;

        // Re-read under the lock, for the same reason a create does (#137): the
        // resolve above ran on the pool, and a delete since then would put this
        // facility under one that is gone.
        if !repo::facility_is_live(&mut *transaction, tenant_id, parent_id).await? {
            return Err(parent_no_longer_there());
        }

        refuse_cycle(&mut *transaction, tenant_id, id, parent_id).await?;
    }

    let updated = repo::update_facility_fields(
        &mut *transaction,
        tenant_id,
        id,
        &FacilityFields {
            name: request.name.as_deref().map(str::trim),
            facility_type: request.facility_type_id.map(FacilityType::as_db),
            parent_facility_id: parent,
            owner_party_id: owner,
            address_json: address.as_ref(),
            attributes_json: request.additional_attributes.as_ref(),
        },
        actor,
    )
    .await?;

    if updated == 0 {
        return Err(AppError::not_found("Facility"));
    }

    transaction.commit().await?;

    // The row as it now is, against the row as it was. `load` was already the
    // last thing this function did; reading it here rather than after the audit
    // write is what gives the record an *after* to compare against (#135).
    let after = load(state, tenant_id, id).await?;
    let (old_value, new_value) = facility_changes(&before, &after).halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Facility.Updated",
            action: "UPDATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

/// What one update moved, in the vocabulary the API publishes.
///
/// Every field an [`UpdateFacilityRequest`] can change is listed — `address`
/// and `additionalAttributes` included, which the old record named on neither
/// side, so a request that changed only an address produced a record in which
/// the address did not appear.
///
/// `facilityId` and `recordStatusId` are absent because no update can move
/// them: the code is fixed at creation, and the lifecycle status moves only
/// through `POST /facilities/{id}/transition`, which writes its own record with
/// its own action.
fn facility_changes(before: &Facility, after: &Facility) -> ChangeSet {
    let mut changes = ChangeSet::new();

    changes.field("name", &before.name, &after.name);
    changes.field(
        "facilityTypeId",
        &before.facility_type_id,
        &after.facility_type_id,
    );
    changes.field(
        "parentFacilityId",
        &before.parent_facility_id,
        &after.parent_facility_id,
    );
    changes.field(
        "ownerPartyId",
        &before.owner_party_id,
        &after.owner_party_id,
    );
    changes.field("address", &before.address, &after.address);
    changes.field(
        "additionalAttributes",
        &before.additional_attributes,
        &after.additional_attributes,
    );

    changes
}

/// Soft-deletes a facility, refusing while anything still sits under it.
///
/// Not a cascade. Deleting a building would otherwise take its floors and their
/// rooms with it in one call, and the caller who asked to delete one row would
/// have deleted a hundred. Refusing names the count, so the caller can decide
/// whether to re-parent the children or delete them too — which is a decision,
/// not a default.
///
/// **The count and the delete are one transaction, under the same lock a
/// re-parenting takes** (#137). Counted on the pool and deleted afterwards, the
/// refusal only held against children that already existed: a create naming
/// this facility as its parent, resolving that parent a moment before the
/// delete landed, produced a live facility under a deleted one in 19 of 20
/// rounds. Nobody chose that — the delete reported success, the create reported
/// success, and the decision this refusal exists to force was never put to
/// anyone.
///
/// It is also the failure that hides. `find_facility` and `list_facilities`
/// join the parent on `deleted_at IS NULL`, so such a row reads back as a root;
/// the dangling reference stays in the column, visible to an export or a repair
/// script and to nothing a user can see.
pub async fn delete_facility(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require("master-data:facility:delete")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let mut transaction = state.pool.begin().await?;

    repo::lock_facility_hierarchy(&mut transaction, tenant_id).await?;

    // **A record with a change awaiting approval is not deleted** (FR-MDM-010,
    // **D-60**) — the party path's sibling, and the same defect: deleting a
    // parked record left its approval undecidable for ever, 500 on both
    // APPROVE and REJECT. See `party::delete_party` for the reasoning and for
    // why the read is the locking one.
    //
    // **Before the children check**, because the two refusals are not
    // equivalent. "Remove the facilities under this one" is work the caller can
    // go and do; "a change is awaiting approval" is a process in another module
    // that has to be decided first, and a caller who cleared the children and
    // came back would meet it anyway. The more specific state answers first.
    //
    // The hierarchy lock is taken above and this row lock below it, in that
    // order, on every facility path that takes both. `governance::raise` takes
    // only the row lock and never the advisory one, so the two cannot cycle.
    let parked =
        repo::lock_record_status(&mut transaction, tenant_id, GovernedEntity::Facility, id)
            .await?
            .ok_or_else(|| AppError::not_found("Facility"))?;

    governance::refuse_if_awaiting_approval(parked)?;

    let children = repo::children_of(&mut *transaction, tenant_id, id).await?;

    if children > 0 {
        return Err(AppError::conflict(format!(
            "{children} facilities are still under this one"
        )));
    }

    let removed = repo::soft_delete_facility(&mut *transaction, tenant_id, id, actor).await?;

    if removed == 0 {
        return Err(AppError::not_found("Facility"));
    }

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Facility.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Refuses a re-parenting that would close a loop.
///
/// The hierarchy is a tree and `parent_facility_id` is a self-reference, so the
/// database can express *points at a facility* and cannot express *and not at
/// one of its own descendants*. Walking up from the proposed parent is the
/// check: if `id` is on that path, then making `parent` the parent of `id`
/// would make `id` its own ancestor.
///
/// It needs a test more than most rules here do, because the failure is not a
/// wrong answer — a cycle in storage makes any traversal loop until something
/// times out.
///
/// **Runs inside the caller's transaction, under
/// [`repo::lock_facility_hierarchy`]** (#133). On the pool it was a read whose
/// answer expired before the write that depended on it.
///
/// **A path it could not walk to the end is refused, not assumed safe** (#134).
/// Past `MAX_FACILITY_DEPTH` the walk returns a prefix, and `id` being absent
/// from a prefix is not evidence that `id` is not an ancestor — that is how the
/// bound came to allow the corruption it was there to survive. Refusing a move
/// this check cannot verify errs towards a tree that stays a tree; the caller
/// is told the depth is the reason, because "no" without one is indistinguishable
/// from a defect.
async fn refuse_cycle(
    executor: impl sqlx::PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    parent: Uuid,
) -> Result<(), AppError> {
    let ancestry =
        repo::facility_ancestors(executor, tenant_id, parent, MAX_FACILITY_DEPTH).await?;

    if ancestry.ids.contains(&id) {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentFacilityId",
            "consistency",
            "CYCLE",
            "That facility is under this one; moving it there would close a loop",
        )]));
    }

    if ancestry.truncated {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentFacilityId",
            "consistency",
            "TOO_DEEP",
            format!(
                "That facility sits more than {MAX_FACILITY_DEPTH} levels deep, so this move \
                 cannot be checked for a loop. Move it nearer the root first"
            ),
        )]));
    }

    Ok(())
}

/// The parent named by the request went away between resolving it and writing.
///
/// The same 422 `resolve_parent` gives, deliberately: from the caller's side
/// nothing distinguishes a parent that never existed from one deleted while
/// their request was in flight, and both are answered by naming the field and
/// letting them send it again. A 409 would suggest a conflict they could
/// resolve, and there is nothing for them to resolve — the facility is gone.
fn parent_no_longer_there() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "parentFacilityId",
        "exists",
        "NOT_FOUND",
        "No facility with that facilityId",
    )])
}

/// The surrogate id behind a `parentFacilityId`, or a 422 naming the field.
async fn resolve_parent(
    state: &AppState,
    tenant_id: Uuid,
    code: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_facility_id_by_code(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => Err(AppError::validation(vec![ValidationDetail::new(
            "parentFacilityId",
            "exists",
            "NOT_FOUND",
            "No facility with that facilityId",
        )])),
    }
}

/// The surrogate id behind an `ownerPartyId`, or a 422 naming the field.
///
/// The same shape `managerPartyId` uses on an employee profile: a reference
/// that does not resolve is the caller's mistake and is named, not a
/// foreign-key violation surfacing as a 500.
async fn resolve_owner(
    state: &AppState,
    tenant_id: Uuid,
    code: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_party_id_by_code(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => Err(AppError::validation(vec![ValidationDetail::new(
            "ownerPartyId",
            "exists",
            "NOT_FOUND",
            "No party with that partyId",
        )])),
    }
}

/// The address as the column stores it.
///
/// `{}` rather than `null` when no address is given, matching the column's own
/// default so that a facility created without one and a facility created before
/// this field existed read back the same way.
fn address_json(address: Option<&PostalAddress>) -> Result<Value, AppError> {
    match address {
        None => Ok(json!({})),
        Some(address) => serde_json::to_value(address).map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("serializing a postal address: {error}"),
        }),
    }
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Facility, AppError> {
    repo::find_facility(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Facility"))
}

/// The unique index on `(tenant_id, facility_code)` among live rows, as a 409.
///
/// A duplicate code is the caller telling the truth about a facility that
/// already exists, not a malformed request — the same reason
/// `duplicate_party_to_conflict` answers 409 rather than 422.
fn duplicate_facility_to_conflict(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.constraint() == Some("uq_mdm_facilities_tenant_id_facility_code") {
            return AppError::conflict("That facilityId is already in use");
        }
    }

    AppError::from(error)
}
