//! Reading back what happened to a master-data record (FR-MDM-009).
//!
//! **The write path already existed**, from #80's first endpoint; FR-MDM-009
//! says "record master data changes in the audit log" and that half shipped in
//! Sprint 5. What was missing is the ability to *ask*, which is what makes the
//! requirement worth having: a chain nobody can read answers no questions.
//!
//! # Shape: a sub-resource per entity, not a module-wide feed
//!
//! `GET /parties/{id}/audit` answers "what happened to this supplier".
//! A `/master-data/audit` with filters would answer "what changed last week",
//! which is a different question and one the audit module's own surface is for
//! (FR-AUD-004, Phase 6). Building it here would mean building it twice and
//! then deciding which one is authoritative.
//!
//! The per-entity shape also keeps the permission story simple: the object is
//! named in the path, so the check is "may this caller read this record's
//! history" rather than "which of these rows may they see".

use uuid::Uuid;

use super::domain::{AuditRecord, TransitionTarget};
use super::repository as repo;
use super::{FACILITY_READ, PARTY_READ, ROLE_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// The permission that opens this surface — with the record's own, never alone.
///
/// A new string rather than the audit module's own `audit:read`. That module
/// has no endpoints yet, and minting its permission here would seed a
/// catalogue row that the audit module then has to honour — a control defined
/// by the first caller that needed it rather than by the module that owns it.
/// When FR-AUD-004 lands, `audit:read` is its to define, and whether this one
/// folds into it is a decision that surface can make with its own requirements
/// in front of it.
///
/// What it grants is the *question*, not the content: see
/// [`list_audit_records`] for why the record's own read permission is required
/// alongside it (#136).
pub const AUDIT_READ: &str = "master-data:audit:read";

/// One page of a record's history, oldest first.
///
/// **Two permissions open it, and a third decides what is in it.**
///
/// `master-data:audit:read` is the governance half — may this caller ask who
/// changed what. The record's own read permission is the content half, because
/// a record's history is made of the record's own field values: `Party.Created`
/// carries the party code, its type and its status, and `Facility.Updated`
/// carries the name, the type and both references. A history needing only
/// `audit:read` would be a way around `master-data:party:read` by exactly the
/// argument that produced the role gate below, and #97 stated that argument in
/// so many words — "a row is made of both surfaces, and a view needing only one
/// would be a way around the other".
///
/// **That was not the first answer.** Until #136 `audit:read` alone opened the
/// surface, deliberately and with a test asserting it, on the reading that a
/// governance permission is separable from the content it governs. What
/// settled it the other way is not that the first reading was unarguable but
/// that this one function applied both rules at once — the role half gated, the
/// party half not — and an asymmetry inside one function is a rule nobody can
/// state. Decision **D-12** records the trade: every caller of a history now
/// needs one permission more than it did.
///
/// **`master-data:party-role:read` decides whether the role records are in
/// it.** #81 keeps `roles` and `profiles` off the aggregate for a caller
/// without it, and a role assignment's audit record names the role type — so
/// returning them here would put *this party is a supplier* one URL away from a
/// permission that exists to refuse exactly that.
///
/// The filter is applied in SQL rather than after the fact, so the withheld
/// rows do not consume the page and `meta.total` counts what the caller can
/// actually see. Filtering a page after it is read would give a caller who
/// cannot see role changes a list with holes in it and a total that disagrees.
pub async fn list_audit_records(
    state: &AppState,
    caller: &Authenticated,
    target: TransitionTarget,
    id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<AuditRecord>, PageMeta), AppError> {
    caller.require(AUDIT_READ)?;
    caller.require(record_read_permission(target))?;

    let tenant_id = caller.tenant_id();

    // The record has to exist and be this tenant's before its history does.
    // Without this, a caller could tell a party that does not exist from one in
    // another tenant by whether the answer was empty or a 404.
    if repo::find_record_status(&state.pool, tenant_id, target, id)
        .await?
        .is_none()
    {
        return Err(AppError::not_found(target.missing()));
    }

    // Role records are only in scope for a caller who may read roles. A
    // facility has none, so the flag is irrelevant there and is still passed
    // honestly rather than hard-coded.
    let include_roles = caller.claims.has_permission(ROLE_READ);

    let total = repo::count_audit_records(&state.pool, tenant_id, id, include_roles).await?;
    let records = repo::list_audit_records(
        &state.pool,
        tenant_id,
        id,
        include_roles,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((records, pagination.meta(total.max(0) as u64)))
}

/// The permission that opens the record whose history is being asked for.
///
/// Here rather than on [`TransitionTarget`] because the permission catalogue is
/// a fact about this layer; `domain` neither knows the strings nor should.
fn record_read_permission(target: TransitionTarget) -> &'static str {
    match target {
        TransitionTarget::Party => PARTY_READ,
        TransitionTarget::Facility => FACILITY_READ,
    }
}
