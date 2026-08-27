//! The internal reference counter (`document_ref_sequences`, `0023_document.sql`).
//!
//! `documents.document_ref` is the handle a draft has for the whole of its life
//! before it has a number, and Database Schema §6.6 documents its shape as
//! `DOC-2026-000123` — tenant-wide, year-scoped, sequential. It is **not**
//! `document_number`: the number is the business identifier the type's rule
//! renders and a submit assigns, and a document has one of those only after it
//! has been committed to.
//!
//! # The allocator is `allocate_bucket`'s, copied on purpose
//!
//! [`numbering_repository::allocate_bucket`][ab] solved this exact problem for
//! document numbers and its reasoning transfers unchanged: there is no read to
//! race, the number returned is the number stored minus one so the caller never
//! re-reads to learn what it got, and two callers in one bucket are serialised
//! by the unique index rather than by a lock somebody remembered to take.
//! Writing a second allocator that was subtly different is what #200 was.
//!
//! What differs is the **bucket**, and it is the whole reason this is a separate
//! table rather than a reuse: a document number's bucket is scoped to the
//! document *type*, and a reference's is scoped to the tenant. §6.6 makes the
//! ref unique per tenant, so two types creating documents on the same day take
//! successive refs rather than the same one.
//!
//! [ab]: crate::modules::document_type::numbering_repository::allocate_bucket

use chrono::{Datelike, Utc};
use uuid::Uuid;

/// How wide the sequence renders. Six digits, matching §6.6's example and
/// `document_type_numbering_rules.sequence_padding`'s default — one shape for
/// both counters, so a reader is not asked to learn two.
const REFERENCE_PADDING: usize = 6;

/// The bucket a reference falls in.
///
/// A pure function with its own test rather than an expression inside the
/// allocator, for [`scope_key`][sk]'s reason: a bucket computed one way on write
/// and another way on read is a sequence that restarts at the wrong moment.
///
/// [sk]: crate::modules::document_type::numbering::scope_key
pub fn reference_key(at: chrono::DateTime<Utc>) -> String {
    at.year().to_string()
}

/// Renders a reference from a bucket and a sequence.
pub fn render(reference_key: &str, sequence: i64) -> String {
    format!("DOC-{reference_key}-{sequence:0>REFERENCE_PADDING$}")
}

/// Takes the next reference number in one bucket, creating the bucket if this is
/// its first.
///
/// **The transaction is the caller's**, and that is what makes the sequence
/// gapless: a create that fails after this point rolls the counter back with it,
/// and the bucket row stays locked until the caller commits. The trade is that
/// creations within one tenant-year serialise from here to the commit — a
/// creation transaction is short, and if it ever bites, gaps in an internal
/// handle cost nothing and `allocate_committed`'s shape is one function away.
/// The [Sprint 9 construction plan](../../../../../projects/planning/04.%20Sprint%209%20Document%20Construction%20Plan.md)
/// §3 records it as the one question it leaves owed, so a measurement can move
/// it.
pub async fn allocate_reference(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    reference_key: &str,
) -> Result<String, sqlx::Error> {
    let allocated = sqlx::query_scalar!(
        r#"
        INSERT INTO document_ref_sequences (id, tenant_id, reference_key, next_sequence)
        VALUES ($1, $2, $3, 2)
        ON CONFLICT (tenant_id, reference_key) DO UPDATE
            SET next_sequence = document_ref_sequences.next_sequence + 1,
                updated_at = now()
        RETURNING next_sequence - 1 AS "allocated!"
        "#,
        Uuid::now_v7(),
        tenant_id,
        reference_key,
    )
    .fetch_one(&mut **transaction)
    .await?;

    Ok(render(reference_key, allocated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn a_reference_renders_the_shape_the_schema_documents() {
        assert_eq!(render("2026", 123), "DOC-2026-000123");
    }

    #[test]
    fn a_sequence_wider_than_the_padding_is_not_truncated() {
        // `{:0>6}` pads and never cuts. A tenant that issues more than a million
        // documents in a year gets a longer reference rather than a colliding
        // one — and `document_ref` is VARCHAR(64), so there is room.
        assert_eq!(render("2026", 1_234_567), "DOC-2026-1234567");
    }

    #[test]
    fn the_bucket_is_the_year_the_reference_is_issued_in() {
        let at = Utc
            .with_ymd_and_hms(2026, 12, 31, 23, 59, 59)
            .single()
            .expect("a timestamp");

        assert_eq!(reference_key(at), "2026");
    }
}
