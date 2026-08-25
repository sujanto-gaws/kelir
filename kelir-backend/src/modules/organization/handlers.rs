//! Routes over tenants (FR-ORG-001) and departments (FR-ORG-002).
//!
//! Departments came first; tenant administration arrived with decision
//! **D-18**, which supersedes **D-7**. Positions (FR-ORG-003) still have no
//! table at all.
//!
//! **The tenant routes carry a condition no other route in the system does.**
//! Besides their permission, the caller must be signed in to the deployment's
//! *administering* tenant — see `service::require_tenant_administrator`. A
//! tenant is not a row inside a tenant, so `tenant_id` scoping cannot express
//! who may touch it and a permission alone cannot either.
//!
//! **Assigning a user to a department has no route here**, which is decision
//! **D-8** working as intended: FR-IDM-008 is the edge `users.department_id`,
//! and that column is set through the user surface in `identity`. A second
//! place to write it would be a second thing to keep in step.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::department::{CreateDepartmentRequest, Department, UpdateDepartmentRequest};
use super::department_service;
use super::domain::{CreateTenantRequest, TenantView, UpdateTenantRequest};
use super::service;
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/departments",
            get(list_departments).post(create_department),
        )
        .route(
            "/departments/{id}",
            get(get_department)
                .put(update_department)
                .delete(delete_department),
        )
        .route("/tenants", get(list_tenants).post(create_tenant))
        .route(
            "/tenants/{id}",
            get(get_tenant).put(update_tenant).delete(delete_tenant),
        )
}

#[utoipa::path(
    get, path = "/api/v1/organization/tenants", tag = "organization",
    params(Pagination),
    responses(
        (status = 200, description = "Tenants", body = [TenantView]),
        (status = 403, description = "Missing organization:tenant:read, or not signed in to the administering tenant")
    ),
    security(("bearer" = []))
)]
async fn list_tenants(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<TenantView>>, AppError> {
    let (tenants, meta) = service::list_tenants(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(tenants, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/organization/tenants/{id}", tag = "organization",
    responses(
        (status = 200, description = "The tenant", body = TenantView),
        (status = 404, description = "No such tenant")
    ),
    security(("bearer" = []))
)]
async fn get_tenant(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<TenantView>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_tenant(&state, &caller, id).await?,
    )))
}

/// Create a tenant, together with the administrator who can sign in to it.
#[utoipa::path(
    post, path = "/api/v1/organization/tenants", tag = "organization",
    request_body = CreateTenantRequest,
    responses(
        (status = 201, description = "Created, with its first administrator", body = TenantView),
        (status = 409, description = "The tenant code, or the administrator's username or email, is already in use"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn create_tenant(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateTenantRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<TenantView>>), AppError> {
    let tenant = service::create_tenant(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(tenant))))
}

/// Rename a tenant, or change its status.
///
/// `tenantCode` is not updatable — it is the handle users sign in with, and
/// changing it would strand them at a login form with no way to learn the new
/// value.
#[utoipa::path(
    put, path = "/api/v1/organization/tenants/{id}", tag = "organization",
    request_body = UpdateTenantRequest,
    responses(
        (status = 200, description = "Updated; suspending a tenant also revokes its refresh tokens", body = TenantView),
        (status = 400, description = "Refusing to suspend the tenant the request came from"),
        (status = 404, description = "No such tenant")
    ),
    security(("bearer" = []))
)]
async fn update_tenant(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateTenantRequest>,
) -> Result<Json<ItemEnvelope<TenantView>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_tenant(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/organization/tenants/{id}", tag = "organization",
    responses(
        (status = 204, description = "Soft-deleted; its refresh tokens are revoked and its users can no longer sign in. An access token already issued remains valid until it expires."),
        (status = 400, description = "Refusing to delete the tenant the request came from")
    ),
    security(("bearer" = []))
)]
async fn delete_tenant(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    service::delete_tenant(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/organization/departments", tag = "organization",
    params(Pagination),
    responses(
        (status = 200, description = "Departments", body = [Department]),
        (status = 403, description = "Missing organization:department:read")
    ),
    security(("bearer" = []))
)]
async fn list_departments(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<Department>>, AppError> {
    let (departments, meta) =
        department_service::list_departments(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(departments, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/organization/departments/{id}", tag = "organization",
    responses(
        (status = 200, description = "The department", body = Department),
        (status = 404, description = "No such department")
    ),
    security(("bearer" = []))
)]
async fn get_department(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<Department>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        department_service::get_department(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/organization/departments", tag = "organization",
    request_body = CreateDepartmentRequest,
    responses(
        (status = 201, description = "Created", body = Department),
        (status = 409, description = "That departmentId is already in use"),
        (status = 422, description = "Validation failed, or a reference names something that does not exist")
    ),
    security(("bearer" = []))
)]
async fn create_department(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateDepartmentRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<Department>>), AppError> {
    let department = department_service::create_department(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(department))))
}

#[utoipa::path(
    put, path = "/api/v1/organization/departments/{id}", tag = "organization",
    request_body = UpdateDepartmentRequest,
    responses(
        (status = 200, description = "Updated", body = Department),
        (status = 404, description = "No such department"),
        (status = 422, description = "Validation failed, or the move would close a loop in the hierarchy")
    ),
    security(("bearer" = []))
)]
async fn update_department(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateDepartmentRequest>,
) -> Result<Json<ItemEnvelope<Department>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        department_service::update_department(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/organization/departments/{id}", tag = "organization",
    responses(
        (status = 204, description = "Retired; the department is soft-deleted"),
        (status = 404, description = "No such department"),
        (status = 409, description = "Sub-departments, users or employee profiles still point at it")
    ),
    security(("bearer" = []))
)]
async fn delete_department(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    department_service::delete_department(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
