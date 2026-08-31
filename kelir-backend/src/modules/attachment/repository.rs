//! The statements behind `attachments` (Database Schema §8.2; [#244]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{Attachment, VirusScanStatus};

/// What the insert is given. Everything here is the server's: the caller
/// supplies bytes, a name and a description, and every other value on this
/// struct is derived from them or from the request's identity.
pub struct NewAttachment<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub file_name: &'a str,
    pub original_file_name: &'a str,
    pub mime_type: &'a str,
    pub file_size: i64,
    pub checksum: &'a str,
    pub storage_reference: &'a str,
    pub description: Option<&'a str>,
    pub created_by: Option<Uuid>,
}

/// Records an attachment whose bytes are already stored.
///
/// **`virus_scan_status` is not a parameter**, and that is the point of writing
/// it as a literal: this item creates rows in exactly one scan state, and a
/// column the caller could set is a column an upload could set to `CLEAN`
/// ([#244] AC3). The scan moves it, in [#246](https://github.com/sujanto-gaws/kelir/issues/246).
///
/// `current_version_number` takes its default of 1 and nothing writes
/// `attachment_versions`; versioning is FR-ATT-007 and Sprint 13 stretch.
pub async fn insert_attachment<'e, E: PgExecutor<'e>>(
    executor: E,
    attachment: &NewAttachment<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO attachments
            (id, tenant_id, document_id, file_name, original_file_name, mime_type,
             file_size, checksum, storage_reference, description,
             virus_scan_status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'PENDING', $11)
        "#,
        attachment.id,
        attachment.tenant_id,
        attachment.document_id,
        attachment.file_name,
        attachment.original_file_name,
        attachment.mime_type,
        attachment.file_size,
        attachment.checksum,
        attachment.storage_reference,
        attachment.description,
        attachment.created_by,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Reads one attachment, scoped by tenant in the statement rather than by the
/// caller — the [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
/// [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson.
pub async fn find_attachment<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Attachment>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, document_id, original_file_name, mime_type, file_size, checksum,
               description, virus_scan_status, created_at, created_by
        FROM attachments
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Attachment {
        id: row.id,
        document_id: row.document_id,
        original_file_name: row.original_file_name,
        mime_type: row.mime_type,
        file_size: row.file_size,
        checksum: row.checksum,
        description: row.description,
        virus_scan_status: VirusScanStatus::from_db(&row.virus_scan_status),
        created_at: row.created_at,
        created_by: row.created_by,
    }))
}

/// What the download path needs, which is not what a list shows.
///
/// **`storage_reference` lives here and reaches no caller.** It is on this
/// struct because the handler has to fetch the object, and on no serialized type
/// for the reason [`super::domain::Attachment`] gives.
pub struct StoredFile {
    pub original_file_name: String,
    pub mime_type: String,
    pub storage_reference: String,
    pub virus_scan_status: VirusScanStatus,
}

/// Reads one attachment for serving, scoped by tenant **and by its document**.
///
/// **Both, and the document is the load-bearing half.** An attachment id alone
/// would let a caller who may read document A fetch an attachment hanging on
/// document B by guessing an id — [#245](https://github.com/sujanto-gaws/kelir/issues/245)
/// AC1's *download resolves through the document's own read permission*, held
/// in the statement rather than by the service remembering to compare.
pub async fn find_stored_file<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
) -> Result<Option<StoredFile>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT original_file_name, mime_type, storage_reference, virus_scan_status
        FROM attachments
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| StoredFile {
        original_file_name: row.original_file_name,
        mime_type: row.mime_type,
        storage_reference: row.storage_reference,
        virus_scan_status: VirusScanStatus::from_db(&row.virus_scan_status),
    }))
}

/// A document's attachments, newest first.
///
/// Newest first, unlike `comments`: this is a list of records rather than a
/// conversation, and the file somebody just added is the one they came for.
pub async fn list_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Attachment>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, document_id, original_file_name, mime_type, file_size, checksum,
               description, virus_scan_status, created_at, created_by
        FROM attachments
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        ORDER BY created_at DESC, id DESC
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        document_id,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Attachment {
            id: row.id,
            document_id: row.document_id,
            original_file_name: row.original_file_name,
            mime_type: row.mime_type,
            file_size: row.file_size,
            checksum: row.checksum,
            description: row.description,
            virus_scan_status: VirusScanStatus::from_db(&row.virus_scan_status),
            created_at: row.created_at,
            created_by: row.created_by,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
///
/// The same three clauses the page filters on and no join — written out for the
/// reason `workflow::repository::inbox` states and drifted from
/// ([#279](https://github.com/sujanto-gaws/kelir/issues/279)): a count over a
/// wider rule reports rows the page does not show.
pub async fn count_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM attachments
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}
