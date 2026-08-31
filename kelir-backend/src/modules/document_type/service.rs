//! Document type use cases (FR-DTYPE-001, 002, 003).
//!
//! **The binding checks and the write they guard are in one transaction, under
//! a lock covering what the checks read.** That is coding standard §2.5's rule
//! and it was bought expensively: #133 found two callers each walking a path the
//! other was about to change, and #137 found a parent re-read that had happened
//! a moment too early. Here the same shape is a document type bound to a form
//! that is soft-deleted between "does this form exist?" and the insert — which
//! leaves a type pointing at a definition no read returns, and a renderer with
//! nothing to render.
//!
//! **There are two such checks on an update and they guard different things.**
//! [`check_bindings`] asks whether the form being bound *may* be bound — it
//! exists, it is live, it is published — and holds that form. [`guard_rebinding`]
//! asks what the change does to documents that already exist, and holds the type
//! row: a document's foreign key takes a `FOR KEY SHARE` on that row, which
//! `FOR UPDATE` conflicts with, so the two serialize by construction rather than
//! by anything Sprint 9 has to remember.

use serde_json::json;
use uuid::Uuid;

use super::domain::{
    validate_create, validate_update, CreateDocumentTypeRequest, DocumentType, DocumentTypeStatus,
    DocumentTypeSummary, SecurityLevel, UpdateDocumentTypeRequest, WorkflowBinding,
};
use super::repository::{self as repo, DocumentTypeFields, NewDocumentType};
use super::{TYPE_CREATE, TYPE_DELETE, TYPE_READ, TYPE_UPDATE};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::modules::workflow::repository::definition as workflow_repository;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a document type (naming convention §7).
const OBJECT_TYPE: &str = "DOCUMENT_TYPE";

pub async fn list_types(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<DocumentTypeSummary>, PageMeta), AppError> {
    caller.require(TYPE_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_types(&state.pool, tenant_id).await?;
    let types = repo::list_types(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((types, pagination.meta(total.max(0) as u64)))
}

pub async fn get_type(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<DocumentType, AppError> {
    caller.require(TYPE_READ)?;

    repo::find_type(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))
}

pub async fn create_type(
    state: &AppState,
    caller: &Authenticated,
    request: CreateDocumentTypeRequest,
) -> Result<DocumentType, AppError> {
    caller.require(TYPE_CREATE)?;
    validate_create(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let id = Uuid::now_v7();

    let mut transaction = state.pool.begin().await?;

    // Inside the transaction, under a share lock, and *before* the insert. The
    // ordering is the point: a check that runs on the pool answers a question
    // about a moment that has already passed.
    check_bindings(
        &mut transaction,
        tenant_id,
        request.form_id,
        request.list_id,
    )
    .await?;

    repo::insert_type(
        &mut *transaction,
        &NewDocumentType {
            id,
            tenant_id,
            type_code: request.type_code.trim(),
            name: request.name.trim(),
            description: trimmed(request.description.as_deref()),
            category: trimmed(request.category.as_deref()),
            form_id: request.form_id,
            list_id: request.list_id,
            default_security_level: request
                .default_security_level
                .unwrap_or(SecurityLevel::Internal)
                .as_db(),
            retention_policy_id: request.retention_policy_id,
            target_entity_type: trimmed(request.target_entity_type.as_deref()),
            status: request.status.unwrap_or(DocumentTypeStatus::Active).as_db(),
            created_by: actor,
        },
    )
    .await
    .map_err(insert_error)?;

    check_workflow_bindings(&mut transaction, tenant_id, &request.workflows).await?;

    repo::replace_workflows(&mut transaction, tenant_id, id, &request.workflows, actor).await?;

    transaction.commit().await?;

    // Read back before the record is written (#135).
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "DocumentType.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "typeCode": created.type_code,
                "name": created.name,
                "formId": created.form_id,
                "listId": created.list_id,
                "status": created.status,
                "workflows": created.workflows.len(),
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_type(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateDocumentTypeRequest,
) -> Result<DocumentType, AppError> {
    caller.require(TYPE_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_type(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    let mut transaction = state.pool.begin().await?;

    // The type row is read and held before anything below decides on it. Both
    // guards that follow answer questions about state a concurrent writer could
    // move, and coding standard §2.5 puts the lock on what the *check* read.
    let bound_form = repo::lock_type_binding(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    // Only a binding the caller is *changing* is checked, and only when it
    // names something. Re-checking an unchanged binding would refuse an
    // unrelated edit because a form was retired weeks ago — which is a real
    // state, and the fix for it is to change the binding, not to be unable to
    // rename the type.
    check_bindings(
        &mut transaction,
        tenant_id,
        request.form_id.flatten(),
        request.list_id.flatten(),
    )
    .await?;

    if let Some(next_form) = request.form_id {
        if next_form != bound_form {
            guard_rebinding(&mut transaction, tenant_id, id, &before.type_code).await?;
        }
    }

    let affected = repo::update_type(
        &mut *transaction,
        tenant_id,
        id,
        &DocumentTypeFields {
            name: request.name.as_deref().map(str::trim),
            description: request
                .description
                .as_ref()
                .map(|value| trimmed(value.as_deref())),
            category: request
                .category
                .as_ref()
                .map(|value| trimmed(value.as_deref())),
            form_id: request.form_id,
            list_id: request.list_id,
            default_security_level: request.default_security_level.map(SecurityLevel::as_db),
            retention_policy_id: request.retention_policy_id,
            target_entity_type: request
                .target_entity_type
                .as_ref()
                .map(|value| trimmed(value.as_deref())),
            status: request.status.map(DocumentTypeStatus::as_db),
        },
        actor,
    )
    .await?;

    if affected == 0 {
        // Retired between the read and the write; rolling back rather than
        // writing bindings onto a row that is no longer live.
        return Err(AppError::not_found("Document type"));
    }

    if let Some(workflows) = &request.workflows {
        check_workflow_bindings(&mut transaction, tenant_id, workflows).await?;
        repo::replace_workflows(&mut transaction, tenant_id, id, workflows, actor).await?;
    }

    transaction.commit().await?;

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135).
    let mut changes = ChangeSet::new();
    changes.field("name", &before.name, &after.name);
    changes.field("description", &before.description, &after.description);
    changes.field("category", &before.category, &after.category);
    changes.field("formId", &before.form_id, &after.form_id);
    changes.field("listId", &before.list_id, &after.list_id);
    changes.field(
        "defaultSecurityLevel",
        &before.default_security_level,
        &after.default_security_level,
    );
    changes.field(
        "retentionPolicyId",
        &before.retention_policy_id,
        &after.retention_policy_id,
    );
    changes.field(
        "targetEntityType",
        &before.target_entity_type,
        &after.target_entity_type,
    );
    changes.field("status", &before.status, &after.status);
    changes.field("workflows", &before.workflows, &after.workflows);

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "DocumentType.Updated",
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

pub async fn delete_type(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(TYPE_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_type(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Document type"))?;

    // Refused rather than cascaded. A document *is* its type — the type says
    // which form it renders and how it is numbered — so retiring one under
    // live documents leaves those documents pointing at something no read
    // returns. Deprecating the type is the way to stop new documents being
    // created from it, and that is an update.
    if repo::has_documents(&state.pool, tenant_id, id).await? {
        return Err(AppError::conflict(format!(
            "`{}` has documents created from it and cannot be retired; set its \
             status to DEPRECATED instead, which stops new ones",
            before.type_code
        )));
    }

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("Document type"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "DocumentType.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "typeCode": before.type_code,
                "name": before.name,
                "status": before.status,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Refuses a form rebinding that would re-render documents nobody filled in
/// that way (#165 AC3).
///
/// **The decision, stated where the code is rather than left to be inferred:
/// changing a type's form binding is allowed, and existing documents keep the
/// revision they were filled against.** Refusing outright was the alternative
/// and it is worse — a form definition is revised by publishing the next
/// revision, so a type that could never be re-pointed would be stuck on
/// revision 1 forever the moment one document existed, which makes the
/// revision mechanism unusable rather than safe.
///
/// **What makes "keep the revision they were filled against" true rather than
/// hoped for.** A document carries its own `form_id`, pinned at creation
/// ([Database Schema](../../../../docs/design/02.%20Database%20Schema.md) §6.6),
/// and that column is what an old document re-renders through — so moving the
/// *type's* binding cannot reach it. But §6.6 makes the column **nullable**, and
/// a document that pinned nothing has only its type's current binding to render
/// against. Those documents, and only those, are what this refuses over: it is
/// the difference between a guarantee and a comment describing one.
///
/// It is deliberately not a `NOT NULL` on `documents.form_id`. That would
/// forbid a document of a type that binds no form, which §6.2 permits, and it
/// would settle a column Sprint 9's [#167] has not written to yet — a
/// constraint reaching forward into an unbuilt surface rather than a rule about
/// this one.
///
/// [#167]: https://github.com/sujanto-gaws/kelir/issues/167
async fn guard_rebinding(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    type_code: &str,
) -> Result<(), AppError> {
    let unpinned =
        repo::count_documents_without_a_pinned_form(&mut **transaction, tenant_id, id).await?;

    if unpinned == 0 {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "formId",
        "reference",
        "DOCUMENTS_WITHOUT_A_PINNED_FORM",
        format!(
            "`{type_code}` has {unpinned} document(s) that pinned no form revision, so \
             they render against whatever this type is bound to; changing the \
             binding would re-render them against a definition they were not \
             filled against. Documents created against a form pin it and are \
             unaffected"
        ),
    )]))
}

/// Holds and checks the workflow definitions this write is binding
/// ([#187](https://github.com/sujanto-gaws/kelir/issues/187) AC2).
///
/// `SELECT ... FOR SHARE` inside the write's transaction, before the write, on
/// each definition named — coding standard §2.5's rule and [`check_bindings`]'
/// shape for the form binding beside it. A share lock blocks the retirement that
/// would invalidate the check and does not block a second type binding the same
/// workflow, which is the normal case rather than a rare one.
///
/// **A draft is refused, and that is the same argument publishing settles for a
/// form.** A running approval executes the revision it started against, so
/// binding a draft would bind a definition that can still change under
/// documents already routed by it — which is what publication exists to
/// prevent.
///
/// # Rebinding, decided rather than defaulted (AC3)
///
/// > **Changing a type's workflow binding is allowed, running instances are
/// > unaffected, and future submissions of that type use the new binding.**
///
/// There is no [`guard_rebinding`] for workflows, and the reason is a real
/// difference rather than an inconsistency between two similar surfaces.
/// **D-30** needed a guard because `documents.form_id` is *nullable*: a document
/// that pinned no revision renders against whatever its type currently binds, so
/// re-pointing the type reaches backwards into documents already created.
/// `workflow_instances.workflow_definition_id` is `NOT NULL` and names a
/// revision row, so **no instance can be in that condition** — every running
/// approval has already pinned one, and nothing reaches it through the type. The
/// guard D-30 needed has nothing here to guard.
///
/// `workflow_seam.rs` holds the test that makes this checkable rather than
/// merely stated: a type bound to A, a document submitted and running, the type
/// rebound to B, and the running instance still on A with its state intact.
async fn check_workflow_bindings(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    workflows: &[WorkflowBinding],
) -> Result<(), AppError> {
    for (index, binding) in workflows.iter().enumerate() {
        let path = format!("workflows.{index}.workflowDefinitionId");
        let id = binding.workflow_definition_id;

        match workflow_repository::lock_bindable_definition(transaction, tenant_id, id).await? {
            None => {
                return Err(binding_error(
                    &path,
                    "NOT_FOUND",
                    format!("no workflow definition {id} in this tenant"),
                ))
            }
            Some(status) if status != "ACTIVE" => {
                return Err(binding_error(
                    &path,
                    "NOT_PUBLISHED",
                    format!(
                        "workflow definition {id} is {status} and only a published \
                         revision may be bound — a running approval executes the \
                         revision it started against"
                    ),
                ))
            }
            Some(_) => {}
        }
    }

    Ok(())
}

/// Holds and checks whichever bindings this write is setting.
///
/// A form has to exist, be live, and be **published**. The last is the one
/// worth arguing: `documents.form_id` pins the type's form at creation, so
/// binding a draft means pinning a definition that can still change under every
/// document already created from it — which is exactly the thing publication
/// exists to prevent (#156).
async fn check_bindings(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    form_id: Option<Uuid>,
    list_id: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(form_id) = form_id {
        match repo::lock_bindable_form(transaction, tenant_id, form_id).await? {
            None => {
                return Err(binding_error(
                    "formId",
                    "NOT_FOUND",
                    format!("no form definition {form_id} in this tenant"),
                ))
            }
            Some(form) if form.status != "PUBLISHED" => {
                return Err(binding_error(
                    "formId",
                    "NOT_PUBLISHED",
                    format!(
                        "form definition {form_id} is {} and only a published \
                         revision may be bound — a document pins the revision it \
                         was created against",
                        form.status
                    ),
                ))
            }
            Some(_) => {}
        }
    }

    if let Some(list_id) = list_id {
        if !repo::lock_bindable_list(transaction, tenant_id, list_id).await? {
            return Err(binding_error(
                "listId",
                "NOT_FOUND",
                format!("no list definition {list_id} in this tenant"),
            ));
        }
    }

    Ok(())
}

fn binding_error(path: &str, code: &str, message: String) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        path,
        "reference",
        code,
        message,
    )])
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

fn insert_error(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a document type with this typeCode already exists")
        }
        // A retention policy that does not exist arrives as a foreign-key
        // violation rather than through `check_bindings`: unlike a form, it is
        // not a RAD definition this module locks, and a 422 naming the field is
        // still better than a 500.
        sqlx::Error::Database(database) if database.is_foreign_key_violation() => {
            AppError::validation(vec![ValidationDetail::new(
                "retentionPolicyId",
                "reference",
                "NOT_FOUND",
                "a reference on this document type names a row that does not exist",
            )])
        }
        _ => error.into(),
    }
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<DocumentType, AppError> {
    repo::find_type(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("document type {id} vanished after it was written"),
        })
}
