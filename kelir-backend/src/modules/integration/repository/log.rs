//! `integration_logs` (Database Schema §12.5): the test call's insert
//! (FR-INT-002, #547 — **the table's first writer**) and the log reader's
//! list and detail (FR-INT-006, #548).
//!
//! Append-only: there is an insert and reads, and nothing else. The payload
//! columns take what `service::test_call` has already masked, and the reads
//! return them as stored; nothing here sees a secret or resolves one.
//!
//! # The list and its index
//!
//! `idx_integration_logs_tenant_system_started` is
//! `(tenant_id, external_system_id, started_at)`. With `externalSystemId`
//! the list is a range scan of that index, in `started_at` order; `from` and
//! `to` narrow the same range. Without it the index still serves the
//! `tenant_id` prefix, and the tenant's rows are sorted by `started_at` after.
//! `status` and `endpointId` are filters on the rows the index yields — never a
//! scan beyond the tenant's own rows. The table has no `deleted_at`: nothing is
//! ever removed from it.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::integration::domain::{
    IntegrationDirection, IntegrationLog, IntegrationLogStatus, IntegrationLogSummary,
};

pub struct NewIntegrationLog<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub external_system_id: Uuid,
    /// `REST` for a test call.
    pub integration_type: &'a str,
    /// The URL that was, or would have been, called.
    pub endpoint: Option<&'a str>,
    pub method: &'a str,
    pub correlation_id: &'a str,
    /// `IntegrationEndpoint`, and the endpoint's id.
    pub entity_type: &'a str,
    pub entity_id: Uuid,
    pub request_payload_json: Option<Value>,
    pub response_payload_json: Option<Value>,
    pub status_code: Option<i32>,
    /// `SUCCESS` or `FAILED`.
    pub status: &'a str,
    pub error_message: Option<&'a str>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: i32,
}

pub async fn insert_log<'e, E: PgExecutor<'e>>(
    executor: E,
    log: &NewIntegrationLog<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO integration_logs
            (id, tenant_id, external_system_id, direction, integration_type, endpoint, method,
             correlation_id, entity_type, entity_id, request_payload_json,
             response_payload_json, status_code, status, error_message, started_at,
             completed_at, duration_ms)
        VALUES ($1, $2, $3, 'OUTBOUND', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                $16, $17)
        "#,
        log.id,
        log.tenant_id,
        log.external_system_id,
        log.integration_type,
        log.endpoint,
        log.method,
        log.correlation_id,
        log.entity_type,
        log.entity_id,
        log.request_payload_json,
        log.response_payload_json,
        log.status_code,
        log.status,
        log.error_message,
        log.started_at,
        log.completed_at,
        log.duration_ms,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// The list's filters, already reduced to column values.
pub struct IntegrationLogFilter<'a> {
    pub external_system_id: Option<Uuid>,
    /// `(entity_type, entity_id)` — a test call's endpoint.
    pub entity: Option<(&'a str, Uuid)>,
    pub status: Option<&'a str>,
    /// Inclusive.
    pub from: Option<DateTime<Utc>>,
    /// Exclusive.
    pub to: Option<DateTime<Utc>>,
}

struct SummaryRow {
    id: Uuid,
    external_system_id: Option<Uuid>,
    external_system_code: Option<String>,
    external_system_name: Option<String>,
    direction: String,
    integration_type: Option<String>,
    method: Option<String>,
    endpoint: Option<String>,
    entity_type: Option<String>,
    entity_id: Option<Uuid>,
    status: String,
    status_code: Option<i32>,
    error_message: Option<String>,
    correlation_id: String,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    duration_ms: Option<i32>,
}

impl From<SummaryRow> for IntegrationLogSummary {
    fn from(row: SummaryRow) -> Self {
        Self {
            id: row.id,
            external_system_id: row.external_system_id,
            external_system_code: row.external_system_code,
            external_system_name: row.external_system_name,
            direction: IntegrationDirection::from_db(&row.direction),
            integration_type: row.integration_type,
            method: row.method,
            endpoint: row.endpoint,
            entity_type: row.entity_type,
            entity_id: row.entity_id,
            status: IntegrationLogStatus::from_db(&row.status),
            status_code: row.status_code,
            error_message: row.error_message,
            correlation_id: row.correlation_id,
            started_at: row.started_at,
            completed_at: row.completed_at,
            duration_ms: row.duration_ms,
        }
    }
}

/// How many rows the list's filters match.
///
/// **Changed together with [`list_logs`]**: a `meta.total` counted over a
/// different population than the rows is not a smaller version of the same
/// answer.
pub async fn count_logs(
    pool: &PgPool,
    tenant_id: Uuid,
    filter: &IntegrationLogFilter<'_>,
) -> Result<i64, sqlx::Error> {
    let (entity_type, entity_id) = filter.entity.unzip();

    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM integration_logs l
        WHERE l.tenant_id = $1
          AND ($2::uuid IS NULL OR l.external_system_id = $2)
          AND ($3::text IS NULL OR (l.entity_type = $3 AND l.entity_id = $4))
          AND ($5::text IS NULL OR l.status = $5)
          AND ($6::timestamptz IS NULL OR l.started_at >= $6)
          AND ($7::timestamptz IS NULL OR l.started_at < $7)
        "#,
        tenant_id,
        filter.external_system_id,
        entity_type,
        entity_id,
        filter.status,
        filter.from,
        filter.to,
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of rows, newest first; `id` breaks a tie in `started_at`, so a
/// page boundary never repeats or skips a row.
///
/// The system's code and name are joined **whatever its state**: a log names
/// the system a call was made to, and a system deactivated since is still that
/// system.
pub async fn list_logs(
    pool: &PgPool,
    tenant_id: Uuid,
    filter: &IntegrationLogFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<Vec<IntegrationLogSummary>, sqlx::Error> {
    let (entity_type, entity_id) = filter.entity.unzip();

    let rows = sqlx::query_as!(
        SummaryRow,
        r#"
        SELECT l.id, l.external_system_id,
               s.system_code AS "external_system_code?",
               s.system_name AS "external_system_name?",
               l.direction, l.integration_type, l.method, l.endpoint, l.entity_type,
               l.entity_id, l.status, l.status_code, l.error_message, l.correlation_id,
               l.started_at, l.completed_at, l.duration_ms
        FROM integration_logs l
        LEFT JOIN external_systems s
               ON s.id = l.external_system_id AND s.tenant_id = l.tenant_id
        WHERE l.tenant_id = $1
          AND ($2::uuid IS NULL OR l.external_system_id = $2)
          AND ($3::text IS NULL OR (l.entity_type = $3 AND l.entity_id = $4))
          AND ($5::text IS NULL OR l.status = $5)
          AND ($6::timestamptz IS NULL OR l.started_at >= $6)
          AND ($7::timestamptz IS NULL OR l.started_at < $7)
        ORDER BY l.started_at DESC, l.id DESC
        LIMIT $8 OFFSET $9
        "#,
        tenant_id,
        filter.external_system_id,
        entity_type,
        entity_id,
        filter.status,
        filter.from,
        filter.to,
        limit,
        offset,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

/// One row whole, in the caller's tenant only — another tenant's id is
/// `None`, exactly as an id that does not exist.
pub async fn find_log(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<IntegrationLog>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT l.id, l.external_system_id,
               s.system_code AS "external_system_code?",
               s.system_name AS "external_system_name?",
               l.direction, l.integration_type, l.method, l.endpoint, l.entity_type,
               l.entity_id, l.status, l.status_code, l.error_message, l.correlation_id,
               l.started_at, l.completed_at, l.duration_ms,
               l.document_id, l.request_payload_json, l.response_payload_json
        FROM integration_logs l
        LEFT JOIN external_systems s
               ON s.id = l.external_system_id AND s.tenant_id = l.tenant_id
        WHERE l.tenant_id = $1 AND l.id = $2
        "#,
        tenant_id,
        id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| IntegrationLog {
        summary: SummaryRow {
            id: row.id,
            external_system_id: row.external_system_id,
            external_system_code: row.external_system_code,
            external_system_name: row.external_system_name,
            direction: row.direction,
            integration_type: row.integration_type,
            method: row.method,
            endpoint: row.endpoint,
            entity_type: row.entity_type,
            entity_id: row.entity_id,
            status: row.status,
            status_code: row.status_code,
            error_message: row.error_message,
            correlation_id: row.correlation_id,
            started_at: row.started_at,
            completed_at: row.completed_at,
            duration_ms: row.duration_ms,
        }
        .into(),
        document_id: row.document_id,
        request_payload: row.request_payload_json,
        response_payload: row.response_payload_json,
    }))
}
