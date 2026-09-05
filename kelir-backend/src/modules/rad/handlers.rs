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

use super::domain::action::{Action, ActionContext};
use super::domain::render::RenderableList;
use super::domain::submission::{Submission, SubmitFormRequest};
use super::domain::{
    CreateFormRequest, CreateListRequest, Form, FormSummary, ListDefinition, ListSummary,
    LookupOption, LookupQuery, LookupSource, UpdateFormRequest, UpdateListRequest,
};
use super::service;
use super::service::render::{ListRow, RowQuery};
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
        // A filled-in form, as a sub-collection of the revision it was filled
        // in against (naming convention §5). Not `POST /forms/{id}/submit`: the
        // request creates a row that can be read back, which is a resource
        // rather than a verb — and the row is what makes JFSS S8.1's overwrite
        // provable rather than merely performed.
        .route("/forms/{id}/submissions", post(submit_form))
        .route("/lists", get(list_lists).post(create_list))
        .route(
            "/lists/{id}",
            get(get_list).put(update_list).delete(delete_list),
        )
        // The options a lookup field offers, as a sub-collection of the source
        // that produces them (naming convention §5). There is deliberately no
        // `GET /lookups` beside it: the sources are a static allow-list rather
        // than stored rows, and a form author discovers them from the refusal a
        // bad binding earns, which names every one of them. An endpoint whose
        // whole answer is four constants would be a surface to keep in step
        // with them for no reader that needs it.
        // **The render read, and it is deliberately not `/lists/{id}`.** That
        // route serves the builder: any status, `rad:list:read`, the definition
        // as stored. This one serves a renderer: `ACTIVE` only, `document:read`
        // — the permission of the rows behind it — and the definition resolved
        // against what can actually be drawn. A `?for=render` on the first
        // would be one endpoint answering two questions with two permissions,
        // which is the shape a reader cannot tell apart from a bug.
        //
        // By key rather than by id because a rendered list is reached from a
        // URL somebody bookmarks, and §5.6's unique index makes the key the
        // name a menu and a document type already use.
        .route("/lists/by-key/{list_key}", get(get_renderable_list))
        // The rows, as a sub-collection of the list that arranges them (naming
        // convention §5). By id: the caller has just been handed one by the
        // read above, and paging through a key would resolve it again per page.
        .route("/lists/{id}/rows", get(list_rows))
        .route("/lookups/{source}/options", get(list_lookup_options))
        // The configured actions of one context (§5.10). A query parameter
        // rather than `/actions/list`, because `context` selects a subset of
        // one collection rather than naming a different resource — naming
        // convention §5, and the same reading `GET /documents?status=` takes.
        .route("/actions", get(list_actions))
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
    post, path = "/api/v1/rad/forms/{id}/submissions", tag = "rad",
    request_body = SubmitFormRequest,
    responses(
        (status = 201, description = "Stored, carrying the server's own re-evaluated payload", body = Submission),
        (status = 403, description = "Missing rad:form:submit"),
        (status = 404, description = "No such form definition"),
        (status = 409, description = "That revision is not published"),
        (status = 422, description = "The payload failed validation, or an expression produced no value")
    ),
    security(("bearer" = []))
)]
async fn submit_form(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<SubmitFormRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<Submission>>), AppError> {
    let submission = service::submission::submit_form(&state, &caller, id, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(submission))))
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

#[utoipa::path(
    get, path = "/api/v1/rad/lookups/{source}/options", tag = "rad",
    params(
        ("source" = String, Path,
         description = "Master-data source: customer, employee, facility or supplier"),
        LookupQuery,
    ),
    responses(
        (status = 200, description = "One page of the options this lookup offers", body = [LookupOption]),
        (status = 403, description = "Missing the permission that opens the underlying master data"),
        (status = 404, description = "No such lookup source"),
        (status = 422, description = "The search is longer than anything it could match")
    ),
    security(("bearer" = []))
)]
async fn list_lookup_options(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(source): PathParam<String>,
    QueryParams(query): QueryParams<LookupQuery>,
) -> Result<Json<ListEnvelope<LookupOption>>, AppError> {
    // Resolved before the service runs, and this is not a read: the sources are
    // four constants, so nothing about the caller's data has been touched when
    // an unknown one answers 404. The permission check still comes first over
    // anything stored, inside `master_data::service`.
    let source = LookupSource::from_key(&source).ok_or_else(|| AppError::not_found("Lookup"))?;

    let (options, meta) = service::lookup::list_options(&state, &caller, source, &query).await?;

    Ok(Json(ListEnvelope::new(options, meta)))
}

/// What a `context` query names, and the refusal a wrong one earns.
///
/// A typed `context` rather than a free string, so an unknown value is a 422
/// naming the four rather than an empty list — which would read as *this
/// deployment has configured no actions* and is the failure
/// [#326](https://github.com/sujanto-gaws/kelir/issues/326) took in a different
/// panel: nothing distinguishes "none" from "you asked the wrong question".
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
struct ActionQuery {
    context: ActionContext,
}

#[utoipa::path(
    get, path = "/api/v1/rad/actions", tag = "rad",
    params(ActionQuery),
    responses(
        (status = 200, description = "The actions this caller may invoke in that context", body = [Action]),
        (status = 422, description = "`context` is not one of LIST, DETAIL, DOCUMENT or TASK")
    ),
    security(("bearer" = []))
)]
async fn list_actions(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<ActionQuery>,
) -> Result<Json<ListEnvelope<Action>>, AppError> {
    let actions = service::action::list_actions(&state, &caller, query.context).await?;
    // Every row the caller may invoke, so the count is the length rather than a
    // second query: the filter is applied in the service and a `total` from the
    // database would be the *unfiltered* count, which would tell the caller how
    // many actions they were not shown.
    let total = actions.len() as u64;
    let meta = crate::response::PageMeta::new(1, total.max(1) as u32, total);

    Ok(Json(ListEnvelope::new(actions, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/rad/lists/by-key/{list_key}", tag = "rad",
    params(("list_key" = String, Path, description = "The list's tenant-unique key")),
    responses(
        (status = 200, description = "The list as a renderer needs it", body = RenderableList),
        (status = 403, description = "Missing document:read"),
        (status = 404, description = "No such list"),
        (status = 409, description = "The list is a draft or is deprecated"),
        (status = 422, description = "The definition declares something this renderer cannot serve")
    ),
    security(("bearer" = []))
)]
async fn get_renderable_list(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(list_key): PathParam<String>,
) -> Result<Json<ItemEnvelope<RenderableList>>, AppError> {
    let list = service::render::renderable_list(&state, &caller, &list_key).await?;

    Ok(Json(ItemEnvelope::new(list)))
}

#[utoipa::path(
    get, path = "/api/v1/rad/lists/{id}/rows", tag = "rad",
    params(RowQuery),
    responses(
        (status = 200, description = "One page of the rows the list arranges", body = [ListRow]),
        (status = 403, description = "Missing document:read"),
        (status = 404, description = "No such list, or it is not ACTIVE"),
        (status = 422, description = "A filter or sort the definition does not declare")
    ),
    security(("bearer" = []))
)]
async fn list_rows(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    QueryParams(query): QueryParams<RowQuery>,
) -> Result<Json<ListEnvelope<ListRow>>, AppError> {
    let (rows, meta) = service::render::list_rows(&state, &caller, id, &query).await?;

    Ok(Json(ListEnvelope::new(rows, meta)))
}
