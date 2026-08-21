use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    AssignRoleRequest, CreatePartyRequest, PartyAggregate, PartyRoles, PartySummary, RoleView,
    RoleViewQuery, RoleViewRow, UpdatePartyRequest,
};
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
        .route("/parties/{id}/roles", get(get_party_roles))
        .route(
            "/parties/{id}/roles/{roleTypeId}",
            axum::routing::put(assign_role).delete(remove_role),
        )
        // The role views (SDD §12.2). Three paths rather than
        // `/parties?role=SUPPLIER` because that is how the design names them,
        // and because a client that asked for suppliers should not be able to
        // ask for them wrongly — there is no role parameter to get wrong.
        .route("/suppliers", get(list_suppliers))
        .route("/customers", get(list_customers))
        .route("/employees", get(list_employees))
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

// ---------------------------------------------------------------------------
// Roles and role profiles (FR-MDM-002)
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/master-data/parties/{id}/roles", tag = "master-data",
    responses(
        (status = 200, description = "The party's roles and their profiles", body = PartyRoles),
        (status = 403, description = "Missing master-data:party-role:read"),
        (status = 404, description = "No such party")
    ),
    security(("bearer" = []))
)]
async fn get_party_roles(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<PartyRoles>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_party_roles(&state, &caller, id).await?,
    )))
}

/// `PUT` rather than `POST`: assigning a role is idempotent — a party either
/// holds SUPPLIER or it does not — and the role type is what identifies the
/// assignment, so it belongs in the path rather than repeated in the body.
#[utoipa::path(
    put, path = "/api/v1/master-data/parties/{id}/roles/{roleTypeId}", tag = "master-data",
    request_body = AssignRoleRequest,
    responses(
        (status = 200, description = "The party already held this role; it and its profile are updated", body = PartyRoles),
        (status = 201, description = "Role assigned", body = PartyRoles),
        (status = 403, description = "Missing master-data:party-role:assign"),
        (status = 404, description = "No such party"),
        (status = 409, description = "That profile number is already in use"),
        (status = 422, description = "Validation failed, or no such role type")
    ),
    security(("bearer" = []))
)]
async fn assign_role(
    State(state): State<AppState>,
    caller: Authenticated,
    Path((id, role_type_id)): Path<(Uuid, String)>,
    JsonBody(request): JsonBody<AssignRoleRequest>,
) -> Result<(axum::http::StatusCode, Json<ItemEnvelope<PartyRoles>>), AppError> {
    let (created, roles) =
        service::assign_role(&state, &caller, id, &role_type_id, request).await?;

    let status = if created {
        axum::http::StatusCode::CREATED
    } else {
        axum::http::StatusCode::OK
    };

    Ok((status, Json(ItemEnvelope::new(roles))))
}

#[utoipa::path(
    delete, path = "/api/v1/master-data/parties/{id}/roles/{roleTypeId}", tag = "master-data",
    responses(
        (status = 204, description = "Role removed; the party and its other roles are untouched"),
        (status = 403, description = "Missing master-data:party-role:remove"),
        (status = 404, description = "No such party, or it does not hold that role")
    ),
    security(("bearer" = []))
)]
async fn remove_role(
    State(state): State<AppState>,
    caller: Authenticated,
    Path((id, role_type_id)): Path<(Uuid, String)>,
) -> Result<axum::http::StatusCode, AppError> {
    service::remove_role(&state, &caller, id, &role_type_id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Role views (FR-MDM-002, FR-MDM-008)
// ---------------------------------------------------------------------------

/// The three views differ only in which role they are over, so they share one
/// body. The [`RoleView`] is chosen here, by the route — never taken from the
/// request.
async fn list_role_view(
    state: &AppState,
    caller: &Authenticated,
    view: RoleView,
    query: &RoleViewQuery,
) -> Result<Json<ListEnvelope<RoleViewRow>>, AppError> {
    let (rows, meta) = service::list_role_view(state, caller, view, query).await?;

    Ok(Json(ListEnvelope::new(rows, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/master-data/suppliers", tag = "master-data",
    params(RoleViewQuery),
    responses(
        (status = 200, description = "Parties holding the SUPPLIER role", body = [RoleViewRow]),
        (status = 403, description = "Missing master-data:party:read or master-data:party-role:read"),
        (status = 422, description = "A filter names a value outside its vocabulary")
    ),
    security(("bearer" = []))
)]
async fn list_suppliers(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(query): Query<RoleViewQuery>,
) -> Result<Json<ListEnvelope<RoleViewRow>>, AppError> {
    list_role_view(&state, &caller, RoleView::Supplier, &query).await
}

#[utoipa::path(
    get, path = "/api/v1/master-data/customers", tag = "master-data",
    params(RoleViewQuery),
    responses(
        (status = 200, description = "Parties holding the CUSTOMER role", body = [RoleViewRow]),
        (status = 403, description = "Missing master-data:party:read or master-data:party-role:read"),
        (status = 422, description = "A filter names a value outside its vocabulary")
    ),
    security(("bearer" = []))
)]
async fn list_customers(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(query): Query<RoleViewQuery>,
) -> Result<Json<ListEnvelope<RoleViewRow>>, AppError> {
    list_role_view(&state, &caller, RoleView::Customer, &query).await
}

#[utoipa::path(
    get, path = "/api/v1/master-data/employees", tag = "master-data",
    params(RoleViewQuery),
    responses(
        (status = 200, description = "Parties holding the EMPLOYEE role", body = [RoleViewRow]),
        (status = 403, description = "Missing master-data:party:read or master-data:party-role:read"),
        (status = 422, description = "A filter names a value outside its vocabulary")
    ),
    security(("bearer" = []))
)]
async fn list_employees(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(query): Query<RoleViewQuery>,
) -> Result<Json<ListEnvelope<RoleViewRow>>, AppError> {
    list_role_view(&state, &caller, RoleView::Employee, &query).await
}
