//! The statements behind `attachments`, `attachment_categories` and
//! `document_external_references` (Database Schema §8; [#244], [#254]).
//!
//! [#244]: https://github.com/sujanto-gaws/kelir/issues/244
//! [#254]: https://github.com/sujanto-gaws/kelir/issues/254

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{Attachment, AttachmentCategory, ExternalReference, VirusScanStatus};

/// The joined category, or `None` — **decided by the id and by nothing else**.
///
/// A `LEFT JOIN` gives four nullable columns that are either all present or all
/// absent, and reading them independently would let a half-built category
/// through if one were ever missing. The id is the one that says whether there
/// is a row.
fn category_from(
    id: Option<Uuid>,
    code: Option<String>,
    name: Option<String>,
    is_system: Option<bool>,
) -> Option<AttachmentCategory> {
    Some(AttachmentCategory {
        id: id?,
        code: code?,
        name: name?,
        is_system: is_system?,
    })
}

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
    pub category_id: Option<Uuid>,
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
             file_size, checksum, storage_reference, description, category_id,
             virus_scan_status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'PENDING', $12)
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
        attachment.category_id,
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
        SELECT a.id, a.document_id, a.original_file_name, a.mime_type, a.file_size, a.checksum,
               a.description, a.virus_scan_status, a.created_at, a.created_by,
               c.id AS "category_id?", c.category_code AS "category_code?",
               c.name AS "category_name?", c.is_system AS "category_is_system?"
        FROM attachments a
        LEFT JOIN attachment_categories c
               ON c.id = a.category_id AND c.tenant_id = a.tenant_id
        WHERE a.tenant_id = $1 AND a.id = $2 AND a.deleted_at IS NULL
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
        category: category_from(
            row.category_id,
            row.category_code,
            row.category_name,
            row.category_is_system,
        ),
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
        SELECT a.id, a.document_id, a.original_file_name, a.mime_type, a.file_size, a.checksum,
               a.description, a.virus_scan_status, a.created_at, a.created_by,
               c.id AS "category_id?", c.category_code AS "category_code?",
               c.name AS "category_name?", c.is_system AS "category_is_system?"
        FROM attachments a
        LEFT JOIN attachment_categories c
               ON c.id = a.category_id AND c.tenant_id = a.tenant_id
        WHERE a.tenant_id = $1 AND a.document_id = $2 AND a.deleted_at IS NULL
        ORDER BY a.created_at DESC, a.id DESC
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
            category: category_from(
                row.category_id,
                row.category_code,
                row.category_name,
                row.category_is_system,
            ),
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

/// An attachment nobody has scanned yet.
pub struct PendingScan {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub storage_reference: String,
}

/// The oldest attachments still waiting for a scanner.
///
/// # It reads across every tenant, and that is deliberate ([#294] AC2)
///
/// **There is no `tenant_id` in this predicate because there is no caller to
/// scope it to.** This is a worker: it holds no session, acts for nobody, and
/// scans what a deployment has stored. A tenant filter here would mean either a
/// worker per tenant or a list of tenants somebody has to maintain, and the
/// files of whichever tenant was left off would sit `PENDING` for ever —
/// which reads to a person as an upload that never became downloadable.
///
/// The rows it returns carry their own `tenant_id`, and
/// [`record_scan_result`] takes it back as a predicate, so the write is scoped
/// even though the read is not.
///
/// **Oldest first, and no claim is taken.** Two workers reading the same batch
/// scan the same file twice and reach the same answer, which costs a scan and
/// changes nothing — where a claim would need either a fifth `virus_scan_status`
/// the `CHECK` does not permit, or a database transaction held open across a
/// network call to another service. [`record_scan_result`]'s predicate is what
/// makes the duplicate harmless, and it is the same predicate that makes the
/// transition happen exactly once.
pub async fn pending_scans<'e, E: PgExecutor<'e>>(
    executor: E,
    limit: i64,
) -> Result<Vec<PendingScan>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, tenant_id, storage_reference
        FROM attachments
        WHERE virus_scan_status = 'PENDING' AND deleted_at IS NULL
        ORDER BY created_at
        LIMIT $1
        "#,
        limit
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PendingScan {
            id: row.id,
            tenant_id: row.tenant_id,
            storage_reference: row.storage_reference,
        })
        .collect())
}

/// Writes a scan result, **and only over `PENDING`**.
///
/// The predicate is [#246](https://github.com/sujanto-gaws/kelir/issues/246)
/// AC5, both halves of it, held in the statement rather than in the worker:
///
/// * *A scan result moves the row exactly once* — a second writer, whether a
///   duplicate scan or a retry, matches no row and changes nothing.
/// * *It cannot move back out of `INFECTED`* — nor out of `CLEAN` or `FAILED`,
///   because `PENDING` is the only status this statement will write over. There
///   is no route in this product from a decided scan to any other value.
///
/// Returns the number of rows moved, which the caller logs: zero is not an error
/// and is worth seeing, because it means somebody else answered first.
///
/// # The tenant is in the predicate, and it should never be what refuses
/// ([#294] AC1, AC3)
///
/// It was not, until #294: this was the one write in the Sprint 12 surface
/// scoped by `id` alone. Nothing was wrong — every id came from
/// [`pending_scans`] one line above, which is right to read across tenants —
/// and that is exactly the shape [#106](https://github.com/sujanto-gaws/kelir/issues/106)
/// and [#121](https://github.com/sujanto-gaws/kelir/issues/121) cost this
/// project three sprints of findings over: **a statement whose scope depends on
/// its caller having chosen correctly**.
///
/// The tenant comes from the row `pending_scans` returned, so this predicate can
/// only fail if a scan result is being written against a row from a tenant the
/// worker did not read it from — which is a bug, and now a silent no-op with a
/// logged zero rather than a write. **No behaviour changes**, which is AC3, and
/// the test that reaches it proves the scan still lands rather than proving the
/// predicate fires.
///
/// [#294]: https://github.com/sujanto-gaws/kelir/issues/294
pub async fn record_scan_result<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    status: &str,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE attachments
        SET virus_scan_status = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2
          AND virus_scan_status = 'PENDING' AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        status
    )
    .execute(executor)
    .await?
    .rows_affected();

    Ok(affected)
}

/// What a delete needs to know about an attachment, **locked for the
/// transaction** ([#254] AC2).
///
/// Scoped by document as well as by tenant, for `comment::repository::lock_comment`'s
/// stated reason: the surface is `/documents/{id}/attachments/{attachmentId}`,
/// and an attachment reached through the wrong document is not one this caller
/// asked for.
pub struct AttachmentRow {
    pub id: Uuid,
    pub created_by: Option<Uuid>,
    pub original_file_name: String,
    pub file_size: i64,
    pub checksum: String,
}

pub async fn lock_attachment(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
) -> Result<Option<AttachmentRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, created_by, original_file_name, file_size, checksum
        FROM attachments
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| AttachmentRow {
        id: row.id,
        created_by: row.created_by,
        original_file_name: row.original_file_name,
        file_size: row.file_size,
        checksum: row.checksum,
    }))
}

/// Marks an attachment deleted. **The stored object is not touched** (**D-52**).
///
/// The row keeps its `storage_reference`, its checksum and its name, which is
/// what makes the delete recoverable and what a retention sweep will read when
/// something writes one. `attachments.retention_policy_id` is where that lives;
/// nothing writes it yet, and this comment is that fact rather than an omission.
pub async fn soft_delete_attachment<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE attachments
        SET deleted_at = now(), updated_at = now(), updated_by = $4
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id,
        actor
    )
    .execute(executor)
    .await?;

    Ok(result.rows_affected())
}

/// Every category this tenant may file something under (FR-ATT-006).
///
/// **System rows first, then the tenant's own, each alphabetically.** A picker
/// whose order changes with the data is a picker people misclick.
pub async fn list_categories<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
) -> Result<Vec<AttachmentCategory>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, category_code, name, is_system
        FROM attachment_categories
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY is_system DESC, name
        "#,
        tenant_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AttachmentCategory {
            id: row.id,
            code: row.category_code,
            name: row.name,
            is_system: row.is_system,
        })
        .collect())
}

/// Whether this tenant has that category — the check behind
/// [`super::domain::category_not_found`].
pub async fn category_exists<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "one!"
        FROM attachment_categories
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(found.is_some())
}

/// What the insert of an external reference is given (FR-ATT-010).
pub struct NewReference<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub label: &'a str,
    pub url: &'a str,
    pub description: Option<&'a str>,
    pub category_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

/// Records a link. **No bytes, no scan, no storage** — see the table's comment.
pub async fn insert_reference<'e, E: PgExecutor<'e>>(
    executor: E,
    reference: &NewReference<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO document_external_references
            (id, tenant_id, document_id, label, url, description, category_id, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        reference.id,
        reference.tenant_id,
        reference.document_id,
        reference.label,
        reference.url,
        reference.description,
        reference.category_id,
        reference.created_by,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// One reference, scoped by tenant in the statement.
pub async fn find_reference<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<ExternalReference>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT r.id, r.document_id, r.label, r.url, r.description, r.created_at, r.created_by,
               c.id AS "category_id?", c.category_code AS "category_code?",
               c.name AS "category_name?", c.is_system AS "category_is_system?"
        FROM document_external_references r
        LEFT JOIN attachment_categories c
               ON c.id = r.category_id AND c.tenant_id = r.tenant_id
        WHERE r.tenant_id = $1 AND r.id = $2 AND r.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| ExternalReference {
        id: row.id,
        document_id: row.document_id,
        label: row.label,
        url: row.url,
        description: row.description,
        category: category_from(
            row.category_id,
            row.category_code,
            row.category_name,
            row.category_is_system,
        ),
        created_at: row.created_at,
        created_by: row.created_by,
    }))
}

/// A document's external references, newest first — the same order its
/// attachments use, because both are lists of records rather than a
/// conversation.
pub async fn list_references_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<ExternalReference>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT r.id, r.document_id, r.label, r.url, r.description, r.created_at, r.created_by,
               c.id AS "category_id?", c.category_code AS "category_code?",
               c.name AS "category_name?", c.is_system AS "category_is_system?"
        FROM document_external_references r
        LEFT JOIN attachment_categories c
               ON c.id = r.category_id AND c.tenant_id = r.tenant_id
        WHERE r.tenant_id = $1 AND r.document_id = $2 AND r.deleted_at IS NULL
        ORDER BY r.created_at DESC, r.id DESC
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
        .map(|row| ExternalReference {
            id: row.id,
            document_id: row.document_id,
            label: row.label,
            url: row.url,
            description: row.description,
            category: category_from(
                row.category_id,
                row.category_code,
                row.category_name,
                row.category_is_system,
            ),
            created_at: row.created_at,
            created_by: row.created_by,
        })
        .collect())
}

/// How many the page is drawn from, under the same predicate — the duplication
/// `count_for_document` above explains, for the same reason.
pub async fn count_references_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM document_external_references
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}

/// The reference a delete is about, locked — [`lock_attachment`]'s shape.
pub struct ReferenceRow {
    pub id: Uuid,
    pub created_by: Option<Uuid>,
    pub label: String,
}

pub async fn lock_reference(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
) -> Result<Option<ReferenceRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, created_by, label
        FROM document_external_references
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| ReferenceRow {
        id: row.id,
        created_by: row.created_by,
        label: row.label,
    }))
}

/// Marks a reference deleted. Soft, like everything else in this schema — and
/// here it costs nothing to argue about, because there are no bytes behind it.
pub async fn soft_delete_reference<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE document_external_references
        SET deleted_at = now(), updated_at = now(), updated_by = $4
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id,
        actor
    )
    .execute(executor)
    .await?;

    Ok(result.rows_affected())
}
