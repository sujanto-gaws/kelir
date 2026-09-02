//! The one statement behind the task inbox (FR-TASK-001, 002; [#179]).
//!
//! # The visibility rule, stated once and enforced in the query
//!
//! > A caller sees a task when it is in their tenant **and** either they are its
//! > `assignee_user_id`, **or** it has no assignee and its `candidate_role_id`
//! > is a role the caller currently holds **in the task's
//! > `candidate_department_id`, if it has one**.
//!
//! The last clause arrived with
//! [#225](https://github.com/sujanto-gaws/kelir/issues/225) and closes the half
//! of `DEPARTMENT_ROLE` that was resolved, stored, and then read by nothing. A
//! grant naming no department satisfies a department-scoped task — an
//! *optional* department-scoped grant (`0002`) is the role held generally, not
//! the role held nowhere — and `repository::task::holds_role` carries the
//! reasoning in full, because the inbox and the decision must answer this
//! identically or the queue lists work the API then refuses.
//!
//! Enforced here, in the `WHERE` clause, and **not in the handler** ([#179]
//! AC3). A rule in a handler is a rule the next caller of the repository can
//! step around; a rule in the statement is one the database applies to every
//! caller there will ever be. It is also the difference the [#106]/[#121] lesson
//! cost three sprints of coverage findings to learn: a test that asserts around
//! a query proves something about the handler and nothing about the rows.
//!
//! # Overdue is the database's opinion, and that is the whole of the rule
//!
//! [#185](https://github.com/sujanto-gaws/kelir/issues/185) AC4 asks that
//! *overdue* be computed against **a single clock**, and names the failure it is
//! avoiding: a task that is late on one screen and not on another is a bug
//! report nobody can reproduce. The clock is **`now()` in these statements** —
//! the same one `insert_task` stamped `due_at` from — and the answer travels to
//! the client as a boolean rather than a date to compare. A browser that
//! subtracted `dueAt` from its own clock would be a second opinion, and the two
//! would differ by whatever the viewer's machine is wrong by.
//!
//! **A task with no `due_at` is not overdue**, which the predicate says
//! explicitly rather than relying on `NULL < now()` being unknown. AC5 names
//! that trap because the other spelling of it — a null read as the epoch — is
//! the one that reports every undated task as years late.
//!
//! **And overdue means still open.** A task somebody finished after its date
//! passed is a fact about last week; the indicator exists to say what needs
//! doing now, and colouring completed rows red would bury the ones that do.
//!
//! # The roles come from `user_roles`, not from the token
//!
//! A token's `roles` claim is a snapshot from sign-in. A role granted an hour
//! ago is not in it; a role revoked an hour ago still is. **The second is the
//! one that matters**: an inbox built from a stale claim offers somebody tasks
//! they may no longer act on, [`super::super::service::task`]'s own check then
//! refuses them, and the result reads as a broken product while being a leak.
//! The grant's validity window is honoured in the same predicate, because a
//! grant that has expired is not a grant.
//!
//! # This lives in `workflow` and is called from `task_inbox`
//!
//! Coding standard §2.2 keeps a repository private to its module, with
//! cross-module access going through the owning module's service. `workflow_tasks`
//! is this module's table, so the statement is here and
//! [`crate::modules::task_inbox`] reaches it through
//! [`super::super::service::inbox`]. A second module writing its own SQL against
//! these rows would be a second implementation of the rule above, and the two
//! would drift.
//!
//! [#106]: https://github.com/sujanto-gaws/kelir/issues/106
//! [#121]: https://github.com/sujanto-gaws/kelir/issues/121
//! [#179]: https://github.com/sujanto-gaws/kelir/issues/179

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// **One axis with four points**, which is what the inbox is asked along
/// ([#179] AC2, [#185] AC3, [#256] AC2).
///
/// `overdue ⊂ open ⊂ all` and `completed ⊂ all`, and the two subsets are
/// disjoint: a task that is late is still open, because a finished one is not
/// late, it is done. Offering them as flags that combine would let a caller ask
/// for *completed and overdue*, which is a question with no answer, and would
/// need two controls on a screen to express one choice.
///
/// **FR-TASK-009 is why `Completed` exists**, and it arrived as a fourth point
/// rather than a second endpoint: the visibility rule is a `WHERE` clause, and a
/// second statement over these rows would be a second implementation of it
/// ([#256] AC2).
///
/// [#256]: https://github.com/sujanto-gaws/kelir/issues/256
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InboxScope {
    /// What is waiting for me — the default an inbox opens on.
    #[default]
    Open,
    /// What is waiting and already late.
    Overdue,
    /// What has been through my hands and is finished.
    Completed,
    /// Everything, open and finished alike.
    All,
}

impl InboxScope {
    /// The three predicates the statement takes, derived **here** so the axis
    /// has one definition rather than one per query.
    fn predicates(self) -> (bool, bool, bool) {
        match self {
            Self::Open => (true, false, false),
            // Overdue narrows open rather than replacing it, so both are set —
            // which also keeps the filter honest if the two predicates ever
            // disagree about what "still open" means.
            Self::Overdue => (true, true, false),
            Self::Completed => (false, false, true),
            Self::All => (false, false, false),
        }
    }
}

/// What the inbox may be narrowed to.
#[derive(Debug, Clone, Default)]
pub struct InboxFilters {
    pub scope: InboxScope,
    pub document_id: Option<Uuid>,
    /// One task, by id.
    ///
    /// **Added by a re-read, not by a requirement.** The detail view used to
    /// read a page of a thousand and pick its row out of it, which answers 404
    /// for the oldest task of anybody holding more than a thousand — a busy
    /// approver's queue, silently, on a task the visibility rule says they may
    /// see. Filtering in the statement makes the read exact and keeps the
    /// visibility rule a single predicate rather than two that could disagree.
    pub task_id: Option<Uuid>,
    /// Free text, matched against the task's own name and the document it is
    /// about (FR-SRH-003, [#256] AC3).
    ///
    /// **Through the same statement**, so the search narrows the list the
    /// visibility rule already produced rather than searching `workflow_tasks`
    /// and filtering afterwards. Already escaped for `LIKE` by
    /// `task_inbox::domain` — a person typing `%` is searching for a percent
    /// sign, not for everything.
    pub search: Option<String>,
}

/// One row of the inbox.
pub struct InboxRow {
    pub id: Uuid,
    pub task_ref: String,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    pub document_ref: String,
    pub document_number: Option<String>,
    pub document_title: String,
    pub task_name: String,
    pub task_type: String,
    pub status: String,
    pub priority: String,
    pub due_at: Option<DateTime<Utc>>,
    /// Whether this task is past its date **and still open**, as the database
    /// answered it in the same statement that read the row.
    pub is_overdue: bool,
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    pub candidate_role_code: Option<String>,
    pub delegated_from_user_id: Option<Uuid>,
    pub delegated_from_display_name: Option<String>,
    pub workflow_name: String,
    pub current_state: String,
    pub created_at: DateTime<Utc>,
    /// What was decided, on a task that has been decided ([#256] AC5).
    pub action: Option<String>,
    /// The reason given with it — FR-TASK-006's record, written in Sprint 11
    /// and visible until now only on the document's own history.
    pub decision_comment: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// The caller's page of tasks.
///
/// The document's title, reference and number are joined in rather than fetched
/// per row: an inbox of twenty tasks would otherwise be twenty-one round trips,
/// and a person cannot act on "task 7 of instance 9" without knowing which
/// document it is about.
///
/// **The join to `documents` carries no permission of its own**, and that is
/// deliberate: it exposes the same three fields the task already implies the
/// existence of, to somebody the task is assigned to. A caller who may not read
/// documents at all still cannot open one — the workspace refuses — and a task
/// whose subject is unnameable is a task nobody can act on.
pub async fn list_for_caller(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    filters: &InboxFilters,
    limit: i64,
    offset: i64,
) -> Result<Vec<InboxRow>, sqlx::Error> {
    let (open_only, overdue_only, completed_only) = filters.scope.predicates();

    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.task_ref, t.workflow_instance_id, t.document_id,
               d.document_ref, d.document_number, d.title AS document_title,
               t.task_name, t.task_type, t.status, t.priority, t.due_at,
               (t.due_at IS NOT NULL
                AND t.due_at < now()
                AND t.status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')) AS "is_overdue!",
               t.assignee_user_id, t.candidate_role_id,
               r.role_code AS "candidate_role_code?",
               t.delegated_from_user_id,
               f.display_name AS "delegated_from_display_name?",
               w.name AS workflow_name, i.current_state, t.created_at,
               t.action, t.comment AS decision_comment, t.completed_at
        FROM workflow_tasks t
        JOIN documents d ON d.id = t.document_id AND d.deleted_at IS NULL
        JOIN workflow_instances i ON i.id = t.workflow_instance_id
        JOIN workflow_definitions w ON w.id = i.workflow_definition_id
        LEFT JOIN roles r ON r.id = t.candidate_role_id
        -- Whose work this is, when it is not the holder's own (#184). Joined
        -- rather than resolved per row for the reason the document join above
        -- gives: an inbox of twenty tasks would otherwise be twenty-one round
        -- trips, and "approve this on Ani's behalf" is the sentence the screen
        -- has to be able to write.
        LEFT JOIN users f ON f.id = t.delegated_from_user_id AND f.tenant_id = t.tenant_id
        WHERE t.tenant_id = $1 AND t.deleted_at IS NULL
          AND (
                t.assignee_user_id = $2
             OR (t.assignee_user_id IS NULL AND EXISTS (
                    SELECT 1 FROM user_roles ur
                    WHERE ur.tenant_id = $1 AND ur.user_id = $2 AND ur.deleted_at IS NULL
                      AND ur.role_id = t.candidate_role_id
                      AND (ur.valid_from IS NULL OR ur.valid_from <= current_date)
                      AND (ur.valid_to   IS NULL OR ur.valid_to   >= current_date)
                      AND (t.candidate_department_id IS NULL
                           OR ur.department_id IS NULL
                           OR ur.department_id = t.candidate_department_id)
                ))
          )
          AND ($3 = false OR t.status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS'))
          -- Late, and still open. Spelled out rather than left to `NULL < now()`
          -- being unknown, because the other spelling of an undated task is one
          -- read as the epoch and reported years late (#185 AC5).
          AND ($6 = false OR (t.due_at IS NOT NULL
                              AND t.due_at < now()
                              AND t.status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')))
          -- Finished, whichever way (#256 AC1). `CANCELLED` is here because a
          -- task that was withdrawn has still been through the holder's hands
          -- and is still not waiting for them; hiding it would leave the two
          -- questions the axis asks — *waiting* and *done* — with rows in
          -- neither.
          AND ($9 = false OR t.status IN ('COMPLETED', 'CANCELLED'))
          AND ($4::uuid IS NULL OR t.document_id = $4)
          AND ($5::uuid IS NULL OR t.id = $5)
          -- The search, in the same statement rather than after it (#256 AC3).
          -- `ESCAPE` because the term is already escaped for `LIKE` and a person
          -- typing `%` means a percent sign.
          AND ($10::text IS NULL
               OR t.task_name ILIKE '%' || $10 || '%' ESCAPE '\'
               OR d.title ILIKE '%' || $10 || '%' ESCAPE '\'
               OR d.document_number ILIKE '%' || $10 || '%' ESCAPE '\')
        -- **Totally ordered** (#256 AC6). `created_at` alone leaves ties, and a
        -- page boundary inside a tie is a row shown twice or not at all.
        ORDER BY t.created_at DESC, t.id DESC
        LIMIT $7 OFFSET $8
        "#,
        tenant_id,
        user_id,
        open_only,
        filters.document_id,
        filters.task_id,
        overdue_only,
        limit,
        offset,
        completed_only,
        filters.search.as_deref()
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| InboxRow {
            id: row.id,
            task_ref: row.task_ref,
            workflow_instance_id: row.workflow_instance_id,
            document_id: row.document_id,
            document_ref: row.document_ref,
            document_number: row.document_number,
            document_title: row.document_title,
            task_name: row.task_name,
            task_type: row.task_type,
            status: row.status,
            priority: row.priority,
            due_at: row.due_at,
            is_overdue: row.is_overdue,
            assignee_user_id: row.assignee_user_id,
            candidate_role_id: row.candidate_role_id,
            candidate_role_code: row.candidate_role_code,
            delegated_from_user_id: row.delegated_from_user_id,
            delegated_from_display_name: row.delegated_from_display_name,
            workflow_name: row.workflow_name,
            current_state: row.current_state,
            created_at: row.created_at,
            action: row.action,
            decision_comment: row.decision_comment,
            completed_at: row.completed_at,
        })
        .collect())
}

/// How many the caller can see, under the same rule.
///
/// **The same predicate, written once more rather than referenced**, which is
/// the one duplication in this file. It is here because `meta.total` and the
/// page must agree: a count over a wider rule reports rows the page does not
/// show, and a person paging through an inbox that says "23" and ends at 19 has
/// no way to tell a bug from a task somebody else took. The cross-user test
/// asserts **both**, for that reason.
pub async fn count_for_caller(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    filters: &InboxFilters,
) -> Result<i64, sqlx::Error> {
    let (open_only, overdue_only, completed_only) = filters.scope.predicates();

    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM workflow_tasks t
        JOIN documents d ON d.id = t.document_id AND d.deleted_at IS NULL
        -- **The join the page has and this one did not**
        -- ([#279](https://github.com/sujanto-gaws/kelir/issues/279)). A task
        -- whose document is soft-deleted was counted and not listed, so an inbox
        -- said 23 and ended at 19 — which the comment above this function
        -- forbids in as many words, and which is the one duplication in this
        -- file drifting exactly where it was warned it would.
        --
        -- **It can no longer be dropped silently.** #256's search reads
        -- `d.title` and `d.document_number` in this statement as well as in the
        -- page, so removing the join stops the crate compiling rather than
        -- changing an answer. The test guards the semantics — a task whose
        -- document is gone — and the compiler now guards the join itself.
        WHERE t.tenant_id = $1 AND t.deleted_at IS NULL
          AND (
                t.assignee_user_id = $2
             OR (t.assignee_user_id IS NULL AND EXISTS (
                    SELECT 1 FROM user_roles ur
                    WHERE ur.tenant_id = $1 AND ur.user_id = $2 AND ur.deleted_at IS NULL
                      AND ur.role_id = t.candidate_role_id
                      AND (ur.valid_from IS NULL OR ur.valid_from <= current_date)
                      AND (ur.valid_to   IS NULL OR ur.valid_to   >= current_date)
                      AND (t.candidate_department_id IS NULL
                           OR ur.department_id IS NULL
                           OR ur.department_id = t.candidate_department_id)
                ))
          )
          AND ($3 = false OR t.status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS'))
          -- Late, and still open. Spelled out rather than left to `NULL < now()`
          -- being unknown, because the other spelling of an undated task is one
          -- read as the epoch and reported years late (#185 AC5).
          AND ($6 = false OR (t.due_at IS NOT NULL
                              AND t.due_at < now()
                              AND t.status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')))
          AND ($7 = false OR t.status IN ('COMPLETED', 'CANCELLED'))
          AND ($4::uuid IS NULL OR t.document_id = $4)
          AND ($5::uuid IS NULL OR t.id = $5)
          AND ($8::text IS NULL
               OR t.task_name ILIKE '%' || $8 || '%' ESCAPE '\'
               OR d.title ILIKE '%' || $8 || '%' ESCAPE '\'
               OR d.document_number ILIKE '%' || $8 || '%' ESCAPE '\')
        "#,
        tenant_id,
        user_id,
        open_only,
        filters.document_id,
        filters.task_id,
        overdue_only,
        completed_only,
        filters.search.as_deref()
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// Whether one task is visible to the caller, under the same rule.
///
/// The detail view's gate. It reaches the *rows* rather than re-deriving the
/// rule in a service, so a task another user holds answers 404 from the same
/// predicate the list filters on rather than from a second one that could
/// disagree with it.
pub async fn is_visible_to(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    task_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "found!"
        FROM workflow_tasks t
        -- The third statement #279 found disagreeing, joined for the reason the
        -- count is: this gate answered *visible* for a task the read behind it
        -- then answered 404 for.
        JOIN documents d ON d.id = t.document_id AND d.deleted_at IS NULL
        WHERE t.tenant_id = $1 AND t.id = $3 AND t.deleted_at IS NULL
          AND (
                t.assignee_user_id = $2
             OR (t.assignee_user_id IS NULL AND EXISTS (
                    SELECT 1 FROM user_roles ur
                    WHERE ur.tenant_id = $1 AND ur.user_id = $2 AND ur.deleted_at IS NULL
                      AND ur.role_id = t.candidate_role_id
                      AND (ur.valid_from IS NULL OR ur.valid_from <= current_date)
                      AND (ur.valid_to   IS NULL OR ur.valid_to   >= current_date)
                      AND (t.candidate_department_id IS NULL
                           OR ur.department_id IS NULL
                           OR ur.department_id = t.candidate_department_id)
                ))
          )
        "#,
        tenant_id,
        user_id,
        task_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(found.is_some())
}
