use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{Tenant, TenantStatus};

/// A tenant row as the administration surface reads it.
///
/// Distinct from [`Tenant`], which is what the sign-in resolver needs and
/// nothing more. Keeping them apart is what stops the resolver — the hottest
/// path in the system, run before every credential check — growing a
/// correlated subquery it has no use for.
#[derive(Debug, Clone)]
pub struct TenantRecord {
    pub id: Uuid,
    pub tenant_code: String,
    pub name: String,
    pub status: TenantStatus,
    pub user_count: i64,
    pub created_at: DateTime<Utc>,
}

/// Looks a tenant up by its canonical code.
///
/// The one query in the system that does not filter `tenant_id`: `tenants` is
/// the table that *defines* the partition, so there is no outer tenant to scope
/// it by. `deleted_at IS NULL` still applies — a soft-deleted tenant resolves to
/// nothing, exactly like an unknown one.
///
/// The caller passes a code already normalised by
/// [`super::domain::normalize_tenant_code`]; comparing the column directly keeps
/// the unique index in play.
pub async fn find_by_code(
    executor: impl PgExecutor<'_>,
    tenant_code: &str,
) -> Result<Option<Tenant>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, tenant_code, name, status
        FROM tenants
        WHERE tenant_code = $1 AND deleted_at IS NULL
        "#,
        tenant_code
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Tenant {
        id: row.id,
        tenant_code: row.tenant_code,
        name: row.name,
        status: TenantStatus::from_db(&row.status),
    }))
}

/// Live tenants, newest first, with the users each one holds.
///
/// **The one list in the system that is not scoped by `tenant_id`**, for the
/// same reason [`find_by_code`] is not: `tenants` defines the partition, so
/// there is no outer tenant to scope it by. What stands in for that scoping is
/// the caller check in the service — administration is performed only from the
/// deployment's default tenant — and it is the whole of the boundary, so it is
/// worth knowing that this function offers none of its own.
///
/// `user_count` counts live users only. A soft-deleted account is not somebody
/// whose session suspension would end, and the number exists to answer exactly
/// that question.
pub async fn list(
    executor: impl PgExecutor<'_>,
    limit: i64,
    offset: i64,
) -> Result<Vec<TenantRecord>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT t.id, t.tenant_code, t.name, t.status, t.created_at,
               (
                   SELECT count(*)
                   FROM users u
                   WHERE u.tenant_id = t.id AND u.deleted_at IS NULL
               ) AS "user_count!"
        FROM tenants t
        WHERE t.deleted_at IS NULL
        ORDER BY t.created_at DESC, t.tenant_code
        LIMIT $1 OFFSET $2
        "#,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantRecord {
            id: row.id,
            tenant_code: row.tenant_code,
            name: row.name,
            status: TenantStatus::from_db(&row.status),
            user_count: row.user_count,
            created_at: row.created_at,
        })
        .collect())
}

pub async fn count(executor: impl PgExecutor<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!("SELECT count(*) FROM tenants WHERE deleted_at IS NULL")
        .fetch_one(executor)
        .await
        .map(|count| count.unwrap_or(0))
}

pub async fn find(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<TenantRecord>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT t.id, t.tenant_code, t.name, t.status, t.created_at,
               (
                   SELECT count(*)
                   FROM users u
                   WHERE u.tenant_id = t.id AND u.deleted_at IS NULL
               ) AS "user_count!"
        FROM tenants t
        WHERE t.id = $1 AND t.deleted_at IS NULL
        "#,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| TenantRecord {
        id: row.id,
        tenant_code: row.tenant_code,
        name: row.name,
        status: TenantStatus::from_db(&row.status),
        user_count: row.user_count,
        created_at: row.created_at,
    }))
}

/// Inserts a tenant. The caller passes an already-normalised code.
///
/// A duplicate code raises the unique violation on `uq_tenants_tenant_code`
/// rather than being pre-checked: a check followed by an insert is two
/// statements a concurrent creator can slip between, and the constraint is the
/// thing that is actually true.
pub async fn insert(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_code: &str,
    name: &str,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO tenants (id, tenant_code, name, status, created_by, updated_by)
        VALUES ($1, $2, $3, 'ACTIVE', $4, $4)
        "#,
        id,
        tenant_code,
        name,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Applies the fields an update actually carries, leaving the rest alone.
///
/// Returns the number of rows changed, so the service can tell "no such tenant"
/// from "updated".
pub async fn update_fields(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    name: Option<&str>,
    status: Option<&str>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE tenants
        SET name = COALESCE($2, name),
            status = COALESCE($3, status),
            updated_by = $4,
            updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        "#,
        id,
        name,
        status,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Soft-deletes a tenant. `INACTIVE` as well as `deleted_at`, mirroring how a
/// user is deactivated: a row that is gone should not also read as `ACTIVE` to
/// anything that forgets the `deleted_at` filter.
pub async fn soft_delete(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE tenants
        SET status = 'INACTIVE', deleted_at = now(), updated_by = $2, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        "#,
        id,
        deleted_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Revokes every refresh token belonging to a tenant, returning how many.
///
/// Suspending or deleting a tenant has to end its users' sessions, not merely
/// stop new sign-ins — the same rule `identity::service` applies to a
/// deactivated account, for the same reason: a token issued a minute ago is
/// still valid otherwise. Access tokens are stateless and live out their
/// fifteen minutes (architecture 01 §18.1); this is the half that can be
/// revoked, and it is what stops the session being extended.
pub async fn revoke_sessions(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    reason: &str,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = now(), revoked_reason = $2
        WHERE tenant_id = $1 AND revoked_at IS NULL
        "#,
        tenant_id,
        reason
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}
