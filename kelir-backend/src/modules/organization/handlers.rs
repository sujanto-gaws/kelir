//! Routes over departments (FR-ORG-002).
//!
//! The organization module's first endpoints. Tenant administration
//! (FR-ORG-001) still has none — decision **D-7** keeps a deployment
//! single-tenant, so the surface would manage rows nobody can sign in to — and
//! positions (FR-ORG-003) have no table at all.
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
