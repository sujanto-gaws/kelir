//! The statements behind `mdm_change_requests` (Database Schema §4; [#255]).
//!
//! [#255]: https://github.com/sujanto-gaws/kelir/issues/255

use uuid::Uuid;

use super::super::domain::{ChangeAttempt, ChangeRequest, GovernedEntity, RecordStatus};

/// Records an open change, and **fails if one is already open for the record**.
///
/// The failure is the partial unique index rather than a check this function
/// makes: `uq_mdm_change_requests_open_per_record` is what makes *one open
/// change per record* true when two submissions arrive together, and the
/// service turns the violation into [`super::super::domain::already_in_flight`].
pub struct NewChangeRequest {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub entity: GovernedEntity,
    pub entity_id: Uuid,
    pub previous: RecordStatus,
    pub actor: Option<Uuid>,
}

pub async fn insert_change_request(
    transaction: &mut sqlx::PgTransaction<'_>,
    change: &NewChangeRequest,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_change_requests
            (id, tenant_id, document_id, entity_type, entity_id,
             previous_record_status, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        "#,
        change.id,
        change.tenant_id,
        change.document_id,
        change.entity.as_db(),
        change.entity_id,
        change.previous.as_db(),
        change.actor,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(())
}

/// The open change a document carries, **locked** for the transaction that is
/// settling it.
///
/// Locked because two paths can reach a settlement — the approval that closes
/// the process and, later, whatever cancels an instance — and the second must
/// find the row already resolved rather than resolve it again.
pub async fn lock_open_change_for_document(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<Option<ChangeRequest>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, document_id, entity_type, entity_id, previous_record_status
        FROM mdm_change_requests
        WHERE tenant_id = $1 AND document_id = $2 AND resolved_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    // A row whose `entity_type` this build cannot place is not a change this
    // build can settle — `GovernedEntity::from_db`'s reasoning, and the reason
    // this returns `None` rather than guessing an entity.
    Ok(row.and_then(|row| {
        Some(ChangeRequest {
            id: row.id,
            document_id: row.document_id,
            entity: GovernedEntity::from_db(&row.entity_type)?,
            entity_id: row.entity_id,
            previous_record_status: RecordStatus::from_db(&row.previous_record_status),
        })
    }))
}

/// Closes a change, one way or the other.
pub async fn resolve_change_request(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    id: Uuid,
    outcome: &str,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE mdm_change_requests
        SET outcome = $3, resolved_at = now(), updated_at = now(), updated_by = $4
        WHERE tenant_id = $1 AND id = $2 AND resolved_at IS NULL
        "#,
        tenant_id,
        id,
        outcome,
        actor,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(result.rows_affected())
}

/// Whether this record has a change awaiting approval right now.
///
/// Used by the direct-edit refusal, which asks the **record's own status**
/// first; this answers the second question a refusal wants to be able to
/// mention: *which* document is holding it.
pub async fn open_change_for_record(
    executor: impl sqlx::PgExecutor<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT document_id
        FROM mdm_change_requests
        WHERE tenant_id = $1 AND entity_type = $2 AND entity_id = $3 AND resolved_at IS NULL
        "#,
        tenant_id,
        entity.as_db(),
        entity_id
    )
    .fetch_optional(executor)
    .await
}

/// Moves a record's governance status **conditionally on where it is**, inside
/// the caller's transaction.
///
/// The condition is what makes this safe to call from a workflow's closing
/// transaction: a record somebody moved by hand in between does not silently
/// take the write, it takes none, and the caller sees zero rows.
pub async fn move_record_status_in(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
    from: RecordStatus,
    to: RecordStatus,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let affected = match entity {
        GovernedEntity::Party => sqlx::query!(
            r#"
                UPDATE mdm_parties
                SET record_status = $4, updated_at = now(), updated_by = $5
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND record_status = $3
                "#,
            tenant_id,
            entity_id,
            from.as_db(),
            to.as_db(),
            actor
        )
        .execute(&mut **transaction)
        .await?
        .rows_affected(),
        GovernedEntity::Facility => sqlx::query!(
            r#"
                UPDATE mdm_facilities
                SET record_status = $4, updated_at = now(), updated_by = $5
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL AND record_status = $3
                "#,
            tenant_id,
            entity_id,
            from.as_db(),
            to.as_db(),
            actor
        )
        .execute(&mut **transaction)
        .await?
        .rows_affected(),
    };

    Ok(affected)
}

/// Where a record is, read **under a lock**, inside the caller's transaction.
///
/// `FOR UPDATE` rather than the unlocked read `service::record_status` makes:
/// this one is followed by a write in the same transaction, and coding standard
/// §2.5 puts the lock on what the check read.
pub async fn lock_record_status(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
) -> Result<Option<RecordStatus>, sqlx::Error> {
    let value = match entity {
        GovernedEntity::Party => {
            sqlx::query_scalar!(
                r#"
                SELECT record_status
                FROM mdm_parties
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
                FOR UPDATE
                "#,
                tenant_id,
                entity_id
            )
            .fetch_optional(&mut **transaction)
            .await?
        }
        GovernedEntity::Facility => {
            sqlx::query_scalar!(
                r#"
                SELECT record_status
                FROM mdm_facilities
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
                FOR UPDATE
                "#,
                tenant_id,
                entity_id
            )
            .fetch_optional(&mut **transaction)
            .await?
        }
    };

    Ok(value.map(|status| RecordStatus::from_db(&status)))
}

/// Every change proposed for one record, newest first.
///
/// **Not paginated**, deliberately: a governed record has a handful of changes
/// over its life, one open at a time, and a page over a list that short would be
/// a parameter nobody sets. If a deployment ever proves otherwise, this is one
/// statement and a `Pagination`.
pub async fn changes_for_record(
    executor: impl sqlx::PgExecutor<'_>,
    tenant_id: Uuid,
    entity: GovernedEntity,
    entity_id: Uuid,
) -> Result<Vec<ChangeAttempt>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, document_id, outcome, previous_record_status,
               created_at, resolved_at, created_by
        FROM mdm_change_requests
        WHERE tenant_id = $1 AND entity_type = $2 AND entity_id = $3 AND deleted_at IS NULL
        ORDER BY created_at DESC, id DESC
        "#,
        tenant_id,
        entity.as_db(),
        entity_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ChangeAttempt {
            id: row.id,
            document_id: row.document_id,
            outcome: row.outcome,
            previous_record_status: RecordStatus::from_db(&row.previous_record_status),
            raised_at: row.created_at,
            resolved_at: row.resolved_at,
            raised_by: row.created_by,
        })
        .collect())
}
