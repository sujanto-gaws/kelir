//! Claiming a task, and recording a decision on it (FR-WF-006, FR-WF-007;
//! [#176], [#177]).
//!
//! # The concurrency shape this project has now produced five times
//!
//! A task is read, checked for whether it may be acted on, then written. That is
//! check-then-act, and it is the shape behind [#105], [#133], [#137] and the
//! reason [coding standard](../../../../../docs/standards/01.%20Coding%20Standard.md)
//! §2.5 gained its lock rule. Here it has the sharpest consequence it has had:
//! **a decision recorded against a task somebody else already decided is a
//! signature on the wrong document.**
//!
//! So the check runs in the transaction that writes, under a lock covering what
//! it read — and the write itself carries the predicate again, so two callers
//! who both passed the check produce one update of one row and one update of
//! none.
//!
//! # The lock ordering
//!
//! **Instance first, then task**, which is [`super::engine`]'s rule and applies
//! to every path in this module. The check reads the *instance's* state to
//! choose a transition, so §2.5 puts a lock on that too — and two paths taking
//! the two rows in opposite orders is a deadlock at exactly the concurrency this
//! feature is for.
//!
//! # Permission, and then the row
//!
//! `workflow:task:execute` says the caller may work tasks at all.
//! [`domain::task::refuse_unless_theirs`][super::super::domain::task::refuse_unless_theirs]
//! says whether *this* task is theirs. They are different questions, and a
//! deployment that grants the permission broadly and relies on the second is
//! doing exactly what it should.
//!
//! [#105]: https://github.com/sujanto-gaws/kelir/issues/105
//! [#133]: https://github.com/sujanto-gaws/kelir/issues/133
//! [#137]: https://github.com/sujanto-gaws/kelir/issues/137
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176
//! [#177]: https://github.com/sujanto-gaws/kelir/issues/177

use serde_json::json;
use uuid::Uuid;

use super::super::domain::task::{claim_lost, refuse_unless_open, refuse_unless_theirs};
use super::super::domain::{DecisionAction, Graph, TaskStatus, WorkflowTask};
use super::super::repository::{
    definition as definition_repo, instance as instance_repo, task as repo,
};
use super::super::{TASK_EXECUTE, TASK_OBJECT_TYPE};
use super::assignment::AssignmentContext;
use super::engine;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::document::repository as document_repo;
use crate::state::AppState;

/// What a decision answers with.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionResult {
    pub task_id: Uuid,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    pub action: DecisionAction,
    /// Where the process was and where it now is.
    ///
    /// Both ends rather than only the target, for `TransitionResult`'s reason
    /// one module over: a client that sent `APPROVE` already knows what it
    /// asked for; what it cannot know without being told is what the process was
    /// when the decision landed.
    pub previous_state: String,
    pub current_state: String,
    /// The document status the transition projected — the seam, visible in the
    /// response rather than requiring a second read to discover.
    pub document_status: crate::modules::document::domain::DocumentStatus,
}

/// Claims an unassigned role task ([#176] AC3).
pub async fn claim_task(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<WorkflowTask, AppError> {
    caller.require(TASK_EXECUTE)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    let mut transaction = state.pool.begin().await?;

    let locked = repo::lock_task(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    refuse_unless_open(locked.status)?;

    // A task offered to a role is claimable by a holder of that role and by
    // nobody else. Read from `user_roles` rather than the token's claim — see
    // `repository::task::holds_role`, which says why the stale-claim direction
    // is the one that matters.
    let role_id = locked.candidate_role_id.ok_or(AppError::Forbidden)?;

    if !repo::holds_role(&mut *transaction, tenant_id, user_id, role_id).await? {
        return Err(AppError::Forbidden);
    }

    if repo::claim(&mut transaction, tenant_id, id, user_id).await? == 0 {
        // The statement lost. Re-read under the lock this transaction still
        // holds, so the refusal can say which of the two happened — taken, or
        // finished — because those are different situations for the person who
        // lost.
        let now = repo::lock_task(&mut transaction, tenant_id, id)
            .await?
            .ok_or_else(|| AppError::not_found("Task"))?;

        return Err(claim_lost(now.status, now.assignee_user_id));
    }

    repo::record_task_history(
        &mut transaction,
        tenant_id,
        &repo::TaskHistoryEntry {
            task_id: id,
            instance_id: locked.workflow_instance_id,
            document_id: locked.document_id,
            from: Some(locked.status),
            to: TaskStatus::Assigned,
            action: None,
            actor: Some(user_id),
        },
    )
    .await?;

    transaction.commit().await?;

    let claimed = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Workflow.TaskClaimed",
            action: "UPDATE",
            object_type: TASK_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: None,
            reason: None,
            old_value: Some(json!({ "status": TaskStatus::Created, "assigneeUserId": null })),
            new_value: Some(json!({
                "status": claimed.status,
                "assigneeUserId": claimed.assignee_user_id,
            })),
        },
    )
    .await;

    Ok(claimed)
}

/// Records a decision, moves the process, and projects the document's status
/// ([#177]).
///
/// One transaction, and the order is the item: lock the instance, lock the task,
/// check, write, fire, project, record. A decision that committed without the
/// transition — or a transition without the decision — would be the pair of
/// writes [#168](https://github.com/sujanto-gaws/kelir/issues/168) calls
/// unrecoverable, one seam over.
pub async fn decide(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    action: DecisionAction,
) -> Result<DecisionResult, AppError> {
    caller.require(TASK_EXECUTE)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    // **Everything the decision reads that cannot move is resolved before the
    // transaction opens**, which is coding standard §2.5's rule and the reason
    // this path holds one pooled connection rather than two (**D-35**). Three
    // things qualify, and each for a stated reason:
    //
    //   * the **task**, for its instance id — which the lock ordering needs
    //     before it can take the instance first;
    //   * the **definition**, which an instance pins and therefore cannot
    //     change underneath this request;
    //   * the **document** and the instance's **variables**, which a condition
    //     evaluates against. A submitted document is not editable, so its form
    //     data is as stable here as it is under a lock.
    //
    // What is *not* resolved here is anything the decision depends on being
    // still true — the task's status and the instance's state — and those are
    // read again under the lock below, which is the read that counts.
    let subject = repo::find_task(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    let instance_row =
        instance_repo::find_instance(&state.pool, tenant_id, subject.workflow_instance_id)
            .await?
            .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    // **A deprecated definition still decides the approvals already running
    // against it.** `engine::start` refuses to *begin* one (#187's rule, checked
    // again where a binding cannot be), and this path deliberately does not: an
    // instance pins its revision, and refusing here would strand every approval
    // in flight the moment an administrator retired the workflow.
    let definition =
        definition_repo::definition_of_instance(&state.pool, instance_row.workflow_definition_id)
            .await?
            .ok_or_else(|| AppError::Internal {
                source: anyhow::anyhow!(
                    "instance {} runs definition {} which does not exist",
                    instance_row.id,
                    instance_row.workflow_definition_id
                ),
            })?;

    let graph = Graph::parse(&definition.definition_json);

    let document = document_repo::find_document(&state.pool, tenant_id, subject.document_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    let variables = variable_context(
        &instance_repo::variables_of(&state.pool, tenant_id, instance_row.id).await?,
    );

    let mut transaction = state.pool.begin().await?;

    // **Instance first.** The check below reads the instance's state to choose a
    // transition, and §2.5 puts the lock on what the check read.
    let instance =
        instance_repo::lock_instance(&mut transaction, tenant_id, subject.workflow_instance_id)
            .await?
            .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    // **Then the task**, and this read is the one that counts: the read above
    // was on the pool, so a concurrent decision could have completed the task
    // between the two.
    let task = repo::lock_task(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    refuse_unless_open(task.status)?;

    let holds_candidate_role = match task.candidate_role_id {
        Some(role_id) => repo::holds_role(&mut *transaction, tenant_id, user_id, role_id).await?,
        None => false,
    };

    refuse_unless_theirs(user_id, task.assignee_user_id, holds_candidate_role)?;

    // The write, carrying the predicate again: two callers who both passed the
    // check above produce one update of one row and one update of none.
    if repo::complete(&mut transaction, tenant_id, id, action, user_id).await? == 0 {
        let now = repo::lock_task(&mut transaction, tenant_id, id)
            .await?
            .ok_or_else(|| AppError::not_found("Task"))?;

        refuse_unless_open(now.status)?;

        return Err(AppError::conflict(
            "this task changed while the decision was being applied",
        ));
    }

    repo::record_task_history(
        &mut transaction,
        tenant_id,
        &repo::TaskHistoryEntry {
            task_id: id,
            instance_id: instance.id,
            document_id: task.document_id,
            from: Some(task.status),
            to: TaskStatus::Completed,
            action: Some(action),
            actor: Some(user_id),
        },
    )
    .await?;

    repo::record_decision(
        &mut transaction,
        tenant_id,
        &repo::Decision {
            document_id: task.document_id,
            instance_id: instance.id,
            task_id: id,
            approver: user_id,
            approver_role_id: task.candidate_role_id,
            action,
            decision_level: Some(&subject.task_definition_key),
        },
    )
    .await?;

    let fired = engine::fire(
        &mut transaction,
        tenant_id,
        instance.id,
        task.document_id,
        &graph,
        &instance.current_state,
        engine::transition_of(action),
        Some(user_id),
        AssignmentContext {
            owner_user_id: document.created_by,
            requested_department_id: document.requested_for_department_id,
            owner_department_id: None,
        },
        &engine::EvaluationContext {
            document: engine::document_facts(
                document.status,
                document.document_type_id,
                document.document_number.as_deref(),
            ),
            form_data: document.form_data.clone(),
            variables,
        },
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            // Naming convention §7's own worked example.
            event_type: "Workflow.TaskCompleted",
            action: action.as_db(),
            object_type: TASK_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: None,
            reason: None,
            old_value: Some(json!({
                "status": task.status,
                "instanceState": fired.from_state,
            })),
            // AC6's list exactly: who, which task, which transition, and the
            // resulting state.
            new_value: Some(json!({
                "status": TaskStatus::Completed,
                "documentId": task.document_id,
                "workflowInstanceId": instance.id,
                "transition": {
                    "from": fired.from_state,
                    "action": action,
                    "to": fired.to_state,
                },
                "instanceState": fired.to_state,
                "documentStatus": fired.document_status,
                "outcome": fired.outcome,
            })),
        },
    )
    .await;

    Ok(DecisionResult {
        task_id: id,
        workflow_instance_id: instance.id,
        document_id: task.document_id,
        action,
        previous_state: fired.from_state,
        current_state: fired.to_state,
        document_status: fired.document_status,
    })
}

/// The instance's variables as JSON Logic sees them (JWSS §6.1).
///
/// A flat object under `variables`, keyed as the definition declared them, so a
/// routing condition reads `{"var": "variables.amount"}` and gets the number the
/// instance started with rather than a string.
fn variable_context(variables: &[super::super::domain::WorkflowVariable]) -> serde_json::Value {
    let mut object = serde_json::Map::new();

    for variable in variables {
        object.insert(variable.key.clone(), variable.value.clone());
    }

    serde_json::Value::Object(object)
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<WorkflowTask, AppError> {
    repo::find_task(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("task {id} vanished after it was written"),
        })
}
