//! Attaching a file to a document (FR-ATT-001, FR-ATT-003; [#244]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use axum::body::Bytes;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::domain::{self, Attachment, VirusScanStatus, MAX_FILE_NAME};
use super::repository as repo;
use super::{ATTACHMENT_CREATE, ATTACHMENT_OBJECT_TYPE, ATTACHMENT_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document::service::document as document_service;
use crate::response::{PageMeta, Pagination};
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

    // **What the bytes are, not what the caller called them** (#245 AC4). The
    // declared `Content-Type` is still recorded below, because what a client
    // claimed is a fact about the request; it is simply not the fact this
    // decision is made on.
    //
    // The size limit is not checked here and that is not an omission: it is
    // enforced on the request body before any of it is read (#245 AC3), by the
    // layer `handlers::routes` puts on this route. A check at this point would
    // be a check on bytes already in hand.
    let detected = domain::detect_mime_type(&file.bytes);
    let allowed = &state.config.storage_allowed_mime_types;

    if !domain::type_is_allowed(detected, allowed) {
        return Err(domain::type_not_allowed(detected, allowed));
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

    // **The row and its event are one transaction; the object is not in it.**
    // #244 AC2 chose the object-first order and that is unchanged — what is new
    // is that the row and the timeline entry now stand or fall together
    // (#247 AC2, #248 AC3). The three states are: object only (recoverable, and
    // the failure this order allows), object plus row plus event, and nothing.
    // There is no state in which a file is recorded and its timeline is silent.
    let mut transaction = state.pool.begin().await?;

    repo::insert_attachment(
        &mut *transaction,
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

    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            // **The event links to what it describes** (#248 AC2), so a
            // timeline can offer the file rather than only mention it.
            attachment_id: Some(id),
            comment_id: None,
            event_type: "Attachment.Added",
            category: crate::modules::activity::domain::EventCategory::Attachment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Attached a file",
            // **Empty, and the `attachment_id` above is why** (#292, **D-45**).
            // This carried the original file name and the size until a caller
            // holding `activity:read` and no `attachment:read` was found
            // reading both off the timeline. A file name is routinely the
            // sensitive part — *2026-redundancy-list.pdf* needs no contents to
            // do damage — and this module's own header says an attachment is as
            // private as the document it hangs on. Its name has to be too.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

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
            ip_address: caller.ip_address(),
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

/// One attachment's bytes, and the three questions asked before they are served
/// ([#245]).
///
/// # The document decides, not the attachment id
///
/// **[#245] AC1 and AC2 are one statement, not two checks.**
/// `repository::find_stored_file` is scoped by tenant *and* by `document_id`, so
/// an attachment hanging on a document this caller cannot read is not found —
/// rather than found and then refused. The document itself is read first,
/// through its own module's service, which answers 404 for a document that is
/// not this tenant's, is deleted, or does not exist. **So the answer is the same
/// whether or not the attachment exists**, which is AC2, and it is the same
/// because there is nowhere for the two answers to differ.
///
/// # The scan gate is here, and it arrived one item early
///
/// [#246](https://github.com/sujanto-gaws/kelir/issues/246) AC2 and AC4 are the
/// download-side gate: refused unless `CLEAN`, enforced where the bytes are
/// served. The [construction plan](../../../../projects/planning/07.%20Sprint%2012%20Collaboration%20Construction%20Plan.md)
/// sequences that item after this one, and **taking its gate here is deliberate
/// rather than scope creep**: `modules::attachment`'s own documentation says an
/// attachment cannot be retrieved until the gate lands, nothing sets `CLEAN`
/// yet, and a download shipped without it would serve every unscanned byte in
/// the product while making that sentence false. #246 keeps the scanner, the
/// status transitions, the once-only move and the behaviour when the scanner is
/// unreachable.
///
/// **`PENDING`, `INFECTED` and `FAILED` are all refusals and all distinguishable**
/// (#246 AC2, AC3), because *not yet* and *never* need different things from the
/// person holding the file.
///
/// [#245]: https://github.com/sujanto-gaws/kelir/issues/245
pub async fn download(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    attachment_id: Uuid,
) -> Result<StoredBytes, AppError> {
    caller.require(ATTACHMENT_READ)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let stored =
        repo::find_stored_file(&state.pool, caller.tenant_id(), document.id, attachment_id)
            .await?
            .ok_or_else(|| AppError::not_found("Attachment"))?;

    if stored.virus_scan_status != VirusScanStatus::Clean {
        return Err(domain::not_yet_cleared(stored.virus_scan_status));
    }

    // **The event is written before the bytes are handed over, and a failure to
    // write it fails the download** (#248 AC1).
    //
    // A deliberate stance rather than an oversight of the *fail the action*
    // rule: if this product cannot record that somebody took a copy of a file,
    // it should not give them the copy. A download is the one read in the system
    // that takes information out of it, and a timeline with a gap exactly where
    // a copy was made is worse than a refused download.
    //
    // Its own transaction, because there is nothing else to join: the action
    // *is* the event.
    let mut transaction = state.pool.begin().await?;

    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id: caller.tenant_id(),
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: Some(attachment_id),
            comment_id: None,
            event_type: "Attachment.Downloaded",
            category: crate::modules::activity::domain::EventCategory::Attachment,
            actor_user_id: Some(caller.user_id()),
            actor_name: Some(caller.username()),
            action_summary: "Downloaded a file",
            // The name goes for the reason it goes above, and **the event is
            // not weaker for it**: that somebody took a copy is the whole point
            // of this row, and the actor, the time and the link all survive
            // (#292, **D-45**).
            details: serde_json::json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    let bytes = state.storage.get(&stored.storage_reference).await?;

    Ok(StoredBytes {
        original_file_name: stored.original_file_name,
        mime_type: stored.mime_type,
        bytes,
    })
}

/// What a download hands back to the handler.
pub struct StoredBytes {
    pub original_file_name: String,
    pub mime_type: String,
    pub bytes: axum::body::Bytes,
}

/// A document's attachments, newest first.
///
/// **Listed even while they are `PENDING`**, and the row says so. A file
/// somebody uploaded that vanishes from the list until a scanner clears it looks
/// like a lost upload; a file that is listed with its status is a file whose
/// state a person can see. What `PENDING` refuses is the *bytes*, in
/// [`download`].
pub async fn list_attachments(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<Attachment>, PageMeta), AppError> {
    caller.require(ATTACHMENT_READ)?;

    let document = document_service::get_document(state, caller, document_id).await?;
    let tenant_id = caller.tenant_id();

    let total = repo::count_for_document(&state.pool, tenant_id, document.id).await?;
    let attachments = repo::list_for_document(
        &state.pool,
        tenant_id,
        document.id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((attachments, pagination.meta(total.max(0) as u64)))
}
