//! Reading and moving `record_status` (§4.1, §4.16).
//!
//! **Two statements, one per table, rather than one with the table name
//! interpolated.** `mdm_parties` and `mdm_facilities` are identifiers, and an
//! identifier that reaches SQL resolves through an allow-list or not at all
//! (coding standard §2.5) — here the allow-list is the `match`, and each arm is
//! a `query!` the compiler checks against the real column. A `format!` over a
//! table name would compile to a query nothing verifies.

use uuid::Uuid;

use sqlx::PgExecutor;

use crate::modules::master_data::domain::{RecordStatus, TransitionTarget};

/// Where a record has got to, or `None` if there is no such live record.
pub async fn find_record_status(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    target: TransitionTarget,
    id: Uuid,
) -> Result<Option<RecordStatus>, sqlx::Error> {
    let value = match target {
        TransitionTarget::Party => {
            sqlx::query_scalar!(
                r#"
                SELECT record_status FROM mdm_parties
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                id
            )
            .fetch_optional(executor)
            .await?
        }
        TransitionTarget::Facility => {
            sqlx::query_scalar!(
                r#"
                SELECT record_status FROM mdm_facilities
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                id
            )
            .fetch_optional(executor)
            .await?
        }
    };

    Ok(value.as_deref().map(RecordStatus::from_db))
}

/// Writes the new status, but only if the record is still in `from`.
///
/// The `record_status = $3` predicate is the whole point. Reading the status
/// and then writing it back unconditionally is check-then-act across two
/// statements: two callers who both read `ACTIVE` would both be allowed to
/// move, and the second would overwrite the first from a state that no longer
/// held — the failure #105 was about, one column over. Here the second write
/// matches nothing and the service answers 409.
///
/// Returns the rows affected, so zero means *someone got there first* rather
/// than *no such record* — the caller has already established the record
/// exists.
pub async fn move_record_status(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    target: TransitionTarget,
    id: Uuid,
    from: RecordStatus,
    to: RecordStatus,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = match target {
        TransitionTarget::Party => {
            sqlx::query!(
                r#"
                UPDATE mdm_parties
                SET record_status = $4, updated_by = $5, updated_at = now()
                WHERE tenant_id = $1 AND id = $2 AND record_status = $3
                  AND deleted_at IS NULL
                "#,
                tenant_id,
                id,
                from.as_db(),
                to.as_db(),
                updated_by
            )
            .execute(executor)
            .await?
        }
        TransitionTarget::Facility => {
            sqlx::query!(
                r#"
                UPDATE mdm_facilities
                SET record_status = $4, updated_by = $5, updated_at = now()
                WHERE tenant_id = $1 AND id = $2 AND record_status = $3
                  AND deleted_at IS NULL
                "#,
                tenant_id,
                id,
                from.as_db(),
                to.as_db(),
                updated_by
            )
            .execute(executor)
            .await?
        }
    };

    Ok(result.rows_affected())
}
