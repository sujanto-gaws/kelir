//! Reading the master-data slice of `audit_events` (§10.2).
//!
//! Append-only and tenant-scoped; nothing here writes. The write path is
//! `modules::audit`.
//!
//! **`previous_hash` and `current_hash` are not selected.** Not filtered out
//! downstream — never read, so there is no path by which they could reach a
//! response. Chain verification is FR-AUD-003, Phase 6; until something checks
//! the chain, publishing it would let a client believe it had been checked
//! (#100 AC7).

use sqlx::PgPool;
use uuid::Uuid;

use crate::modules::master_data::domain::AuditRecord;

/// How many events this record has, before paging.
///
/// **Counts what the page shows.** `count_audit_records` and
/// [`list_audit_records`] take the same `include_roles` flag for the same
/// reason `count_role_view` and `list_role_view` share their `matched` block:
/// a total produced by different criteria than the rows describes a population
/// the caller never sees (#106 F6).
pub async fn count_audit_records(
    pool: &PgPool,
    tenant_id: Uuid,
    object_id: Uuid,
    include_roles: bool,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM audit_events
        WHERE tenant_id = $1
          AND object_id = $2
          AND ($3 OR action NOT IN ('ROLE_ASSIGNED', 'ROLE_UPDATED', 'ROLE_REMOVED'))
        "#,
        tenant_id,
        object_id,
        include_roles
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of what happened to a record, oldest first.
///
/// Oldest first because the question is "how did this get here", and a history
/// read backwards has to be reassembled before it answers one. `id` breaks the
/// tie: `created_at` is `now()` and two events written in the same transaction
/// can share it, while the ids are UUIDv7 and carry their own order.
pub async fn list_audit_records(
    pool: &PgPool,
    tenant_id: Uuid,
    object_id: Uuid,
    include_roles: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditRecord>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT e.id, e.event_type, e.action, e.created_at, e.actor_user_id,
               u.username AS "actor_username?",
               e.reason, e.old_value_json, e.new_value_json
        FROM audit_events e
        LEFT JOIN users u ON u.id = e.actor_user_id AND u.tenant_id = e.tenant_id
        WHERE e.tenant_id = $1
          AND e.object_id = $2
          AND ($3 OR e.action NOT IN ('ROLE_ASSIGNED', 'ROLE_UPDATED', 'ROLE_REMOVED'))
        ORDER BY e.created_at, e.id
        LIMIT $4 OFFSET $5
        "#,
        tenant_id,
        object_id,
        include_roles,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AuditRecord {
            id: row.id,
            event_type: row.event_type,
            action: row.action,
            occurred_at: row.created_at,
            actor_user_id: row.actor_user_id,
            actor_username: row.actor_username,
            reason: row.reason,
            old_value: row.old_value_json,
            new_value: row.new_value_json,
        })
        .collect())
}
