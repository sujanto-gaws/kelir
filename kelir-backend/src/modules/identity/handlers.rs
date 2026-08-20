use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::domain::{
    CreateRoleRequest, CreateUserRequest, Permission, Role, UpdateRoleRequest, UpdateUserRequest,
    User,
};
use super::service;
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetPasswordRequest {
    pub password: String,
}

/// Every route here requires a token: taking [`Authenticated`] is what enforces
/// it (FR-API-008), and each handler then names the permission it needs.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", get(get_user).put(update_user))
        .route("/users/{id}", delete(deactivate_user))
        .route("/users/{id}/password", post(set_password))
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", get(get_role).put(update_role))
        .route("/roles/{id}", delete(delete_role))
        .route("/permissions", get(list_permissions))
}

#[utoipa::path(
    get, path = "/api/v1/identity/users", tag = "identity",
    params(Pagination),
    responses((status = 200, description = "Users", body = [User]), (status = 403, description = "Missing identity:user:read")),
    security(("bearer" = []))
)]
async fn list_users(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<User>>, AppError> {
    let (users, meta) = service::list_users(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(users, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/identity/users/{id}", tag = "identity",
    responses((status = 200, description = "The user", body = User), (status = 404, description = "No such user")),
    security(("bearer" = []))
)]
async fn get_user(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<User>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_user(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/identity/users", tag = "identity",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "Created", body = User),
        (status = 409, description = "Username or email already in use"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn create_user(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateUserRequest>,
) -> Result<(axum::http::StatusCode, Json<ItemEnvelope<User>>), AppError> {
    let user = service::create_user(&state, &caller, request).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ItemEnvelope::new(user)),
    ))
}

#[utoipa::path(
    put, path = "/api/v1/identity/users/{id}", tag = "identity",
    request_body = UpdateUserRequest,
    responses((status = 200, description = "Updated", body = User)),
    security(("bearer" = []))
)]
async fn update_user(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateUserRequest>,
) -> Result<Json<ItemEnvelope<User>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_user(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/identity/users/{id}", tag = "identity",
    responses(
        (status = 204, description = "Deactivated"),
        (status = 400, description = "Refusing to deactivate your own account")
    ),
    security(("bearer" = []))
)]
async fn deactivate_user(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    service::deactivate_user(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/v1/identity/users/{id}/password", tag = "identity",
    request_body = SetPasswordRequest,
    responses((status = 204, description = "Password set; every session for that user ends")),
    security(("bearer" = []))
)]
async fn set_password(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<SetPasswordRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    service::set_password(&state, &caller, id, &request.password).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/identity/roles", tag = "identity",
    params(Pagination),
    responses((status = 200, description = "Roles with their permissions", body = [Role])),
    security(("bearer" = []))
)]
async fn list_roles(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<Role>>, AppError> {
    let (roles, meta) = service::list_roles(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(roles, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/identity/roles/{id}", tag = "identity",
    responses((status = 200, description = "The role", body = Role)),
    security(("bearer" = []))
)]
async fn get_role(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<Role>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_role(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/identity/roles", tag = "identity",
    request_body = CreateRoleRequest,
    responses((status = 201, description = "Created", body = Role)),
    security(("bearer" = []))
)]
async fn create_role(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateRoleRequest>,
) -> Result<(axum::http::StatusCode, Json<ItemEnvelope<Role>>), AppError> {
    let role = service::create_role(&state, &caller, request).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ItemEnvelope::new(role)),
    ))
}

#[utoipa::path(
    put, path = "/api/v1/identity/roles/{id}", tag = "identity",
    request_body = UpdateRoleRequest,
    responses((status = 200, description = "Updated", body = Role)),
    security(("bearer" = []))
)]
async fn update_role(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateRoleRequest>,
) -> Result<Json<ItemEnvelope<Role>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_role(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/identity/roles/{id}", tag = "identity",
    responses(
        (status = 204, description = "Deleted"),
        (status = 409, description = "System roles cannot be deleted")
    ),
    security(("bearer" = []))
)]
async fn delete_role(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    service::delete_role(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/identity/permissions", tag = "identity",
    responses((status = 200, description = "The permission catalogue", body = [Permission])),
    security(("bearer" = []))
)]
async fn list_permissions(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<Vec<Permission>>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::list_permissions(&state, &caller).await?,
    )))
}
