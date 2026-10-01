use axum::extract::State;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::delegation::{CreateDelegationRequest, Delegation};
use super::delegation_service;
use super::domain::{
    CreateRoleRequest, CreateUserRequest, Permission, Role, RoleQuery, UpdateRoleRequest,
    UpdateUserRequest, User, UserQuery,
};
use super::service;
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::modules::workflow::domain::OpenTaskNeedingRole;
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
        .route("/roles/{id}/open-tasks", get(list_open_tasks_of_role))
        .route("/permissions", get(list_permissions))
        .route(
            "/delegations",
            get(list_delegations).post(create_delegation),
        )
        .route("/delegations/{id}", delete(end_delegation))
}

#[utoipa::path(
    get, path = "/api/v1/identity/users", tag = "identity",
    params(UserQuery),
    responses(
        (status = 200, description = "Users; a soft-deleted user is never listed", body = [User]),
        (status = 403, description = "Missing identity:user:read"),
        (status = 422, description = "A query parameter that does not parse, such as a status outside the vocabulary")
    ),
    security(("bearer" = []))
)]
async fn list_users(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<UserQuery>,
) -> Result<Json<ListEnvelope<User>>, AppError> {
    let (users, meta) = service::list_users(&state, &caller, &query).await?;

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
    PathParam(id): PathParam<Uuid>,
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
    responses(
        (status = 200, description = "Updated", body = User),
        (status = 422, description = "Validation failed, including a role id listed twice in roleIds")
    ),
    security(("bearer" = []))
)]
async fn update_user(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
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
    PathParam(id): PathParam<Uuid>,
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
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<SetPasswordRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    service::set_password(&state, &caller, id, &request.password).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/identity/roles", tag = "identity",
    params(RoleQuery),
    responses(
        (status = 200, description = "Roles with their permissions. For a caller holding \
            `workflow:task:reassign`, each also carries `liveHolders` and `openTasks` (#508, \
            D-91 (2)); both are omitted for anybody else. `liveHolders` 0 beside a non-zero \
            `openTasks` is a role whose last holder has left while open tasks still need it: \
            `GET /api/v1/identity/roles/{id}/open-tasks` lists them, and \
            `POST /api/v1/workflow/tasks/{id}/reassign` clears each one.", body = [Role]),
        (status = 403, description = "Missing identity:role:read")
    ),
    security(("bearer" = []))
)]
async fn list_roles(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<RoleQuery>,
) -> Result<Json<ListEnvelope<Role>>, AppError> {
    let (roles, meta) = service::list_roles(&state, &caller, &query).await?;

    Ok(Json(ListEnvelope::new(roles, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/identity/roles/{id}", tag = "identity",
    responses(
        (status = 200, description = "The role. `liveHolders` and `openTasks` as on the list: \
            present only for a caller holding `workflow:task:reassign` (#508).", body = Role),
        (status = 403, description = "Missing identity:role:read"),
        (status = 404, description = "No live role by that id in this tenant")
    ),
    security(("bearer" = []))
)]
async fn get_role(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Role>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_role(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/identity/roles", tag = "identity",
    request_body = CreateRoleRequest,
    responses(
        (status = 201, description = "Created", body = Role),
        (status = 422, description = "VALIDATION_ERROR, a detail per field: roleCode or name blank (REQUIRED) or longer than its column, 64 and 200 characters once trimmed (TOO_LONG) (#575); a permission id listed twice in permissionIds (DUPLICATE_IN_ARRAY)")
    ),
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
    responses(
        (status = 200, description = "Updated", body = Role),
        (status = 422, description = "VALIDATION_ERROR, a detail per field: a present name blank (REQUIRED) or over 200 characters once trimmed (TOO_LONG) (#575); a permission id listed twice in permissionIds (DUPLICATE_IN_ARRAY)")
    ),
    security(("bearer" = []))
)]
async fn update_role(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
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
        (status = 404, description = "No live role by that id in this tenant"),
        (status = 409, description = "Refused, one reason at a time, checked in this order:\n\n\
            - `CONFLICT`: a system role.\n\
            - `ROLE_HAS_OPEN_TASKS`: a role that open tasks still need (D-89). The message says \
            how many; `GET /api/v1/identity/roles/{id}/open-tasks` lists them (#532).\n\
            - `ROLE_NAMED_BY_PUBLISHED_DEFINITION`: no open task needs the role, but a published \
            workflow definition names it in a task's assignment or a transition's allowedBy \
            (D-91 (3)). That is an ACTIVE revision, or a DEPRECATED one with approvals still \
            running on it. The message names each by key, name and revision.")
    ),
    security(("bearer" = []))
)]
async fn delete_role(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    service::delete_role(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// The open tasks a delete of this role waits on (**D-89**, [#532]): what
/// `delete_role`'s `ROLE_HAS_OPEN_TASKS` counts, one row per task.
///
/// **Guarded by `identity:role:delete` alone.** Each row names its document by
/// number and title without a `document:read` check: the product owner decided
/// on 2026-09-26 that this is the minimum needed to explain one refused delete,
/// and that whoever may make the delete may read it. No form data and no
/// attachments are shown.
///
/// **A read outside the delete's transaction**: it does not take the role's
/// lock, so by the time it is read it can differ from the count a delete takes.
/// It is a diagnostic, and that is acceptable.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
#[utoipa::path(
    get, path = "/api/v1/identity/roles/{id}/open-tasks", tag = "identity",
    params(Pagination),
    responses(
        (status = 200, description = "The open tasks that need the role, oldest first; `meta.total` is the count a delete refuses on", body = [OpenTaskNeedingRole]),
        (status = 403, description = "Missing identity:role:delete"),
        (status = 404, description = "No live role by that id in this tenant")
    ),
    security(("bearer" = []))
)]
async fn list_open_tasks_of_role(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<OpenTaskNeedingRole>>, AppError> {
    let (tasks, meta) = service::list_open_tasks_of_role(&state, &caller, id, &pagination).await?;

    Ok(Json(ListEnvelope::new(tasks, meta)))
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

// ---------------------------------------------------------------------------
// Delegation windows (FR-IDM-006, #184)
// ---------------------------------------------------------------------------

#[utoipa::path(
    get, path = "/api/v1/identity/delegations", tag = "identity",
    params(Pagination),
    responses(
        (status = 200, description = "The tenant's delegation windows, newest first", body = [Delegation]),
        (status = 403, description = "Missing identity:delegation:read")
    ),
    security(("bearer" = []))
)]
async fn list_delegations(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<Delegation>>, AppError> {
    let (delegations, meta) =
        delegation_service::list_delegations(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(delegations, meta)))
}

#[utoipa::path(
    post, path = "/api/v1/identity/delegations", tag = "identity",
    request_body = CreateDelegationRequest,
    responses(
        (status = 201, description = "Opened, in the caller's own name", body = Delegation),
        (status = 403, description = "Missing identity:delegation:create"),
        (status = 422, description = "The delegate is not an active user in this tenant, \
                                      the window has already ended or ends before it starts, \
                                      or the scope names something this engine cannot honour")
    ),
    security(("bearer" = []))
)]
async fn create_delegation(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateDelegationRequest>,
) -> Result<(axum::http::StatusCode, Json<ItemEnvelope<Delegation>>), AppError> {
    let delegation = delegation_service::create_delegation(&state, &caller, request).await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(ItemEnvelope::new(delegation)),
    ))
}

#[utoipa::path(
    delete, path = "/api/v1/identity/delegations/{id}", tag = "identity",
    responses(
        (status = 204, description = "Ended; it stops routing from this moment"),
        (status = 403, description = "Missing identity:delegation:delete"),
        (status = 404, description = "No such delegation in this tenant")
    ),
    security(("bearer" = []))
)]
async fn end_delegation(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    delegation_service::end_delegation(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
