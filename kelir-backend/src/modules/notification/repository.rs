//! The statements behind `notifications` (Database Schema §11.3; [#251]).
//!
//! **Every read and every write in this file carries `recipient_user_id = $2`**
//! (#251 AC7). Not one of them is scoped by a handler that remembered to filter:
//! that is the [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
//! [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson, which cost
//! this project three sprints of coverage findings, and it is why the predicate
//! is in the SQL rather than in a `Vec::retain` above it.
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{Notification, NotificationType};

/// What one notification records.
pub struct NewNotification<'a> {
    pub tenant_id: Uuid,
    pub recipient_user_id: Uuid,
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub notification_type: NotificationType,
    pub title: &'a str,
    pub body: &'a str,
    /// Who caused it, for `created_by`. `None` for anything the engine did on
    /// nobody's behalf.
    pub actor: Option<Uuid>,
}

/// Appends one notification.
///
/// **Takes an executor, and every caller passes a transaction** — the shape
/// `activity::repository::insert_event` established and for the reason
/// [`super::service::notify`] states: a notification must not outlive the
/// action it announces, and must not be lost when that action commits.
pub async fn insert<'e, E: PgExecutor<'e>>(
    executor: E,
    notification: &NewNotification<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO notifications
            (id, tenant_id, recipient_user_id, document_id, workflow_instance_id, task_id,
             notification_type, title, body, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        Uuid::now_v7(),
        notification.tenant_id,
        notification.recipient_user_id,
        notification.document_id,
        notification.workflow_instance_id,
        notification.task_id,
        notification.notification_type.as_db(),
        notification.title,
        notification.body,
        notification.actor,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Everybody who currently holds a role, optionally within one department.
///
/// **The inverse of `workflow::repository::task::holds_role`, and it has to
/// stay its inverse** — same validity window, same `department_id IS NULL`
/// reading of an unscoped grant (**D-39**: a null grant satisfies a
/// department-scoped task, because a scoped grant is the *narrowing*). A
/// notification going to somebody the inbox will not offer the task to, or not
/// going to somebody it will, is the two queries having drifted.
///
/// **A suspended account is not told**, which the inbox's own reads do not have
/// to say because a locked account cannot sign in to see one. `status =
/// 'ACTIVE'` is `delegation_repository`'s predicate, reused rather than
/// reinvented: a notification for somebody who cannot act on it is noise that
/// outlives them.
///
/// Ordered by id so a fan-out is deterministic, which is what makes a test able
/// to assert *these people and no others* rather than *this many rows*.
pub async fn holders_of_role<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    role_id: Uuid,
    department: Option<Uuid>,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"
        SELECT DISTINCT ur.user_id
        FROM user_roles ur
        JOIN users u ON u.id = ur.user_id AND u.deleted_at IS NULL AND u.status = 'ACTIVE'
        WHERE ur.tenant_id = $1 AND ur.role_id = $2 AND ur.deleted_at IS NULL
          AND (ur.valid_from IS NULL OR ur.valid_from <= current_date)
          AND (ur.valid_to   IS NULL OR ur.valid_to   >= current_date)
          AND ($3::uuid IS NULL OR ur.department_id IS NULL OR ur.department_id = $3)
        ORDER BY ur.user_id
        "#,
        tenant_id,
        role_id,
        department
    )
    .fetch_all(executor)
    .await?;

    Ok(rows)
}

/// One page of a person's notifications, newest first.
pub async fn list_for_recipient<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    recipient_user_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Notification>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, document_id, workflow_instance_id, task_id, notification_type,
               title, body, read_at, created_at
        FROM notifications
        WHERE tenant_id = $1 AND recipient_user_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC, id DESC
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        recipient_user_id,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Notification {
            id: row.id,
            document_id: row.document_id,
            workflow_instance_id: row.workflow_instance_id,
            task_id: row.task_id,
            notification_type: NotificationType::from_db(&row.notification_type),
            title: row.title,
            body: row.body,
            read_at: row.read_at,
            created_at: row.created_at,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
pub async fn count_for_recipient<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    recipient_user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM notifications
        WHERE tenant_id = $1 AND recipient_user_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        recipient_user_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}

/// How many are waiting — the badge, which is not the page's total.
pub async fn count_unread<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    recipient_user_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM notifications
        WHERE tenant_id = $1 AND recipient_user_id = $2
          AND deleted_at IS NULL AND read_at IS NULL
        "#,
        tenant_id,
        recipient_user_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}

/// Marks one notification read, **idempotently** (#251 AC5).
///
/// # `read_at IS NULL` is what makes the second call a no-op rather than a lie
///
/// Without it a repeat would move `read_at` forward, so *when did I read this*
/// would answer *the last time I clicked it* — and a client that re-sends on
/// retry would rewrite a timestamp it did not mean to touch.
///
/// **Returns whether the row exists and belongs to this caller, not whether it
/// changed.** Those are different questions and only the first is a 404: a
/// second call on an already-read notification succeeded the first time and has
/// nothing to report. So the `SELECT` decides the answer and the `UPDATE`
/// decides nothing — which is why they are one statement rather than two calls
/// with a race between them.
pub async fn mark_read<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    recipient_user_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        WITH mine AS (
            SELECT id FROM notifications
            WHERE tenant_id = $1 AND recipient_user_id = $2 AND id = $3 AND deleted_at IS NULL
        ), touched AS (
            UPDATE notifications
            SET read_at = now(), updated_by = $2, updated_at = now()
            WHERE id IN (SELECT id FROM mine) AND read_at IS NULL
            RETURNING id
        )
        SELECT count(*) AS "found!" FROM mine
        "#,
        tenant_id,
        recipient_user_id,
        id
    )
    .fetch_one(executor)
    .await?;

    Ok(found > 0)
}

/// Marks everything this person has waiting as read, and says how many that was.
///
/// **`read_at IS NULL` again**, so calling it twice reports `0` the second time
/// rather than restamping a inbox-worth of rows.
pub async fn mark_all_read<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    recipient_user_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE notifications
        SET read_at = now(), updated_by = $2, updated_at = now()
        WHERE tenant_id = $1 AND recipient_user_id = $2
          AND deleted_at IS NULL AND read_at IS NULL
        "#,
        tenant_id,
        recipient_user_id
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}
