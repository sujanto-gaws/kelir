//! Queries over `documents` (Database Schema §6.6).
//!
//! Tenant-scoped and soft-delete aware throughout, for the reasons the RAD and
//! document-type repositories state.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use super::super::domain::{Document, DocumentPriority, DocumentStatus, EntityType};

pub struct NewDocument<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_ref: &'a str,
    pub document_type_id: Uuid,
    /// Pinned from the type's binding at creation. See [`super::super`]'s
    /// module docs and **D-30**.
    pub form_id: Option<Uuid>,
    pub title: &'a str,
    pub form_data: &'a Value,
    pub priority: &'a str,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<Uuid>,
    pub requested_for_department_id: Option<Uuid>,
    pub requested_for_facility_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

/// What an update may change. `None` leaves the column alone; the nested
/// `Option` on a nullable column distinguishes "leave it" from "clear it",
/// which `COALESCE` alone cannot express.
///
/// There is no `status` and no `document_number`, which is not an omission: a
/// transition is [`super::status`]'s statement and a number is the submit's, and
/// each is written by a statement that also writes the history or the timestamp
/// that goes with it. A field on this struct would be a second door to both.
pub struct DocumentFields<'a> {
    pub title: Option<&'a str>,
    pub form_data: Option<&'a Value>,
    pub priority: Option<&'a str>,
    pub entity_type: Option<Option<&'a str>>,
    pub entity_id: Option<Option<Uuid>>,
    pub requested_for_department_id: Option<Option<Uuid>>,
    pub requested_for_facility_id: Option<Option<Uuid>>,
}

/// What the write path needs to know before it decides, read under a lock.
///
/// Returned rather than a bare status so that the caller can refuse *and say
/// why* without a second read: "this is submitted", "this type binds no form",
/// "this document pinned revision 3".
///
/// **It carries the form data and the department too**, and that is the submit's
/// requirement rather than a convenience. Everything the submit decides on has
/// to be read under the same lock as the status it checks: a payload read on the
/// pool and then re-evaluated could be a payload an edit replaced a moment
/// after, and the number would then be attached to arithmetic over data nobody
/// submitted. Coding standard §2.5 puts the lock on what the check read, and the
/// payload is what the check is about.
pub struct LockedDocument {
    pub status: DocumentStatus,
    pub document_type_id: Uuid,
    pub form_id: Option<Uuid>,
    pub form_data: Value,
    pub requested_for_department_id: Option<Uuid>,
}

/// Reads a document **and holds it** for the rest of the transaction.
///
/// `FOR UPDATE`, and coding standard §2.5 is why: every caller of this reads the
/// status, decides on it, and then writes. A check that runs on the pool answers
/// a question about a moment that has already passed — #133 and #137 are both
/// that shape, and #105 is the check-then-act it generalises.
pub async fn lock_document(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<LockedDocument>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT status, document_type_id, form_id, form_data_json,
               requested_for_department_id
        FROM documents
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| LockedDocument {
        status: DocumentStatus::from_db(&row.status),
        document_type_id: row.document_type_id,
        form_id: row.form_id,
        form_data: row.form_data_json,
        requested_for_department_id: row.requested_for_department_id,
    }))
}

pub async fn insert_document<'e, E: PgExecutor<'e>>(
    executor: E,
    document: &NewDocument<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO documents
            (id, tenant_id, document_ref, document_type_id, form_id, title,
             status, form_data_json, priority, entity_type, entity_id,
             requested_by, requested_for_department_id, requested_for_facility_id,
             created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, 'DRAFT', $7, $8, $9, $10, $11, $12, $13, $11, $11)
        "#,
        document.id,
        document.tenant_id,
        document.document_ref,
        document.document_type_id,
        document.form_id,
        document.title,
        document.form_data,
        document.priority,
        document.entity_type,
        document.entity_id,
        document.created_by,
        document.requested_for_department_id,
        document.requested_for_facility_id,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Applies whichever fields the update is setting.
///
/// **The `WHERE` carries `status = 'DRAFT'`**, and it is not redundant with the
/// caller's check even though the caller holds the row: it is what makes the
/// editable rule a property of the statement rather than of whoever remembered
/// to call `lock_document` first. A future caller that forgets writes nothing
/// and gets `0` back.
pub async fn update_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &DocumentFields<'_>,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE documents SET
            title          = COALESCE($3, title),
            form_data_json = COALESCE($4, form_data_json),
            priority       = COALESCE($5, priority),
            entity_type    = CASE WHEN $6  THEN $7  ELSE entity_type END,
            entity_id      = CASE WHEN $8  THEN $9  ELSE entity_id END,
            requested_for_department_id
                           = CASE WHEN $10 THEN $11 ELSE requested_for_department_id END,
            requested_for_facility_id
                           = CASE WHEN $12 THEN $13 ELSE requested_for_facility_id END,
            updated_by     = $14,
            updated_at     = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        fields.title,
        fields.form_data,
        fields.priority,
        fields.entity_type.is_some(),
        fields.entity_type.flatten(),
        fields.entity_id.is_some(),
        fields.entity_id.flatten(),
        fields.requested_for_department_id.is_some(),
        fields.requested_for_department_id.flatten(),
        fields.requested_for_facility_id.is_some(),
        fields.requested_for_facility_id.flatten(),
        actor,
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Discards a draft.
///
/// Soft, and draft-only. A submitted document is *cancelled* rather than
/// deleted — the two answer different questions ("I opened this by mistake"
/// against "this request is withdrawn") and the second has to leave a row an
/// auditor can find, which is [`super::status`]'s transition and not this.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE documents
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        actor
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Loads a document whole, with its metadata.
pub async fn find_document<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Document>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, document_ref, document_number, document_type_id, form_id, title,
               status, priority, form_data_json, entity_type, entity_id,
               requested_by, requested_for_department_id, requested_for_facility_id,
               submitted_at, created_by, created_at, updated_at
        FROM documents
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

    Ok(Some(Document {
        id: row.id,
        document_ref: row.document_ref,
        document_number: row.document_number,
        document_type_id: row.document_type_id,
        form_id: row.form_id,
        title: row.title,
        status: DocumentStatus::from_db(&row.status),
        priority: DocumentPriority::from_db(&row.priority),
        form_data: row.form_data_json,
        metadata: super::metadata::metadata_of(executor, tenant_id, id).await?,
        // `from_db` refuses a value this build does not know rather than
        // guessing, so an `entity_type` written by a future release reads back
        // as no link instead of as the wrong link. See `domain::link`.
        entity_type: row.entity_type.as_deref().and_then(EntityType::from_db),
        entity_id: row.entity_id,
        requested_by: row.requested_by,
        requested_for_department_id: row.requested_for_department_id,
        requested_for_facility_id: row.requested_for_facility_id,
        submitted_at: row.submitted_at,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// What the submit writes, in the transaction that took the number.
pub struct Submission<'a> {
    pub document_number: &'a str,
    pub form_data: &'a Value,
    pub submitted_at: DateTime<Utc>,
}

/// Moves a draft to `SUBMITTED`, assigning its number and storing the server's
/// payload.
///
/// **One statement, conditional on the document still being a draft.** Two
/// callers submitting the same document at once cannot both succeed, and the
/// loser gets `0` rather than a second number — which is [#168]'s AC5 as a
/// property of the statement rather than of the order the service happened to
/// do things in.
///
/// [#168]: https://github.com/sujanto-gaws/kelir/issues/168
pub async fn mark_submitted(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    submission: &Submission<'_>,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE documents SET
            document_number = $3,
            form_data_json  = $4,
            status          = 'SUBMITTED',
            submitted_at    = $5,
            requested_by    = COALESCE(requested_by, $6),
            updated_by      = $6,
            updated_at      = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = 'DRAFT'
        "#,
        tenant_id,
        id,
        submission.document_number,
        submission.form_data,
        submission.submitted_at,
        actor,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}
