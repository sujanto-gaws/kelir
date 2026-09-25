//! Queries for `external_systems` (§12.1).

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::{like_contains, split};
use crate::modules::integration::domain::{
    AuthType, ExternalSystem, ExternalSystemStatus, ExternalSystemType, RetryPolicy,
};

pub struct NewExternalSystem<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub system_code: &'a str,
    pub system_name: &'a str,
    pub system_type: Option<&'a str>,
    pub base_url: Option<&'a str>,
    pub auth_type: Option<&'a str>,
    pub timeout_seconds: i32,
    pub retry_policy: Value,
    pub description: Option<&'a str>,
    pub created_by: Option<Uuid>,
}

/// What an edit may change. `None` leaves a column alone; the nested `Option`
/// on a nullable column tells *leave it* from *clear it*.
pub struct ExternalSystemFields<'a> {
    pub system_name: Option<&'a str>,
    pub system_type: Option<Option<&'a str>>,
    pub base_url: Option<Option<&'a str>>,
    pub auth_type: Option<Option<&'a str>>,
    pub timeout_seconds: Option<i32>,
    pub retry_policy: Option<Value>,
    pub description: Option<Option<&'a str>>,
    pub status: Option<&'a str>,
}

/// The list's filters, already reduced to column values.
pub struct ExternalSystemFilter<'a> {
    pub search: Option<&'a str>,
    pub status: Option<&'a str>,
    pub system_type: Option<&'a str>,
}

/// One row, before its vocabularies are read.
struct Row {
    id: Uuid,
    system_code: String,
    system_name: String,
    system_type: Option<String>,
    base_url: Option<String>,
    auth_type: Option<String>,
    timeout_seconds: i32,
    retry_policy_json: Value,
    description: Option<String>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for ExternalSystem {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            system_code: row.system_code,
            system_name: row.system_name,
            system_type: row
                .system_type
                .as_deref()
                .and_then(ExternalSystemType::from_db),
            base_url: row.base_url,
            auth_type: row.auth_type.as_deref().and_then(AuthType::from_db),
            timeout_seconds: row.timeout_seconds,
            retry_policy: RetryPolicy::from_db(row.retry_policy_json),
            description: row.description,
            status: ExternalSystemStatus::from_db(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// How many systems the list's filters match.
///
/// **Changed together with [`list_external_systems`]**: a `meta.total`
/// counted over a different population than the rows is not a smaller version
/// of the same answer.
pub async fn count_external_systems(
    pool: &PgPool,
    tenant_id: Uuid,
    filter: &ExternalSystemFilter<'_>,
) -> Result<i64, sqlx::Error> {
    let search = filter.search.map(like_contains);

    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM external_systems
        WHERE tenant_id = $1
          AND deleted_at IS NULL
          AND ($2::text IS NULL OR system_code ILIKE $2 OR system_name ILIKE $2)
          AND ($3::text IS NULL OR status = $3)
          AND ($4::text IS NULL OR system_type = $4)
        "#,
        tenant_id,
        search,
        filter.status,
        filter.system_type,
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of systems, ordered by code.
pub async fn list_external_systems(
    pool: &PgPool,
    tenant_id: Uuid,
    filter: &ExternalSystemFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<Vec<ExternalSystem>, sqlx::Error> {
    let search = filter.search.map(like_contains);

    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT id, system_code, system_name, system_type, base_url, auth_type,
               timeout_seconds, retry_policy_json, description, status,
               created_at, updated_at
        FROM external_systems
        WHERE tenant_id = $1
          AND deleted_at IS NULL
          AND ($2::text IS NULL OR system_code ILIKE $2 OR system_name ILIKE $2)
          AND ($3::text IS NULL OR status = $3)
          AND ($4::text IS NULL OR system_type = $4)
        ORDER BY system_code, id
        LIMIT $5 OFFSET $6
        "#,
        tenant_id,
        search,
        filter.status,
        filter.system_type,
        limit,
        offset,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(ExternalSystem::from).collect())
}

pub async fn find_external_system<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<ExternalSystem>, sqlx::Error> {
    let row = sqlx::query_as!(
        Row,
        r#"
        SELECT id, system_code, system_name, system_type, base_url, auth_type,
               timeout_seconds, retry_policy_json, description, status,
               created_at, updated_at
        FROM external_systems
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(ExternalSystem::from))
}

/// Whether a live system with this id exists in this tenant — the gate every
/// endpoint and credential route passes before it reads a child row.
pub async fn external_system_exists<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM external_systems
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        ) AS "exists!"
        "#,
        tenant_id,
        id,
    )
    .fetch_one(executor)
    .await
}

pub async fn insert_external_system<'e, E: PgExecutor<'e>>(
    executor: E,
    system: &NewExternalSystem<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO external_systems
            (id, tenant_id, system_code, system_name, system_type, base_url, auth_type,
             timeout_seconds, retry_policy_json, description, status, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'ACTIVE', $11, $11)
        "#,
        system.id,
        system.tenant_id,
        system.system_code,
        system.system_name,
        system.system_type,
        system.base_url,
        system.auth_type,
        system.timeout_seconds,
        system.retry_policy,
        system.description,
        system.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_external_system<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &ExternalSystemFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (type_set, system_type) = split(fields.system_type);
    let (url_set, base_url) = split(fields.base_url);
    let (auth_set, auth_type) = split(fields.auth_type);
    let (description_set, description) = split(fields.description);

    sqlx::query!(
        r#"
        UPDATE external_systems
        SET system_name = COALESCE($3, system_name),
            system_type = CASE WHEN $4 THEN $5 ELSE system_type END,
            base_url = CASE WHEN $6 THEN $7 ELSE base_url END,
            auth_type = CASE WHEN $8 THEN $9 ELSE auth_type END,
            timeout_seconds = COALESCE($10, timeout_seconds),
            retry_policy_json = COALESCE($11, retry_policy_json),
            description = CASE WHEN $12 THEN $13 ELSE description END,
            status = COALESCE($14, status),
            updated_by = $15,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.system_name,
        type_set,
        system_type,
        url_set,
        base_url,
        auth_set,
        auth_type,
        fields.timeout_seconds,
        fields.retry_policy,
        description_set,
        description,
        fields.status,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Takes a system out of service.
///
/// **`AND status <> 'INACTIVE'` makes a repeat a no-op rather than a second
/// write**, so the service can tell *deactivated now* from *already inactive*
/// by the count and record the first only. The service reads the row first and
/// returns early for an inactive one, so this predicate is reached by a second
/// deactivation racing the first — **this layer is unexercised** by the tests.
pub async fn deactivate_external_system<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE external_systems
        SET status = 'INACTIVE', updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status <> 'INACTIVE'
        "#,
        tenant_id,
        id,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}
