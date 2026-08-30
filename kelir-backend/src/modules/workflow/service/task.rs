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
//! # Handing a task on is a third thing, and it is not a decision
//!
//! [`delegate`] (FR-WF-009, FR-TASK-008; [#184]) changes who an open task is
//! for. It records no `approval_decisions` row, writes no `workflow_history`
//! row and does not call [`super::engine::fire`] — **the process has not
//! moved**. What the hand-off does write is a `workflow_task_history` row,
//! which is the record of *what happened to this task* — and that is precisely
//! what happened to it.
//!
//! **This paragraph used to rest on a constraint that no longer exists**, and
//! is restated rather than reworded because the correction is the interesting
//! part. It read: *a `workflow_history` row could not be written for it even if
//! that were wanted — `ck_workflow_history_moved` refuses `from_state IS NOT
//! DISTINCT FROM to_state`, which is exactly what a hand-off would produce, and
//! the constraint is right.* The constraint was not right: it also refused a
//! legal self-transition, and [#259] dropped it. Nothing about the hand-off
//! changes. A `from_state = to_state` row is now writable and this path still
//! does not write one, because it does not call `fire` — which is the reason it
//! writes none, and always was.
//!
//! [#259]: https://github.com/sujanto-gaws/kelir/issues/259
//!
//! A JWSS definition may still declare a `DELEGATE` transition; nothing fires
//! one, for the reason [`super::engine`] gives about `AUTO`. It is said here so
//! that the vocabulary in §7.3 does not read as evidence that this route drives
//! it.
//!
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176
//! [#177]: https://github.com/sujanto-gaws/kelir/issues/177
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184

use serde_json::json;
use uuid::Uuid;

use super::super::domain::task::{
    claim_lost, delegate_unavailable, normalize_comment, refuse_self_delegation,
    refuse_unless_held_by, refuse_unless_open, refuse_unless_theirs,
};
use super::super::domain::{
    DecisionAction, DelegateRequest, Graph, TaskStatus, TransitionAction, WorkflowTask,
};
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
use crate::modules::identity::delegation_repository as delegation_repo;
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

    // The department travels with the role (#225). A `DEPARTMENT_ROLE`
    // assignment resolved to both and stored both; checking only the role
    // would let Procurement's approver claim Finance's task.
    if !repo::holds_role(
        &mut *transaction,
        tenant_id,
        user_id,
        role_id,
        locked.candidate_department_id,
    )
    .await?
    {
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
            comment: None,
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

/// Hands an open task to somebody else (FR-WF-009, FR-TASK-008; [#184]).
///
/// # What it is for, against what a delegation window is for
///
/// A window ([`crate::modules::identity::delegation_service`]) is **prospective**:
/// it redirects work that has not arrived yet, and [#184] AC3 is the decision
/// that it does not reach back for tasks already sitting on somebody's desk.
/// This is the other half of that decision — the retrospective one. A task
/// already assigned when a person goes on leave is handed over here, explicitly,
/// by the person who holds it.
///
/// Neither substitutes for the other, and the pair is what makes AC3 a design
/// rather than a limitation: opening a window silently reassigning work already
/// in progress would move approvals out from under people mid-decision, and a
/// window that could not be complemented by a hand-off would leave those tasks
/// stranded for the length of the leave.
///
/// # One lock, and it is the task's
///
/// [`super::engine`]'s ordering rule is *instance first, then task*, and this
/// path takes only the second of them — which keeps the rule rather than bending
/// it, because a path that takes one lock cannot invert an order. The instance
/// is not read and not moved: nothing here depends on where the process is, only
/// on who holds the task, and locking a running instance to change an assignee
/// would block every decision on it for no benefit.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
pub async fn delegate(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: DelegateRequest,
) -> Result<WorkflowTask, AppError> {
    // **The same permission a decision needs, and no second one beside it.**
    // `workflow:task:execute` is "may this account work tasks"; whether *this*
    // task is theirs to hand over is a question about the row, answered by
    // `refuse_unless_held_by` below. A `workflow:task:delegate` would be a
    // permission to split off the ability to stop working on something, which
    // is `mod.rs`'s argument against splitting `claim` off `execute`.
    caller.require(TASK_EXECUTE)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    refuse_self_delegation(user_id, request.delegate_user_id)?;

    // Normalized before the lock, for the reason `decide` gives: a comment too
    // long is too long whatever the task turns out to be.
    let comment = normalize_comment(request.comment)?;

    let mut transaction = state.pool.begin().await?;

    let task = repo::lock_task(&mut transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    refuse_unless_open(task.status)?;
    refuse_unless_held_by(user_id, task.assignee_user_id)?;

    // Read inside the transaction rather than before it — coding standard §2.5
    // is satisfied either way, and here it buys correctness: an account
    // deactivated between the check and the write would otherwise receive the
    // task anyway, and this is the one write in the codebase whose whole point
    // is that somebody else can act.
    if !delegation_repo::user_is_available(&mut *transaction, tenant_id, request.delegate_user_id)
        .await?
    {
        return Err(delegate_unavailable());
    }

    if repo::delegate(
        &mut transaction,
        tenant_id,
        id,
        request.delegate_user_id,
        user_id,
    )
    .await?
        == 0
    {
        // The predicate carried the check again and lost. Under this
        // transaction's lock that is unreachable through this service; it is
        // mapped rather than assumed away, because the statement is what makes
        // the refusal true for a caller this service does not have.
        return Err(AppError::conflict(
            "this task changed hands while it was being handed over",
        ));
    }

    repo::record_task_history(
        &mut transaction,
        tenant_id,
        &repo::TaskHistoryEntry {
            task_id: id,
            instance_id: task.workflow_instance_id,
            document_id: task.document_id,
            // **From its own status to its own status**, which is the honest
            // shape: nothing about the task's progress changed. The row is here
            // for its `action` and its actor — the record that this task changed
            // hands, and who did it — and it is the only place the *chain* of
            // hand-offs survives, since the task's own column names whose
            // authority rather than who passed it on.
            from: Some(task.status),
            to: task.status,
            action: Some(TransitionAction::Delegate),
            comment: comment.as_deref(),
            actor: Some(user_id),
        },
    )
    .await?;

    transaction.commit().await?;

    let delegated = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Workflow.TaskDelegated",
            action: TransitionAction::Delegate.as_db(),
            object_type: TASK_OBJECT_TYPE,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: None,
            // **Not the comment**, which is `decide`'s rule and its reason:
            // this trail is read through `master-data:audit:read` by people who
            // hold no permission over the document, and a note about why an
            // approval was handed over is prose about somebody's requisition.
            // **D-12** and **D-32** drew that line; the note lives on the task's
            // own history row, behind `workflow:task:read`.
            reason: None,
            old_value: Some(json!({
                "assigneeUserId": task.assignee_user_id,
                "delegatedFromUserId": task.delegated_from_user_id,
            })),
            new_value: Some(json!({
                "assigneeUserId": delegated.assignee_user_id,
                "delegatedFromUserId": delegated.delegated_from_user_id,
                "documentId": task.document_id,
                "workflowInstanceId": task.workflow_instance_id,
                "commented": comment.is_some(),
            })),
        },
    )
    .await;

    Ok(delegated)
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
    comment: Option<String>,
) -> Result<DecisionResult, AppError> {
    caller.require(TASK_EXECUTE)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    // Normalized before anything is read, because it is the one refusal here
    // that depends on nothing in the database: a comment of four thousand
    // characters is too long whatever the task turns out to be, and finding
    // that out after two queries and a lock would be two queries and a lock
    // spent on a request that was never going to commit. Whether the *edge*
    // needs one is `engine::fire`'s to say — that depends on which transition
    // `condition` picks, which is not known until it is picked.
    let comment = normalize_comment(comment)?;

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
    let definition = definition_repo::definition_of_instance(
        &state.pool,
        tenant_id,
        instance_row.workflow_definition_id,
    )
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

    // Both halves of the grant, for the reason `claim_task` gives one screen up
    // and `repository::task::holds_role` gives in full (#225).
    let holds_candidate_role = match task.candidate_role_id {
        Some(role_id) => {
            repo::holds_role(
                &mut *transaction,
                tenant_id,
                user_id,
                role_id,
                task.candidate_department_id,
            )
            .await?
        }
        None => false,
    };

    refuse_unless_theirs(user_id, task.assignee_user_id, holds_candidate_role)?;

    // The write, carrying the predicate again: two callers who both passed the
    // check above produce one update of one row and one update of none.
    if repo::complete(
        &mut transaction,
        tenant_id,
        id,
        action,
        comment.as_deref(),
        user_id,
    )
    .await?
        == 0
    {
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
            action: Some(action.transition()),
            comment: comment.as_deref(),
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
            comment: comment.as_deref(),
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
            document_type_id: document.document_type_id,
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
        // The history row's provenance, and the third of the three places one
        // comment lands — the task (what was decided here), the decision record
        // (what was decided about this document), and the history (how the
        // document got here). Written from one value in one transaction, so the
        // three cannot disagree about what the approver said; #182 AC2 is that
        // the reason is visible where the decision is, and the history is the
        // one of the three a person reads.
        engine::DecisionProvenance {
            task_id: Some(id),
            // Off the locked row, which is the only place it could honestly
            // come from: the server wrote it when a window redirected this task
            // or when its holder handed it over, so it is a fact rather than a
            // claim (#184 AC4). It reaches the history row, and it reaches the
            // `allowedBy` check, which measures this caller against the rule as
            // the person whose work they are doing.
            on_behalf_of: task.delegated_from_user_id,
            comment: comment.as_deref(),
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
            // **The comment is not copied here, and `commented` below says only
            // that there was one.** `audit_events.reason` is the field for it
            // and this is deliberately not written into it: the audit trail is
            // read through `master-data:audit:read` by people who hold no
            // permission over the document, and a decision comment is prose an
            // approver wrote about somebody's requisition. **D-12** refused to
            // hand a record's field values back through its change history
            // without the record's own read permission, and **D-32** applied
            // the same line to form data. The reason itself lives on the
            // history row (§7.11), behind `workflow:instance:read`, which is
            // the permission the people it was written for hold.
            //
            // The flag is what an auditor actually needs from here: whether a
            // decision the workflow required a reason for was recorded with
            // one. That is a question about the control, and it is answerable
            // without reading the answer.
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
                "commented": comment.is_some(),
                // Both parties, for the reason the history row records them
                // (#184 AC4) — an approval taken on somebody else's authority
                // is a different fact from one taken on the approver's own, and
                // an audit trail that showed only the signature could not tell
                // them apart. Null on every decision nobody was standing in for.
                "onBehalfOfUserId": task.delegated_from_user_id,
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
