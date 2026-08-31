//! Form definition use cases (FR-RAD-002).
//!
//! **Publishing is the shape worth reading first.** A published revision is
//! immutable, because documents pin the exact `rad_forms.id` they were created
//! against — so editing one would change what an old document renders as, years
//! later, with nothing recording that it moved. Editing a published form
//! therefore creates the *next* revision as a draft, which is why
//! [`create_revision`] exists beside [`update_form`] rather than inside it.

use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::{
    validate_create_form, validate_update_form, CreateFormRequest, Form, FormStatus, FormSummary,
    UpdateFormRequest,
};
use super::super::repository::form::{self as repo, FormFields, NewForm};
use super::super::{FORM_CREATE, FORM_DELETE, FORM_PUBLISH, FORM_READ, FORM_UPDATE};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a form definition (naming convention §7).
const OBJECT_TYPE: &str = "RAD_FORM";

/// The JFSS specification version a definition is recorded against.
///
/// Read from the document rather than assumed: `version` is required by the
/// meta-schema and is the spec version, so a document declaring `2.0.0` is
/// stored as `2.0.0` and a renderer can tell. Defaulted only if a document
/// somehow validated without one, which the meta-schema does not allow.
fn jfss_version(definition: &Value) -> String {
    definition
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("2.0.1")
        .to_owned()
}

pub async fn list_forms(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<FormSummary>, PageMeta), AppError> {
    caller.require(FORM_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_forms(&state.pool, tenant_id).await?;
    let forms = repo::list_forms(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((forms, pagination.meta(total.max(0) as u64)))
}

pub async fn get_form(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Form, AppError> {
    caller.require(FORM_READ)?;

    repo::find_form(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))
}

pub async fn create_form(
    state: &AppState,
    caller: &Authenticated,
    request: CreateFormRequest,
) -> Result<Form, AppError> {
    caller.require(FORM_CREATE)?;
    validate_create_form(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let form_key = request.form_key.trim();

    // Revision 1 or nothing. A second create under a key that already has
    // revisions is a caller who means `create_revision`, and guessing which
    // they meant would silently fork a form's history.
    if repo::highest_revision(&state.pool, tenant_id, form_key)
        .await?
        .is_some()
    {
        return Err(AppError::conflict(format!(
            "form `{form_key}` already exists; publish the current revision and \
             create the next one rather than creating the key again"
        )));
    }

    let id = Uuid::now_v7();

    repo::insert_form(
        &state.pool,
        &NewForm {
            id,
            tenant_id,
            form_key,
            title: request.title.trim(),
            revision: 1,
            jfss_version: &jfss_version(&request.definition),
            definition_json: &request.definition,
            entity_id: request.entity_id,
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_to_conflict)?;

    // Read back before the record is written, so the record says what the row
    // holds rather than what the request asked for (#135).
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            // The definition is deliberately not in the record. It is up to a
            // megabyte of JSON, the audit trail keeps every version of every
            // record forever, and `rad_forms` already keeps the definition
            // itself under a revision that never changes once published.
            new_value: Some(json!({
                "formKey": created.form_key,
                "title": created.title,
                "revision": created.revision,
                "jfssVersion": created.jfss_version,
                "entityId": created.entity_id,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_form(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateFormRequest,
) -> Result<Form, AppError> {
    caller.require(FORM_UPDATE)?;
    validate_update_form(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_form(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))?;

    if before.status != FormStatus::Draft {
        return Err(published_is_immutable(&before));
    }

    let affected = repo::update_draft(
        &state.pool,
        tenant_id,
        id,
        &FormFields {
            title: request.title.as_deref().map(str::trim),
            definition_json: request.definition.as_ref(),
            entity_id: request.entity_id,
        },
        actor,
    )
    .await?;

    // Zero rows means the revision stopped being a draft between the read above
    // and this write — a publish landing in the gap. The predicate in the
    // statement is what makes that a refusal rather than an edit to a revision
    // that had just become immutable.
    if affected == 0 {
        let now = repo::find_form(&state.pool, tenant_id, id)
            .await?
            .ok_or_else(|| AppError::not_found("Form definition"))?;

        return Err(published_is_immutable(&now));
    }

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135). The definition is compared
    // rather than stored: a caller who resends an identical document has
    // changed nothing, and a record saying otherwise is noise in the trail
    // somebody will one day read to answer "when did this form change?".
    let mut changes = ChangeSet::new();
    changes.field("title", &before.title, &after.title);
    changes.field("entityId", &before.entity_id, &after.entity_id);
    changes.field(
        "definition",
        &definition_marker(&before.definition),
        &definition_marker(&after.definition),
    );

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.Updated",
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

/// Publishes a draft revision, fixing it for every document that pins it.
pub async fn publish_form(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Form, AppError> {
    caller.require(FORM_PUBLISH)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_form(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))?;

    if before.status != FormStatus::Draft {
        return Err(AppError::conflict(format!(
            "revision {} of `{}` is {:?} and only a draft can be published",
            before.revision, before.form_key, before.status
        )));
    }

    if repo::publish(&state.pool, tenant_id, id, actor).await? == 0 {
        // Somebody else published it first. Their name is on it, which is
        // correct — the second call did not publish anything.
        return Err(AppError::conflict(format!(
            "revision {} of `{}` was published by another request",
            before.revision, before.form_key
        )));
    }

    let after = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.Published",
            action: "UPDATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({ "status": before.status })),
            new_value: Some(json!({
                "status": after.status,
                "publishedAt": after.published_at,
                "publishedBy": after.published_by,
            })),
        },
    )
    .await;

    Ok(after)
}

/// Creates the next revision of a form as a draft, from an existing one.
///
/// The path an edit to a published form takes. It reads the revision the caller
/// names rather than the highest one, so "revise this specific version" is
/// expressible — and takes the next free number, which is the highest plus one
/// including soft-deleted revisions.
pub async fn create_revision(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateFormRequest,
) -> Result<Form, AppError> {
    // The permission to create a form, not to update one: this makes a new row.
    caller.require(FORM_CREATE)?;
    validate_update_form(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let source = repo::find_form(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))?;

    let definition = request.definition.unwrap_or(source.definition);
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .unwrap_or(&source.title);
    let entity_id = match request.entity_id {
        None => source.entity_id,
        Some(value) => value,
    };

    let next = repo::highest_revision(&state.pool, tenant_id, &source.form_key)
        .await?
        .unwrap_or(source.revision)
        + 1;

    let new_id = Uuid::now_v7();

    repo::insert_form(
        &state.pool,
        &NewForm {
            id: new_id,
            tenant_id,
            form_key: &source.form_key,
            title,
            revision: next,
            jfss_version: &jfss_version(&definition),
            definition_json: &definition,
            entity_id,
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_to_conflict)?;

    let created = load(state, tenant_id, new_id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.RevisionCreated",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: new_id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "formKey": created.form_key,
                "revision": created.revision,
                "fromRevision": source.revision,
                "fromId": source.id,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn delete_form(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(FORM_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_form(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Form definition"))?;

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("Form definition"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadForm.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "formKey": before.form_key,
                "revision": before.revision,
                "status": before.status,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// A stable stand-in for the definition in an audit record.
///
/// The definition itself is up to a megabyte and the trail keeps every version
/// forever. What a reader of the trail needs is *whether* it changed, and a
/// length plus a component count answers that without storing the document a
/// second time. It is not a hash: a hash tells a reader nothing they can act on
/// when the two differ.
fn definition_marker(definition: &Value) -> Value {
    json!({
        "bytes": definition.to_string().len(),
        "components": definition
            .get("components")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
    })
}

fn published_is_immutable(form: &Form) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "status",
        "immutable",
        "NOT_A_DRAFT",
        format!(
            "revision {} of `{}` is published and cannot be edited; create the \
             next revision instead — documents pin the revision they were \
             created against",
            form.revision, form.form_key
        ),
    )])
}

fn duplicate_to_conflict(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a form definition with this key and revision already exists")
        }
        _ => error.into(),
    }
}

/// Reads a form back, treating its absence as an error rather than a `None`.
///
/// It was just written inside this request, so a miss means the row is gone —
/// which is a 500 and not a 404, and saying so keeps the two apart.
async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Form, AppError> {
    repo::find_form(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("form {id} vanished after it was written"),
        })
}
