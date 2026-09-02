//! The upload route (FR-ATT-001, FR-ATT-003; [#244]).
//!
//! Mounted under `/api/v1/documents/{document_id}/attachments`, because an
//! attachment has no life of its own: it exists against a document, it is as
//! private as that document, and a top-level `/attachments` would be a surface
//! whose first question is always *which document is this about*.
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;

use super::domain::{self, AddReferenceRequest, Attachment, AttachmentCategory, ExternalReference};
use super::service::{self, UploadedFile};
use crate::error::AppError;
use crate::extract::{JsonBody, MultipartBody, PathParam, QueryParams};
use crate::middleware::auth::Authenticated;
use crate::response::{ItemEnvelope, ListEnvelope, Pagination};
use crate::state::AppState;

/// The routes, and the one layer that is a security control rather than a
/// convenience.
///
/// **`DefaultBodyLimit` is how [#245] AC3 is satisfied**, and it is why the
/// limit is a router concern rather than a service one: the layer refuses a body
/// larger than the deployment accepts **before any of it is read**, where a
/// check inside the handler could only measure what had already arrived. A limit
/// on bytes you are holding is a limit on your disk.
///
/// It is applied to the upload route alone. Axum's global default is 2 MB, which
/// every JSON route in this product wants and this one does not.
///
/// [#245]: https://github.com/sujanto-gaws/kelir/issues/245
pub fn routes(max_upload_bytes: usize) -> Router<AppState> {
    Router::new()
        .route(
            "/",
            post(upload_attachment).layer(DefaultBodyLimit::max(max_upload_bytes)),
        )
        .route("/", get(list_attachments))
        .route(
            "/{attachment_id}",
            get(download_attachment).delete(delete_attachment),
        )
}

/// The external-reference routes, mounted at
/// `/api/v1/documents/{id}/references` (FR-ATT-010; [#254]).
///
/// **A sibling of `/attachments` rather than a shape inside it.** A reference is
/// not a file — no bytes, no scan, no download — and a surface that served both
/// through one collection would have to answer *which of these can I download*
/// with a field rather than with a route. **D-53** carries the argument; this
/// mount is what it looks like from outside.
///
/// [#254]: https://github.com/sujanto-gaws/kelir/issues/254
pub fn reference_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_references).post(add_reference))
        .route("/{reference_id}", axum::routing::delete(delete_reference))
}

/// The category list, mounted at `/api/v1/attachment-categories` (FR-ATT-006).
pub fn category_routes() -> Router<AppState> {
    Router::new().route("/", get(list_categories))
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
    let file = read_file_part(multipart, state.config.storage_max_upload_bytes).await?;

    Ok(Json(ItemEnvelope::new(
        service::upload(&state, &caller, document_id, file).await?,
    )))
}

/// Turns a multipart failure into the refusal it actually is.
///
/// **A body over the limit and a body that is malformed arrive the same way**,
/// as a `MultipartError`, and only its status tells them apart.
/// `DefaultBodyLimit` refuses while the body is being read, so the error can
/// surface from **either** `next_field` or `bytes` depending on where the read
/// stopped — which is why this is one function called from both places rather
/// than a check at the point that seemed likely. It was written at the likely
/// point first, and the test for it failed with `FILE_REQUIRED`: somebody who
/// sent an oversized file was told they had forgotten to attach one, which is
/// [#245] AC6 exactly backwards.
///
/// [#245]: https://github.com/sujanto-gaws/kelir/issues/245
fn multipart_failure(error: axum::extract::multipart::MultipartError, limit: usize) -> AppError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        tracing::debug!(%error, %limit, "an upload was over the limit");

        return domain::file_too_large(limit);
    }

    tracing::debug!(%error, "a multipart body could not be read");

    domain::no_file_part()
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
async fn read_file_part(
    mut multipart: Multipart,
    max_upload_bytes: usize,
) -> Result<UploadedFile, AppError> {
    let mut file: Option<UploadedFile> = None;
    let mut description: Option<String> = None;
    let mut category_id: Option<Uuid> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| multipart_failure(error, max_upload_bytes))?
    {
        match field.name() {
            Some("file") => {
                let original_file_name = field.file_name().unwrap_or_default().to_owned();
                let declared_mime_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_owned();

                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| multipart_failure(error, max_upload_bytes))?;

                file = Some(UploadedFile {
                    original_file_name,
                    declared_mime_type,
                    bytes,
                    description: None,
                    category_id: None,
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
            // **A categoryId that is not a uuid is refused rather than
            // dropped** ([#254] AC1). Every other unrecognised part is ignored
            // — a browser form gains fields for its own reasons — but this one
            // was *sent*, and a file filed under nothing because the id was
            // malformed is the silent drop `extract`'s own header exists to
            // stop.
            Some("categoryId") => {
                let text = field.text().await.unwrap_or_default();
                let trimmed = text.trim();

                if !trimmed.is_empty() {
                    category_id =
                        Some(trimmed.parse::<Uuid>().map_err(|_| {
                            AppError::bad_request("`categoryId` is not a valid uuid")
                        })?);
                }
            }
            _ => {}
        }
    }

    let mut file = file.ok_or_else(domain::no_file_part)?;
    file.description = description;
    file.category_id = category_id;

    Ok(file)
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/attachments", tag = "attachment",
    params(Pagination),
    responses(
        (status = 200, description = "The document's attachments, newest first, each with its scan status", body = [Attachment]),
        (status = 403, description = "Missing attachment:read or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see")
    ),
    security(("bearer" = []))
)]
pub async fn list_attachments(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<Attachment>>, AppError> {
    let (attachments, meta) =
        service::list_attachments(&state, &caller, document_id, &pagination).await?;

    Ok(Json(ListEnvelope::new(attachments, meta)))
}

/// The bytes.
///
/// **`Content-Disposition: attachment`, always.** Serving caller-supplied bytes
/// inline lets an uploaded HTML or SVG file run as script on this origin, which
/// is a stored cross-site scripting hole with the product's own session behind
/// it. The allow-list makes that hard to reach and this makes it not worth
/// reaching — two independent controls, because neither wants to be the only
/// one.
///
/// The file name is the one that was uploaded, quoted, with quotes and control
/// characters stripped: a header value is a place a caller-controlled string can
/// inject a second header.
#[utoipa::path(
    get, path = "/api/v1/documents/{id}/attachments/{attachment_id}", tag = "attachment",
    responses(
        (status = 200, description = "The file", content_type = "application/octet-stream"),
        (status = 403, description = "Missing attachment:read or document:read"),
        (status = 404, description = "No such document or attachment, or not one this caller may see"),
        (status = 409, description = "The scan has not cleared this file: PENDING, INFECTED or FAILED")
    ),
    security(("bearer" = []))
)]
pub async fn download_attachment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((document_id, attachment_id)): PathParam<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    let stored = service::download(&state, &caller, document_id, attachment_id).await?;

    let disposition = format!(
        "attachment; filename=\"{}\"",
        stored
            .original_file_name
            .chars()
            .filter(|character| !character.is_control() && *character != '"')
            .collect::<String>()
    );

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, stored.mime_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        stored.bytes,
    )
        .into_response())
}

/// **204 and no body**, which is `delete_document`'s answer to the same
/// question. A deleted attachment leaves the list and the download refuses it;
/// there is nothing left to return.
#[utoipa::path(
    delete, path = "/api/v1/documents/{id}/attachments/{attachment_id}", tag = "attachment",
    responses(
        (status = 204, description = "Deleted. The stored object is kept — the delete is soft (D-52)"),
        (status = 403, description = "Missing attachment:delete or document:read, or the file is somebody else's upload"),
        (status = 404, description = "No such document or attachment, or one this caller may not see")
    ),
    security(("bearer" = []))
)]
pub async fn delete_attachment(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((document_id, attachment_id)): PathParam<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    service::delete_attachment(&state, &caller, document_id, attachment_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/api/v1/documents/{id}/references", tag = "attachment",
    request_body = AddReferenceRequest,
    responses(
        (status = 200, description = "The stored reference. It has no size, no scan status and no download", body = ExternalReference),
        (status = 403, description = "Missing attachment:reference or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see"),
        (status = 422, description = "An empty label, a URL that is neither http nor https, or a categoryId this tenant does not have")
    ),
    security(("bearer" = []))
)]
pub async fn add_reference(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    JsonBody(request): JsonBody<AddReferenceRequest>,
) -> Result<Json<ItemEnvelope<ExternalReference>>, AppError> {
    Ok(Json(ItemEnvelope::new(
        service::add_reference(&state, &caller, document_id, request).await?,
    )))
}

#[utoipa::path(
    get, path = "/api/v1/documents/{id}/references", tag = "attachment",
    params(Pagination),
    responses(
        (status = 200, description = "The document's external references, newest first", body = [ExternalReference]),
        (status = 403, description = "Missing attachment:read or document:read"),
        (status = 404, description = "No such document, or it is not one this caller may see")
    ),
    security(("bearer" = []))
)]
pub async fn list_references(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam(document_id): PathParam<Uuid>,
    QueryParams(pagination): QueryParams<Pagination>,
) -> Result<Json<ListEnvelope<ExternalReference>>, AppError> {
    let (references, meta) =
        service::list_references(&state, &caller, document_id, &pagination).await?;

    Ok(Json(ListEnvelope::new(references, meta)))
}

#[utoipa::path(
    delete, path = "/api/v1/documents/{id}/references/{reference_id}", tag = "attachment",
    responses(
        (status = 204, description = "Removed"),
        (status = 403, description = "Missing attachment:delete or document:read, or the reference is somebody else's"),
        (status = 404, description = "No such document or reference, or one this caller may not see")
    ),
    security(("bearer" = []))
)]
pub async fn delete_reference(
    State(state): State<AppState>,
    caller: Authenticated,
    PathParam((document_id, reference_id)): PathParam<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    service::delete_reference(&state, &caller, document_id, reference_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// **Not paginated**, deliberately: this is a tenant's own vocabulary, it is
/// four rows in a fresh deployment, and a picker that arrives one page at a time
/// is a picker missing options.
#[utoipa::path(
    get, path = "/api/v1/attachment-categories", tag = "attachment",
    responses(
        (status = 200, description = "Every category this tenant can file something under, system rows first", body = [AttachmentCategory]),
        (status = 403, description = "Missing attachment:read")
    ),
    security(("bearer" = []))
)]
pub async fn list_categories(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ListEnvelope<AttachmentCategory>>, AppError> {
    let categories = service::list_categories(&state, &caller).await?;
    let total = categories.len() as u64;

    Ok(Json(ListEnvelope::new(
        categories,
        Pagination::default().meta(total),
    )))
}
