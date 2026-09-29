//! Reading the integration log (FR-INT-006, #548).
//!
//! Both use cases are under [`LOG_READ`] alone — not
//! `integration:external-system:read`: a log reader need not manage systems,
//! and a system manager does not see every call's payload by default (#548
//! AC2). A filter naming a system or an endpoint is not checked against the
//! registry: an id that is not the caller's matches no row, which is the same
//! answer a real system with no calls gives.
//!
//! **Nothing here resolves or unmasks anything** (#548 AC3). The rows are
//! returned as the repository read them.

use uuid::Uuid;

use super::test_call::LOG_ENTITY_TYPE;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::integration::domain::log::validate_query;
use crate::modules::integration::domain::{
    IntegrationLog, IntegrationLogQuery, IntegrationLogSummary,
};
use crate::modules::integration::repository::log::{self as repo, IntegrationLogFilter};
use crate::modules::integration::LOG_READ;
use crate::response::PageMeta;
use crate::state::AppState;

pub async fn list_logs(
    state: &AppState,
    caller: &Authenticated,
    query: &IntegrationLogQuery,
) -> Result<(Vec<IntegrationLogSummary>, PageMeta), AppError> {
    caller.require(LOG_READ)?;
    validate_query(query)?;

    let tenant_id = caller.tenant_id();
    let pagination = query.pagination();
    let filter = IntegrationLogFilter {
        external_system_id: query.external_system_id,
        entity: query.endpoint_id.map(|id| (LOG_ENTITY_TYPE, id)),
        status: query.status.map(|status| status.as_db()),
        from: query.from,
        to: query.to,
    };

    let total = repo::count_logs(&state.pool, tenant_id, &filter).await?;
    let logs = repo::list_logs(
        &state.pool,
        tenant_id,
        &filter,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((logs, pagination.meta(total.max(0) as u64)))
}

pub async fn get_log(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<IntegrationLog, AppError> {
    caller.require(LOG_READ)?;

    repo::find_log(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Integration log"))
}
