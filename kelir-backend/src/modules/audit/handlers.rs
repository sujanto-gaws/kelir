//! The audit search routes (FR-AUD-004; [#252]).
//!
//! **`/api/v1/audit`, module-wide and filtered** — which is the shape
//! `master_data::service::audit_record` said this surface would have when it
//! declined to build it: *`GET /parties/{id}/audit` answers "what happened to
//! this supplier"; a `/master-data/audit` with filters would answer "what
//! changed last week", which is a different question and one the audit module's
//! own surface is for*. Both exist, and neither is authoritative over the
//! other — they answer different questions and check different permissions.
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use super::domain::{AuditEvent, AuditSearch};
use super::service;
use crate::error::AppError;
use crate::extract::QueryParams;
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(search_audit))
        .route("/object-types", get(object_types))
}

#[utoipa::path(
    get, path = "/api/v1/audit", tag = "audit",
    params(AuditSearch),
    responses(
        (status = 200, description = "One page of the trail, newest first. A row whose object this caller may not read carries `valuesWithheld: true` and no values — the row is never hidden", body = [AuditEvent]),
        (status = 403, description = "Missing audit:read"),
        (status = 422, description = "A date range that ends before it starts")
    ),
    security(("bearer" = []))
)]
pub async fn search_audit(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<AuditSearch>,
) -> Result<Json<ListEnvelope<AuditEvent>>, AppError> {
    let pagination = query.pagination();
    let (events, meta) = service::search_audit(&state, &caller, &query, &pagination).await?;

    Ok(Json(ListEnvelope::new(events, meta)))
}

/// **Read from the rows rather than from a constant**, so the control offers
/// what this deployment's trail actually holds.
#[utoipa::path(
    get, path = "/api/v1/audit/object-types", tag = "audit",
    responses(
        (status = 200, description = "Every object type this tenant's trail holds", body = [String]),
        (status = 403, description = "Missing audit:read")
    ),
    security(("bearer" = []))
)]
pub async fn object_types(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<Vec<String>>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::object_types(&state, &caller).await?,
    )))
}
