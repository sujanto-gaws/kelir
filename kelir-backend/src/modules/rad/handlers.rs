//! Routes over form and list definitions (FR-RAD-002, FR-RAD-003).
//!
//! Every route here requires a token: taking [`Authenticated`] is what enforces
//! it (FR-API-008), and each handler's service names the permission it needs.
//!
//! **There is no builder UI behind these and this does not narrow one.** The
//! builder is FR-RAD-004 and stays in Phase 7 under decision **D-2**; what this
//! module offers is storage and retrieval of a definition somebody produced,
//! which is what #157's document type binds and what Sprint 8's renderer reads.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    CreateFormRequest, CreateListRequest, Form, FormSummary, ListDefinition, ListSummary,
    UpdateFormRequest, UpdateListRequest,
};
use super::service;
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/forms", get(list_forms).post(create_form))
        .route(
            "/forms/{id}",
            get(get_form).put(update_form).delete(delete_form),
        )
        // Publishing is a verb sub-resource rather than a field on the update
        // payload, for the reason a lifecycle transition is one (#99, naming
        // convention §5): it is not a field edit, it needs its own permission,
        // and a `status` a caller could set to PUBLISHED would be a way past
        // that permission.
        .route("/forms/{id}/publish", post(publish_form))
        // A new revision is a new row, so it is a POST that creates something
        // under the revision it is derived from.
        .route("/forms/{id}/revisions", post(create_revision))
        .route("/lists", get(list_lists).post(create_list))
        .route(
            "/lists/{id}",
            get(get_list).put(update_list).delete(delete_list),
        )
}

#[utoipa::path(
    get, path = "/api/v1/rad/forms", tag = "rad",
    params(Pagination),
    responses(
        (status = 200, description = "Form definitions, without their documents", body = [FormSummary]),
        (status = 403, description = "Missing rad:form:read")
    ),
    security(("bearer" = []))
)]
async fn list_forms(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<FormSummary>>, AppError> {
    let (forms, meta) = service::form::list_forms(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(forms, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/rad/forms/{id}", tag = "rad",
    responses(
        (status = 200, description = "The form definition, JFSS document included", body = Form),
        (status = 404, description = "No such form definition")
    ),
    security(("bearer" = []))
)]
async fn get_form(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Form>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::form::get_form(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/rad/forms", tag = "rad",
    request_body = CreateFormRequest,
    responses(
        (status = 201, description = "Created as revision 1, in DRAFT", body = Form),
        (status = 409, description = "That formKey already has revisions"),
        (status = 422, description = "The definition is not JFSS, or uses an operator no registry approves")
    ),
    security(("bearer" = []))
)]
async fn create_form(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateFormRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<Form>>), AppError> {
    let form = service::form::create_form(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(form))))
}

#[utoipa::path(
    put, path = "/api/v1/rad/forms/{id}", tag = "rad",
    request_body = UpdateFormRequest,
    responses(
        (status = 200, description = "Updated", body = Form),
        (status = 404, description = "No such form definition"),
        (status = 422, description = "The revision is published, or the definition is invalid")
    ),
    security(("bearer" = []))
)]
async fn update_form(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateFormRequest>,
) -> Result<Json<ItemEnvelope<Form>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::form::update_form(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/rad/forms/{id}/publish", tag = "rad",
    responses(
        (status = 200, description = "Published; the revision is now immutable", body = Form),
        (status = 403, description = "Missing rad:form:publish"),
        (status = 404, description = "No such form definition"),
        (status = 409, description = "Only a draft can be published")
    ),
    security(("bearer" = []))
)]
async fn publish_form(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Form>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::form::publish_form(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/rad/forms/{id}/revisions", tag = "rad",
    request_body = UpdateFormRequest,
    responses(
        (status = 201, description = "The next revision, in DRAFT", body = Form),
        (status = 404, description = "No such form definition"),
        (status = 422, description = "The definition is invalid")
    ),
    security(("bearer" = []))
)]
async fn create_revision(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateFormRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<Form>>), AppError> {
    let form = service::form::create_revision(&state, &caller, id, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(form))))
}

#[utoipa::path(
    delete, path = "/api/v1/rad/forms/{id}", tag = "rad",
    responses(
        (status = 204, description = "Retired; the revision is soft-deleted"),
        (status = 404, description = "No such form definition")
    ),
    security(("bearer" = []))
)]
async fn delete_form(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<StatusCode, AppError> {
    service::form::delete_form(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/rad/lists", tag = "rad",
    params(Pagination),
    responses(
        (status = 200, description = "List definitions, without their columns and filters", body = [ListSummary]),
        (status = 403, description = "Missing rad:list:read")
    ),
    security(("bearer" = []))
)]
async fn list_lists(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<ListSummary>>, AppError> {
    let (lists, meta) = service::list::list_lists(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(lists, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/rad/lists/{id}", tag = "rad",
    responses(
        (status = 200, description = "The list definition with its columns and filters", body = ListDefinition),
        (status = 404, description = "No such list definition")
    ),
    security(("bearer" = []))
)]
async fn get_list(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<ListDefinition>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::list::get_list(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/rad/lists", tag = "rad",
    request_body = CreateListRequest,
    responses(
        (status = 201, description = "Created", body = ListDefinition),
        (status = 409, description = "That listKey is already in use"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn create_list(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateListRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<ListDefinition>>), AppError> {
    let list = service::list::create_list(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(list))))
}

#[utoipa::path(
    put, path = "/api/v1/rad/lists/{id}", tag = "rad",
    request_body = UpdateListRequest,
    responses(
        (status = 200, description = "Updated; a collection that is sent replaces the stored set", body = ListDefinition),
        (status = 404, description = "No such list definition"),
        (status = 422, description = "Validation failed")
    ),
    security(("bearer" = []))
)]
async fn update_list(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateListRequest>,
) -> Result<Json<ItemEnvelope<ListDefinition>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::list::update_list(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/rad/lists/{id}", tag = "rad",
    responses(
        (status = 204, description = "Retired; the list is soft-deleted"),
        (status = 404, description = "No such list definition")
    ),
    security(("bearer" = []))
)]
async fn delete_list(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<StatusCode, AppError> {
    service::list::delete_list(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
