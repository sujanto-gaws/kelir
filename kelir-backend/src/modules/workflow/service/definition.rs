//! Workflow definition use cases (FR-WF-001, 002, 003; [#174]).
//!
//! **Publishing is the shape worth reading first**, and it is
//! `rad::service::form`'s for the same reason: a published revision is what
//! running instances execute, so editing one would change the rules an approval
//! is being decided under, mid-approval, with nothing recording that it moved.
//! Editing an active definition therefore creates the *next* revision as a
//! draft, which is why [`create_revision`] exists beside [`update_definition`]
//! rather than inside it.
//!
//! # Validation runs at save and again at publish, and the second is not
//! decoration
//!
//! [#174] AC3 asks for a definition whose graph does not terminate to be
//! *"refused at save time rather than at run time"*, and JWSS §8 requires only
//! that such a definition never reach `ACTIVE`. Running at save satisfies both
//! and is stricter than either.
//!
//! The publish check is the **conformance** requirement (JWSS §1.3 clause 2) and
//! it is genuinely reachable: a row can arrive in `workflow_definitions` from a
//! migration, a restore, or a hand-written `INSERT`, and the publish is the last
//! gate before instances start running it. Coding standard §2.5 does not accept
//! a paragraph explaining why a second line exists in place of a test that
//! reaches it, so `tests/workflow_definitions.rs` writes an invalid definition
//! through the pool and publishes it through the API.
//!
//! [#174]: https://github.com/sujanto-gaws/kelir/issues/174

use serde_json::{json, Value};
use uuid::Uuid;

use super::super::domain::jwss;
use super::super::domain::{
    definition::{initial_state, jwss_version},
    validate_create, validate_update, CreateWorkflowRequest, Graph, UpdateWorkflowRequest,
    WorkflowDefinition, WorkflowDefinitionStatus, WorkflowDefinitionSummary,
};
use super::super::repository::{definition as repo, projection};
use super::super::{
    DEFINITION_CREATE, DEFINITION_DELETE, DEFINITION_OBJECT_TYPE, DEFINITION_PUBLISH,
    DEFINITION_READ, DEFINITION_UPDATE,
};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

pub async fn list_definitions(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<WorkflowDefinitionSummary>, PageMeta), AppError> {
    caller.require(DEFINITION_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_definitions(&state.pool, tenant_id).await?;
    let definitions = repo::list_definitions(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((definitions, pagination.meta(total.max(0) as u64)))
}

pub async fn get_definition(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<WorkflowDefinition, AppError> {
    caller.require(DEFINITION_READ)?;

    repo::find_definition(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow definition"))
}

pub async fn create_definition(
    state: &AppState,
    caller: &Authenticated,
    request: CreateWorkflowRequest,
) -> Result<WorkflowDefinition, AppError> {
    caller.require(DEFINITION_CREATE)?;
    validate_create(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let workflow_key = request.workflow_key.trim();

    // Revision 1 or nothing. A second create under a key that already has
    // revisions is a caller who means `create_revision`, and guessing which they
    // meant would silently fork a workflow's history.
    if repo::highest_version(&state.pool, tenant_id, workflow_key)
        .await?
        .is_some()
    {
        return Err(AppError::conflict(format!(
            "workflow `{workflow_key}` already exists; publish the current revision \
             and create the next one rather than creating the key again"
        )));
    }

    let id = Uuid::now_v7();

    repo::insert_definition(
        &state.pool,
        &repo::NewDefinition {
            id,
            tenant_id,
            workflow_key,
            name: request.name.trim(),
            description: trimmed(request.description.as_deref()),
            version: 1,
            jwss_version: &jwss_version(&request.definition),
            definition_json: &request.definition,
            initial_state: &initial_state(&request.definition),
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
            event_type: "Workflow.Created",
            action: "CREATE",
            object_type: DEFINITION_OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            // The definition itself is deliberately not in the record — the
            // reasoning `rad::service::form` gives about a JFSS document, and
            // the same numbers: the trail keeps every version forever, and
            // `workflow_definitions` already keeps the document under a revision
            // that never changes once published.
            new_value: Some(json!({
                "workflowKey": created.workflow_key,
                "name": created.name,
                "version": created.version,
                "jwssVersion": created.jwss_version,
                "initialState": created.initial_state,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_definition(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateWorkflowRequest,
) -> Result<WorkflowDefinition, AppError> {
    caller.require(DEFINITION_UPDATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_definition(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow definition"))?;

    if before.status != WorkflowDefinitionStatus::Draft {
        return Err(published_is_immutable(&before));
    }

    let next_initial = request.definition.as_ref().map(initial_state);
    let next_jwss = request.definition.as_ref().map(jwss_version);

    let affected = repo::update_draft(
        &state.pool,
        tenant_id,
        id,
        &repo::DefinitionFields {
            name: request.name.as_deref().map(str::trim),
            description: request.description.as_deref().map(str::trim),
            definition_json: request.definition.as_ref(),
            initial_state: next_initial.as_deref(),
            jwss_version: next_jwss.as_deref(),
        },
        actor,
    )
    .await?;

    // Zero rows means the revision stopped being a draft between the read above
    // and this write — a publish landing in the gap. The predicate in the
    // statement is what makes that a refusal rather than an edit to a revision
    // that had just become immutable.
    if affected == 0 {
        let now = repo::find_definition(&state.pool, tenant_id, id)
            .await?
            .ok_or_else(|| AppError::not_found("Workflow definition"))?;

        return Err(published_is_immutable(&now));
    }

    let after = load(state, tenant_id, id).await?;

    let mut changes = ChangeSet::new();
    changes.field("name", &before.name, &after.name);
    changes.field("description", &before.description, &after.description);
    changes.field("initialState", &before.initial_state, &after.initial_state);
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
            event_type: "Workflow.Updated",
            action: "UPDATE",
            object_type: DEFINITION_OBJECT_TYPE,
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

/// Publishes a draft revision, fixing it for every instance that starts against
/// it, and writing the projections in the same transaction.
pub async fn publish_definition(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<WorkflowDefinition, AppError> {
    caller.require(DEFINITION_PUBLISH)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_definition(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow definition"))?;

    if before.status != WorkflowDefinitionStatus::Draft {
        return Err(AppError::conflict(format!(
            "revision {} of `{}` is {:?} and only a draft can be published",
            before.version, before.workflow_key, before.status
        )));
    }

    // **The conformance check** (JWSS §1.3 clause 2, §8). The save path has
    // already run these rules, so this fires only for a row that reached the
    // database another way — and the publish is the last gate before instances
    // start executing it. See the module documentation for the test that
    // reaches it.
    let problems = jwss::validate_definition(&before.definition);

    if !problems.is_empty() {
        return Err(AppError::validation(problems));
    }

    let graph = Graph::parse(&before.definition, before.version);

    let mut transaction = state.pool.begin().await?;

    if repo::publish(&mut *transaction, tenant_id, id, actor).await? == 0 {
        // Somebody else published it first. Their name is on it, which is
        // correct — the second call published nothing.
        return Err(AppError::conflict(format!(
            "revision {} of `{}` was published by another request",
            before.version, before.workflow_key
        )));
    }

    // In the publish transaction, so a definition is never `ACTIVE` with a
    // projection that does not match it. The foreign key on
    // `workflow_instances.current_state` reads these rows, so a publish that
    // committed without them would produce a definition nothing could start.
    projection::replace(&mut transaction, tenant_id, id, &graph, actor).await?;

    transaction.commit().await?;

    let after = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Workflow.Published",
            action: "UPDATE",
            object_type: DEFINITION_OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({ "status": before.status })),
            new_value: Some(json!({
                "status": after.status,
                "publishedAt": after.published_at,
                "publishedBy": after.published_by,
                "states": graph.states.len(),
                "transitions": graph.transitions.len(),
            })),
        },
    )
    .await;

    Ok(after)
}

/// Creates the next revision of a workflow as a draft, from an existing one.
///
/// The path an edit to a published definition takes. It reads the revision the
/// caller names rather than the highest one, so "revise this specific version"
/// is expressible — and takes the next free number, which is the highest plus
/// one including soft-deleted revisions.
pub async fn create_revision(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateWorkflowRequest,
) -> Result<WorkflowDefinition, AppError> {
    // The permission to create a workflow, not to update one: this makes a new
    // row.
    caller.require(DEFINITION_CREATE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let source = repo::find_definition(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow definition"))?;

    let definition = request.definition.unwrap_or(source.definition);
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .unwrap_or(&source.name);

    let next = repo::highest_version(&state.pool, tenant_id, &source.workflow_key)
        .await?
        .unwrap_or(source.version)
        + 1;

    let new_id = Uuid::now_v7();

    repo::insert_definition(
        &state.pool,
        &repo::NewDefinition {
            id: new_id,
            tenant_id,
            workflow_key: &source.workflow_key,
            name,
            description: request
                .description
                .as_deref()
                .map(str::trim)
                .or(source.description.as_deref()),
            version: next,
            jwss_version: &jwss_version(&definition),
            definition_json: &definition,
            initial_state: &initial_state(&definition),
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
            event_type: "Workflow.RevisionCreated",
            action: "CREATE",
            object_type: DEFINITION_OBJECT_TYPE,
            object_id: new_id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "workflowKey": created.workflow_key,
                "version": created.version,
                "fromVersion": source.version,
                "fromId": source.id,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn delete_definition(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(DEFINITION_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_definition(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow definition"))?;

    // Refused rather than cascaded, which is `delete_type`'s decision one module
    // over and for its reason: an instance *is* its definition — the definition
    // says which transitions exist and who may fire them — so retiring one under
    // a running approval leaves that approval unable to move. Deprecating the
    // definition is how new documents stop routing to it, and that is an update.
    if repo::has_live_instances(&state.pool, tenant_id, id).await? {
        return Err(AppError::conflict(format!(
            "revision {} of `{}` has running approvals and cannot be retired; \
             deprecate it instead, which stops new documents routing to it",
            before.version, before.workflow_key
        )));
    }

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("Workflow definition"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Workflow.Deleted",
            action: "DELETE",
            object_type: DEFINITION_OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({
                "workflowKey": before.workflow_key,
                "version": before.version,
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
/// `rad::service::form::definition_marker`'s reasoning, on the other definition
/// kind: what a reader of the trail needs is *whether* it changed, and a state
/// and transition count answers that without storing the document a second time.
/// It is not a hash, because a hash tells a reader nothing they can act on when
/// two differ.
fn definition_marker(definition: &Value) -> Value {
    let graph = Graph::parse(definition, 0);

    json!({
        "bytes": definition.to_string().len(),
        "states": graph.states.len(),
        "transitions": graph.transitions.len(),
    })
}

fn published_is_immutable(definition: &WorkflowDefinition) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "status",
        "immutable",
        "NOT_A_DRAFT",
        format!(
            "revision {} of `{}` is published and cannot be edited; create the next \
             revision instead — a running approval executes the revision it started \
             against",
            definition.version, definition.workflow_key
        ),
    )])
}

fn duplicate_to_conflict(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a workflow definition with this key and version already exists")
        }
        _ => error.into(),
    }
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// Reads a definition back, treating its absence as an error rather than a
/// `None`.
///
/// It was just written inside this request, so a miss means the row is gone —
/// which is a 500 and not a 404, and saying so keeps the two apart.
async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<WorkflowDefinition, AppError> {
    repo::find_definition(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("workflow definition {id} vanished after it was written"),
        })
}
