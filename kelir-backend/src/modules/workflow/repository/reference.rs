//! The instance and task reference counters (§7.10).
//!
//! `workflow_instances.instance_ref` (`WFI-2026-000123`) and
//! `workflow_tasks.task_ref` (`TASK-2026-000123`) are `NOT NULL` and unique per
//! tenant, and nothing produced either before this sprint.
//!
//! **The shape is `document_ref_sequences`', which is
//! `0020_numbering_buckets.sql`'.** One row per bucket, insert-or-advance in a
//! single statement, `RETURNING next_sequence - 1` so the caller never re-reads
//! to learn what it got, and **no read to race** — the unique index serialises
//! two callers in one bucket and callers in different buckets do not contend at
//! all. Copying a proven allocator beats writing a third one that is subtly
//! different, which is what [#200](https://github.com/sujanto-gaws/kelir/issues/200)
//! was.
//!
//! **One table serves both, keyed by the prefix and the year.** Two tables
//! differing only in what they count would be two places for the next fix to
//! land in one of. The prefix is part of the key, so an instance and a task
//! never contend and neither can hand out the other's number.
//!
//! # It allocates inside the caller's transaction, and that is the trade
//!
//! A rolled-back transition therefore leaves no hole *and* no reference on a
//! task that was never created. The cost is that two transitions in one
//! tenant-year serialise from the allocation to the commit — the same trade
//! `document_ref_sequences` takes, accepted for the same reason: a transition
//! transaction is short, and if it ever bites, gaps in an internal handle cost
//! nothing and the gap-tolerant shape is one function away.

use uuid::Uuid;

/// The prefix an allocation is filed under.
///
/// A closed set rather than a string parameter, because the key is what keeps
/// two counters apart and a typo in it would silently merge them — the failure
/// [#200](https://github.com/sujanto-gaws/kelir/issues/200) was, one table over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Instance,
    Task,
}

impl RefKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Instance => "WFI",
            Self::Task => "TASK",
        }
    }
}

/// Allocates the next reference of its kind for this tenant and year.
///
/// The year is a parameter rather than read from the clock inside the
/// statement, so that the reference a row is given and the reference a test
/// asserts come from the same moment.
pub async fn allocate(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    kind: RefKind,
    year: i32,
) -> Result<String, sqlx::Error> {
    let reference_key = format!("{}-{year}", kind.prefix());

    let sequence = sqlx::query_scalar!(
        r#"
        INSERT INTO workflow_ref_sequences (id, tenant_id, reference_key, next_sequence)
        VALUES ($1, $2, $3, 2)
        ON CONFLICT (tenant_id, reference_key) DO UPDATE
            SET next_sequence = workflow_ref_sequences.next_sequence + 1,
                updated_at    = now()
        RETURNING next_sequence - 1 AS "issued!"
        "#,
        Uuid::now_v7(),
        tenant_id,
        reference_key,
    )
    .fetch_one(&mut **transaction)
    .await?;

    Ok(format!("{}-{year}-{sequence:06}", kind.prefix()))
}
