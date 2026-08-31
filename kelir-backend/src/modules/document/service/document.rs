//! Creating a document, reading it, editing the draft, discarding it
//! (FR-DOC-001, 002, 005, 006; [#167]).
//!
//! # The form is pinned at creation, and that is what makes D-30 true
//!
//! A document stores `form_id` — the exact revision of the definition its type
//! was bound to **at the moment it was created**. That is what lets an old
//! document re-render against the definition it was actually filled in against
//! when the type is later re-pointed at a newer revision, which is #165's AC3
//! and the decision recorded as **D-30**.
//!
//! `document_type::service::guard_rebinding` refuses to move a type's binding
//! while any document of it pinned *nothing*, and its own doc comment says why
//! that population is the only one a rebinding can reach. **This service is what
//! keeps that population empty**: every document it creates pins whatever the
//! type binds. The one row that guard exists for can now only arrive from a type
//! that binds no form at all — which §6.2 permits — rather than from a document
//! that forgot.
//!
//! # Validated on every write, and `required` is not a refusal on a draft
//!
//! [#167]'s AC2 asks for form data to be validated server-side on every write
//! and not only at submit, because *a draft holding data its own form would
//! reject is a draft that cannot be submitted, discovered late*. Taken literally
//! that makes an empty draft unsaveable, so the line is drawn where the sentence
//! actually is — a value that is present and wrong is refused, a value that is
//! missing is unfinished — and the mechanism is
//! [`Strictness`][crate::modules::rad::service::evaluation::Strictness] on the
//! one evaluator rather than a second, laxer pipeline. That type is where the
//! three forgiven failure classes are listed with the reasoning for each.
//!
//! **The stored payload is the server's answer either way.** A draft's form data
//! has been through the projection, the `sequenceKey` overwrite, the
//! calculations and the conditional stripping exactly as a submission's has, so
//! a client that tampered with a computed total does not get to keep it in a
//! draft and submit it later.
//!
//! # What an audit record says about a JSON blob
//!
//! AC5 requires an update's record to name only the fields that moved, and asks
//! this service to state what a change to `form_data_json` records.
//!
//! > **It records the data keys whose values moved, and neither the old value
//! > nor the new one.**
//!
//! Two reasons, and the second is load-bearing. A form's data is arbitrary
//! tenant content — salaries, bank details, the medical grounds for a leave
//! request — and the audit trail is read through its own permission by people
//! who hold none over the document. **D-12** already refused to hand back a
//! record's field values through its change history without the record's own
//! read permission; copying every keystroke of every form into a table with a
//! different permission would be that finding, at scale, over data nobody
//! classified. The changed-key list is also what an auditor actually asks for —
//! *when did the amount last move, and who moved it* — and the values themselves
//! belong to `document_versions` behind the document's own permission when
//! FR-DOC-008 lands.
//!
//! Every other field audits normally, through [`ChangeSet::field`], with its
//! values.
//!
//! [#167]: https://github.com/sujanto-gaws/kelir/issues/167

use std::collections::BTreeSet;

use chrono::Utc;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::super::domain::{
    link, validate_create, validate_update, CreateDocumentRequest, Document, DocumentPriority,
    DocumentStatus, MetadataSet, UpdateDocumentRequest,
};
use super::super::repository::{self as repo, DocumentFields, LockedDocument, NewDocument};
use super::super::{DOCUMENT_CREATE, DOCUMENT_DELETE, DOCUMENT_READ, DOCUMENT_UPDATE, OBJECT_TYPE};
use super::form::{self, PinnedForm};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::modules::rad::service::evaluation::Strictness;
use crate::state::AppState;

pub async fn get_document(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Document, AppError> {
    caller.require(DOCUMENT_READ)?;

    repo::find_document(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))
}

pub async fn create_document(
    state: &AppState,
    caller: &Authenticated,
    request: CreateDocumentRequest,
) -> Result<Document, AppError> {
    caller.require(DOCUMENT_CREATE)?;
    validate_create(&request)?;

    let entity = link::check_pair(request.entity_type, request.entity_id)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let id = Uuid::now_v7();

    let mut transaction = state.pool.begin().await?;

    // Inside the transaction, under a lock, and *before* the insert — the same
    // ordering `document_type::service::create_type` states and for the same
    // reason. Locking the type row also serialises this creation against a
    // concurrent rebinding of it: `guard_rebinding` takes `FOR UPDATE` on the
    // same row, and the foreign key this insert takes would otherwise let a
    // document be created against a binding that moved a moment ago.
    let pinned = form::lock_pinned_form(&mut transaction, tenant_id, request.document_type_id)
        .await?
        .ok_or_else(|| {
            AppError::validation(vec![ValidationDetail::new(
                "documentTypeId",
                "reference",
                "NOT_FOUND",
                format!(
                    "no document type {} in this tenant",
                    request.document_type_id
                ),
            )])
        })?;

    if let Some(entity) = entity {
        check_entity(&mut transaction, tenant_id, entity).await?;
    }

    // The server's answer, never the client's — see the module documentation.
    let form_data = secure(
        &pinned,
        request.form_data.as_ref().unwrap_or(&empty_object()),
        Strictness::Draft,
    )?;

    let document_ref = repo::allocate_reference(
        &mut transaction,
        tenant_id,
        &repo::reference_key(Utc::now()),
    )
    .await?;

    let priority = request.priority.unwrap_or(DocumentPriority::Normal);

    repo::insert_document(
        &mut *transaction,
        &NewDocument {
            id,
            tenant_id,
            document_ref: &document_ref,
            document_type_id: request.document_type_id,
            // The pin. See the module documentation and **D-30**.
            form_id: pinned.form_id,
            title: request.title.trim(),
            form_data: &form_data,
            priority: priority.as_db(),
            entity_type: entity.map(|entity| entity.entity_type.as_db()),
            entity_id: entity.map(|entity| entity.entity_id),
            requested_for_department_id: request.requested_for_department_id,
            requested_for_facility_id: request.requested_for_facility_id,
            created_by: actor,
        },
    )
    .await
    .map_err(insert_error)?;

    if let Some(metadata) = &request.metadata {
        repo::replace_metadata(&mut transaction, tenant_id, id, metadata, actor).await?;
    }

    // A document begins its life at DRAFT, and the history says so rather than
    // starting at the first transition. A history whose first row is
    // `DRAFT -> SUBMITTED` cannot answer "when was this created and by whom"
    // from inside itself, which is what a history is for.
    repo::record_transition(
        &mut transaction,
        tenant_id,
        id,
        None,
        super::super::domain::DocumentStatus::Draft,
        actor,
        None,
    )
    .await?;

    // **In the same transaction as the document** (#247 AC2). A document that
    // rolled back did not happen, and a timeline saying it did would be worse
    // than one that never mentioned it.
    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id,
            document_id: Some(id),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: None,
            event_type: "Document.Created",
            category: crate::modules::activity::domain::EventCategory::Document,
            actor_user_id: actor,
            actor_name: Some(caller.username()),
            action_summary: "Created the document",
            details: serde_json::json!({ "documentTypeId": request.document_type_id }),
        },
    )
    .await?;

    transaction.commit().await?;

    // Read back before the record is written (#135).
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Document.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "documentRef": created.document_ref,
                "documentTypeId": created.document_type_id,
                "formId": created.form_id,
                "title": created.title,
                "status": created.status,
                // Keys, not values. See the module documentation.
                "formDataKeys": keys_of(&created.form_data),
                "metadataKeys": created.metadata.keys().collect::<Vec<_>>(),
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_document(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateDocumentRequest,
) -> Result<Document, AppError> {
    caller.require(DOCUMENT_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_document(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    let mut transaction = state.pool.begin().await?;

    // Read and held before anything below decides on it. AC4 asks for the
    // editable check to run in the same transaction as the write, under a lock
    // covering what it read — coding standard §2.5, and the shape #133 and #137
    // were filed for.
    let locked = repo::lock_document(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    refuse_unless_editable(&locked)?;

    // Both halves or neither, and both-null clears the link. Only checked when
    // the request is changing it: re-checking an unchanged link would refuse an
    // unrelated edit because a supplier was retired last month, which is a real
    // state whose fix is to change the link rather than to be unable to correct
    // a typo.
    let entity_change = match (request.entity_type, request.entity_id) {
        (None, None) => None,
        (entity_type, entity_id) => Some(link::check_pair(
            entity_type.flatten(),
            entity_id.flatten(),
        )?),
    };

    if let Some(Some(entity)) = entity_change {
        check_entity(&mut transaction, tenant_id, entity).await?;
    }

    let form_data = match &request.form_data {
        Some(submitted) => {
            let pinned = form::pinned_form_of(&mut transaction, tenant_id, &locked).await?;

            Some(secure(&pinned, submitted, Strictness::Draft)?)
        }
        None => None,
    };

    let affected = repo::update_document(
        &mut *transaction,
        tenant_id,
        id,
        &DocumentFields {
            title: request.title.as_deref().map(str::trim),
            form_data: form_data.as_ref(),
            priority: request.priority.map(DocumentPriority::as_db),
            entity_type: entity_change
                .map(|entity| entity.map(|entity| entity.entity_type.as_db())),
            entity_id: entity_change.map(|entity| entity.map(|entity| entity.entity_id)),
            requested_for_department_id: request.requested_for_department_id,
            requested_for_facility_id: request.requested_for_facility_id,
        },
        actor,
    )
    .await?;

    if affected == 0 {
        // Submitted or deleted between the locked read and the write. The
        // statement carries its own status predicate, so this is what that
        // answering zero looks like from here.
        return Err(not_editable(&locked));
    }

    if let Some(metadata) = &request.metadata {
        repo::replace_metadata(&mut transaction, tenant_id, id, metadata, actor).await?;
    }

    transaction.commit().await?;

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135).
    let mut changes = ChangeSet::new();
    changes.field("title", &before.title, &after.title);
    changes.field("priority", &before.priority, &after.priority);
    changes.field("entityType", &before.entity_type, &after.entity_type);
    changes.field("entityId", &before.entity_id, &after.entity_id);
    changes.field(
        "requestedForDepartmentId",
        &before.requested_for_department_id,
        &after.requested_for_department_id,
    );
    changes.field(
        "requestedForFacilityId",
        &before.requested_for_facility_id,
        &after.requested_for_facility_id,
    );

    // Keys, not values, and the same list on both halves — see the module
    // documentation for why the trail does not carry form data.
    let moved = moved_keys(&before.form_data, &after.form_data);
    if !moved.is_empty() {
        let moved = Value::from(moved.into_iter().collect::<Vec<_>>());
        changes.field("formData", &json!({}), &json!({ "changedKeys": moved }));
    }

    let moved_metadata = moved_metadata_keys(&before.metadata, &after.metadata);
    if !moved_metadata.is_empty() {
        changes.field(
            "metadata",
            &json!({}),
            &json!({ "changedKeys": moved_metadata.into_iter().collect::<Vec<_>>() }),
        );
    }

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Document.Updated",
            action: "UPDATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

/// Discards a draft.
///
/// **Soft, and drafts only.** A submitted document is *cancelled* rather than
/// deleted: the two answer different questions — "I opened this by mistake"
/// against "this request is withdrawn" — and the second has to leave a row an
/// auditor can find, which is [`super::status`]'s transition.
pub async fn delete_document(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(DOCUMENT_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_document(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    let mut transaction = state.pool.begin().await?;

    let locked = repo::lock_document(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    refuse_unless_discardable(&locked)?;

    if repo::soft_delete(&mut *transaction, tenant_id, id, actor).await? == 0 {
        return Err(not_discardable(&locked));
    }

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Document.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "documentRef": before.document_ref,
                "documentTypeId": before.document_type_id,
                "title": before.title,
                "status": before.status,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Runs the payload through the Tamper-Proof Pattern at the stated moment.
///
/// A document whose type binds **no** form has nothing to validate against, and
/// its form data is refused rather than stored unchecked: storing arbitrary JSON
/// under a column called `form_data_json` would be data no definition explains
/// and no submit could later accept.
pub fn secure(
    pinned: &PinnedForm,
    submitted: &Value,
    strictness: Strictness,
) -> Result<Value, AppError> {
    let Some(definition) = &pinned.definition else {
        if submitted.as_object().is_none_or(Map::is_empty) {
            return Ok(json!({}));
        }

        return Err(AppError::validation(vec![ValidationDetail::new(
            "formData",
            "reference",
            "NO_FORM_BOUND",
            "this document's type binds no form definition, so there is nothing to \
             validate its data against; bind a published form to the type first",
        )]));
    };

    crate::modules::rad::service::evaluation::secure_payload_as(
        &crate::modules::rad::evaluator::RuleEvaluator::new(),
        definition,
        submitted,
        strictness,
    )
    .map_err(AppError::validation)
}

/// Refuses a write to a document whose content is fixed (AC4).
///
/// **A returned document passes**, which is [#183] AC1: it was sent back to be
/// corrected, and one that could not be corrected would be a rejection with a
/// longer name.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
fn refuse_unless_editable(locked: &LockedDocument) -> Result<(), AppError> {
    if locked.status.is_editable() {
        return Ok(());
    }

    Err(not_editable(locked))
}

/// Refuses a delete of a document that is more than a draft.
///
/// **Separate from [`refuse_unless_editable`] since [#183]**, and the two now
/// differ on exactly one status. A returned document may be *edited* and may
/// not be *discarded*: it holds a number, a status history and a live process
/// waiting for it to come back, so deleting it would strand the instance that
/// returned it.
///
/// [#183]: https://github.com/sujanto-gaws/kelir/issues/183
fn refuse_unless_discardable(locked: &LockedDocument) -> Result<(), AppError> {
    if locked.status.is_discardable() {
        return Ok(());
    }

    Err(not_discardable(locked))
}

fn not_editable(locked: &LockedDocument) -> AppError {
    AppError::conflict(format!(
        "this document is {} and only a draft or a returned document can be \
         edited; a submitted document is moved through PUT /documents/{{id}}/status",
        locked.status.as_db()
    ))
}

fn not_discardable(locked: &LockedDocument) -> AppError {
    // `RETURNED` is named rather than lumped in with the rest: it is the one
    // status a caller has just been told it may *edit*, so "only a draft" on
    // its own reads as a contradiction.
    let extra = if locked.status == DocumentStatus::Returned {
        " — a returned document has a number and a process waiting for it, so it \
          is corrected and sent again, or cancelled"
    } else {
        ""
    };

    AppError::conflict(format!(
        "this document is {} and only a draft can be discarded; a submitted \
         document is withdrawn through PUT /documents/{{id}}/status{extra}",
        locked.status.as_db()
    ))
}

async fn check_entity(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entity: super::super::domain::EntityLink,
) -> Result<(), AppError> {
    if repo::lock_linked_entity(transaction, tenant_id, entity).await? {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "entityId",
        "reference",
        "NOT_FOUND",
        format!(
            "no {} {} in this tenant",
            entity.entity_type.missing().to_lowercase(),
            entity.entity_id
        ),
    )]))
}

fn empty_object() -> Value {
    json!({})
}

/// The top-level data keys a payload carries.
fn keys_of(form_data: &Value) -> Vec<&String> {
    form_data
        .as_object()
        .map(|object| object.keys().collect())
        .unwrap_or_default()
}

/// The keys whose values differ between two payloads, added and removed
/// included.
///
/// A `BTreeSet`, so the list is ordered and two equal changes encode
/// identically. That is finding 5 of the Sprint 8 construction — a banner that
/// cried wolf on every submit because `serde_json` orders a map by key and
/// JavaScript orders an object by insertion — and the fix there was the same
/// one: compare a canonical encoding rather than an incidental one.
fn moved_keys(before: &Value, after: &Value) -> BTreeSet<String> {
    let empty = Map::new();
    let before = before.as_object().unwrap_or(&empty);
    let after = after.as_object().unwrap_or(&empty);

    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

fn moved_metadata_keys(before: &MetadataSet, after: &MetadataSet) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

fn insert_error(error: sqlx::Error) -> AppError {
    match &error {
        // `uq_documents_tenant_id_document_ref`. Reachable only if two
        // creations took the same reference, which the counter's unique index
        // makes impossible — so this is a 409 rather than a 500 for the sake of
        // saying something true if it ever happens.
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a document with this reference already exists")
        }
        // A department or facility that does not exist arrives as a foreign-key
        // violation: unlike the entity link, they are not polymorphic and the
        // constraint is the check. A 422 naming the field beats a 500.
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            AppError::validation(vec![ValidationDetail::new(
                "requestedForDepartmentId",
                "reference",
                "NOT_FOUND",
                "a reference on this document names a row that does not exist",
            )])
        }
        _ => error.into(),
    }
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Document, AppError> {
    repo::find_document(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("document {id} vanished after it was written"),
        })
}
