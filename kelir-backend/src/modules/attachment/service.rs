//! Attaching a file to a document (FR-ATT-001, FR-ATT-003; [#244]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use axum::body::Bytes;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::domain::{self, Attachment, MAX_FILE_NAME};
use super::repository as repo;
use super::{ATTACHMENT_CREATE, ATTACHMENT_OBJECT_TYPE};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document::service::document as document_service;
use crate::state::AppState;

/// One file, as it arrived out of the multipart body.
pub struct UploadedFile {
    pub original_file_name: String,
    /// What the caller *said* it is. Recorded, not trusted — validating by
    /// content is [#245](https://github.com/sujanto-gaws/kelir/issues/245) AC4,
    /// and the two disagree exactly when it matters.
    pub declared_mime_type: String,
    pub bytes: Bytes,
    pub description: Option<String>,
}

/// Stores a file against a document, then records it.
///
/// # The order is the decision, and it is the one that can be recovered from
///
/// **The object is written first and the row second** ([#244] AC2). No
/// transaction spans object storage and PostgreSQL, so one of the two failures
/// is possible and the choice is which:
///
/// * *Row without object* — a download that answers 500 to somebody who did
///   nothing wrong, on a document that says it has an attachment. Nothing on any
///   screen can distinguish it from a bug, and the person who uploaded the file
///   believes it is there.
/// * *Object without row* — bytes nobody can reach, costing storage. Invisible
///   to every caller, identifiable by the absence of its row, and deletable by a
///   sweep whenever one is written.
///
/// The second is the recoverable one, so the second is the one this order
/// allows. **The sweep is not in this sprint** and is named here rather than
/// assumed: `storage_reference` is derived from ids this row would have carried,
/// so an orphan is findable by listing the prefix and asking which have no row.
///
/// # What is checked before anything is written
///
/// **The document is read through its own module's service** (coding standard
/// §2.2), which requires `document:read` and answers 404 for a document that is
/// not this tenant's, is deleted, or does not exist — so [#244] AC5's *the
/// refusal does not confirm the document exists* is satisfied by reusing the
/// answer the document surface already gives rather than by a second rule beside
/// it.
///
/// Two permissions, not one: `attachment:create` is *may this account attach
/// files at all*, and `document:read` is *may it see this document*. A caller
/// holding the first and not the second can attach nothing anywhere, which is
/// the correct reading of an attachment being as private as what it hangs on.
pub async fn upload(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    file: UploadedFile,
) -> Result<Attachment, AppError> {
    caller.require(ATTACHMENT_CREATE)?;

    // 404 for a document this caller may not see, before a byte is stored.
    let document = document_service::get_document(state, caller, document_id).await?;

    if file.bytes.is_empty() {
        return Err(domain::empty_file());
    }

    let name_length = file.original_file_name.chars().count();

    if name_length > MAX_FILE_NAME {
        return Err(domain::file_name_too_long(name_length));
    }

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();
    let id = Uuid::now_v7();
    let file_name = domain::safe_file_name(&file.original_file_name);
    let reference = domain::storage_reference(tenant_id, document.id, id, &file_name);

    // **A checksum over the bytes as stored**, so a later read can tell a
    // corrupted object from a different one. `sha256:` prefixed because §8.2's
    // own example is, and an unprefixed digest is a digest whose algorithm is a
    // guess.
    let checksum = format!("sha256:{:x}", Sha256::digest(&file.bytes));

    // `i64` because the column is `BIGINT` and the cast is the one place a file
    // larger than 8 exabytes would be a problem, which is not a problem.
    let file_size = i64::try_from(file.bytes.len()).unwrap_or(i64::MAX);

    // The object first. See this function's own documentation.
    state.storage.put(&reference, file.bytes).await?;

    repo::insert_attachment(
        &state.pool,
        &repo::NewAttachment {
            id,
            tenant_id,
            document_id: document.id,
            file_name: &file_name,
            original_file_name: &file.original_file_name,
            mime_type: &file.declared_mime_type,
            file_size,
            checksum: &checksum,
            storage_reference: &reference,
            description: file.description.as_deref(),
            created_by: Some(actor),
        },
    )
    .await?;

    let stored = repo::find_attachment(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("attachment {id} vanished after it was written"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Attachment.Added",
            action: "CREATE",
            object_type: ATTACHMENT_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(actor),
            ip_address: None,
            // **Not the description**, which is prose somebody wrote about
            // somebody else's document — the line **D-12** and **D-32** drew for
            // the decision comment, applied to the one free-text field this
            // surface accepts.
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "documentId": document.id,
                "originalFileName": stored.original_file_name,
                "mimeType": stored.mime_type,
                "fileSize": stored.file_size,
                "checksum": stored.checksum,
                "virusScanStatus": stored.virus_scan_status,
            })),
        },
    )
    .await;

    Ok(stored)
}
