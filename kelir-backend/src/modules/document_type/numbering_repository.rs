//! Queries for `document_type_numbering_rules` (§6.3).
//!
//! **`lock_active_rule` is the one that matters**, and its `FOR UPDATE` is the
//! whole of coding standard §2.5 applied to this surface. Everything else here
//! is ordinary.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::numbering::{GapPolicy, NumberingRule, SequenceScope};

/// The rule's mutable state, read under the lock.
pub struct LockedRule {
    pub id: Uuid,
    pub rule_template: String,
    pub sequence_scope: SequenceScope,
    pub sequence_padding: i32,
    pub gap_policy: GapPolicy,
    pub sequence_key: String,
    pub next_sequence: i64,
}

/// Reads the active rule for a type **and holds it** for the rest of the
/// transaction.
///
/// `FOR UPDATE` on the rule row, and the lock is chosen by what the check reads
/// rather than by what the write touches (coding standard §2.5). The read is
/// this row's `next_sequence`; the write is this row's `next_sequence`; so the
/// row is the right granularity, and a coarser lock would serialise types that
/// share nothing.
///
/// **The window this closes is the entire defect.** Without it, two submissions
/// read `next_sequence = 41` a microsecond apart, both render `…-000041`, and
/// both write `42`. One of the two documents then carries a number another
/// document already has, and `uq_documents_tenant_id_document_number` refuses
/// whichever commits second — at submit time, after the work is done. That is
/// #105's shape, and #133 and #137 are the same shape in another module.
pub async fn lock_active_rule(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Option<LockedRule>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, rule_template, sequence_scope, sequence_padding,
               allow_gaps, sequence_key, next_sequence
        FROM document_type_numbering_rules
        WHERE tenant_id = $1 AND document_type_id = $2
          AND is_active AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| LockedRule {
        id: row.id,
        rule_template: row.rule_template,
        sequence_scope: SequenceScope::from_db(&row.sequence_scope),
        sequence_padding: row.sequence_padding,
        gap_policy: GapPolicy::from_db(row.allow_gaps),
        sequence_key: row.sequence_key,
        next_sequence: row.next_sequence,
    }))
}

/// Advances the counter, and moves the bucket if the scope rolled over.
///
/// Called only with the row already locked by [`lock_active_rule`] in the same
/// transaction — which is why it needs no predicate of its own beyond the id.
pub async fn advance(
    transaction: &mut sqlx::PgTransaction<'_>,
    rule_id: Uuid,
    sequence_key: &str,
    next_sequence: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE document_type_numbering_rules
        SET sequence_key = $2, next_sequence = $3, updated_at = now()
        WHERE id = $1
        "#,
        rule_id,
        sequence_key,
        next_sequence,
    )
    .execute(&mut **transaction)
    .await
    .map(|_| ())
}

pub async fn find_rule<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Option<NumberingRule>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, document_type_id, rule_template, sequence_scope, sequence_padding,
               allow_gaps, sequence_key, next_sequence, is_active
        FROM document_type_numbering_rules
        WHERE tenant_id = $1 AND document_type_id = $2
          AND is_active AND deleted_at IS NULL
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| NumberingRule {
        id: row.id,
        document_type_id: row.document_type_id,
        rule_template: row.rule_template,
        sequence_scope: SequenceScope::from_db(&row.sequence_scope),
        sequence_padding: row.sequence_padding,
        gap_policy: GapPolicy::from_db(row.allow_gaps),
        sequence_key: row.sequence_key,
        next_sequence: row.next_sequence,
        is_active: row.is_active,
    }))
}

/// Replaces a type's active rule.
///
/// Deactivates the previous one rather than deleting it, and inserts the new
/// one, in one transaction. The partial unique index allows a single *active*
/// rule per type, so the deactivation has to land first — and keeping the old
/// row is what lets `documents.document_number` still be explained years later
/// by the rule that produced it.
pub async fn replace_rule(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    rule: &NewRule<'_>,
    actor: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE document_type_numbering_rules
        SET is_active = false, updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND document_type_id = $2 AND is_active AND deleted_at IS NULL
        "#,
        tenant_id,
        document_type_id,
        actor,
    )
    .execute(&mut **transaction)
    .await?;

    let id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO document_type_numbering_rules
            (id, tenant_id, document_type_id, rule_template, sequence_scope,
             sequence_padding, allow_gaps, sequence_key, next_sequence, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        id,
        tenant_id,
        document_type_id,
        rule.rule_template,
        rule.sequence_scope,
        rule.sequence_padding,
        rule.allow_gaps,
        rule.sequence_key,
        rule.next_sequence,
        actor,
    )
    .execute(&mut **transaction)
    .await?;

    Ok(id)
}

pub struct NewRule<'a> {
    pub rule_template: &'a str,
    pub sequence_scope: &'a str,
    pub sequence_padding: i32,
    pub allow_gaps: bool,
    pub sequence_key: &'a str,
    pub next_sequence: i64,
}

/// The highest `next_sequence` any rule for this type has reached in the bucket
/// a new rule would start in.
///
/// Read before a rule is replaced, so that a caller cannot rewind the counter
/// past a number already issued. It looks at *every* rule for the type,
/// including deactivated ones: replacing a rule does not un-issue the numbers
/// the old one produced.
pub async fn highest_issued<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
    sequence_key: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT max(next_sequence)
        FROM document_type_numbering_rules
        WHERE tenant_id = $1 AND document_type_id = $2 AND sequence_key = $3
        "#,
        tenant_id,
        document_type_id,
        sequence_key
    )
    .fetch_one(executor)
    .await
}

/// Deactivates a type's rule without replacing it.
pub async fn deactivate(
    pool: &PgPool,
    tenant_id: Uuid,
    document_type_id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE document_type_numbering_rules
        SET is_active = false, updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND document_type_id = $2 AND is_active AND deleted_at IS NULL
        "#,
        tenant_id,
        document_type_id,
        actor,
    )
    .execute(pool)
    .await
    .map(|result| result.rows_affected())
}
