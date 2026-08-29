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

/// What the inbox may be narrowed to.
///
/// `open_only` rather than a status list: FR-TASK-009 (completed tasks) is
/// unscheduled, and an inbox that could be asked for `CANCELLED` would be
/// answering a question nobody has specified. The flag is what a screen needs —
/// *what is waiting for me* against *what has been through my hands*.
#[derive(Debug, Clone)]
pub struct InboxFilters {
    pub open_only: bool,
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
    pub assignee_user_id: Option<Uuid>,
    pub candidate_role_id: Option<Uuid>,
    pub candidate_role_code: Option<String>,
    pub delegated_from_user_id: Option<Uuid>,
    pub delegated_from_display_name: Option<String>,
    pub workflow_name: String,
    pub current_state: String,
    pub created_at: DateTime<Utc>,
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
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.task_ref, t.workflow_instance_id, t.document_id,
               d.document_ref, d.document_number, d.title AS document_title,
               t.task_name, t.task_type, t.status, t.priority, t.due_at,
               t.assignee_user_id, t.candidate_role_id,
               r.role_code AS "candidate_role_code?",
               t.delegated_from_user_id,
               f.display_name AS "delegated_from_display_name?",
               w.name AS workflow_name, i.current_state, t.created_at
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
          AND ($4::uuid IS NULL OR t.document_id = $4)
          AND ($5::uuid IS NULL OR t.id = $5)
        ORDER BY t.created_at DESC
        LIMIT $6 OFFSET $7
        "#,
        tenant_id,
        user_id,
        filters.open_only,
        filters.document_id,
        filters.task_id,
        limit,
        offset
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
            assignee_user_id: row.assignee_user_id,
            candidate_role_id: row.candidate_role_id,
            candidate_role_code: row.candidate_role_code,
            delegated_from_user_id: row.delegated_from_user_id,
            delegated_from_display_name: row.delegated_from_display_name,
            workflow_name: row.workflow_name,
            current_state: row.current_state,
            created_at: row.created_at,
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
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM workflow_tasks t
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
          AND ($4::uuid IS NULL OR t.document_id = $4)
          AND ($5::uuid IS NULL OR t.id = $5)
        "#,
        tenant_id,
        user_id,
        filters.open_only,
        filters.document_id,
        filters.task_id,
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
