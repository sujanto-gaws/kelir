//! `outbox_events`: the insert, the claim, and the settlement (Database Schema
//! §12.8; ADR-0041).

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::domain::Settlement;

/// One row, as [`super::write`] inserts it.
pub struct NewRow<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub aggregate_type: &'a str,
    pub aggregate_id: Uuid,
    pub event_type: &'a str,
    pub payload_json: &'a Value,
    pub correlation_id: &'a str,
}

/// Inserts one event, `PENDING`.
///
/// **A transaction, not a pool**, which is the point of an outbox: an event
/// written on a second connection is an event that can commit without the
/// write it records, or be lost when that write commits.
pub async fn insert(
    transaction: &mut sqlx::PgTransaction<'_>,
    row: &NewRow<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO outbox_events
            (id, tenant_id, aggregate_type, aggregate_id, event_type, payload_json,
             correlation_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        row.id,
        row.tenant_id,
        row.aggregate_type,
        row.aggregate_id,
        row.event_type,
        row.payload_json,
        row.correlation_id,
    )
    .execute(&mut **transaction)
    .await
    .map(|_| ())
}

/// A row a worker has claimed.
pub struct Claimed {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub event_type: String,
    pub payload_json: Value,
    /// Attempts made before this one.
    pub attempt_count: i32,
}

/// Claims up to `limit` due rows, oldest first, under a lease.
///
/// **Due** is three things, and they are the three ways a row can be waiting:
///
/// - `PENDING` — never attempted;
/// - `FAILED` with `next_attempt_at` passed — its retry is due;
/// - `PROCESSING` with `next_attempt_at` passed — **its lease expired**, which
///   is what a worker that crashed mid-delivery leaves behind. Claimed again
///   rather than lost: at-least-once, which LHCS §5.2 makes the handler's
///   problem by requiring it to be idempotent.
///
/// **The predicate names all three statuses first**, and then the time test.
/// Written as `PENDING OR (FAILED/PROCESSING AND due)`, the planner scanned
/// `idx_outbox_events_pending` once per disjunct. The index's own predicate
/// is those three statuses, so stating it outright gets a single scan.
///
/// **`FOR UPDATE SKIP LOCKED`**, so two workers — two replicas of this process
/// — each take different rows rather than queueing behind one another's.
///
/// **Across tenants, and deliberately**: the worker serves the deployment, as
/// `notification::worker` does. Every statement it issues after the claim
/// names the row's own tenant.
pub async fn claim(
    pool: &PgPool,
    limit: i64,
    lease_seconds: f64,
) -> Result<Vec<Claimed>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        UPDATE outbox_events o
        SET status = 'PROCESSING',
            next_attempt_at = now() + make_interval(secs => $2)
        FROM (
            SELECT id
            FROM outbox_events
            WHERE status IN ('PENDING', 'PROCESSING', 'FAILED')
              AND (status = 'PENDING' OR next_attempt_at <= now())
            ORDER BY created_at, id
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        ) due
        WHERE o.id = due.id
        RETURNING o.id, o.tenant_id, o.event_type, o.payload_json, o.attempt_count
        "#,
        limit,
        lease_seconds,
    )
    .fetch_all(pool)
    .await?;

    let mut claimed: Vec<Claimed> = rows
        .into_iter()
        .map(|row| Claimed {
            id: row.id,
            tenant_id: row.tenant_id,
            event_type: row.event_type,
            payload_json: row.payload_json,
            attempt_count: row.attempt_count,
        })
        .collect();

    // `RETURNING` promises no order, and an aggregate's events are delivered in
    // the order they were written. The id is a v7 UUID, so it sorts by time.
    claimed.sort_by_key(|row| row.id);

    Ok(claimed)
}

/// Records how an attempt went.
///
/// **`attempt_count` counts every attempt, successful or not**, so a processed
/// row says how many it took. `last_error` keeps the latest reason while the row
/// is failing and is cleared by a success.
///
/// Guarded on `PROCESSING`: a row this worker held past its lease, which
/// another worker then claimed and settled, is not settled twice.
pub async fn settle<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    attempt: i32,
    settlement: Settlement,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let retry_in = match settlement {
        Settlement::Failed(delay) => Some(delay.as_secs_f64()),
        Settlement::Processed | Settlement::DeadLetter => None,
    };

    sqlx::query!(
        r#"
        UPDATE outbox_events
        SET status = $3::text,
            attempt_count = $4,
            next_attempt_at = now() + make_interval(secs => $5),
            processed_at = CASE WHEN $3::text = 'PROCESSED' THEN now() END,
            last_error = $6
        WHERE tenant_id = $1 AND id = $2 AND status = 'PROCESSING'
        "#,
        tenant_id,
        id,
        settlement.as_db(),
        attempt,
        retry_in,
        last_error,
    )
    .execute(executor)
    .await
    .map(|_| ())
}
