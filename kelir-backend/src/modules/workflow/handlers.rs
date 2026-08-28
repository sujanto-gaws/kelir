//! Routes over workflow definitions, instances and tasks (FR-WF-001..004, 006,
//! 007, 013, 014).
//!
//! Every route requires a token: taking [`Authenticated`] is what enforces it
//! (FR-API-008), and each handler's service names the permission it needs.
//!
//! # Two verb sub-resources, and each is a transaction rather than a field
//!
//! * `POST /definitions/{id}/publication` — publishing, which fixes a revision
//!   for every instance that will run it. `rad::handlers`' spelling for the same
//!   operation on a form; two names for one thing across two modules would be a
//!   difference with no reason behind it.
//! * `POST /tasks/{id}/claim` and `POST /tasks/{id}/decision` — taking a task,
//!   and deciding it. Both have their own permission and their own
//!   preconditions, which is why neither is a `PUT` of a field.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    CreateWorkflowRequest, DecisionRequest, UpdateWorkflowRequest, WorkflowDefinition,
    WorkflowDefinitionSummary, WorkflowTask,
};
use super::service::instance::DocumentWorkflow;
use super::service::task::DecisionResult;
use super::service::{
    definition as definition_service, instance as instance_service, task as task_service,
};
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/definitions",
            get(list_definitions).post(create_definition),
        )
        .route(
            "/definitions/{id}",
            get(get_definition)
                .put(update_definition)
                .delete(delete_definition),
        )
        .route("/definitions/{id}/publication", post(publish_definition))
        .route("/definitions/{id}/revisions", post(create_revision))
        .route("/instances/{id}", get(get_instance))
        .route("/tasks/{id}/claim", post(claim_task))
        .route("/tasks/{id}/decision", post(decide_task))
}

#[utoipa::path(
    get, path = "/api/v1/workflow/definitions", tag = "workflow",
    params(("page" = Option<u32>, Query, description = "1-based page"),
           ("pageSize" = Option<u32>, Query, description = "rows per page")),
    responses(
        (status = 200, description = "Workflow definitions in the caller's tenant", body = [WorkflowDefinitionSummary]),
        (status = 403, description = "Missing workflow:definition:read")
    ),
    security(("bearer" = []))
)]
async fn list_definitions(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<WorkflowDefinitionSummary>>, AppError> {
    let (definitions, meta) =
        definition_service::list_definitions(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(definitions, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/workflow/definitions/{id}", tag = "workflow",
    responses(
        (status = 200, description = "The definition, with its JWSS document", body = WorkflowDefinition),
        (status = 404, description = "No such definition")
    ),
    security(("bearer" = []))
)]
async fn get_definition(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<WorkflowDefinition>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        definition_service::get_definition(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/workflow/definitions", tag = "workflow",
    request_body = CreateWorkflowRequest,
    responses(
        (status = 201, description = "Created as revision 1, in DRAFT", body = WorkflowDefinition),
        (status = 403, description = "Missing workflow:definition:create"),
        (status = 409, description = "That workflow key already has revisions"),
        (status = 422, description = "The definition is not a valid JWSS document, or its graph does not terminate")
    ),
    security(("bearer" = []))
)]
async fn create_definition(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateWorkflowRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<WorkflowDefinition>>), AppError> {
    let definition = definition_service::create_definition(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(definition))))
}

#[utoipa::path(
    put, path = "/api/v1/workflow/definitions/{id}", tag = "workflow",
    request_body = UpdateWorkflowRequest,
    responses(
        (status = 200, description = "Updated", body = WorkflowDefinition),
        (status = 404, description = "No such definition"),
        (status = 422, description = "The revision is published, or the definition is invalid")
    ),
    security(("bearer" = []))
)]
async fn update_definition(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateWorkflowRequest>,
) -> Result<Json<ItemEnvelope<WorkflowDefinition>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        definition_service::update_definition(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/workflow/definitions/{id}/publication", tag = "workflow",
    responses(
        (status = 200, description = "Published, and its states and transitions projected", body = WorkflowDefinition),
        (status = 403, description = "Missing workflow:definition:publish"),
        (status = 404, description = "No such definition"),
        (status = 409, description = "The revision is not a draft"),
        (status = 422, description = "The stored definition does not satisfy the JWSS structural rules")
    ),
    security(("bearer" = []))
)]
async fn publish_definition(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<WorkflowDefinition>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        definition_service::publish_definition(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/workflow/definitions/{id}/revisions", tag = "workflow",
    request_body = UpdateWorkflowRequest,
    responses(
        (status = 201, description = "The next revision, as a draft", body = WorkflowDefinition),
        (status = 403, description = "Missing workflow:definition:create"),
        (status = 404, description = "No such definition")
    ),
    security(("bearer" = []))
)]
async fn create_revision(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateWorkflowRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<WorkflowDefinition>>), AppError> {
    let created = definition_service::create_revision(&state, &caller, id, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(created))))
}

#[utoipa::path(
    delete, path = "/api/v1/workflow/definitions/{id}", tag = "workflow",
    responses(
        (status = 204, description = "Retired"),
        (status = 404, description = "No such definition"),
        (status = 409, description = "Approvals are still running against this revision")
    ),
    security(("bearer" = []))
)]
async fn delete_definition(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<StatusCode, AppError> {
    definition_service::delete_definition(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/workflow/instances/{id}", tag = "workflow",
    responses(
        (status = 200, description = "The running process, its variables and its tasks", body = DocumentWorkflow),
        (status = 403, description = "Missing workflow:instance:read"),
        (status = 404, description = "No such instance")
    ),
    security(("bearer" = []))
)]
async fn get_instance(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<DocumentWorkflow>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        instance_service::get_instance(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/workflow/tasks/{id}/claim", tag = "workflow",
    responses(
        (status = 200, description = "Claimed, and now assigned to the caller", body = WorkflowTask),
        (status = 403, description = "Missing workflow:task:execute, or the caller does not hold the candidate role"),
        (status = 404, description = "No such task"),
        (status = 409, description = "Somebody else claimed it, or it is no longer open")
    ),
    security(("bearer" = []))
)]
async fn claim_task(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<WorkflowTask>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        task_service::claim_task(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/workflow/tasks/{id}/decision", tag = "workflow",
    request_body = DecisionRequest,
    responses(
        (status = 200, description = "Decided; the process moved and the document's status followed", body = DecisionResult),
        (status = 403, description = "Missing workflow:task:execute, or this task is not the caller's"),
        (status = 404, description = "No such task"),
        (status = 409, description = "The task was already decided, or the process moved underneath the decision"),
        (status = 422, description = "The definition has no such transition from where the process is, \
                                      or the transition requires a comment and none was given, \
                                      or the comment is too long")
    ),
    security(("bearer" = []))
)]
async fn decide_task(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<DecisionRequest>,
) -> Result<Json<ItemEnvelope<DecisionResult>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        task_service::decide(&state, &caller, id, request.action, request.comment).await?,
    )))
}
