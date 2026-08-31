//! The timeline route (FR-ACT-001; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::ActivityEvent;
use super::service;
use crate::error::AppError;
use crate::extract::{PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(list_activity))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/activity", tag = "activity",
    params(Pagination),
    responses(
        (status = 200, description = "The document's timeline, newest first", body = [ActivityEvent]),
        (status = 403, description = "Missing activity:read or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see")
    ),
    security(("bearer" = []))
)]
pub async fn list_activity(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<ActivityEvent>>, AppError> {
    let (events, meta) = service::list_activity(&state, &caller, document_id, &pagination).await?;

    Ok(Json(ListEnvelope::new(events, meta)))
}
