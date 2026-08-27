//! Queries over `document_metadata` (Database Schema §6.9).

use uuid::Uuid;

use super::super::domain::{MetadataEntry, MetadataSet, MetadataType};

/// One document's metadata, keyed.
pub async fn metadata_of<'e, E: sqlx::PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<MetadataSet, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT metadata_key, metadata_value, data_type
        FROM document_metadata
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        ORDER BY metadata_key
        "#,
        tenant_id,
        document_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.metadata_key,
                MetadataEntry {
                    value: row.metadata_value,
                    data_type: MetadataType::from_db(&row.data_type),
                },
            )
        })
        .collect())
}

/// Replaces a document's metadata with the set it was sent.
///
/// **Hard-deletes the previous rows rather than soft-deleting them**, which is
/// the one place this module departs from the soft-delete convention and is
/// worth stating. A soft-deleted metadata row would collide with the row that
/// replaces it: `uq_document_metadata_document_id_metadata_key` is partial on
/// `deleted_at IS NULL`, so re-inserting the same key is legal — and the table
/// would then accumulate one dead row per key per edit, for values whose history
/// is `document_versions`' job (FR-DOC-008) rather than this table's.
///
/// The audit record is what makes the change recoverable, and it names the keys
/// that moved.
pub async fn replace_metadata(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    metadata: &MetadataSet,
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM document_metadata WHERE tenant_id = $1 AND document_id = $2",
        tenant_id,
        document_id
    )
    .execute(&mut **transaction)
    .await?;

    for (key, entry) in metadata {
        sqlx::query!(
            r#"
            INSERT INTO document_metadata
                (id, tenant_id, document_id, metadata_key, metadata_value, data_type,
                 created_by, updated_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
            "#,
            Uuid::now_v7(),
            tenant_id,
            document_id,
            key.trim(),
            entry.value,
            entry.data_type.as_db(),
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}
