//! The comment routes (FR-CMT-001 to FR-CMT-004; [#249], [#253]).
//!
//! Mounted under `/api/v1/documents/{id}/comments`, beside the attachments the
//! same document carries and for the same reason: a comment has no life of its
//! own, and a top-level `/comments` would be a surface whose first question is
//! always *which document is this about*.
//!
//! **The edit and the delete keep that nesting**, where
//! [the concept document](../../../../docs/concepts/02.%20Handling%20Attachments%20Comments%20and%20Activity%20Log.md)
//! §12 sketched them as `PATCH /comments/{id}` and `DELETE /comments/{id}`. Two
//! departures, both deliberate: the path carries the document because *which
//! conversation* is the question every one of these routes has to answer before
//! it can check a permission, and the verb is **PUT** because that is the update
//! verb everywhere else in this product — a comment's whole representation is
//! its body, so there is nothing for a `PATCH` to be partial about.
//!
//! **A reply has no route of its own.** It is a `POST` to this collection
//! carrying `parentCommentId`, which is §12.3's shape and is what one-level
//! threading makes honest: nothing about writing a reply differs from writing a
//! comment.
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249
//! [#253]: https://github.com/sujanto-gaws/kelir/issues/253

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{AddCommentRequest, Comment, EditCommentRequest};
use super::service;
use crate::error::AppError;
use crate::extract::{JsonBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_comments).post(add_comment))
        .route("/{comment_id}", put(edit_comment).delete(delete_comment))
}

#[utoipa::path(
    post, path = "/api/v1/documents/{id}/comments", tag = "comment",
    request_body = AddCommentRequest,
    responses(
        (status = 200, description = "The stored comment, or reply", body = Comment),
        (status = 403, description = "Missing comment:create or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see"),
        (status = 422, description = "An empty body, one over the length bound, or a parentCommentId this document does not hold or that names a reply")
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
    put, path = "/api/v1/documents/{id}/comments/{comment_id}", tag = "comment",
    request_body = EditCommentRequest,
    responses(
        (status = 200, description = "The comment as it now reads, with editedAt stamped", body = Comment),
        (status = 403, description = "Missing comment:update or document:read, or the comment is somebody else's"),
        (status = 404, description = "No such document or comment, or one this caller may not see; a deleted comment is not editable"),
        (status = 422, description = "An empty body, or one over the length bound")
    ),
    security(("bearer" = []))
)]
pub async fn edit_comment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((document_id, comment_id)): PathParam<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<EditCommentRequest>,
) -> Result<Json<ItemEnvelope<Comment>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::edit_comment(&state, &caller, document_id, comment_id, request).await?,
    )))
}

/// **204 and no body**, which is `delete_document`'s answer to the same
/// question. There is nothing to return: a deleted comment is either a tombstone
/// the list will serve or a row the surface no longer admits, and which of those
/// it is depends on replies this caller can simply read.
#[utoipa::path(
    delete, path = "/api/v1/documents/{id}/comments/{comment_id}", tag = "comment",
    responses(
        (status = 204, description = "Deleted. It stays in the conversation as a tombstone while it has replies"),
        (status = 403, description = "Missing comment:delete or document:read, or the comment is somebody else's"),
        (status = 404, description = "No such document or comment, or one this caller may not see")
    ),
    security(("bearer" = []))
)]
pub async fn delete_comment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((document_id, comment_id)): PathParam<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    service::delete_comment(&state, &caller, document_id, comment_id).await?;

    Ok(StatusCode::NO_CONTENT)
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
