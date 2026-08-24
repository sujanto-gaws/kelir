//! The role-filtered list endpoints — `/suppliers`, `/customers`, `/employees`
//! (FR-MDM-002, FR-MDM-008).
//!
//! Split out of `service.rs` by #112 with no behaviour change. A projection
//! over the party surface rather than an entity of its own, which is why it is
//! one function: the repository holds the query, and the two permissions it
//! requires are the whole of the use case.

use super::domain::{RoleView, RoleViewQuery, RoleViewRow};
use super::repository as repo;
use super::{PARTY_READ, ROLE_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::PageMeta;
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
