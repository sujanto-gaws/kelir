//! Reading a running process (FR-WF-014, FR-DOC-012; [#175], [#178]).
//!
//! The read half of the seam. `GET /api/v1/documents/{id}/workflow` is what the
//! document workspace's Workflow tab renders, and it is the surface on which the
//! projection [`super::super`] describes becomes visible to a person: the
//! instance's state beside the document's status, with the definition's own name
//! for each.

use uuid::Uuid;

use super::super::domain::{Graph, WorkflowHistoryEntry, WorkflowInstance, WorkflowTask};
use super::super::repository::{
    definition as definition_repo, history as history_repo, instance as repo, task as task_repo,
};
use super::super::{INSTANCE_READ, TASK_READ};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::document::repository as document_repo;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// The process of one document, with the tasks it has generated.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentWorkflow {
    pub instance: WorkflowInstance,
    /// Every task of the instance, oldest first — the ones already decided as
    /// well as the open one.
    ///
    /// **The decided ones are the point.** A workflow tab showing only what is
    /// outstanding cannot answer "who approved this and when", which is the
    /// first question anybody opens it with.
    ///
    /// **This is the tasks, not the history.** FR-WF-012's account of how the
    /// document got here arrived with [#181] and is
    /// `GET /api/v1/documents/{id}/workflow/history`, paginated — a long-running
    /// process is exactly where a list embedded in another payload stops
    /// working, which is why it is a route of its own rather than a third field
    /// here. The two overlap and are not the same: a task tells you who was
    /// asked, a history row tells you what moved.
    ///
    /// [#181]: https://github.com/sujanto-gaws/kelir/issues/181
    pub tasks: Vec<WorkflowTask>,
}

/// The process deciding a document, or a 404 when none is.
///
/// **`document:read` is not required here and `workflow:instance:read` is**,
/// which is worth stating because it looks like the opposite of #170's rule.
/// It is not: #170 refused to let a document open *master data* the caller could
/// not read directly, by delegating to that module's service. Here the thing
/// being read is the workflow module's own, so the workflow module's permission
/// is the one that governs it — and the route is reached from a document
/// workspace whose own read permission the router already required.
pub async fn workflow_of_document(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
) -> Result<DocumentWorkflow, AppError> {
    caller.require(INSTANCE_READ)?;

    let tenant_id = caller.tenant_id();

    let instance_id = repo::instance_id_of_document(&state.pool, tenant_id, document_id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    load_instance(state, caller, instance_id).await
}

/// One instance by id.
pub async fn get_instance(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<DocumentWorkflow, AppError> {
    caller.require(INSTANCE_READ)?;

    load_instance(state, caller, id).await
}

async fn load_instance(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<DocumentWorkflow, AppError> {
    let tenant_id = caller.tenant_id();

    let row = repo::find_instance(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    let variables = repo::variables_of(&state.pool, tenant_id, id).await?;

    // The state's display name comes from the definition rather than from the
    // projection, for the reason `mod.rs` gives about the engine: the JSON is
    // the authority, and a screen reading a projection would be reading a copy.
    let state_name =
        definition_repo::definition_of_instance(&state.pool, tenant_id, row.workflow_definition_id)
            .await?
            .map(|definition| Graph::parse(&definition.definition_json))
            .and_then(|graph| {
                graph
                    .state(&row.current_state)
                    .map(|state| state.name.clone())
            })
            .unwrap_or_else(|| row.current_state.clone());

    let instance = repo::to_instance(row, state_name, variables);

    // The tasks need the task read permission, and a caller who holds one and
    // not the other gets the instance with an empty task list rather than a
    // refusal — the shape #101 established for a screen whose parts are governed
    // separately: *a caller who may read parties and not roles has a working
    // screen, not a forbidden one*.
    let tasks = if caller.require(TASK_READ).is_ok() {
        task_repo::tasks_of_instance(&state.pool, caller.tenant_id(), instance.id).await?
    } else {
        Vec::new()
    };

    Ok(DocumentWorkflow { instance, tasks })
}

/// One document's workflow history, oldest first and paginated ([#181] AC3).
///
/// **`workflow:instance:read`, and deliberately not `master-data:audit:read`**
/// ([#181] AC4). This is the document's own account of its progress, shown to
/// the approver deciding it; requiring the governance permission would refuse
/// it to exactly the people it is for, and would make a user-facing screen
/// depend on a row the audit trail exists to keep unchangeable. It is the same
/// permission [`workflow_of_document`] takes, for the reason stated there: the
/// thing being read is the workflow module's own.
///
/// **Paginated rather than whole.** A process that is returned and resubmitted
/// several times accumulates rows without bound, and the read that stops
/// working is the one nobody capped — which `get_task` already had to learn
/// once, when it asked for a thousand rows and `find`-ed the one it wanted.
///
/// [#181]: https://github.com/sujanto-gaws/kelir/issues/181
pub async fn history_of_document(
    state: &AppState,
    caller: &Authenticated,
    document_id: Uuid,
    pagination: &Pagination,
) -> Result<(Vec<WorkflowHistoryEntry>, PageMeta), AppError> {
    caller.require(INSTANCE_READ)?;

    let tenant_id = caller.tenant_id();

    // **The document is resolved first, so an unknown id is a 404 rather than
    // an empty page.** A document nobody has submitted has no history and
    // neither has one that does not exist, and answering both with `[]` would
    // make a typo look like a document waiting to be approved.
    if document_repo::find_document(&state.pool, tenant_id, document_id)
        .await?
        .is_none()
    {
        return Err(AppError::not_found("Document"));
    }

    let total = history_repo::count_for_document(&state.pool, tenant_id, document_id).await?;

    let entries = history_repo::list_for_document(
        &state.pool,
        tenant_id,
        document_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    #[allow(
        clippy::cast_sign_loss,
        reason = "count(*) over a table with no negative rows"
    )]
    Ok((entries, pagination.meta(total as u64)))
}
