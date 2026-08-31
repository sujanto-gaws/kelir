//! The comment routes (FR-CMT-001; [#249]).
//!
//! Mounted under `/api/v1/documents/{id}/comments`, beside the attachments the
//! same document carries and for the same reason: a comment has no life of its
//! own, and a top-level `/comments` would be a surface whose first question is
//! always *which document is this about*.
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{AddCommentRequest, Comment};
use super::service;
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(list_comments).post(add_comment))
}

#[utoipa::path(
    post, path = "/api/v1/documents/{id}/comments", tag = "comment",
    request_body = AddCommentRequest,
    responses(
        (status = 200, description = "The stored comment", body = Comment),
        (status = 403, description = "Missing comment:create or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see"),
        (status = 422, description = "An empty body, or one over the length bound")
    ),
    security(("bearer" = []))
)]
pub async fn add_comment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    JsonBody(request): JsonBody<AddCommentRequest>,
) -> Result<Json<ItemEnvelope<Comment>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::add_comment(&state, &caller, document_id, request).await?,
    )))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/comments", tag = "comment",
    params(Pagination),
    responses(
        (status = 200, description = "The document's comments, oldest first", body = [Comment]),
        (status = 403, description = "Missing comment:read or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see")
    ),
    security(("bearer" = []))
)]
pub async fn list_comments(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<Comment>>, AppError> {
    let (comments, meta) =
        service::list_comments(&state, &caller, document_id, &pagination).await?;

    Ok(Json(ListEnvelope::new(comments, meta)))
}
