//! The upload route (FR-ATT-001, FR-ATT-003; [#244]).
//!
//! Mounted under `/api/v1/documents/{document_id}/attachments`, because an
//! attachment has no life of its own: it exists against a document, it is as
//! private as that document, and a top-level `/attachments` would be a surface
//! whose first question is always *which document is this about*.
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use axum::extract::{Multipart, State};
use axum::routing::post;
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{self, Attachment};
use super::service::{self, UploadedFile};
use crate::error::AppError;
use crate::extract::{MultipartBody, PathParam};
use crate::middleware::auth::Authenticated;
use crate::response::ItemEnvelope;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", post(upload_attachment))
}

#[utoipa::path(
    post, path = "/api/v1/documents/{id}/attachments", tag = "attachment",
    request_body(content = String, description = "multipart/form-data with a `file` part and an optional `description`", content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "The stored attachment. `virusScanStatus` is PENDING and stays there until the scan runs", body = Attachment),
        (status = 403, description = "Missing attachment:create or document:read"),
        (status = 415, description = "The body is not multipart/form-data"),
        (status = 404, description = "No such document, or it is not one this caller may see"),
        (status = 422, description = "No `file` part, an empty file, or a file name over the limit")
    ),
    security(("bearer" = []))
)]
pub async fn upload_attachment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    MultipartBody(multipart): MultipartBody,
) -> Result<Json<ItemEnvelope<Attachment>>, AppError> {
    let file = read_file_part(multipart).await?;

    Ok(Json(ItemEnvelope::new(
        service::upload(&state, &caller, document_id, file).await?,
    )))
}

/// Pulls the one file part and the optional description out of the body.
///
/// **A malformed multipart body is a 422 and not a 500.** `Multipart::next_field`
/// fails on a body that is not what the content type promised, which is a
/// property of what the caller sent — so it is reported as such rather than
/// surfacing as an internal error with the parser's wording in it.
///
/// **Parts other than `file` and `description` are ignored** rather than
/// refused. A browser form gains fields for reasons that have nothing to do with
/// this API, and rejecting an unrecognised part would make this route fail on a
/// page it does not control. The `deny_unknown_fields` instinct that governs
/// this project's JSON bodies is right there and wrong here: a JSON body is a
/// contract, and a form post is a browser's rendering of one.
async fn read_file_part(mut multipart: Multipart) -> Result<UploadedFile, AppError> {
    let mut file: Option<UploadedFile> = None;
    let mut description: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|error| {
        tracing::debug!(%error, "a multipart body could not be read");

        domain::no_file_part()
    })? {
        match field.name() {
            Some("file") => {
                let original_file_name = field.file_name().unwrap_or_default().to_owned();
                let declared_mime_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_owned();

                let bytes = field.bytes().await.map_err(|error| {
                    tracing::debug!(%error, "a multipart file part could not be read");

                    domain::no_file_part()
                })?;

                file = Some(UploadedFile {
                    original_file_name,
                    declared_mime_type,
                    bytes,
                    description: None,
                });
            }
            Some("description") => {
                let text = field.text().await.unwrap_or_default();
                let trimmed = text.trim();

                // Absent rather than empty, so a form that always sends the
                // field does not store a row of blanks — `normalize_comment`'s
                // rule one module over.
                if !trimmed.is_empty() {
                    description = Some(trimmed.to_owned());
                }
            }
            _ => {}
        }
    }

    let mut file = file.ok_or_else(domain::no_file_part)?;
    file.description = description;

    Ok(file)
}
