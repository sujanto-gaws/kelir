//! Queries for `workflow_tasks`, its history, and the decisions recorded
//! against it (§7.6, §7.7, §7.8).

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::workflow::domain::{
    DecisionAction, TaskStatus, TransitionAction, WorkflowTask,
};

/// The columns a transition writes when it generates a task.
///
/// `assignee_user_id` and `candidate_role_id` are both here and **at most one of
/// them is ever `Some`** — [`super::super::service::assignment`] is what makes
/// that true, and [`crate::modules::workflow::domain::task`] says why it matters
/// to the person looking at the inbox.
pub struct NewTask<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub task_ref: &'a str,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    pub task_definition_key: &'a str,
    pub task_name: &'a str,
    pub task_type: &'a str,
    pub priority: &'a str,
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    pub candidate_department_id: Option<Uuid>,
    /// Whose authority the assignee is exercising, when a delegation window put
    /// the task in their hands rather than the delegator's
    /// ([#184](https://github.com/sujanto-gaws/kelir/issues/184) AC2).
    ///
    /// Written by the same statement that writes the assignee, from the same
    /// [`ResolvedAssignment`][r]: a task that says who it is for and does not
    /// say why them is a task whose decision cannot record both parties.
    ///
    /// [r]: super::super::service::assignment::ResolvedAssignment
    pub delegated_from_user_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

/// A task as the engine reads it under a lock.
pub struct LockedTask {
    pub id: Uuid,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    pub status: TaskStatus,
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    /// The department the assignment resolved to, when the rule was a
    /// `DEPARTMENT_ROLE`. Read under the lock because the authorization that
    /// follows depends on it (#225) — a locked row missing a field the check
    /// needs is a check made against the pool.
    pub candidate_department_id: Option<Uuid>,
    /// Whose authority this task's holder is exercising, if anybody's.
    ///
    /// Read under the lock because both things that follow depend on it: the
    /// `allowedBy` check measures the actor against this person as well as
    /// themselves, and the history row records the pair
    /// ([#184](https://github.com/sujanto-gaws/kelir/issues/184) AC4, AC5).
    pub delegated_from_user_id: Option<Uuid>,
}

pub async fn insert_task(
    transaction: &mut sqlx::PgTransaction<'_>,
    task: &NewTask<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO workflow_tasks
            (id, tenant_id, task_ref, workflow_instance_id, document_id,
             task_definition_key, task_name, task_type, status, priority,
             assignee_user_id, candidate_role_id, candidate_department_id,
             delegated_from_user_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                -- A task with a named assignee is `ASSIGNED` from the moment it
                -- exists; one offered to a role is `CREATED` until somebody
                -- claims it. The two statuses are what the inbox's `MINE` and
                -- `ROLE` are derived from, so deriving them here from the
                -- assignment keeps one answer rather than two.
                --
                -- **A delegated task is `ASSIGNED`, not `DELEGATED`** (#184).
                -- `DELEGATED` is in §7.6's `CHECK` and outside
                -- `uq_workflow_tasks_open_per_instance` and outside the inbox's
                -- open filter, so writing it would leave a running process with
                -- no open task and hide the work from the person just given it.
                -- Who holds the task is the assignee; the status says where the
                -- work has got to.
                CASE WHEN $10::uuid IS NULL THEN 'CREATED' ELSE 'ASSIGNED' END,
                $9, $10, $11, $12, $13, $14)
        "#,
        task.id,
        task.tenant_id,
        task.task_ref,
        task.workflow_instance_id,
        task.document_id,
        task.task_definition_key,
        task.task_name,
        task.task_type,
        task.priority,
        task.assignee_user_id,
        task.candidate_role_id,
        task.candidate_department_id,
        task.delegated_from_user_id,
        task.created_by,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// The open task of an instance, held for the rest of the transaction.
///
/// **Taken after the instance, never before it** — see
/// [`super::instance::lock_instance`], which states the ordering this module
/// keeps on every path.
pub async fn lock_open_task_of_instance(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    instance_id: Uuid,
) -> Result<Option<LockedTask>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, workflow_instance_id, document_id, status,
               assignee_user_id, candidate_role_id, candidate_department_id,
               delegated_from_user_id
        FROM workflow_tasks
        WHERE tenant_id = $1 AND workflow_instance_id = $2 AND deleted_at IS NULL
          AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')
        FOR UPDATE
        "#,
        tenant_id,
        instance_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| LockedTask {
        id: row.id,
        workflow_instance_id: row.workflow_instance_id,
        document_id: row.document_id,
        status: TaskStatus::from_db(&row.status),
        assignee_user_id: row.assignee_user_id,
        candidate_role_id: row.candidate_role_id,
        candidate_department_id: row.candidate_department_id,
        delegated_from_user_id: row.delegated_from_user_id,
    }))
}

/// One task, held for the rest of the transaction.
pub async fn lock_task(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<LockedTask>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, workflow_instance_id, document_id, status,
               assignee_user_id, candidate_role_id, candidate_department_id,
               delegated_from_user_id
        FROM workflow_tasks
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| LockedTask {
        id: row.id,
        workflow_instance_id: row.workflow_instance_id,
        document_id: row.document_id,
        status: TaskStatus::from_db(&row.status),
        assignee_user_id: row.assignee_user_id,
        candidate_role_id: row.candidate_role_id,
        candidate_department_id: row.candidate_department_id,
        delegated_from_user_id: row.delegated_from_user_id,
    }))
}

/// Claims an unassigned task, **conditionally on it still being unassigned**
/// ([#176](https://github.com/sujanto-gaws/kelir/issues/176) AC3).
///
/// One statement. Two users claiming simultaneously produce one update of one
/// row and one update of none — there is no window between a read and a write
/// for the second to fall into, because there is no read.
pub async fn claim(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    claimant: Uuid,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_tasks SET
            assignee_user_id = $3,
            status           = 'ASSIGNED',
            updated_by       = $3,
            updated_at       = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
          AND assignee_user_id IS NULL AND status = 'CREATED'
        "#,
        tenant_id,
        id,
        claimant,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Completes a task, **conditionally on it still being open**
/// ([#177](https://github.com/sujanto-gaws/kelir/issues/177) AC2).
///
/// The predicate is the whole guard: a task already decided produces zero rows,
/// and the service turns that into a 409 naming the status rather than a silent
/// second decision.
///
/// `comment` is the reason given with the decision (FR-TASK-006,
/// [#182](https://github.com/sujanto-gaws/kelir/issues/182)), already trimmed
/// and bounded by `domain::task::normalize_comment`. It is written **in the
/// statement that completes the task** rather than in an update beside it, for
/// the reason this module keeps everywhere: a task recorded as decided with the
/// reason still in flight is a task whose reason can be lost by a failure
/// between two writes.
pub async fn complete(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    action: DecisionAction,
    comment: Option<&str>,
    actor: Uuid,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_tasks SET
            status       = 'COMPLETED',
            action       = $3,
            comment      = $4,
            completed_by = $5,
            completed_at = now(),
            -- A decision taken by a role holder who never claimed the task
            -- records them as the assignee, so the row says who did it rather
            -- than leaving an open question beside a completed task.
            assignee_user_id = COALESCE(assignee_user_id, $5),
            updated_by   = $5,
            updated_at   = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
          AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')
        "#,
        tenant_id,
        id,
        action.as_db(),
        comment,
        actor,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Hands an assigned task to somebody else, **conditionally on it still being
/// the caller's** (FR-WF-009, FR-TASK-008; [#184]).
///
/// One statement, carrying the predicate the service already checked, for
/// `claim`'s and `complete`'s reason: two callers who both read a task as theirs
/// produce one update of one row and one update of none. The predicate names the
/// current assignee rather than only the status, because the failure this
/// prevents is not "already decided" — it is handing on a task that somebody
/// else has meanwhile been handed.
///
/// # `delegated_from_user_id` names whose authority, not who passed it on
///
/// `COALESCE` is the whole of that distinction. A task that reaches Budi because
/// Ani's window redirected it already names Ani; if Budi then hands it to Citra,
/// the column still says Ani — because the authority being exercised is still
/// Ani's, and that is the question the `allowedBy` check and the history row
/// both ask it. Overwriting it with Budi would make an edge Ani was allowed to
/// take unreachable by the person now holding her task, and it would drop her
/// name from the record of a decision made on her behalf.
///
/// The *chain* is not lost by this: each hand-off writes its own
/// `workflow_task_history` row, with the person who made it as the actor.
///
/// # The status does not move
///
/// See [`insert_task`]: `DELEGATED` would take the row out of the open set the
/// unique index and the inbox are both written in terms of. The task is still
/// open, still this instance's one open task, and now somebody else's.
///
/// [#184]: https://github.com/sujanto-gaws/kelir/issues/184
pub async fn delegate(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    delegate_user_id: Uuid,
    from_user_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE workflow_tasks SET
            assignee_user_id       = $3,
            delegated_from_user_id = COALESCE(delegated_from_user_id, $4),
            updated_by             = $4,
            updated_at             = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
          AND assignee_user_id = $4
          AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')
        "#,
        tenant_id,
        id,
        delegate_user_id,
        from_user_id,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Appends one row to `workflow_task_history` (§7.7).
///
/// Append-only: the table has no `updated_at` and no `deleted_at`, so a history
/// row is a fact rather than a record. `actor_user_id` is nullable because the
/// engine creates tasks with no person behind the move.
///
/// **Written in the transaction that moved the task**, so a task cannot end in a
/// state its own history does not explain.
pub async fn record_task_history(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entry: &TaskHistoryEntry<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO workflow_task_history
            (id, tenant_id, task_id, workflow_instance_id, document_id,
             old_status, new_status, action, comment, actor_user_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        Uuid::now_v7(),
        tenant_id,
        entry.task_id,
        entry.instance_id,
        entry.document_id,
        entry.from.map(TaskStatus::as_db),
        entry.to.as_db(),
        entry.action.map(TransitionAction::as_db),
        entry.comment,
        entry.actor,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// One move of one task, as the history table records it.
///
/// A record rather than nine positional arguments: three of them are `Uuid` and
/// two are `Option<Uuid>`, so a transposition would compile and file a task's
/// history under another task's document.
pub struct TaskHistoryEntry<'a> {
    pub task_id: Uuid,
    pub instance_id: Uuid,
    pub document_id: Uuid,
    pub from: Option<TaskStatus>,
    pub to: TaskStatus,
    /// **A transition verb, not a decision verb** (#184).
    ///
    /// It was `DecisionAction` — three values — while the only actions a task
    /// carried were the three a decision issues. `DELEGATE` is a fourth thing
    /// that happens to a task and is not a decision about the document, and
    /// widening `DecisionAction` to hold it would put a verb in the request
    /// type that `POST /decision` must refuse. §7.7's column is `VARCHAR(40)`
    /// and §7.3's vocabulary is the transition one, which is what this now
    /// spells.
    pub action: Option<TransitionAction>,
    /// What the person said about it, where they said anything.
    ///
    /// Written by the hand-off (#184) and by nothing else so far: a decision's
    /// reason has three homes already (§7.6, §7.8, §7.11) and this is not a
    /// fourth. §7.7's `comment` column existed with no writer until a task
    /// could change hands without being decided — which is the one thing that
    /// happens to a task where the *only* place a reason could live is the
    /// task's own history.
    pub comment: Option<&'a str>,
    pub actor: Option<Uuid>,
}

/// Records the formal decision (§7.8).
///
/// **Not the same row as the history entry, and `mod.rs` says which is which**:
/// this answers *what was decided about this document*, the history answers
/// *what happened to this task*, and FR-WF-012 — the document's own account of
/// how it got here — is Sprint 11's and is neither.
///
/// `comment` is the reason the approver gave (FR-TASK-006,
/// [#182](https://github.com/sujanto-gaws/kelir/issues/182)) — the same value
/// this transaction writes to the task and to the history row, so the formal
/// record and the account a person reads cannot say different things about one
/// decision.
pub async fn record_decision(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    decision: &Decision<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO approval_decisions
            (id, tenant_id, document_id, workflow_instance_id, task_id,
             approver_user_id, approver_role_id, decision, decision_level,
             comment, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $6)
        "#,
        Uuid::now_v7(),
        tenant_id,
        decision.document_id,
        decision.instance_id,
        decision.task_id,
        decision.approver,
        decision.approver_role_id,
        decision.action.as_db(),
        decision.decision_level,
        decision.comment,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// One decision, as the formal record stores it.
///
/// A record for [`TaskHistoryEntry`]'s reason, and more sharply: four of these
/// fields are ids of four different things, and a decision filed against the
/// wrong task is the failure #177 calls a signature on the wrong document.
pub struct Decision<'a> {
    pub document_id: Uuid,
    pub instance_id: Uuid,
    pub task_id: Uuid,
    pub approver: Uuid,
    pub approver_role_id: Option<Uuid>,
    pub action: DecisionAction,
    /// The task's own definition key — `finance_approval` — which is what a
    /// reporting query asks "who signed off at which step" with. Not invented
    /// here: it is the definition's word for this step.
    pub decision_level: Option<&'a str>,
    /// The reason given with the decision. `None` where the edge did not ask
    /// for one and the approver did not offer one.
    pub comment: Option<&'a str>,
}

/// Whether the caller currently holds a role, **read from `user_roles` rather
/// than from the token**.
///
/// A token's `roles` claim is a snapshot from sign-in: a role granted an hour
/// ago is not in it, and a role revoked an hour ago still is. The second is the
/// one that matters — a check made against a stale claim would let somebody act
/// on an approval they may no longer act on, which is a leak that looks like a
/// working control. The validity window is honoured in the same predicate,
/// because a grant that has expired is not a grant.
///
/// # The department half, and what an unscoped grant means
///
/// `DEPARTMENT_ROLE` resolves to a role **and** a department
/// ([JSON Workflow Schema](../../../../../docs/schema/JSON%20Workflow%20Schema.md)
/// §5.3: *"`roles` plus `user_roles.department_id`"*), and `department` is the
/// task's `candidate_department_id`. Until
/// [#225](https://github.com/sujanto-gaws/kelir/issues/225) this function
/// filtered on the role alone, so a task offered to Finance's approver was
/// decidable by any holder of that role in any department.
///
/// **A grant with no department satisfies a department-scoped task**, and that
/// is a decision rather than a fallthrough. `0002` calls the column an
/// *optional* department-scoped grant: a row naming a department is a grant
/// *within* it, and a row naming none is the same role held generally. Reading
/// the null as "no departments" instead would be the stricter rule and the
/// wrong one twice over — it would strand every approval in every deployment
/// whose grants predate this column being read, which is all of them, and it
/// would make the unscoped grant weaker than the scoped one it is a
/// generalization of.
///
/// **A task with no department is unchanged**: a plain `ROLE` assignment
/// carries no `candidate_department_id`, so every holder of the role qualifies
/// exactly as before.
pub async fn holds_role<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    department: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "found!"
        FROM user_roles
        WHERE tenant_id = $1 AND user_id = $2 AND role_id = $3 AND deleted_at IS NULL
          AND (valid_from IS NULL OR valid_from <= current_date)
          AND (valid_to   IS NULL OR valid_to   >= current_date)
          AND ($4::uuid IS NULL OR department_id IS NULL OR department_id = $4)
        LIMIT 1
        "#,
        tenant_id,
        user_id,
        role_id,
        department
    )
    .fetch_optional(executor)
    .await?;

    Ok(found.is_some())
}

/// Reads one task.
pub async fn find_task<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkflowTask>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT t.id, t.task_ref, t.workflow_instance_id, t.document_id,
               t.task_definition_key, t.task_name, t.task_type, t.status,
               t.assignee_user_id, t.candidate_role_id, r.role_code AS "candidate_role_code?",
               t.candidate_department_id, t.delegated_from_user_id,
               t.priority, t.due_at, t.action,
               t.completed_by, t.completed_at, t.created_at
        FROM workflow_tasks t
        LEFT JOIN roles r ON r.id = t.candidate_role_id
        WHERE t.tenant_id = $1 AND t.id = $2 AND t.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| WorkflowTask {
        id: row.id,
        task_ref: row.task_ref,
        workflow_instance_id: row.workflow_instance_id,
        document_id: row.document_id,
        task_definition_key: row.task_definition_key,
        task_name: row.task_name,
        task_type: row.task_type,
        status: TaskStatus::from_db(&row.status),
        assignee_user_id: row.assignee_user_id,
        candidate_role_id: row.candidate_role_id,
        candidate_role_code: row.candidate_role_code,
        candidate_department_id: row.candidate_department_id,
        delegated_from_user_id: row.delegated_from_user_id,
        priority: row.priority,
        due_at: row.due_at,
        action: row.action.as_deref().and_then(parse_action),
        completed_by: row.completed_by,
        completed_at: row.completed_at,
        created_at: row.created_at,
    }))
}

/// The stored action as the vocabulary this binary issues.
///
/// `None` for a verb a later release writes, rather than a panic or a
/// fabricated value: a newer database read by a rolled-back binary is exactly
/// the N−1 case [release process](../../../../../docs/standards/04.%20Release%20Process.md)
/// §6 requires to keep working. `RETURN` joined the list with
/// [#183](https://github.com/sujanto-gaws/kelir/issues/183), and `DELEGATE`,
/// `ESCALATE`, `SIGN` and the rest of §7.6's `CHECK` are still what this arm
/// answers `None` for.
fn parse_action(value: &str) -> Option<DecisionAction> {
    match value {
        "APPROVE" => Some(DecisionAction::Approve),
        "REJECT" => Some(DecisionAction::Reject),
        "RETURN" => Some(DecisionAction::Return),
        _ => None,
    }
}

/// Tasks of one instance, newest first, for the document workspace.
pub async fn tasks_of_instance(
    pool: &PgPool,
    tenant_id: Uuid,
    instance_id: Uuid,
) -> Result<Vec<WorkflowTask>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.task_ref, t.workflow_instance_id, t.document_id,
               t.task_definition_key, t.task_name, t.task_type, t.status,
               t.assignee_user_id, t.candidate_role_id, r.role_code AS "candidate_role_code?",
               t.candidate_department_id, t.delegated_from_user_id,
               t.priority, t.due_at, t.action,
               t.completed_by, t.completed_at, t.created_at
        FROM workflow_tasks t
        LEFT JOIN roles r ON r.id = t.candidate_role_id
        WHERE t.tenant_id = $1 AND t.workflow_instance_id = $2 AND t.deleted_at IS NULL
        ORDER BY t.created_at
        "#,
        tenant_id,
        instance_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| WorkflowTask {
            id: row.id,
            task_ref: row.task_ref,
            workflow_instance_id: row.workflow_instance_id,
            document_id: row.document_id,
            task_definition_key: row.task_definition_key,
            task_name: row.task_name,
            task_type: row.task_type,
            status: TaskStatus::from_db(&row.status),
            assignee_user_id: row.assignee_user_id,
            candidate_role_id: row.candidate_role_id,
            candidate_role_code: row.candidate_role_code,
            candidate_department_id: row.candidate_department_id,
            delegated_from_user_id: row.delegated_from_user_id,
            priority: row.priority,
            due_at: row.due_at,
            action: row.action.as_deref().and_then(parse_action),
            completed_by: row.completed_by,
            completed_at: row.completed_at,
            created_at: row.created_at,
        })
        .collect())
}
