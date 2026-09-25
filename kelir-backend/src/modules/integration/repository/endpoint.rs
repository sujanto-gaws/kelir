//! Queries for `integration_endpoints` (§12.2).
//!
//! Every read names the system as well as the tenant: an endpoint id is found
//! only under the system it belongs to, so `/external-systems/A/endpoints/{x}`
//! does not answer for an endpoint of system B.

use chrono::{DateTime, Utc};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::split;
use crate::modules::integration::domain::{EndpointStatus, HttpMethod, IntegrationEndpoint};

pub struct NewEndpoint<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub external_system_id: Uuid,
    pub endpoint_code: &'a str,
    pub name: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub description: Option<&'a str>,
    pub created_by: Option<Uuid>,
}

pub struct EndpointFields<'a> {
    pub name: Option<&'a str>,
    pub method: Option<&'a str>,
    pub path: Option<&'a str>,
    pub description: Option<Option<&'a str>>,
    pub status: Option<&'a str>,
}

struct Row {
    id: Uuid,
    external_system_id: Uuid,
    endpoint_code: String,
    name: String,
    method: String,
    path: String,
    description: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for IntegrationEndpoint {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            external_system_id: row.external_system_id,
            endpoint_code: row.endpoint_code,
            name: row.name,
            method: HttpMethod::from_db(&row.method),
            path: row.path,
            description: row.description,
            status: EndpointStatus::from_db(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// **Changed together with [`list_endpoints`]** — the same population.
pub async fn count_endpoints(
    pool: &PgPool,
    tenant_id: Uuid,
    external_system_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM integration_endpoints
        WHERE tenant_id = $1 AND external_system_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of a system's endpoints, `INACTIVE` ones included, ordered by code.
pub async fn list_endpoints(
    pool: &PgPool,
    tenant_id: Uuid,
    external_system_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<IntegrationEndpoint>, sqlx::Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT id, external_system_id, endpoint_code, name, method, path, description,
               status, created_at, updated_at
        FROM integration_endpoints
        WHERE tenant_id = $1 AND external_system_id = $2 AND deleted_at IS NULL
        ORDER BY endpoint_code, id
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        external_system_id,
        limit,
        offset,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(IntegrationEndpoint::from).collect())
}

pub async fn find_endpoint<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    external_system_id: Uuid,
    id: Uuid,
) -> Result<Option<IntegrationEndpoint>, sqlx::Error> {
    let row = sqlx::query_as!(
        Row,
        r#"
        SELECT id, external_system_id, endpoint_code, name, method, path, description,
               status, created_at, updated_at
        FROM integration_endpoints
        WHERE tenant_id = $1 AND external_system_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
        id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(IntegrationEndpoint::from))
}

pub async fn insert_endpoint<'e, E: PgExecutor<'e>>(
    executor: E,
    endpoint: &NewEndpoint<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO integration_endpoints
            (id, tenant_id, external_system_id, endpoint_code, name, method, path,
             description, status, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'ACTIVE', $9, $9)
        "#,
        endpoint.id,
        endpoint.tenant_id,
        endpoint.external_system_id,
        endpoint.endpoint_code,
        endpoint.name,
        endpoint.method,
        endpoint.path,
        endpoint.description,
        endpoint.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_endpoint<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    external_system_id: Uuid,
    id: Uuid,
    fields: &EndpointFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (description_set, description) = split(fields.description);

    sqlx::query!(
        r#"
        UPDATE integration_endpoints
        SET name = COALESCE($4, name),
            method = COALESCE($5, method),
            path = COALESCE($6, path),
            description = CASE WHEN $7 THEN $8 ELSE description END,
            status = COALESCE($9, status),
            updated_by = $10,
            updated_at = now()
        WHERE tenant_id = $1 AND external_system_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
        id,
        fields.name,
        fields.method,
        fields.path,
        description_set,
        description,
        fields.status,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}
