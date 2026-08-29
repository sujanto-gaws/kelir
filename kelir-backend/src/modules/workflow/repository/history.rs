//! The process's own record of how a document got here (FR-WF-012; [#181]).
//!
//! # What this answers, and what `audit_events` answers
//!
//! **Two records of one event, and the relationship is stated rather than
//! left to be inferred** — the problem [#178] had to settle for status, one
//! layer over.
//!
//! | | `workflow_history` | `audit_events` |
//! |---|---|---|
//! | Question | how did this document get here | was this tampered with |
//! | Reader | the approver, in the document workspace | somebody investigating |
//! | Permission | `workflow:instance:read` | `master-data:audit:read` |
//! | Shape | one row per transition, in the transition's transaction | hash-chained, append-only, whole-payload |
//!
//! **Neither is derived from the other.** The engine writes this row; the audit
//! module writes its own from its own call site. Deriving the screen's history
//! from the audit trail would make an audit row something a user-facing feature
//! depends on, and an audit row nobody can change is exactly what that trail is
//! for — [#181] AC5 is this paragraph.
//!
//! # And what `workflow_task_history` answers
//!
//! One *task's* progress — `CREATED` → `ASSIGNED` → `COMPLETED`. Different
//! vocabulary, different key, different reader. `0027`'s header carries the two
//! reasons that table could not hold this one: its `task_id` is `NOT NULL` and
//! a transition needs no task, and its status columns hold task statuses rather
//! than workflow states.
//!
//! [#178]: https://github.com/sujanto-gaws/kelir/issues/178
//! [#181]: https://github.com/sujanto-gaws/kelir/issues/181

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::super::domain::WorkflowHistoryEntry;
use crate::modules::workflow::domain::TransitionAction;

/// One transition, as the engine is about to record it.
pub struct NewHistoryEntry<'a> {
    pub tenant_id: Uuid,
    pub workflow_instance_id: Uuid,
    pub document_id: Uuid,
    /// `None` on the instance's first row: the initial state came from nowhere.
    pub from_state: Option<&'a str>,
    pub to_state: &'a str,
    /// `None` when nothing named an action — the start.
    pub action: Option<TransitionAction>,
    /// The task the decision came from, when a decision is what moved it.
    pub task_id: Option<Uuid>,
    /// The reason given with the decision (FR-TASK-006,
    /// [#182](https://github.com/sujanto-gaws/kelir/issues/182)), trimmed and
    /// bounded before it reaches here. `None` where a transition carried none —
    /// the start, and any edge the definition did not mark.
    pub comment: Option<&'a str>,
    pub actor_user_id: Option<Uuid>,
    /// Whose authority the actor was exercising
    /// ([#184](https://github.com/sujanto-gaws/kelir/issues/184) AC4).
    ///
    /// **Both parties or neither.** A delegated decision that recorded only the
    /// delegate would lose the accountability delegation exists to preserve —
    /// the approval was the delegator's to give — and one that recorded only the
    /// delegator would name somebody who was not there. `None` on every row
    /// nobody was standing in for, which is almost all of them; the actor is not
    /// copied in to avoid the null, because that would make *acting for myself*
    /// and *acting for somebody who happens to be me* the same row.
    ///
    /// It comes from `workflow_tasks.delegated_from_user_id`, which the server
    /// wrote when the task was assigned or handed over — never from the request.
    pub on_behalf_of_user_id: Option<Uuid>,
}

/// Appends a transition to the history.
///
/// **Takes an executor rather than a pool**, so the caller's transaction is the
/// only place this can be written from — [#181] AC1: a transition whose history
/// row is written by a second connection is a transition that can commit
/// without it, and a gap in the answer to *how did this get here* is invisible
/// precisely where it matters.
pub async fn record<'e, E: PgExecutor<'e>>(
    executor: E,
    entry: &NewHistoryEntry<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO workflow_history (
            id, tenant_id, workflow_instance_id, document_id,
            from_state, to_state, action, task_id, comment,
            actor_user_id, on_behalf_of_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
        Uuid::now_v7(),
        entry.tenant_id,
        entry.workflow_instance_id,
        entry.document_id,
        entry.from_state,
        entry.to_state,
        entry.action.map(TransitionAction::as_db),
        entry.task_id,
        entry.comment,
        entry.actor_user_id,
        entry.on_behalf_of_user_id
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// One document's history, oldest first.
///
/// **Ordered by `created_at` and then by `id`**, not by `created_at` alone. Two
/// rows of one transaction share a timestamp to the microsecond often enough to
/// matter — the start writes the instance's first row and, when the initial
/// state declares no task, nothing else does — and a paginated read whose order
/// is not total can show a row twice or not at all across a page boundary. `id`
/// is a v7 UUID, so it breaks the tie in the order the rows were written.
pub async fn list_for_document(
    pool: &PgPool,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<WorkflowHistoryEntry>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT h.id, h.from_state, h.to_state, h.action, h.task_id, h.comment,
               h.actor_user_id, u.username AS "actor_username?",
               h.on_behalf_of_user_id, b.username AS "on_behalf_of_username?",
               h.created_at
        FROM workflow_history h
        LEFT JOIN users u ON u.id = h.actor_user_id AND u.tenant_id = h.tenant_id
        LEFT JOIN users b ON b.id = h.on_behalf_of_user_id AND b.tenant_id = h.tenant_id
        WHERE h.tenant_id = $1 AND h.document_id = $2
        ORDER BY h.created_at, h.id
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        document_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| WorkflowHistoryEntry {
            id: row.id,
            from_state: row.from_state,
            to_state: row.to_state,
            action: row.action,
            task_id: row.task_id,
            comment: row.comment,
            actor_user_id: row.actor_user_id,
            actor_username: row.actor_username,
            on_behalf_of_user_id: row.on_behalf_of_user_id,
            on_behalf_of_username: row.on_behalf_of_username,
            occurred_at: row.created_at,
        })
        .collect())
}

/// How many rows the read above is paging through.
pub async fn count_for_document(
    pool: &PgPool,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM workflow_history
        WHERE tenant_id = $1 AND document_id = $2
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(pool)
    .await
}
