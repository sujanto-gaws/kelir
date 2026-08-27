//! Queries for `document_type_numbering_rules` (§6.3) and the counters beside
//! it in `document_type_sequence_buckets`.
//!
//! **[`allocate_bucket`] is the one that matters.** It is a single statement,
//! and that is the whole of coding standard §2.5 applied to this surface — not
//! because a lock was avoided, but because there is no read to race: the row is
//! inserted or advanced by the same statement that returns the number.
//!
//! **The counters moved out of the rule row in `0020_numbering_buckets.sql`**
//! ([#200](https://github.com/sujanto-gaws/kelir/issues/200), decision
//! **D-21**). A rule row describes the *format* of a number; a bucket holds the
//! *count* for one scope value. The two were one row, which meant a
//! `DEPARTMENT_YEAR` rule — whose scope values are live simultaneously rather
//! than in succession — reset its only counter on every allocation that changed
//! department, and issued `000001` forever.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::numbering::{GapPolicy, NumberingRule, SequenceScope};

/// A rule's format and policy. No counter: see the module comment.
pub struct ActiveRule {
    pub id: Uuid,
    pub rule_template: String,
    pub sequence_scope: SequenceScope,
    pub sequence_padding: i32,
    pub gap_policy: GapPolicy,
}

/// Reads the active rule for a type.
///
/// **No `FOR UPDATE`, and its absence is the point.** The rule row used to be
/// locked because it held the counter the next statement was about to advance.
/// It no longer holds one, so locking it would serialise every allocation of a
/// type behind a row nothing writes — precisely the coarse lock
/// `lock_active_rule`'s own comment argued against when it chose the row over
/// the table. The contended resource is the bucket, and [`allocate_bucket`]
/// contends for exactly that one.
pub async fn find_active_rule<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Option<ActiveRule>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, rule_template, sequence_scope, sequence_padding, allow_gaps
        FROM document_type_numbering_rules
        WHERE tenant_id = $1 AND document_type_id = $2
          AND is_active AND deleted_at IS NULL
        "#,
        tenant_id,
        document_type_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| ActiveRule {
        id: row.id,
        rule_template: row.rule_template,
        sequence_scope: SequenceScope::from_db(&row.sequence_scope),
        sequence_padding: row.sequence_padding,
        gap_policy: GapPolicy::from_db(row.allow_gaps),
    }))
}

/// Takes the next number in one bucket, creating the bucket if this is its
/// first.
///
/// One statement, and the reasoning is worth keeping because the shape it
/// replaces is the one this project has produced four times (#105, #133, #137,
/// #200):
///
/// * **There is no read to race.** `ON CONFLICT … DO UPDATE` is atomic against
///   `uq_document_type_sequence_buckets_type_key`, so two callers reaching it
///   together are serialised by the index rather than by a lock somebody
///   remembered to take. The second waits on the first's row lock, which
///   PostgreSQL takes on its behalf.
/// * **The number returned is the number stored minus one**, so the caller
///   never re-reads to find out what it got. A `RETURNING` that handed back
///   `next_sequence` would name the number the *following* document takes.
/// * **Two scope values never contend.** Different keys are different rows, so
///   two departments submitting at once block on nothing. Under the old shape
///   they shared the rule row and, worse, overwrote each other's counter.
///
/// The transaction is the caller's, which is what makes a
/// [`GapPolicy::Gapless`] rule gapless: a rollback rolls the advanced counter
/// back with it, and the bucket row stays locked until the caller commits.
pub async fn allocate_bucket(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    sequence_key: &str,
) -> Result<i64, sqlx::Error> {
    let allocated = sqlx::query_scalar!(
        r#"
        INSERT INTO document_type_sequence_buckets
            (id, tenant_id, document_type_id, sequence_key, next_sequence)
        VALUES ($1, $2, $3, $4, 2)
        ON CONFLICT (document_type_id, sequence_key) DO UPDATE
            SET next_sequence = document_type_sequence_buckets.next_sequence + 1,
                updated_at = now()
        RETURNING next_sequence - 1 AS "allocated!"
        "#,
        Uuid::now_v7(),
        tenant_id,
        document_type_id,
        sequence_key,
    )
    .fetch_one(&mut **transaction)
    .await?;

    Ok(allocated)
}

/// Seeds a bucket's counter, for a rule configured to start somewhere other
/// than 1.
///
/// The path `nextSequence` takes when a deployment migrating from another
/// system continues its existing numbering. It writes the bucket the clock is
/// in now; a bucket the sequence reaches later starts at 1, which is what
/// "restarts each year" means.
///
/// `DO UPDATE` rather than `DO NOTHING`: re-configuring a rule with a *higher*
/// `nextSequence` is a legitimate correction, and [`super::numbering::validate_set`]
/// has already refused a lower one against [`highest_issued`].
pub async fn seed_bucket(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_type_id: Uuid,
    sequence_key: &str,
    next_sequence: i64,
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO document_type_sequence_buckets
            (id, tenant_id, document_type_id, sequence_key, next_sequence, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (document_type_id, sequence_key) DO UPDATE
            SET next_sequence = EXCLUDED.next_sequence,
                updated_by = EXCLUDED.created_by,
                updated_at = now()
        "#,
        Uuid::now_v7(),
        tenant_id,
        document_type_id,
        sequence_key,
        next_sequence,
        actor,
    )
    .execute(&mut **transaction)
    .await
    .map(|_| ())
}

/// The furthest bucket this type's sequence has reached within one comparison
/// group.
///
/// **`prefix` is what makes the comparison correct**, and its absence is what
/// #200 was. Bucket keys sort chronologically, so `candidate < furthest` reads
/// "this allocation reaches into a bucket the sequence has already passed" —
/// but only among keys that are *in succession*. Two departments are not: they
/// are parallel sequences, and neither has passed the other. Passing the
/// department's own prefix restricts the group to keys that genuinely succeed
/// one another, so one comparison serves every scope.
///
/// A UUID contains no `%` or `_`, so the department prefix carries no `LIKE`
/// metacharacter. An empty prefix matches every key, which is right for the
/// scopes whose buckets are all in one succession.
pub async fn furthest_key<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
    prefix: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT max(sequence_key)
        FROM document_type_sequence_buckets
        WHERE tenant_id = $1 AND document_type_id = $2
          AND sequence_key LIKE $3 || '%'
        "#,
        tenant_id,
        document_type_id,
        prefix
    )
    .fetch_one(executor)
    .await
}

/// A type's active rule, reported with the counter of its furthest bucket.
///
/// The API's `sequenceKey` and `nextSequence` are what they always were — the
/// bucket the sequence is in and the number the next document takes — so the
/// published shape is unchanged by `0020`. What changed is where they are read
/// from, and that a type may now have more of them than the response shows.
pub async fn find_rule<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
) -> Result<Option<NumberingRule>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT rule.id, rule.document_type_id, rule.rule_template, rule.sequence_scope,
               rule.sequence_padding, rule.allow_gaps, rule.is_active,
               bucket.sequence_key AS "sequence_key?",
               bucket.next_sequence AS "next_sequence?"
        FROM document_type_numbering_rules rule
        LEFT JOIN LATERAL (
            SELECT b.sequence_key, b.next_sequence
            FROM document_type_sequence_buckets b
            WHERE b.tenant_id = rule.tenant_id
              AND b.document_type_id = rule.document_type_id
            ORDER BY b.sequence_key DESC
            LIMIT 1
        ) bucket ON TRUE
        WHERE rule.tenant_id = $1 AND rule.document_type_id = $2
          AND rule.is_active AND rule.deleted_at IS NULL
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
        // A rule configured and never allocated from has no bucket yet. Its
        // sequence has not started, so it reports the unstarted state rather
        // than inventing the bucket the clock happens to be in.
        sequence_key: row.sequence_key.unwrap_or_default(),
        next_sequence: row.next_sequence.unwrap_or(1),
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
///
/// **The buckets are untouched**, which is the behaviour `highest_issued` used
/// to have to reconstruct: a counter belongs to the document type, so
/// correcting a template does not restart a sequence.
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
             sequence_padding, allow_gaps, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        id,
        tenant_id,
        document_type_id,
        rule.rule_template,
        rule.sequence_scope,
        rule.sequence_padding,
        rule.allow_gaps,
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
}

/// The number this type's sequence has already reached in one bucket.
///
/// Read before a rule is replaced, so that a caller cannot rewind the counter
/// past a number already issued. It reads the bucket rather than every rule
/// row: buckets survive a rule replacement, so "including deactivated rules" —
/// which is what this used to have to say — is no longer a thing it has to
/// arrange.
pub async fn highest_issued<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_type_id: Uuid,
    sequence_key: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT max(next_sequence)
        FROM document_type_sequence_buckets
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
///
/// The buckets stay. A type whose numbering is cleared and later re-configured
/// continues where it left off, because the numbers it issued were issued.
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
