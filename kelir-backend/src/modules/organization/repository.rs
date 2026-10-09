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
/// whose session suspension would stop being renewed, and the number exists to
/// answer exactly that question.
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

/// Every live tenant's id and code, in one statement (#618).
///
/// Unscoped for the reason [`find_by_code`] is: `tenants` defines the
/// partition. A deployment's tenants are few, and both callers — an
/// integration test call, and creating a tenant (#655) — are an
/// administrator's action, so reading them all keeps the rule deciding which
/// names a tenant may read in one place, in Rust
/// (`integration::domain::secret`), rather than half in this `WHERE`.
pub async fn live_codes(executor: impl PgExecutor<'_>) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    let rows = sqlx::query!("SELECT id, tenant_code FROM tenants WHERE deleted_at IS NULL")
        .fetch_all(executor)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| (row.id, row.tenant_code))
        .collect())
}

/// Serialises tenant creation, for the secret-namespace check (#655).
///
/// The check reads every live tenant's code and refuses a new one that shares
/// an integration secret name with any of them. The unique index cannot
/// enforce that, since the codes differ (`A-B` and `A_B`), so two creations
/// racing would each read the codes without the other's and both insert. The
/// lock is taken before the read, in the creating transaction, and held until
/// it commits: a creation that waits on it reads the codes the one before it
/// committed. Creating a tenant is a rare administrative act, so one lock for
/// the whole deployment costs nothing anybody will measure. Deleting a tenant
/// does not take it: a delete can only make a later check pass that would have
/// failed, never the reverse.
///
/// The two-argument form, keyed like [`super::department_repository`]'s, and
/// the same statement, so it shares that statement's offline query data.
pub async fn lock_tenant_codes(connection: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock($1, hashtext($2::text))",
        TENANT_CODE_LOCK_CLASS,
        TENANT_CODE_LOCK_KEY
    )
    .execute(connection)
    .await
    .map(|_| ())
}

/// Lock class for [`lock_tenant_codes`]: the ASCII of `TNCD`.
/// `tests/organization_tenants.rs` repeats it to hold the lock in a test.
const TENANT_CODE_LOCK_CLASS: i32 = 0x544E_4344;

/// The key within the class. One, since the lock is deployment-wide.
const TENANT_CODE_LOCK_KEY: &str = "tenant_code";

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
/// Suspending or deleting a tenant has to stop its users' sessions being
/// renewed, not merely stop new sign-ins — the same rule `identity::service`
/// applies to a deactivated account, for the same reason: a refresh token
/// issued a minute ago is still valid otherwise. An access token already
/// issued is not reached by this: a suspended or inactive tenant's works until
/// it expires, at most 15 minutes from issue (SDD §11.1, decision D-104), and
/// a deleted tenant's is refused on its next request by `middleware::auth`
/// (D-105). This is the half that can be revoked, and it is what stops the
/// session being extended.
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
