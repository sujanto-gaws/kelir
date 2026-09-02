//! Attaching a file to a document, filing it, taking it away, and recording a
//! link to something this product does not hold (FR-ATT-001, FR-ATT-003,
//! FR-ATT-006, FR-ATT-009, FR-ATT-010; [#244], [#254]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244
//! [#254]: https://github.com/sujanto-gaws/kelir/issues/254

use axum::body::Bytes;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::domain::{
    self, AddReferenceRequest, Attachment, AttachmentCategory, ExternalReference, VirusScanStatus,
    MAX_FILE_NAME,
};
use super::repository as repo;
use super::{
    ATTACHMENT_CREATE, ATTACHMENT_DELETE, ATTACHMENT_OBJECT_TYPE, ATTACHMENT_READ,
    ATTACHMENT_REFERENCE, REFERENCE_OBJECT_TYPE,
};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::activity::domain::EventCategory;
use crate::modules::activity::service::{record as record_activity, Happening};
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
    /// What kind of thing it is (FR-ATT-006), if the person said. Validated
    /// against this tenant's categories before anything is stored.
    pub category_id: Option<Uuid>,
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

    // **Checked before the object is written, not after** ([#254] AC1). A
    // category this tenant does not have is a request that was never going to
    // commit, and finding that out after the bytes are in the bucket leaves the
    // orphan this function's own ordering exists to avoid.
    if let Some(category_id) = file.category_id {
        if !repo::category_exists(&state.pool, tenant_id, category_id).await? {
            return Err(domain::category_not_found());
        }
    }

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
            category_id: file.category_id,
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

    // **The object is read first, and then the event is written** — the order
    // [#293](https://github.com/sujanto-gaws/kelir/issues/293) corrected, and
    // both consequences belong in this comment because each of them is somebody
    // being told something false:
    //
    // * *Recorded and not delivered* — what this used to do. The event was
    //   committed and `storage.get` then failed, so the caller got a 500 and the
    //   timeline said they had downloaded the file. A record of a copy nobody
    //   took is evidence that will be relied on and is wrong.
    // * *Delivered and not recorded* — what reversing the order entirely would
    //   do, and it is the worse one. A false positive is a wasted question; a
    //   false negative is a copy nobody knows about.
    //
    // Reading the object before the event costs neither. The bytes are in this
    // process's memory and have gone nowhere; **the record is still written
    // before they are served**, which is #293 AC2 and the reason below, kept
    // verbatim because it is still the reason: if this product cannot record
    // that somebody took a copy of a file, it should not give them the copy. A
    // failure to write the event still fails the download.
    //
    // What remains, and is accepted: a response that dies on the wire after the
    // event is committed. Nothing on this side can know that, and over-recording
    // is the safe direction for *who has seen this file*.
    let bytes = state.storage.get(&stored.storage_reference).await?;

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

/// Deletes an attachment — softly, and **the bytes stay** (FR-ATT-009;
/// [#254] AC2, AC3, **D-52**, [ADR-0032]).
///
/// # What happens to the stored object is decided, and the answer is *nothing*
///
/// *Soft* and *the bytes are gone* cannot both be true, so this item picked one.
/// The object stays: a delete that removed it could not be undone, on a row that
/// still records what the file was called and what it hashed to, and the
/// question of when bytes actually leave a deployment is a **retention** one —
/// `attachments.retention_policy_id` is the column that will answer it, and
/// nothing writes it yet.
///
/// **It is the same shape [ADR-0030] took for a deleted comment, and the
/// opposite conclusion about what the reader sees.** A comment with replies
/// leaves a tombstone because answers hang from it; an attachment has nothing
/// hanging from it, so it leaves the list entirely. What survives is the
/// timeline: `Attachment.Added`, any `Attachment.Downloaded`, and now
/// `Attachment.Deleted`, which is the account of a file that no longer appears
/// anywhere else.
///
/// # AC3: the download refuses it, and not because this function said so
///
/// `repository::find_stored_file` carries `deleted_at IS NULL`, so a deleted
/// attachment is **not found** on the path that serves the bytes rather than
/// found and then refused. The gate is in the statement, which is
/// [#246](https://github.com/sujanto-gaws/kelir/issues/246)'s rule about where a
/// gate belongs, and it needs no second check here to hold.
///
/// [ADR-0030]: ../../../../docs/architectures/adr/0030.%20A%20Deleted%20Comment%20Leaves%20a%20Tombstone.md
/// [ADR-0032]: ../../../../docs/architectures/adr/0032.%20A%20Soft-Deleted%20Attachment%20Keeps%20Its%20Object.md
pub async fn delete_attachment(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    attachment_id: Uuid,
) -> Result<(), AppError> {
    caller.require(ATTACHMENT_DELETE)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();

    let mut transaction = state.pool.begin().await?;

    let existing = repo::lock_attachment(&mut transaction, tenant_id, document.id, attachment_id)
        .await?
        .ok_or_else(|| AppError::not_found("Attachment"))?;

    refuse_unless_uploader(existing.created_by, actor)?;

    if repo::soft_delete_attachment(
        &mut *transaction,
        tenant_id,
        document.id,
        attachment_id,
        Some(actor),
    )
    .await?
        == 0
    {
        // Unreachable: the row was found and locked under the same predicate.
        return Err(AppError::Internal {
            source: anyhow::anyhow!("attachment {attachment_id} was locked and then not deleted"),
        });
    }

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: Some(attachment_id),
            comment_id: None,
            event_type: "Attachment.Deleted",
            category: EventCategory::Attachment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Deleted a file",
            // **The link outlives the file, and the name still does not travel**
            // (**D-45**). A timeline entry pointing at a row the attachment
            // surface now answers 404 for is the correct record of a deletion:
            // that it happened is the document's history, and what the file was
            // called is behind `attachment:read`.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Attachment.Deleted",
            action: "DELETE",
            object_type: ATTACHMENT_OBJECT_TYPE,
            object_id: attachment_id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            reason: None,
            // **The name and the checksum, on the way out.** The upload recorded
            // both, and a deletion whose row says only *something was deleted*
            // makes the pair unmatchable — which is the one question an auditor
            // asks about a file that is no longer there.
            old_value: Some(json!({
                "documentId": document.id,
                "originalFileName": existing.original_file_name,
                "fileSize": existing.file_size,
                "checksum": existing.checksum,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Records a link to something that lives elsewhere (FR-ATT-010; [#254] AC4,
/// AC5, **D-53**, [ADR-0031]).
///
/// # A different permission, because it grants something different
///
/// `attachment:reference` rather than `attachment:create`: no bytes enter this
/// product, nothing is scanned, nothing is stored beyond a string — and the risk
/// is a link somebody else follows rather than a file this deployment holds. A
/// tenant that wants people to link and not upload can now say so.
///
/// # It is never scanned and never reads `CLEAN` (AC5)
///
/// **Held by the table rather than by this function.**
/// `document_external_references` has no `virus_scan_status` column, so there is
/// no value for a reference to borrow and nothing for the scan worker to find:
/// `pending_scans` reads `attachments`, and a reference is not one.
///
/// [ADR-0031]: ../../../../docs/architectures/adr/0031.%20An%20External%20Reference%20Is%20Not%20an%20Attachment%20Row.md
pub async fn add_reference(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    request: AddReferenceRequest,
) -> Result<ExternalReference, AppError> {
    caller.require(ATTACHMENT_REFERENCE)?;

    // Normalized before the document is read, for `add_comment`'s reason: a
    // `javascript:` URL is refused whatever the document turns out to be.
    let label = domain::normalize_label(request.label)?;
    let url = domain::normalize_url(request.url)?;
    let description = request
        .description
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();
    let id = Uuid::now_v7();

    if let Some(category_id) = request.category_id {
        if !repo::category_exists(&state.pool, tenant_id, category_id).await? {
            return Err(domain::category_not_found());
        }
    }

    let mut transaction = state.pool.begin().await?;

    repo::insert_reference(
        &mut *transaction,
        &repo::NewReference {
            id,
            tenant_id,
            document_id: document.id,
            label: &label,
            url: &url,
            description: description.as_deref(),
            category_id: request.category_id,
            created_by: Some(actor),
        },
    )
    .await?;

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            // **No link column, and it is not one this table can have.** The
            // four on `activity_events` name a workflow instance, a task, an
            // attachment and a comment; a reference is none of them, and adding
            // a fifth foreign key for a `Should` would widen an append-only
            // table for one event type. The entry says a reference was recorded,
            // and the Attachments tab is where it is read.
            attachment_id: None,
            comment_id: None,
            event_type: "Reference.Added",
            category: EventCategory::Attachment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Recorded a link to something outside this document",
            // **Not the URL and not the label** (**D-45**). A URL is the most
            // quotable thing on this row — it names a host, a path and often a
            // customer — and it is behind `attachment:read` like everything else
            // this module holds.
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    let stored = repo::find_reference(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("reference {id} vanished after it was written"),
        })?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Reference.Added",
            action: "CREATE",
            object_type: REFERENCE_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            // **The URL is here and nowhere else.** The audit trail is where a
            // deployment answers *what did somebody point this document at*, and
            // an entry recording that a link was added without recording which
            // link answers nothing. It is behind `audit:read` plus the
            // document's own read (**D-49**), which is the gate that makes this
            // a record rather than a disclosure.
            new_value: Some(json!({
                "documentId": document.id,
                "label": stored.label,
                "url": stored.url,
            })),
        },
    )
    .await;

    Ok(stored)
}

/// A document's external references, newest first.
pub async fn list_references(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<ExternalReference>, PageMeta), AppError> {
    // **`attachment:read`, not `attachment:reference`.** Recording a link and
    // reading what a document points at are different questions, and this is the
    // same one the attachment list asks: *may this account see what hangs on
    // this document*.
    caller.require(ATTACHMENT_READ)?;

    let document = document_service::get_document(state, caller, document_id).await?;
    let tenant_id = caller.tenant_id();

    let total = repo::count_references_for_document(&state.pool, tenant_id, document.id).await?;
    let references = repo::list_references_for_document(
        &state.pool,
        tenant_id,
        document.id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((references, pagination.meta(total.max(0) as u64)))
}

/// Removes a link. Soft, and there are no bytes to argue about.
pub async fn delete_reference(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    reference_id: Uuid,
) -> Result<(), AppError> {
    caller.require(ATTACHMENT_DELETE)?;

    let document = document_service::get_document(state, caller, document_id).await?;

    let tenant_id = caller.tenant_id();
    let actor = caller.user_id();

    let mut transaction = state.pool.begin().await?;

    let existing = repo::lock_reference(&mut transaction, tenant_id, document.id, reference_id)
        .await?
        .ok_or_else(|| AppError::not_found("Reference"))?;

    refuse_unless_uploader(existing.created_by, actor)?;

    if repo::soft_delete_reference(
        &mut *transaction,
        tenant_id,
        document.id,
        reference_id,
        Some(actor),
    )
    .await?
        == 0
    {
        return Err(AppError::Internal {
            source: anyhow::anyhow!("reference {reference_id} was locked and then not deleted"),
        });
    }

    record_activity(
        &mut transaction,
        &Happening {
            tenant_id,
            document_id: Some(document.id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: None,
            event_type: "Reference.Deleted",
            category: EventCategory::Attachment,
            actor_user_id: Some(actor),
            actor_name: Some(caller.username()),
            action_summary: "Removed a link to something outside this document",
            details: json!({}),
        },
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Reference.Deleted",
            action: "DELETE",
            object_type: REFERENCE_OBJECT_TYPE,
            object_id: reference_id,
            actor_user_id: Some(actor),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "documentId": document.id,
                "label": existing.label,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// The categories this tenant can file something under (FR-ATT-006).
///
/// **Behind `attachment:read`**, because the list is what a screen needs to
/// render a picker beside the upload, and an account that may not see a
/// document's attachments has nothing to file.
pub async fn list_categories(
    state: &AppState,
    caller: &Authenticated,
) -> Result<Vec<AttachmentCategory>, AppError> {
    caller.require(ATTACHMENT_READ)?;

    Ok(repo::list_categories(&state.pool, caller.tenant_id()).await?)
}

/// The half of *may I* that a permission cannot answer ([#254] AC2).
///
/// **The uploader, and nobody else** — `comment::service::refuse_unless_author`
/// one module over, and the same construction: `Some(actor)` on both sides, so a
/// row whose `created_by` is null (a user who has since been removed) compares
/// equal to nothing rather than to everyone.
///
/// A bare 403, the same one a missing permission produces. Whose file it is, is
/// something the list already told this caller.
fn refuse_unless_uploader(created_by: Option<Uuid>, actor: Uuid) -> Result<(), AppError> {
    if created_by == Some(actor) {
        return Ok(());
    }

    Err(AppError::Forbidden)
}
