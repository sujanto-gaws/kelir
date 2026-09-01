//! The notification centre's routes (FR-NTF-003; [#251]).
//!
//! **Mounted at `/api/v1/notifications`, with no subject in the path**, which
//! is the difference from comments and attachments: those hang on a document
//! and a top-level collection of them would have *which document* as its first
//! question. A notification hangs on **you**. There is no id in the path
//! because the caller's token is the id, and adding one would be inviting
//! somebody to put a different one there.
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{Notification, UnreadCount};
use super::service;
use crate::error::AppError;
use crate::extract::{PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_notifications))
        .route("/unread-count", get(unread_count))
        .route("/read", post(mark_all_read))
        .route("/{id}/read", post(mark_read))
}

#[utoipa::path(
    get, path = "/api/v1/notifications", tag = "notification",
    params(Pagination),
    responses(
        (status = 200, description = "The caller's own notifications, newest first", body = [Notification]),
        (status = 403, description = "Missing notification:read")
    ),
    security(("bearer" = []))
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    caller: Authenticated,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<Notification>>, AppError> {
    let (items, meta) = service::list_mine(&state, &caller, &pagination).await?;

    Ok(Json(ListEnvelope::new(items, meta)))
}

/// **Its own route rather than a field on the list**, because the badge is
/// asked for far more often than the page and by a client that wants neither
/// rows nor a page's worth of work to get one number.
#[utoipa::path(
    get, path = "/api/v1/notifications/unread-count", tag = "notification",
    responses(
        (status = 200, description = "How many of the caller's notifications are unread", body = UnreadCount),
        (status = 403, description = "Missing notification:read")
    ),
    security(("bearer" = []))
)]
pub async fn unread_count(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<UnreadCount>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::unread_count(&state, &caller).await?,
    )))
}

/// **`POST .../read` rather than `PATCH` on the notification**, because what a
/// caller is doing is not editing a row: `read_at` is not theirs to set to a
/// value of their choosing, and a body carrying one would be a body the server
/// has to refuse. The only readable state is *now*, and the only writer is the
/// person it belongs to.
#[utoipa::path(
    post, path = "/api/v1/notifications/{id}/read", tag = "notification",
    responses(
        (status = 204, description = "Marked read; the same answer if it already was (AC5)"),
        (status = 403, description = "Missing notification:read"),
        (status = 404, description = "No such notification of the caller's")
    ),
    security(("bearer" = []))
)]
pub async fn mark_read(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(id): PathParam<Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    service::mark_read(&state, &caller, id).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Clears the badge, and answers with what is left rather than what was cleared.
#[utoipa::path(
    post, path = "/api/v1/notifications/read", tag = "notification",
    responses(
        (status = 200, description = "How many are unread now, which is 0 unless one arrived meanwhile", body = UnreadCount),
        (status = 403, description = "Missing notification:read")
    ),
    security(("bearer" = []))
)]
pub async fn mark_all_read(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<UnreadCount>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::mark_all_read(&state, &caller).await?,
    )))
}
