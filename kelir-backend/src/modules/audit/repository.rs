//! The search behind `audit_events` (Database Schema §10.2; [#252]).
//!
//! **Reads only.** Nothing in this file writes, and nothing anywhere updates or
//! deletes: [`super::record`] owns the insert and the hash chain, and the table
//! has no `updated_at` and no `deleted_at` for an edit to stamp (#252 AC6).
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{AuditEvent, AuditSearch};

/// One page of the trail, newest first.
///
/// # Every filter is optional and none of them is optional in the statement
///
/// The `($n::type IS NULL OR column = $n)` shape rather than a query built by
/// string concatenation: one prepared statement, checked by `sqlx::query!`
/// against the real schema at compile time, and no path where a filter is
/// forgotten because a branch was. It costs the planner a little and buys the
/// property that a caller cannot reach a version of this query that nobody
/// wrote.
///
/// **`tenant_id` is not optional**, and it is first (#252's own scope, and the
/// [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
/// [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson).
///
/// **Ordered by `created_at DESC, id DESC`** — a total order (#252 AC4).
/// `created_at` alone is not one: rows written inside a single transaction
/// share a timestamp, and a page boundary landing inside such a group would
/// show a row twice or skip it. That is `workflow_history`'s lesson from
/// [#181](https://github.com/sujanto-gaws/kelir/issues/181) and the timeline's
/// from [#247](https://github.com/sujanto-gaws/kelir/issues/247).
pub async fn search<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    filter: &AuditSearch,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditEvent>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, event_type, action, object_type, object_id, actor_user_id,
               ip_address, reason, old_value_json, new_value_json, created_at
        FROM audit_events
        WHERE tenant_id = $1
          AND ($2::uuid        IS NULL OR actor_user_id = $2)
          AND ($3::text        IS NULL OR object_type   = $3)
          AND ($4::uuid        IS NULL OR object_id     = $4)
          AND ($5::text        IS NULL OR event_type    = $5)
          AND ($6::timestamptz IS NULL OR created_at   >= $6)
          AND ($7::timestamptz IS NULL OR created_at   <= $7)
        ORDER BY created_at DESC, id DESC
        LIMIT $8 OFFSET $9
        "#,
        tenant_id,
        filter.actor_user_id,
        filter.object_type,
        filter.object_id,
        filter.event_type,
        filter.from,
        filter.to,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AuditEvent {
            id: row.id,
            event_type: row.event_type,
            action: row.action,
            object_type: row.object_type,
            object_id: row.object_id,
            actor_user_id: row.actor_user_id,
            ip_address: row.ip_address,
            reason: row.reason,
            old_value: row.old_value_json,
            new_value: row.new_value_json,
            // The service decides this; the statement has no caller to ask.
            values_withheld: false,
            occurred_at: row.created_at,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
///
/// **Counting what the caller can see and what they can read are different
/// questions here**, and this answers the first. Every row a search matches is
/// returned — withholding is about a row's *values*, never its existence
/// (#252 AC2) — so the total and the page are drawn from one set and cannot
/// disagree the way a filtered page and an unfiltered count would.
pub async fn count<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    filter: &AuditSearch,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM audit_events
        WHERE tenant_id = $1
          AND ($2::uuid        IS NULL OR actor_user_id = $2)
          AND ($3::text        IS NULL OR object_type   = $3)
          AND ($4::uuid        IS NULL OR object_id     = $4)
          AND ($5::text        IS NULL OR event_type    = $5)
          AND ($6::timestamptz IS NULL OR created_at   >= $6)
          AND ($7::timestamptz IS NULL OR created_at   <= $7)
        "#,
        tenant_id,
        filter.actor_user_id,
        filter.object_type,
        filter.object_id,
        filter.event_type,
        filter.from,
        filter.to
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}

/// Every `object_type` the trail actually holds, for this tenant.
///
/// **Read from the rows rather than listed in code**, because the answer a
/// filter control needs is *what is in here*, and a constant would offer types
/// this deployment has never written and omit ones a plugin has.
pub async fn object_types<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT DISTINCT object_type AS "object_type!"
        FROM audit_events
        WHERE tenant_id = $1
        ORDER BY object_type
        "#,
        tenant_id
    )
    .fetch_all(executor)
    .await
}

/// The bound a `from`/`to` pair is checked against, so a caller cannot ask for
/// a range that ends before it starts.
pub fn range_is_ordered(from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> bool {
    match (from, to) {
        (Some(from), Some(to)) => from <= to,
        _ => true,
    }
}
