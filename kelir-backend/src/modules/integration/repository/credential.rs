//! Queries for `integration_credentials` (§12.3).
//!
//! **No statement here selects anything but the reference.** There is no
//! secret column to select; the point of saying so is that a later change
//! adding one would have to add it here, in a file whose header says it does
//! not belong.

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::split;
use crate::modules::integration::domain::{AuthType, IntegrationCredential};

pub struct NewCredential<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub external_system_id: Uuid,
    pub credential_type: &'a str,
    pub secret_reference: &'a str,
    pub valid_from: Option<NaiveDate>,
    pub valid_to: Option<NaiveDate>,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
}

pub struct CredentialFields<'a> {
    pub credential_type: Option<&'a str>,
    pub secret_reference: Option<&'a str>,
    pub valid_from: Option<Option<NaiveDate>>,
    pub valid_to: Option<Option<NaiveDate>>,
    pub is_active: Option<bool>,
}

struct Row {
    id: Uuid,
    external_system_id: Uuid,
    credential_type: String,
    secret_reference: String,
    valid_from: Option<NaiveDate>,
    valid_to: Option<NaiveDate>,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Row {
    /// `None` for a `credential_type` outside the vocabulary, which the `CHECK`
    /// makes impossible; the row is then skipped rather than guessed at.
    fn into_credential(self) -> Option<IntegrationCredential> {
        Some(IntegrationCredential {
            id: self.id,
            external_system_id: self.external_system_id,
            credential_type: AuthType::from_db(&self.credential_type)?,
            secret_reference: self.secret_reference,
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// **Changed together with [`list_credentials`]** — the same population.
pub async fn count_credentials(
    pool: &PgPool,
    tenant_id: Uuid,
    external_system_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM integration_credentials
        WHERE tenant_id = $1 AND external_system_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of a system's credential references, inactive ones included,
/// oldest first.
pub async fn list_credentials(
    pool: &PgPool,
    tenant_id: Uuid,
    external_system_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<IntegrationCredential>, sqlx::Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"
        SELECT id, external_system_id, credential_type, secret_reference, valid_from,
               valid_to, is_active, created_at, updated_at
        FROM integration_credentials
        WHERE tenant_id = $1 AND external_system_id = $2 AND deleted_at IS NULL
        ORDER BY created_at, id
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        external_system_id,
        limit,
        offset,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().filter_map(Row::into_credential).collect())
}

pub async fn find_credential<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    external_system_id: Uuid,
    id: Uuid,
) -> Result<Option<IntegrationCredential>, sqlx::Error> {
    let row = sqlx::query_as!(
        Row,
        r#"
        SELECT id, external_system_id, credential_type, secret_reference, valid_from,
               valid_to, is_active, created_at, updated_at
        FROM integration_credentials
        WHERE tenant_id = $1 AND external_system_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
        id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.and_then(Row::into_credential))
}

pub async fn insert_credential<'e, E: PgExecutor<'e>>(
    executor: E,
    credential: &NewCredential<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO integration_credentials
            (id, tenant_id, external_system_id, credential_type, secret_reference,
             valid_from, valid_to, is_active, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        "#,
        credential.id,
        credential.tenant_id,
        credential.external_system_id,
        credential.credential_type,
        credential.secret_reference,
        credential.valid_from,
        credential.valid_to,
        credential.is_active,
        credential.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_credential<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    external_system_id: Uuid,
    id: Uuid,
    fields: &CredentialFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (from_set, valid_from) = split(fields.valid_from);
    let (to_set, valid_to) = split(fields.valid_to);

    sqlx::query!(
        r#"
        UPDATE integration_credentials
        SET credential_type = COALESCE($4, credential_type),
            secret_reference = COALESCE($5, secret_reference),
            valid_from = CASE WHEN $6 THEN $7 ELSE valid_from END,
            valid_to = CASE WHEN $8 THEN $9 ELSE valid_to END,
            is_active = COALESCE($10, is_active),
            updated_by = $11,
            updated_at = now()
        WHERE tenant_id = $1 AND external_system_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
        id,
        fields.credential_type,
        fields.secret_reference,
        from_set,
        valid_from,
        to_set,
        valid_to,
        fields.is_active,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Removes a credential reference by soft delete.
pub async fn soft_delete_credential<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    external_system_id: Uuid,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE integration_credentials
        SET deleted_at = now(), updated_by = $4, updated_at = now()
        WHERE tenant_id = $1 AND external_system_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        external_system_id,
        id,
        deleted_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}
