//! Routes over the caller's own tasks (FR-TASK-001, 002, 003).
//!
//! Mounted at `/api/v1/tasks`. **Not `/workflow/tasks`**, which is where the
//! engine's own task routes are: these two paths serve different questions — the
//! engine's are *this task, by id, act on it*, and these are *what is waiting
//! for me*. The permission is the same one, and `mod.rs` says why.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::InboxQuery;
use super::service;
use crate::error::AppError;
use crate::extract::{PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::modules::workflow::service::inbox::{InboxTask, TaskDetail};
use crate::response::{ItemEnvelope, ListEnvelope};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks))
        .route("/{id}", get(get_task))
}

#[utoipa::path(
    get, path = "/api/v1/tasks", tag = "task",
    params(InboxQuery),
    responses(
        (status = 200, description = "Tasks assigned to the caller and to roles the caller holds, distinguishable by `assignment`", body = [InboxTask]),
        (status = 403, description = "Missing workflow:task:read"),
        (status = 422, description = "`scope` is not one of the values the inbox serves")
    ),
    security(("bearer" = []))
)]
async fn list_tasks(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<InboxQuery>,
) -> Result<Json<ListEnvelope<InboxTask>>, AppError> {
    let (tasks, meta) = service::list_tasks(&state, &caller, &query).await?;

    Ok(Json(ListEnvelope::new(tasks, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/tasks/{id}", tag = "task",
    responses(
        (status = 200, description = "The task, the document it is about, the process it belongs to, and the decision being asked", body = TaskDetail),
        (status = 403, description = "Missing workflow:task:read"),
        (status = 404, description = "No such task, or it is not one this caller may see")
    ),
    security(("bearer" = []))
)]
async fn get_task(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<TaskDetail>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_task(&state, &caller, id).await?,
    )))
}
