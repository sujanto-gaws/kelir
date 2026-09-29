//! Writes to `integration_logs` (Database Schema §12.5) — **the table's first
//! writer** (FR-INT-002, #547).
//!
//! Append-only: there is an insert and nothing else. The payload columns take
//! what `service::test_call` has already masked; nothing here sees a secret.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

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
