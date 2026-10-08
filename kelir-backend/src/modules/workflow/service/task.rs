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
//! # The locks these paths take
//!
//! **The instance, then the task, then a role**: `claim_task`, `delegate`,
//! `reassign` and `decide` all take the instance through
//! `lock_instance_then_task` or `lock_instance`, as [`super::engine`] says of
//! its callers. The check reads the *instance's* state to choose a
//! transition, so §2.5 puts a lock on that too — and two paths taking the two
//! rows in opposite orders is a deadlock at exactly the concurrency this
//! feature is for. **This says what these paths lock, not a complete lock
//! order** (ADR-0042 §2): the rows they write take more through their foreign
//! keys, as the next two paragraphs say.
//!
//! **A path takes the instance even when it never names it.** Each of them
//! writes a `workflow_task_history` row, whose foreign key to
//! `workflow_instances` takes `FOR KEY SHARE` on the instance. Until [#619]
//! the hand-off, the claim and the reassign locked only the task, so the key
//! took the instance *after* the task, and a decision holding the instance
//! deadlocked with them. The cost of the fix is that task paths now serialize
//! per instance.
//!
//! **The same row takes the document too**, through its foreign key to
//! `documents`, and a decision also updates the document. So a path that
//! locks the document and the instance has to take the instance first; the
//! submit does since [#663].
//!
//! [#619]: https://github.com/sujanto-gaws/kelir/issues/619
//! [#663]: https://github.com/sujanto-gaws/kelir/issues/663
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
//! # And a reassign is a fourth, taken by somebody who holds nothing
//!
//! [`reassign`] (FR-WF-017, [#512], **D-91**) moves an open task to a user or
//! a role an administrator names. Like the hand-off it fires nothing and moves
//! nothing, and writes one `workflow_task_history` row. Unlike it, the caller
//! is not the task's holder, so the permission is the whole of the check, and
//! the target is resolved by `assignment::reassign_to`, which is the engine's
//! own answer to *is this user or role live*. [ADR-0042] records the rest.
//!
//! **There is no cancel.** A reassign never leaves a task held by nobody, and
//! no route closes a single task (**D-91**).
//!
//! [#176]: https://github.com/sujanto-gaws/kelir/issues/176
//! [#177]: https://github.com/sujanto-gaws/kelir/issues/177
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184
//! [#512]: https://github.com/sujanto-gaws/kelir/issues/512
//! [ADR-0042]: ../../../../../docs/architectures/adr/0042.%20An%20Administrator%20Reassigns%20an%20Open%20Task%20and%20Nothing%20Cancels%20One.md

use std::collections::HashMap;

use serde_json::json;
use uuid::Uuid;

use super::super::domain::task::{
    claim_lost, delegate_unavailable, normalize_comment, reassign_closed, refuse_self_delegation,
    refuse_unless_held_by, refuse_unless_open, refuse_unless_theirs, target_cannot_decide,
};
use super::super::domain::{
    AssigneeType, AssignmentRule, DecisionAction, DelegateRequest, Graph, OpenTaskNeedingRole,
    ReassignTarget, ReassignTaskRequest, TaskStatus, TransitionAction, WorkflowTask,
};
use super::super::repository::{
    definition as definition_repo, instance as instance_repo, task as repo,
};
use super::super::{TASK_EXECUTE, TASK_REASSIGN};
use super::assignment::{self, AssignmentContext};
use super::engine;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry};
use crate::modules::document::repository as document_repo;
use crate::modules::identity::delegation_repository as delegation_repo;
use crate::modules::notification;
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

    // The instance id, read on the pool for the lock order (#619).
    let subject = repo::find_task(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    let mut transaction = state.pool.begin().await?;

    let locked = lock_instance_then_task(
        &mut transaction,
        tenant_id,
        subject.workflow_instance_id,
        id,
    )
    .await?;

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
            object_type: ObjectType::WorkflowTask,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: caller.ip_address(),
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
/// # ~~One lock, and it is the task's~~ The instance, then the task ([#619])
///
/// [`super::engine`]'s ordering rule is *instance first, then task*. **This
/// path used to take only the second**, on the argument that a path taking one
/// lock cannot invert an order, and that locking a running instance to change
/// an assignee would block every decision on it for no benefit. The argument
/// missed a lock: the `workflow_task_history` row below has a foreign key to
/// `workflow_instances`, and inserting it takes `FOR KEY SHARE` on the
/// instance. So the path took the task, then the instance, and deadlocked
/// with a decision taking them the other way. It now takes both, through
/// `lock_instance_then_task`. The instance is still not read and not moved.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
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

    // The instance id, read on the pool for the lock order (#619).
    let subject = repo::find_task(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    let mut transaction = state.pool.begin().await?;

    let task = lock_instance_then_task(
        &mut transaction,
        tenant_id,
        subject.workflow_instance_id,
        id,
    )
    .await?;

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
            action: Some(TransitionAction::Delegate.into()),
            comment: comment.as_deref(),
            actor: Some(user_id),
        },
    )
    .await?;

    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id,
            document_id: Some(task.document_id),
            workflow_instance_id: Some(task.workflow_instance_id),
            task_id: Some(id),
            attachment_id: None,
            comment_id: None,
            event_type: "Workflow.TaskDelegated",
            category: crate::modules::activity::domain::EventCategory::Workflow,
            actor_user_id: Some(user_id),
            actor_name: Some(caller.username()),
            action_summary: "Handed a task to somebody else",
            // **Not who it went to** (#292 AC2, **D-45**). The second party to a
            // delegation is the workflow's fact, kept in `workflow_history` and
            // on the task itself, both behind the workflow's own read. The
            // `task_id` above is how a reader who holds it gets there.
            details: serde_json::json!({}),
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
            object_type: ObjectType::WorkflowTask,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: caller.ip_address(),
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

/// Moves an open task to a user or a role an administrator names (FR-WF-017,
/// [#512], **D-91**).
///
/// # The order, and the lock each step takes
///
/// 1. **The permission**, then the request's shape: exactly one target, and a
///    comment within bounds. Both refuse before anything is read.
/// 2. **The instance, then the task, both `FOR UPDATE`**
///    (`lock_instance_then_task`, [#619]). The history row in step 6 takes the
///    instance through its foreign key whether this path names it or not, so
///    the engine's *instance first, then task* order is kept by taking the
///    instance first, as `decide` does. It used to take only the task, and
///    deadlocked with a decision.
/// 3. **The target, through `assignment::reassign_to`.** A role is read
///    `FOR KEY SHARE`, which `identity::service::delete_role`'s `FOR UPDATE`
///    waits on and which waits on it, so a role being deleted is either gone
///    when this reads it, and refused, or counted by the delete once this
///    commits. Instance, task, then role is the order a decision takes too
///    (`decide` locks the instance and the task, and `engine::fire` resolves
///    the edge's role after).
///    **A closed task is refused with a 409 first**, under the task lock and
///    before the target is read: a task whose instance has moved on would
///    otherwise be judged against a state it is not in.
/// 4. **Whether the target could decide it**, by
///    [`refuse_unless_target_can_decide`]: the instance and its pinned
///    definition are read after the task lock, and the target is measured
///    against the decision edges out of the current state.
/// 5. **The write, carrying the open-status predicate.** Zero rows is a 409.
///    Under the lock the task cannot have closed since step 2, so this is the
///    backstop, not the check.
/// 6. **One `workflow_task_history` row**, `REASSIGN`, in the same
///    transaction. No `workflow_history` row: the process did not move.
///
/// The audit record follows the commit, as every task action's does.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
pub async fn reassign(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: ReassignTaskRequest,
) -> Result<WorkflowTask, AppError> {
    caller.require(TASK_REASSIGN)?;

    let tenant_id = caller.tenant_id();
    let user_id = caller.user_id();

    let target = request.target()?;
    let comment = normalize_comment(request.comment)?;

    // The document's facts an `allowedBy` rule reads, resolved before the
    // transaction as `decide` resolves them: a task's document never changes,
    // and a submitted document's creator and requested department do not
    // either.
    let subject = repo::find_task(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;
    let document = document_repo::find_document(&state.pool, tenant_id, subject.document_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;
    // The owner's department is read now, not at the raise: an edge measures
    // it live (#579, the product owner's Q2).
    let context = AssignmentContext::of_document(
        &state.pool,
        tenant_id,
        document.document_type_id,
        document.created_by,
        document.requested_for_department_id,
    )
    .await?;

    let mut transaction = state.pool.begin().await?;

    let task = lock_instance_then_task(
        &mut transaction,
        tenant_id,
        subject.workflow_instance_id,
        id,
    )
    .await?;

    // A closed task is a 409 before its target is looked at. The checks below
    // judge the target against the instance's *current* state, which a closed
    // task of a multi-stage workflow has left: judged there, its reassign
    // would be a 422 about a decision it will never take.
    if !task.status.is_open() {
        return Err(reassign_closed(task.status));
    }

    let assignment = assignment::reassign_to(&mut transaction, tenant_id, target.clone()).await?;

    refuse_unless_target_can_decide(
        &mut transaction,
        tenant_id,
        task.workflow_instance_id,
        &target,
        &assignment,
        context,
    )
    .await?;

    if repo::reassign(&mut transaction, tenant_id, id, &assignment, user_id).await? == 0 {
        return Err(reassign_closed(task.status));
    }

    // The status the statement wrote, by the rule it wrote it with.
    let status = if assignment.assignee_user_id.is_some() {
        TaskStatus::Assigned
    } else {
        TaskStatus::Created
    };

    repo::record_task_history(
        &mut transaction,
        tenant_id,
        &repo::TaskHistoryEntry {
            task_id: id,
            instance_id: task.workflow_instance_id,
            document_id: task.document_id,
            // Both ends, since a reassign to a role unclaims a claimed task
            // and a reassign to a user assigns an unclaimed one.
            from: Some(task.status),
            to: status,
            action: Some(repo::TaskHistoryAction::Reassign),
            comment: comment.as_deref(),
            actor: Some(user_id),
        },
    )
    .await?;

    transaction.commit().await?;

    let reassigned = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Workflow.TaskReassigned",
            action: "REASSIGN",
            object_type: ObjectType::WorkflowTask,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: caller.ip_address(),
            // **Not the comment**, for `delegate`'s reason: the note is prose
            // about somebody's document, and it lives on the task's history
            // row behind `workflow:task:read` (**D-12**, **D-32**).
            reason: None,
            old_value: Some(json!({
                "status": task.status,
                "assigneeUserId": task.assignee_user_id,
                "candidateRoleId": task.candidate_role_id,
                "candidateDepartmentId": task.candidate_department_id,
                "delegatedFromUserId": task.delegated_from_user_id,
            })),
            new_value: Some(json!({
                "status": reassigned.status,
                "assigneeUserId": reassigned.assignee_user_id,
                "candidateRoleId": reassigned.candidate_role_id,
                "candidateDepartmentId": reassigned.candidate_department_id,
                "delegatedFromUserId": reassigned.delegated_from_user_id,
                "documentId": task.document_id,
                "workflowInstanceId": task.workflow_instance_id,
                "commented": comment.is_some(),
            })),
        },
    )
    .await;

    Ok(reassigned)
}

/// Refuses a reassign to a holder who could not decide the task ([#512], the
/// product owner's decision of 2026-09-26).
///
/// # The rule
///
/// The **decision edges** are the `APPROVE`, `REJECT` and `RETURN` transitions
/// out of the instance's current state, read from the published definition
/// the instance is pinned to, as `decide` reads it. The target passes when it
/// satisfies **at least one** of them, because one decision it can take is a
/// decision the task can get:
///
/// * **A user** passes an edge `assignment::permits` accepts them on, with no
///   one on whose behalf: a reassigned task names nobody it is delegated from.
///   `permits` is the check the decision itself makes, so `USER`, `ROLE`,
///   `OWNER` and `DEPARTMENT_ROLE`, department scope included, mean exactly
///   what they will mean when the user decides.
/// * **A role** passes an edge that names it, by `assignment::names_role`:
///   `ROLE`, or `DEPARTMENT_ROLE` whose department resolves. `OWNER` and
///   `USER` edges are not about a role, so a role target does not pass them.
/// * **An edge with no `allowedBy`** admits anybody (the engine checks
///   nothing on it), so it passes every target.
///
/// An edge whose own rule no longer resolves, such as one naming a deleted
/// role, is refused at the decision too, so it counts as not passed rather
/// than failing the reassign.
///
/// # A state with no decision edges is not judged
///
/// A `RETURNED` state whose task is the owner's correction leaves only by
/// `RESUBMIT`, which is taken on the document, not decided on the task. There
/// is no decision to strand, so the reassign is allowed.
///
/// # Locking
///
/// The instance is already locked: `reassign` takes it before the task, in
/// `lock_instance_then_task` ([#619]), so the state read here cannot move
/// meanwhile. A decision and a resubmit can each move a state with an open
/// task, and both wait on the same instance lock ([#663]).
/// `permits` and `names_role` read each edge's role `FOR KEY SHARE`, as a
/// decision does.
///
/// [#512]: https://github.com/sujanto-gaws/kelir/issues/512
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
async fn refuse_unless_target_can_decide(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
    target: &ReassignTarget,
    assignment: &assignment::ResolvedAssignment,
    context: AssignmentContext,
) -> Result<(), AppError> {
    let instance = instance_repo::find_instance(&mut **transaction, tenant_id, instance_id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    let definition = definition_repo::definition_of_instance(
        &mut **transaction,
        tenant_id,
        instance.workflow_definition_id,
    )
    .await?
    .ok_or_else(|| AppError::Internal {
        source: anyhow::anyhow!(
            "instance {} runs definition {} which does not exist",
            instance.id,
            instance.workflow_definition_id
        ),
    })?;

    let graph = Graph::parse(&definition.definition_json, definition.version);
    let state = &instance.current_state;

    let decisions: Vec<_> = graph
        .actions_from(state)
        .into_iter()
        .filter(|edge| {
            matches!(
                edge.action,
                TransitionAction::Approve | TransitionAction::Reject | TransitionAction::Return
            )
        })
        .collect();

    if decisions.is_empty() {
        return Ok(());
    }

    for edge in &decisions {
        let Some(rule) = &edge.allowed_by else {
            return Ok(());
        };

        let path = format!("transitions.{state}.{}.allowedBy", edge.action.as_db());

        let passes = match target {
            ReassignTarget::User(user) => {
                assignment::permits(
                    transaction,
                    tenant_id,
                    rule,
                    context,
                    Some(*user),
                    None,
                    &path,
                )
                .await
            }
            ReassignTarget::Role(_) => {
                let role_id = assignment
                    .candidate_role_id
                    .ok_or_else(|| AppError::Internal {
                        source: anyhow::anyhow!("a role target resolved to no role"),
                    })?;

                assignment::names_role(transaction, tenant_id, rule, context, role_id, &path).await
            }
        };

        match passes {
            Ok(true) => return Ok(()),
            // Not passed, including an edge whose own rule no longer
            // resolves: the decision would refuse that edge too.
            Ok(false) | Err(AppError::Validation { .. }) => continue,
            Err(other) => return Err(other),
        }
    }

    let mut needs: Vec<String> = decisions
        .iter()
        .filter_map(|edge| edge.allowed_by.as_ref())
        .map(describe_rule)
        .collect();
    needs.sort();
    needs.dedup();

    let field = match target {
        ReassignTarget::User(_) => "userId",
        ReassignTarget::Role(_) => "roleCode",
    };

    Err(target_cannot_decide(field, state, &needs))
}

/// An `allowedBy` rule as an administrator reads it in a refusal.
fn describe_rule(rule: &AssignmentRule) -> String {
    let role = rule.role_code.as_deref().unwrap_or_default();

    match rule.assignee_type {
        AssigneeType::User => format!("user {}", rule.user_id.as_deref().unwrap_or_default()),
        AssigneeType::Role => format!("role `{role}`"),
        AssigneeType::DepartmentRole => format!(
            "role `{role}` in department `{}`",
            rule.department_scope.as_deref().unwrap_or("any")
        ),
        AssigneeType::Owner => "the document's creator".to_owned(),
    }
}

/// The task's instance, then the task, both `FOR UPDATE`: the order every
/// path in this module takes, `decide`'s included ([#619]).
///
/// **A path that writes a `workflow_task_history` row takes the instance
/// whether it locks it or not.** The row's foreign key to `workflow_instances`
/// takes `FOR KEY SHARE` on the instance, which conflicts with `decide`'s
/// `FOR UPDATE`. A path that locked only the task therefore took the task and
/// then the instance, and deadlocked with a decision that held the instance
/// and wanted the task. Taking the instance explicitly and first makes the
/// key's lock one this transaction already holds.
///
/// The row's foreign key to `documents` takes `FOR KEY SHARE` on the document
/// as well, after both locks here. A path that locks the document has to take
/// the instance before it, as the submit does since [#663].
///
/// `instance_id` is the task's, read on the pool before the transaction,
/// because the instance must be locked before the task can be read under a
/// lock. A task never changes instance, so a mismatch under the lock is
/// unreachable today; it is refused with a 409 rather than assumed away,
/// because the lock would then cover the wrong instance.
///
/// A missing instance is a 404 "Workflow instance" and a missing task a 404
/// "Task", as `decide` answers them.
///
/// [#619]: https://github.com/sujanto-gaws/kelir/issues/619
/// [#663]: https://github.com/sujanto-gaws/kelir/issues/663
async fn lock_instance_then_task(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
    id: Uuid,
) -> Result<repo::LockedTask, AppError> {
    instance_repo::lock_instance(transaction, tenant_id, instance_id)
        .await?
        .ok_or_else(|| AppError::not_found("Workflow instance"))?;

    let task = repo::lock_task(transaction, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Task"))?;

    if task.workflow_instance_id != instance_id {
        return Err(AppError::conflict(
            "this task belongs to a different workflow instance than when it was read",
        ));
    }

    Ok(task)
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

    let graph = Graph::parse(&definition.definition_json, definition.version);

    let document = document_repo::find_document(&state.pool, tenant_id, subject.document_id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    let variables = variable_context(
        &instance_repo::variables_of(&state.pool, tenant_id, instance_row.id).await?,
    );

    // On the pool with the document's other facts, before the transaction, so
    // this path still holds one pooled connection at a time (**D-35**). The
    // owner's department is read now, which is what an edge measures (#579,
    // the product owner's Q2); the task's own check reads its snapshot.
    let context = AssignmentContext::of_document(
        &state.pool,
        tenant_id,
        document.document_type_id,
        document.created_by,
        document.requested_for_department_id,
    )
    .await?;

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
            action: Some(action.transition().into()),
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
        context,
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

    // **The timeline, in the decision's own transaction** (#247 AC2). Recorded
    // here rather than inside `engine::fire` on purpose: the engine is a state
    // machine that takes an actor's id and no name, and threading one through
    // would grow the signature its own documentation defends. The service is
    // where the caller, the transaction and the outcome are all in hand.
    crate::modules::activity::service::record(
        &mut transaction,
        &crate::modules::activity::service::Happening {
            tenant_id,
            document_id: Some(task.document_id),
            workflow_instance_id: Some(task.workflow_instance_id),
            task_id: Some(id),
            attachment_id: None,
            comment_id: None,
            event_type: "Workflow.Decided",
            category: crate::modules::activity::domain::EventCategory::Workflow,
            actor_user_id: Some(user_id),
            actor_name: Some(caller.username()),
            action_summary: "Decided a task",
            // **What was decided stays; on whose behalf does not**
            // (#292 AC2, **D-45**). The action and the two states are what
            // moved *this document*, which is the question the timeline exists
            // to answer and which the document's own read already covers.
            // `onBehalfOfUserId` answers a different one — that a delegation
            // happened and who was behind it (#184 AC4) — and `workflow_history`
            // keeps it, behind the workflow's read.
            details: serde_json::json!({
                "action": action.as_db(),
                "from": fired.from_state,
                "to": fired.to_state,
            }),
        },
    )
    .await?;

    // **The owner is told their document moved** (#251 AC2), in the decision's
    // own transaction like the timeline entry above it and for the same reason:
    // a rejection somebody was told about and which then rolled back is worse
    // than one they hear about a moment later.
    //
    // **Not the actor, when the actor is the owner.** An approver deciding
    // their own document — an `OWNER` task, or an owner who also holds the
    // approving role — already knows: they are the one who pressed it. The task
    // path takes the opposite view and says why there; the difference is that
    // this notification announces *somebody else acted on your document*, and
    // when nobody else did there is nothing to announce.
    if let Some(owner) = document.created_by.filter(|owner| *owner != user_id) {
        let body = format!(
            "{} was {} by {}.",
            document
                .document_number
                .as_deref()
                .unwrap_or("Your document"),
            action.as_db().to_lowercase(),
            caller.username(),
        );

        notification::service::notify(
            &mut transaction,
            &notification::service::Telling {
                tenant_id,
                recipient_user_id: owner,
                document_id: Some(task.document_id),
                workflow_instance_id: Some(task.workflow_instance_id),
                task_id: Some(id),
                notification_type: notification::domain::NotificationType::DocumentDecided,
                title: &format!("Your document was {}", action.as_db().to_lowercase()),
                body: &body,
                actor: Some(user_id),
            },
        )
        .await?;
    }

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            // Naming convention §7's own worked example.
            event_type: "Workflow.TaskCompleted",
            action: action.as_db(),
            object_type: ObjectType::WorkflowTask,
            object_id: id,
            actor_user_id: Some(user_id),
            ip_address: caller.ip_address(),
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

/// How many open tasks could not be decided if `role_id` were deleted
/// (**D-89**, [#487]). `repository::task::count_open_tasks_needing_role` says
/// what *needs* means.
///
/// `identity::service::delete_role` asks, and refuses the delete while the
/// answer is not zero. It takes the caller's transaction because the answer is
/// only true under the lock that transaction holds on the role row: see that
/// function for the lock, and `assignment::direct` for the one task creation
/// takes against it.
///
/// [#487]: https://github.com/sujanto-gaws/kelir/issues/487
pub async fn open_tasks_needing_role(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<i64, AppError> {
    Ok(repo::count_open_tasks_needing_role(&mut **transaction, tenant_id, role_id).await?)
}

/// [`open_tasks_needing_role`]'s count for each of `role_ids`, from one
/// statement ([#508]). A role no open task needs is absent, and reads as zero.
///
/// The Roles screen's count beside each role (**D-91** (2)): what an
/// administrator reads to find a role whose last holder has left while tasks
/// still need it, and clears with `POST /api/v1/workflow/tasks/{id}/reassign`.
/// Read on the pool, **outside any delete's lock**, like
/// [`list_open_tasks_needing_role`]: a diagnostic. The caller has checked the
/// permission and read the roles.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
pub async fn open_tasks_needing_roles(
    state: &AppState,
    tenant_id: Uuid,
    role_ids: &[Uuid],
) -> Result<HashMap<Uuid, i64>, AppError> {
    Ok(repo::count_open_tasks_needing_roles(&state.pool, tenant_id, role_ids).await?)
}

/// The tasks [`open_tasks_needing_role`] counts, a page of them, and their
/// number from the same statement ([#532]).
///
/// Read on the pool, **outside any delete's lock**: a diagnostic, not a
/// promise, so a task decided or raised between this read and a delete can
/// make the two answers differ. The caller has checked the permission and
/// that the role is live.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
pub async fn list_open_tasks_needing_role(
    state: &AppState,
    tenant_id: Uuid,
    role_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<OpenTaskNeedingRole>, i64), AppError> {
    let page =
        repo::open_tasks_needing_role(&state.pool, tenant_id, role_id, limit, offset).await?;

    Ok((page.rows, page.total))
}
