use sqlx::PgExecutor;

use super::domain::{Tenant, TenantStatus};

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
