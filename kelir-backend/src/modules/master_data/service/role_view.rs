//! The role-filtered list endpoints — `/suppliers`, `/customers`, `/employees`
//! (FR-MDM-002, FR-MDM-008) — and the same parties as something a form may
//! offer (FR-RAD-007).
//!
//! Split out of `service.rs` by #112 with no behaviour change. A projection over
//! the party surface rather than an entity of its own, which is why each use
//! case here is one function: the repository holds the query, and the two
//! permissions it requires are the whole of it.
//!
//! **Two functions over one query, and the second exists so that a lookup does
//! not have to hold a permission of its own** (#161). `list_role_view_options`
//! is the same rows narrowed to what a chooser shows, behind the same
//! `caller.require` pair — so `rad`'s lookup surface asks this module for a page
//! and never decides who may have one. A lookup that checked its own permission
//! would be a second answer to a question this file already answers, and two
//! answers drift.

use super::domain::{
    MasterDataOption, PartyRoleStatus, PartyStatusCode, RoleView, RoleViewFilters, RoleViewQuery,
    RoleViewRow,
};
use super::repository as repo;
use super::{PARTY_READ, ROLE_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// One page of the parties holding a role — `/suppliers`, `/customers`,
/// `/employees` (FR-MDM-002, FR-MDM-008).
///
/// **Two permissions, and the reason is the row.** A role-view row is a party
/// summary with a supplier number on it, so it is made of both surfaces: the
/// party half is what `master-data:party:read` gates on `/parties`, and the
/// number is role data, which is what [`ROLE_READ`] gates on the aggregate. A
/// view that asked for only one of them would be a way around the other — a
/// caller holding `master-data:party:read` alone would read the supplier
/// numbers the aggregate withholds from them one URL away (#97 AC3), and a
/// caller holding only [`ROLE_READ`] would gain the ability to enumerate
/// parties, which today needs `master-data:party:read`.
///
/// No new permission string is minted for the views. Three endpoints over data
/// two existing permissions already govern is the per-endpoint permission shape
/// D-6 rejected for the catalogue.
///
/// The refusal comes before the query parameters are parsed: a caller who may
/// not see this list learns that, not which of their filters was misspelled.
pub async fn list_role_view(
    state: &AppState,
    caller: &Authenticated,
    view: RoleView,
    query: &RoleViewQuery,
) -> Result<(Vec<RoleViewRow>, PageMeta), AppError> {
    caller.require(PARTY_READ)?;
    caller.require(ROLE_READ)?;

    let filters = query.filters()?;
    let pagination = query.pagination();
    let tenant_id = caller.tenant_id();
    let role_type_code = view.role_type_code();

    let total = repo::count_role_view(&state.pool, tenant_id, role_type_code, &filters).await?;
    let rows = repo::list_role_view(
        &state.pool,
        tenant_id,
        role_type_code,
        &filters,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((rows, pagination.meta(total.max(0) as u64)))
}

/// One page of the parties a form may offer for a role (FR-RAD-007, #161).
///
/// **The same two permissions [`list_role_view`] requires, for the same reason,
/// and deliberately not a new one.** A lookup over suppliers hands back party
/// data and a supplier number, so it is made of the same two surfaces the role
/// view is: `master-data:party:read` opens the party half and [`ROLE_READ`] the
/// number. Requiring exactly what the list requires is what makes the lookup
/// unable to become a way around either — it grants nothing a caller could not
/// get from `GET /master-data/suppliers`, by construction rather than by two
/// checks that have to agree. A `rad:lookup:read` beside them would create the
/// gap it was meant to close.
///
/// **Two filters are the server's rather than the caller's.** A form offers a
/// party the business currently deals with, in a role it currently holds, so
/// `PARTY_ENABLED` and an `ACTIVE` role assignment are fixed here and are not
/// parameters: a disabled supplier is not a supplier a requisition may name, and
/// leaving that to the client would mean every renderer had to remember it. The
/// caller's half of the filtering is `search`, which is what a chooser needs.
///
/// `record_status` is not among them, and that is a decision — see
/// `repository::facility::list_facility_options`, which states it once for both
/// sources.
pub async fn list_role_view_options(
    state: &AppState,
    caller: &Authenticated,
    view: RoleView,
    search: Option<&str>,
    pagination: &Pagination,
) -> Result<(Vec<MasterDataOption>, PageMeta), AppError> {
    caller.require(PARTY_READ)?;
    caller.require(ROLE_READ)?;

    let filters = RoleViewFilters {
        search: search.map(str::to_owned),
        status: Some(PartyStatusCode::PartyEnabled),
        party_type: None,
        role_status: Some(PartyRoleStatus::Active),
    };

    let tenant_id = caller.tenant_id();
    let role_type_code = view.role_type_code();

    let total = repo::count_role_view(&state.pool, tenant_id, role_type_code, &filters).await?;
    let rows = repo::list_role_view(
        &state.pool,
        tenant_id,
        role_type_code,
        &filters,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((
        rows.into_iter().map(as_option).collect(),
        pagination.meta(total.max(0) as u64),
    ))
}

/// A role-view row as something a form may offer.
///
/// The identifier falls back from the role number to the party code, because a
/// party may hold a role without a profile and a chooser still has to tell two
/// records of the same name apart. `RoleViewRow::role_number` is `None` in
/// exactly that case, which the view's own doc comment records as legal.
fn as_option(row: RoleViewRow) -> MasterDataOption {
    MasterDataOption {
        id: row.id,
        name: row.name,
        code: row.role_number.or(Some(row.party_id)),
    }
}
