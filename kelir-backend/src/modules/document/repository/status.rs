//! The status transition and its history row (Database Schema §6.6, §6.10;
//! FR-DOC-007, [#169]).
//!
//! [#169]: https://github.com/sujanto-gaws/kelir/issues/169

use uuid::Uuid;

use super::super::domain::DocumentStatus;

/// Moves a document from one status to another, **conditionally on it still
/// being in the first**.
///
/// One statement, compare-and-swap, `0` when it lost. This is [#169]'s AC3 and
/// it is written as a predicate rather than as a service-level ordering for the
/// reason [record 03] gave when it found the same guard untested in the facility
/// arm of the shared transition service (#139): a check-then-act that lives in a
/// service is a rule somebody can step around by writing a second caller, and a
/// `WHERE` clause is a rule the database enforces on every caller there will
/// ever be.
///
/// The status *this transition was checked against* is bound as `$3`, not the
/// status the row happens to hold now. That is the whole mechanism: two callers
/// who both read `SUBMITTED` and both decided a move is legal produce one update
/// of one row and one update of none.
///
/// [#169]: https://github.com/sujanto-gaws/kelir/issues/169
/// [record 03]: ../../../../../projects/verifications/03.%20Sprint%206%20Surface%20Verification.md
pub async fn move_status(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    from: DocumentStatus,
    to: DocumentStatus,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = sqlx::query!(
        r#"
        UPDATE documents SET
            status       = $4,
            -- Which status means "finished" is decided in `DocumentStatus` and
            -- passed in, rather than spelled again as a literal here. Two
            -- copies of a lifecycle are two lifecycles, and this one would be
            -- the copy nobody looks at.
            completed_at = CASE WHEN $5 THEN now() ELSE completed_at END,
            updated_by   = $6,
            updated_at   = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND status = $3
        "#,
        tenant_id,
        id,
        from.as_db(),
        to.as_db(),
        to == DocumentStatus::Completed,
        actor,
    )
    .execute(&mut **transaction)
    .await?
    .rows_affected();

    Ok(affected)
}

/// Appends one row to `document_status_history`.
///
/// Append-only: the table has no `updated_at` and no `deleted_at` (§1.2), so a
/// history row is a fact rather than a record. `changed_by` is nullable because
/// Phase 5's engine will move documents with no person behind the move, and
/// §6.10 says so in the column comment.
///
/// **Written in the transaction that moved the status**, so a document cannot
/// end up in a state its own history does not explain — which is the pair of
/// writes #168 calls unrecoverable when they are not one transaction.
pub async fn record_transition(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    from: Option<DocumentStatus>,
    to: DocumentStatus,
    actor: Option<Uuid>,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO document_status_history
            (id, tenant_id, document_id, old_status, new_status, changed_by, reason)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        Uuid::now_v7(),
        tenant_id,
        document_id,
        from.map(DocumentStatus::as_db),
        to.as_db(),
        actor,
        reason,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}
