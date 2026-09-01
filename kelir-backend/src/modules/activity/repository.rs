//! The statements behind `activity_events` (Database Schema §10.1; [#247]).
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247

use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{ActivityEvent, EventCategory};

/// What one event records.
pub struct NewActivityEvent<'a> {
    pub tenant_id: Uuid,
    pub document_id: Option<Uuid>,
    pub workflow_instance_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub attachment_id: Option<Uuid>,
    pub comment_id: Option<Uuid>,
    pub event_type: &'a str,
    pub event_category: EventCategory,
    pub actor_user_id: Option<Uuid>,
    pub actor_name: Option<&'a str>,
    pub action_summary: &'a str,
    pub details: Value,
}

/// Appends one event.
///
/// **Takes an executor, and every caller passes a transaction.** That is the
/// whole difference from `modules::audit`, which takes a `&PgPool` and writes on
/// its own connection: an audit row is a control *over* an action and must
/// survive the action failing, while an activity event is part of what the
/// action *produced* and must not (#247 AC2). The signature is where that lives,
/// because a rule in a signature is one the next caller cannot step around.
pub async fn insert_event<'e, E: PgExecutor<'e>>(
    executor: E,
    event: &NewActivityEvent<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO activity_events
            (id, tenant_id, document_id, workflow_instance_id, task_id, attachment_id,
             comment_id, event_type, event_category, actor_user_id, actor_name,
             action_summary, details_json, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $10)
        "#,
        Uuid::now_v7(),
        event.tenant_id,
        event.document_id,
        event.workflow_instance_id,
        event.task_id,
        event.attachment_id,
        event.comment_id,
        event.event_type,
        event.event_category.as_db(),
        event.actor_user_id,
        event.actor_name,
        event.action_summary,
        event.details,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// A document's timeline, newest first.
///
/// **`details_json` comes back as it was written**, and the service is what
/// decides how much of it a caller is served
/// ([`super::domain::disclosable`], #292). The statement is the place for the
/// tenant predicate and the wrong place for a permission one: this module reads
/// one table and the permissions belong to three others.
///
/// **Scoped by tenant in the statement** (#247 AC6), not by the handler that
/// called it — the [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
/// [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson, which cost
/// three sprints of coverage findings.
pub async fn list_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<ActivityEvent>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, document_id, workflow_instance_id, task_id, attachment_id, comment_id,
               event_type, event_category, actor_user_id, actor_name,
               action_summary, details_json, created_at
        FROM activity_events
        WHERE tenant_id = $1 AND document_id = $2
        ORDER BY created_at DESC, id DESC
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        document_id,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ActivityEvent {
            id: row.id,
            document_id: row.document_id,
            workflow_instance_id: row.workflow_instance_id,
            task_id: row.task_id,
            attachment_id: row.attachment_id,
            comment_id: row.comment_id,
            event_type: row.event_type,
            event_category: EventCategory::from_db(&row.event_category),
            actor_user_id: row.actor_user_id,
            actor_name: row.actor_name,
            action_summary: row.action_summary,
            details: row.details_json,
            occurred_at: row.created_at,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
pub async fn count_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM activity_events
        WHERE tenant_id = $1 AND document_id = $2
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}
