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
use super::ROLE_READ;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// The permission this surface needs.
///
/// A new string rather than the audit module's own `audit:read`. That module
/// has no endpoints yet, and minting its permission here would seed a
/// catalogue row that the audit module then has to honour — a control defined
/// by the first caller that needed it rather than by the module that owns it.
/// When FR-AUD-004 lands, `audit:read` is its to define, and whether this one
/// folds into it is a decision that surface can make with its own requirements
/// in front of it.
pub const AUDIT_READ: &str = "master-data:audit:read";

/// One page of a record's history, oldest first.
///
/// **Two permissions, conditionally.** `master-data:audit:read` is what opens
/// the surface; `master-data:party-role:read` is what decides whether the role
/// records are in it. #81 keeps `roles` and `profiles` off the aggregate for a
/// caller without the second, and a role assignment's audit record names the
/// role type — so returning them here would put *this party is a supplier* one
/// URL away from a permission that exists to refuse exactly that.
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
