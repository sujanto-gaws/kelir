//! Routes over document types (FR-DTYPE-001, 002, 003).
//!
//! Every route requires a token: taking [`Authenticated`] is what enforces it
//! (FR-API-008), and each handler's service names the permission it needs.
//!
//! The form and workflow bindings have no routes of their own. They are fields
//! and a collection on the type, which is what #157 means by one item rather
//! than two — a type that cannot name the form it renders is a type nothing can
//! use.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{
    CreateDocumentTypeRequest, DocumentType, DocumentTypeSummary, UpdateDocumentTypeRequest,
};
use super::numbering::{NumberingRule, SetNumberingRuleRequest};
use super::{numbering_service, service};
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_types).post(create_type))
        .route("/{id}", get(get_type).put(update_type).delete(delete_type))
        // The numbering rule is a sub-resource of the type rather than a
        // resource beside it: `uq_document_type_numbering_rules_active` allows
        // one active rule per type, so a type has a numbering rule or it does
        // not. `PUT` says that; a `POST` that conflicts the second time would
        // say it less honestly.
        .route(
            "/{id}/numbering-rule",
            get(get_numbering_rule)
                .put(set_numbering_rule)
                .delete(clear_numbering_rule),
        )
}

#[utoipa::path(
    get, path = "/api/v1/document-types/{id}/numbering-rule", tag = "document-type",
    responses(
        (status = 200, description = "The active numbering rule", body = NumberingRule),
        (status = 404, description = "No such document type, or it has no numbering rule")
    ),
    security(("bearer" = []))
)]
async fn get_numbering_rule(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<NumberingRule>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        numbering_service::get_rule(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    put, path = "/api/v1/document-types/{id}/numbering-rule", tag = "document-type",
    request_body = SetNumberingRuleRequest,
    responses(
        (status = 200, description = "The rule now in force; the previous one is kept, deactivated", body = NumberingRule),
        (status = 404, description = "No such document type"),
        (status = 422, description = "The template is malformed, or the counter would be rewound past a number already issued")
    ),
    security(("bearer" = []))
)]
async fn set_numbering_rule(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<SetNumberingRuleRequest>,
) -> Result<Json<ItemEnvelope<NumberingRule>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        numbering_service::set_rule(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/document-types/{id}/numbering-rule", tag = "document-type",
    responses(
        (status = 204, description = "Deactivated; documents of this type can no longer be numbered"),
        (status = 404, description = "No such document type, or it has no numbering rule")
    ),
    security(("bearer" = []))
)]
async fn clear_numbering_rule(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    numbering_service::clear_rule(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/api/v1/document-types", tag = "document-type",
    params(Pagination),
    responses(
        (status = 200, description = "Document types, without their workflow bindings", body = [DocumentTypeSummary]),
        (status = 403, description = "Missing document-type:read")
    ),
    security(("bearer" = []))
)]
async fn list_types(
    State(state): State<AppState>,
    caller: Authenticated,
    Query(pagination): Query<Pagination>,
) -> Result<Json<ListEnvelope<DocumentTypeSummary>>, AppError> {
    let (types, meta) = service::list_types(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(types, meta)))
}

#[utoipa::path(
    get, path = "/api/v1/document-types/{id}", tag = "document-type",
    responses(
        (status = 200, description = "The document type with its bindings", body = DocumentType),
        (status = 404, description = "No such document type")
    ),
    security(("bearer" = []))
)]
async fn get_type(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<Json<ItemEnvelope<DocumentType>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::get_type(&state, &caller, id).await?,
    )))
}

#[utoipa::path(
    post, path = "/api/v1/document-types", tag = "document-type",
    request_body = CreateDocumentTypeRequest,
    responses(
        (status = 201, description = "Created", body = DocumentType),
        (status = 409, description = "That typeCode is already in use"),
        (status = 422, description = "Validation failed, or a binding names something that does not exist")
    ),
    security(("bearer" = []))
)]
async fn create_type(
    State(state): State<AppState>,
    caller: Authenticated,
    JsonBody(request): JsonBody<CreateDocumentTypeRequest>,
) -> Result<(StatusCode, Json<ItemEnvelope<DocumentType>>), AppError> {
    let document_type = service::create_type(&state, &caller, request).await?;

    Ok((StatusCode::CREATED, Json(ItemEnvelope::new(document_type))))
}

#[utoipa::path(
    put, path = "/api/v1/document-types/{id}", tag = "document-type",
    request_body = UpdateDocumentTypeRequest,
    responses(
        (status = 200, description = "Updated; a collection that is sent replaces the stored set", body = DocumentType),
        (status = 404, description = "No such document type"),
        (status = 422, description = "Validation failed, or a binding names something that does not exist")
    ),
    security(("bearer" = []))
)]
async fn update_type(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
    JsonBody(request): JsonBody<UpdateDocumentTypeRequest>,
) -> Result<Json<ItemEnvelope<DocumentType>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::update_type(&state, &caller, id, request).await?,
    )))
}

#[utoipa::path(
    delete, path = "/api/v1/document-types/{id}", tag = "document-type",
    responses(
        (status = 204, description = "Retired; the type is soft-deleted"),
        (status = 404, description = "No such document type"),
        (status = 409, description = "Documents were created from this type; deprecate it instead")
    ),
    security(("bearer" = []))
)]
async fn delete_type(
    State(state): State<AppState>,
    caller: Authenticated,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    service::delete_type(&state, &caller, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
