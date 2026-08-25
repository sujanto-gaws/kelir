//! Queries for `rad_forms` (§5.3).
//!
//! **Every statement is tenant-scoped and soft-delete aware.** Not as a style
//! rule: a read that forgets `tenant_id` returns another tenant's form
//! definition, and one that forgets `deleted_at IS NULL` resurrects a retired
//! revision into a renderer. `rad_permissions.rs` holds the test that a caller
//! cannot reach either.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::rad::domain::{Form, FormStatus, FormSummary};

/// The columns a create writes.
///
/// `status` is absent: a new revision is always `DRAFT`, which is the column
/// default, and a create that could choose `PUBLISHED` would be a way past the
/// permission that publishing needs.
pub struct NewForm<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub form_key: &'a str,
    pub title: &'a str,
    pub revision: i32,
    pub jfss_version: &'a str,
    pub definition_json: &'a Value,
    pub entity_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

/// What an update may change. `None` leaves the column alone.
///
/// `entity_id` is `Option<Option<Uuid>>` because it is nullable and clearing it
/// is a real edit — `COALESCE` alone cannot express the difference between
/// "leave it" and "clear it".
pub struct FormFields<'a> {
    pub title: Option<&'a str>,
    pub definition_json: Option<&'a Value>,
    pub entity_id: Option<Option<Uuid>>,
}

pub async fn count_forms(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM rad_forms WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_forms(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<FormSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, form_key, title, revision, jfss_version, status, entity_id,
               created_at, updated_at
        FROM rad_forms
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY form_key, revision DESC
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| FormSummary {
            id: row.id,
            form_key: row.form_key,
            title: row.title,
            revision: row.revision,
            jfss_version: row.jfss_version,
            status: FormStatus::from_db(&row.status),
            entity_id: row.entity_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_form<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Form>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, form_key, title, revision, jfss_version, definition_json,
               status, entity_id, published_at, published_by, created_at, updated_at
        FROM rad_forms
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Form {
        id: row.id,
        form_key: row.form_key,
        title: row.title,
        revision: row.revision,
        jfss_version: row.jfss_version,
        status: FormStatus::from_db(&row.status),
        entity_id: row.entity_id,
        definition: row.definition_json,
        published_at: row.published_at,
        published_by: row.published_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// The highest revision of `form_key`, or `None` when the key is unused.
///
/// Counts soft-deleted rows deliberately. `uq_rad_forms_tenant_id_form_key_revision`
/// is partial on `deleted_at IS NULL`, so reusing a retired revision number
/// would insert without complaint and leave two rows that mean the same
/// `(formKey, revision)` — one of which a document may still pin.
pub async fn highest_revision<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    form_key: &str,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT max(revision) FROM rad_forms WHERE tenant_id = $1 AND form_key = $2",
        tenant_id,
        form_key
    )
    .fetch_one(executor)
    .await
}

pub async fn insert_form<'e, E: PgExecutor<'e>>(
    executor: E,
    form: &NewForm<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO rad_forms
            (id, tenant_id, form_key, title, revision, jfss_version,
             definition_json, entity_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        form.id,
        form.tenant_id,
        form.form_key,
        form.title,
        form.revision,
        form.jfss_version,
        form.definition_json,
        form.entity_id,
        form.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Applies an edit to a draft revision.
///
/// **`AND status = 'DRAFT'` is in the statement, not only in the service.** The
/// service reads the row, refuses a published one and then writes; between
/// those two steps a concurrent publish can land, and without this predicate
/// the edit would apply to a revision that had just become immutable. The
/// caller sees zero rows affected and treats it as the same refusal.
pub async fn update_draft<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &FormFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (entity_id_set, entity_id) = match fields.entity_id {
        None => (false, None),
        Some(value) => (true, value),
    };

    sqlx::query!(
        r#"
        UPDATE rad_forms
        SET title = COALESCE($3, title),
            definition_json = COALESCE($4, definition_json),
            entity_id = CASE WHEN $5 THEN $6 ELSE entity_id END,
            updated_by = $7,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        fields.title,
        fields.definition_json,
        entity_id_set,
        entity_id,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Publishes a draft revision.
///
/// The same reasoning as `update_draft`: `status = 'DRAFT'` is a predicate
/// rather than a prior read, so publishing twice writes once. The second call
/// affects no rows, and the second publisher is not recorded as the publisher.
pub async fn publish<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    published_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE rad_forms
        SET status = 'PUBLISHED',
            published_at = now(),
            published_by = $3,
            updated_by = $3,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        published_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Retires a revision by soft delete.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE rad_forms
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        deleted_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// When a form was published and by whom, for the audit record.
pub struct Publication {
    pub published_at: Option<DateTime<Utc>>,
    pub published_by: Option<Uuid>,
}

pub async fn publication<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Publication>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT published_at, published_by FROM rad_forms
         WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL",
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Publication {
        published_at: row.published_at,
        published_by: row.published_by,
    }))
}
