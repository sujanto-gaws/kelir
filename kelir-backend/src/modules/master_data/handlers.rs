use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{CreatePartyRequest, PartyAggregate, PartySummary, UpdatePartyRequest};
use super::service;
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

/// Every route here requires a token: taking [`Authenticated`] is what enforces
/// it (FR-API-008), and each handler then names the permission it needs.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/parties", get(list_parties).post(create_party))
        .route(
            "/parties/{id}",
            get(get_party).put(update_party).delete(delete_party),
        )
}

#[utoipa::path(
    get, path = "/api/v1/master-data/parties", tag = "master-data",
    params(Pagination),
    responses(
        (status = 200, description = "Parties", body = [PartySummary]),
        (status = 403, description = "Missing master-data:party:read")
    ),
    security(("bearer" = []))
)]
async fn list_parties(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<PartySummary>>, AppError> {
    let (parties, meta) = service::list_parties(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(parties, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/master-data/parties/{id}", tag = "master-data",
    responses(
        (status = 200, description = "The party aggregate", body = PartyAggregate),
        (status = 404, description = "No such party")
    ),
    security(("bearer" = []))
)]
async fn get_party(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<PartyAggregate>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_party(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/master-data/parties", tag = "master-data",
    request_body = CreatePartyRequest,
    responses(
        (status = 201, description = "Created", body = PartyAggregate),
        (status = 409, description = "That partyId is already in use"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn create_party(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreatePartyRequest>,
) -> Result<(axum::http::StatusCode, Json<ItemEnvelope<PartyAggregate>>), AppError> {
    let party = service::create_party(&state, &caller, request).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ItemEnvelope::new(party)),
    ))
}

#[utoipa::path(
    put, path = "/api/v1/master-data/parties/{id}", tag = "master-data",
    request_body = UpdatePartyRequest,
    responses(
        (status = 200, description = "Updated", body = PartyAggregate),
        (status = 404, description = "No such party"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn update_party(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdatePartyRequest>,
) -> Result<Json<ItemEnvelope<PartyAggregate>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_party(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/master-data/parties/{id}", tag = "master-data",
    responses(
        (status = 204, description = "Deleted; the party is soft-deleted and its history kept"),
        (status = 404, description = "No such party")
    ),
    security(("bearer" = []))
)]
async fn delete_party(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    service::delete_party(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
