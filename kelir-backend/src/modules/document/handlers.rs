//! Routes over documents (FR-DOC-001..007, 011, 013, 014).
//!
//! Every route requires a token: taking [`Authenticated`] is what enforces it
//! (FR-API-008), and each handler's service names the permission it needs.
//!
//! # Three sub-resources, and each is a verb or a part rather than a field
//!
//! * `POST /{id}/submission` — the submit. A **verb sub-resource** (naming
//!   convention §5), because it is a transaction that takes a number rather than
//!   a value somebody sets.
//! * `PUT /{id}/status` — the transition, for the same reason and with its own
//!   permission (#169 AC2, the shape #99 established).
//! * `GET /{id}/linked-entity` — the resolution, separate because it is the one
//!   read on this surface that another module's permission governs
//!   ([`super::service::link`]).
//!
//! `GET /{id}/status-history` is a part of the document rather than a resource
//! beside it, and it needs nothing but the document's own read permission.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    CreateDocumentRequest, Document, DocumentQuery, DocumentSummary, ResolvedEntity,
    TransitionRequest, TransitionResult, UpdateDocumentRequest,
};
use super::service::{self, StatusHistoryEntry};
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_documents).post(create_document))
        .route(
            "/{id}",
            get(get_document)
                .put(update_document)
                .delete(delete_document),
        )
        .route("/{id}/submission", post(submit_document))
        .route("/{id}/status", put(transition_document))
        .route("/{id}/status-history", get(status_history))
        .route("/{id}/linked-entity", get(resolve_linked_entity))
}

#[utoipa::path(
    get, path = "/api/v1/documents", tag = "document",
    params(DocumentQuery),
    responses(
        (status = 200, description = "Documents in the caller's tenant, newest first", body = [DocumentSummary]),
        (status = 403, description = "Missing document:read"),
        (status = 422, description = "A filter names a value that is not one of the allowed ones")
    ),
    security(("bearer" = []))
)]
async fn list_documents(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(query): QueryParams<DocumentQuery>,
) -> Result<Json<ListEnvelope<DocumentSummary>>, AppError> {
    let (documents, meta) = service::list_documents(&state, &caller, &query).await?;

    Ok(Json(ListEnvelope::new(documents, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}", tag = "document",
    responses(
        (status = 200, description = "The document with its metadata and its link identifiers", body = Document),
        (status = 404, description = "No such document")
    ),
    security(("bearer" = []))
)]
async fn get_document(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Document>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_document(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/documents", tag = "document",
    request_body = CreateDocumentRequest,
    responses(
        (status = 201, description = "Created as a draft, with its reference assigned and its form revision pinned", body = Document),
        (status = 403, description = "Missing document:create"),
        (status = 422, description = "Validation failed, the type does not exist, or the form data is not what its definition accepts")
    ),
    security(("bearer" = []))
)]
async fn create_document(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<Document>>), AppError> {
    let document = service::create_document(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(document))))
}

#[utoipa::path(
    put, path = "/api/v1/documents/{id}", tag = "document",
    request_body = UpdateDocumentRequest,
    responses(
        (status = 200, description = "Updated; a metadata object that is sent replaces the stored set", body = Document),
        (status = 404, description = "No such document"),
        (status = 409, description = "The document is no longer a draft"),
        (status = 422, description = "Validation failed, or the form data is not what its definition accepts")
    ),
    security(("bearer" = []))
)]
async fn update_document(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<UpdateDocumentRequest>,
) -> Result<Json<ItemEnvelope<Document>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_document(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/documents/{id}", tag = "document",
    responses(
        (status = 204, description = "The draft is discarded"),
        (status = 404, description = "No such document"),
        (status = 409, description = "The document is not a draft; cancel it through PUT /status instead")
    ),
    security(("bearer" = []))
)]
async fn delete_document(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<StatusCode, AppError> {
    service::delete_document(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/v1/documents/{id}/submission", tag = "document",
    responses(
        (status = 200, description = "Submitted, with its number assigned and the server's own form data stored", body = Document),
        (status = 403, description = "Missing document:submit"),
        (status = 404, description = "No such document"),
        (status = 409, description = "The document is not a draft"),
        (status = 422, description = "The form data does not satisfy its definition, or the type has no numbering rule")
    ),
    security(("bearer" = []))
)]
async fn submit_document(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Document>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::submit_document(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    put, path = "/api/v1/documents/{id}/status", tag = "document",
    request_body = TransitionRequest,
    responses(
        (status = 200, description = "Moved; both ends are reported", body = TransitionResult),
        (status = 403, description = "Missing document:transition"),
        (status = 404, description = "No such document"),
        (status = 409, description = "The document changed while the transition was being applied"),
        (status = 422, description = "That transition is not legal from where the document is")
    ),
    security(("bearer" = []))
)]
async fn transition_document(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
    JsonBody(request): JsonBody<TransitionRequest>,
) -> Result<Json<ItemEnvelope<TransitionResult>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::transition(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/status-history", tag = "document",
    responses(
        (status = 200, description = "How the document got where it is, oldest first", body = [StatusHistoryEntry]),
        (status = 404, description = "No such document")
    ),
    security(("bearer" = []))
)]
async fn status_history(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<Vec<StatusHistoryEntry>>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::status_history(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/linked-entity", tag = "document",
    responses(
        (status = 200, description = "The linked record, resolved", body = ResolvedEntity),
        (status = 403, description = "Missing the linked entity's own read permission — a document does not open what the master-data surface does not"),
        (status = 404, description = "No such document, no link on it, or the linked record has been retired")
    ),
    security(("bearer" = []))
)]
async fn resolve_linked_entity(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<Json<ItemEnvelope<ResolvedEntity>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::resolve_link(&state, &caller, id).await?,
    )))
}
