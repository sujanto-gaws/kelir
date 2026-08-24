//! Queries for `document_types` and its workflow bindings (§6.2, §6.4).
//!
//! Tenant-scoped and soft-delete aware throughout, for the reasons the RAD
//! repository states.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::domain::{
    DocumentType, DocumentTypeStatus, DocumentTypeSummary, SecurityLevel, WorkflowBinding,
};

pub struct NewDocumentType<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub type_code: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub category: Option<&'a str>,
    pub form_id: Option<Uuid>,
    pub list_id: Option<Uuid>,
    pub default_security_level: &'a str,
    pub retention_policy_id: Option<Uuid>,
    pub target_entity_type: Option<&'a str>,
    pub status: &'a str,
    pub created_by: Option<Uuid>,
}

/// What an update may change. `None` leaves the column alone; the nested
/// `Option` on a nullable column distinguishes "leave it" from "clear it",
/// which `COALESCE` alone cannot express.
pub struct DocumentTypeFields<'a> {
    pub name: Option<&'a str>,
    pub description: Option<Option<&'a str>>,
    pub category: Option<Option<&'a str>>,
    pub form_id: Option<Option<Uuid>>,
    pub list_id: Option<Option<Uuid>>,
    pub default_security_level: Option<&'a str>,
    pub retention_policy_id: Option<Option<Uuid>>,
    pub target_entity_type: Option<Option<&'a str>>,
    pub status: Option<&'a str>,
}

/// What a form has to be for a document type to bind it.
///
/// Returned rather than a bare boolean so the service can say *why* a binding
/// was refused — "no such form" and "that form is a draft" are different
/// mistakes and a caller fixes them differently.
pub struct BindableForm {
    pub status: String,
}

pub async fn count_types(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM document_types WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_types(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<DocumentTypeSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, type_code, name, category, form_id, status, created_at, updated_at
        FROM document_types
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY type_code
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
        .map(|row| DocumentTypeSummary {
            id: row.id,
            type_code: row.type_code,
            name: row.name,
            category: row.category,
            form_id: row.form_id,
            status: DocumentTypeStatus::from_db(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_type<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<DocumentType>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, type_code, name, description, category, form_id, list_id,
               default_security_level, retention_policy_id, target_entity_type,
               status, created_at, updated_at
        FROM document_types
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(DocumentType {
        id: row.id,
        type_code: row.type_code,
        name: row.name,
        description: row.description,
        category: row.category,
        form_id: row.form_id,
        list_id: row.list_id,
        default_security_level: SecurityLevel::from_db(&row.default_security_level),
        retention_policy_id: row.retention_policy_id,
        target_entity_type: row.target_entity_type,
        status: DocumentTypeStatus::from_db(&row.status),
        workflows: workflows_of(executor, tenant_id, id).await?,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

async fn workflows_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Vec<WorkflowBinding>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT workflow_definition_id, condition_expression, priority, valid_from, valid_to
        FROM document_type_workflows
        WHERE tenant_id = $1 AND document_type_id = $2 AND deleted_at IS NULL
        ORDER BY priority, workflow_definition_id
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| WorkflowBinding {
            workflow_definition_id: row.workflow_definition_id,
            condition_expression: row.condition_expression,
            priority: row.priority,
            valid_from: row.valid_from,
            valid_to: row.valid_to,
        })
        .collect())
}

/// Reads a form **and holds it** for the rest of the transaction.
///
/// `FOR SHARE` rather than a plain read, and that is the whole point of this
/// function. Checking that a form exists on the pool and then writing the
/// binding is check-then-act: a soft delete landing in between leaves a
/// document type pointing at a retired definition, which is the shape #133 and
/// #137 fixed on facilities. A share lock blocks the `UPDATE` a soft delete
/// performs until this transaction commits, and blocks nothing else — two types
/// binding the same form do not contend.
pub async fn lock_bindable_form(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    form_id: Uuid,
) -> Result<Option<BindableForm>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT status FROM rad_forms
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR SHARE
        "#,
        tenant_id,
        form_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| BindableForm { status: row.status }))
}

/// The same for a list definition, which has no published state to check.
pub async fn lock_bindable_list(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    list_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_scalar!(
        r#"
        SELECT id FROM rad_lists
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR SHARE
        "#,
        tenant_id,
        list_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.is_some())
}

pub async fn insert_type<'e, E: PgExecutor<'e>>(
    executor: E,
    document_type: &NewDocumentType<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO document_types
            (id, tenant_id, type_code, name, description, category, form_id, list_id,
             default_security_level, retention_policy_id, target_entity_type, status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        document_type.id,
        document_type.tenant_id,
        document_type.type_code,
        document_type.name,
        document_type.description,
        document_type.category,
        document_type.form_id,
        document_type.list_id,
        document_type.default_security_level,
        document_type.retention_policy_id,
        document_type.target_entity_type,
        document_type.status,
        document_type.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_type<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &DocumentTypeFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (description_set, description) = split(fields.description);
    let (category_set, category) = split(fields.category);
    let (form_set, form_id) = split(fields.form_id);
    let (list_set, list_id) = split(fields.list_id);
    let (retention_set, retention_policy_id) = split(fields.retention_policy_id);
    let (entity_set, target_entity_type) = split(fields.target_entity_type);

    sqlx::query!(
        r#"
        UPDATE document_types
        SET name = COALESCE($3, name),
            description = CASE WHEN $4 THEN $5 ELSE description END,
            category = CASE WHEN $6 THEN $7 ELSE category END,
            form_id = CASE WHEN $8 THEN $9 ELSE form_id END,
            list_id = CASE WHEN $10 THEN $11 ELSE list_id END,
            default_security_level = COALESCE($12, default_security_level),
            retention_policy_id = CASE WHEN $13 THEN $14 ELSE retention_policy_id END,
            target_entity_type = CASE WHEN $15 THEN $16 ELSE target_entity_type END,
            status = COALESCE($17, status),
            updated_by = $18,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.name,
        description_set,
        description,
        category_set,
        category,
        form_set,
        form_id,
        list_set,
        list_id,
        fields.default_security_level,
        retention_set,
        retention_policy_id,
        entity_set,
        target_entity_type,
        fields.status,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// `Option<Option<T>>` as the two values a `CASE WHEN` needs.
fn split<T>(field: Option<Option<T>>) -> (bool, Option<T>) {
    match field {
        None => (false, None),
        Some(value) => (true, value),
    }
}

/// Replaces a type's workflow bindings wholesale.
///
/// A hard delete, and the exception coding standard §4 allows rather than the
/// practice it forbids — the same reasoning as the list's columns: these rows
/// are the stored form of an array the caller just sent, and soft-deleting them
/// would leave a dead row per binding per edit that every read has to filter
/// around.
pub async fn replace_workflows(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    workflows: &[WorkflowBinding],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM document_type_workflows WHERE tenant_id = $1 AND document_type_id = $2",
        tenant_id,
        document_type_id
    )
    .execute(&mut **transaction)
    .await?;

    for binding in workflows {
        sqlx::query!(
            r#"
            INSERT INTO document_type_workflows
                (id, tenant_id, document_type_id, workflow_definition_id,
                 condition_expression, priority, valid_from, valid_to, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            Uuid::now_v7(),
            tenant_id,
            document_type_id,
            binding.workflow_definition_id,
            binding.condition_expression.as_deref(),
            binding.priority,
            binding.valid_from,
            binding.valid_to,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

/// Retires a document type by soft delete.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE document_types
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

/// Whether any document was created from this type.
///
/// Read before a delete: retiring a type that documents were created from would
/// leave those documents pointing at something no read returns. Phase 4 has no
/// document-creation endpoint, so this answers `false` today — it is here
/// because the delete is written now and the documents arrive in Sprint 9,
/// and a refusal added later is a refusal somebody has to remember.
pub async fn has_documents<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM documents
            WHERE tenant_id = $1 AND document_type_id = $2 AND deleted_at IS NULL
        ) AS "exists!"
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_one(executor)
    .await
}
