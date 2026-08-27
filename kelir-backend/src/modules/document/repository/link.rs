//! The write-time existence check behind a document's entity link
//! (FR-DOC-011, [#170] AC4).
//!
//! # Why this module reads another module's tables
//!
//! It reads **existence and nothing else** — no code, no name, no field — and it
//! reads it **inside the caller's transaction, holding the row it read**. Both
//! halves are why it is here rather than a call into
//! [`master_data::service`][ms]:
//!
//! * A service call cannot join a transaction it does not own. Coding standard
//!   §2.5 puts the lock on what the *check* read, and a check that ran on the
//!   pool would answer a question about a moment that has already passed —
//!   which is #133 and #137 in this codebase, twice.
//! * There is no permission question to delegate. Whether a row exists is not a
//!   master-data field, and the caller already named the id: a caller who posts
//!   an id and is told it does not exist has learned nothing they did not
//!   supply. **Every question that *is* about master-data content goes through
//!   the service** — see [`super::super::service::link`], which resolves the
//!   link by calling `get_party` / `get_facility` and holds no permission logic
//!   of its own, exactly as [#161] does.
//!
//! `documents.entity_id` carries no foreign key — `0015_document.sql` created it
//! as a bare column because the thing it points at is polymorphic — so this
//! check *is* the constraint. That is the reason it takes a lock rather than
//! merely a read: a party soft-deleted between the check and the insert would
//! leave a document linked to a record no read returns.
//!
//! # What happens afterwards is decided, not defaulted (AC5)
//!
//! **Nothing.** Soft-deleting the party later leaves the document readable, its
//! `entityType` and `entityId` unchanged, and the resolution answering 404
//! naming the *entity* rather than the document. Cascading or nulling the link
//! would rewrite a historical record because a supplier was retired years later:
//! a purchase order that concerned supplier X still concerned supplier X, and a
//! document that quietly forgot what it was about is worse than one that points
//! at something retired.
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161
//! [#170]: https://github.com/sujanto-gaws/kelir/issues/170
//! [ms]: crate::modules::master_data::service

use uuid::Uuid;

use super::super::domain::{EntityLink, EntityType};

/// Whether the linked record exists, is this tenant's, and is not
/// soft-deleted — holding it for the rest of the transaction.
///
/// `FOR SHARE` rather than `FOR UPDATE`: this caller is not changing the row, it
/// is depending on the row continuing to exist. A share lock blocks the delete
/// that would invalidate the check and does not block a concurrent document
/// linking to the same party, which `FOR UPDATE` would — and two requisitions
/// naming one supplier at the same moment is the normal case rather than a rare
/// one.
pub async fn lock_linked_entity(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    link: EntityLink,
) -> Result<bool, sqlx::Error> {
    let found = match link.entity_type {
        EntityType::Party => {
            sqlx::query_scalar!(
                r#"
            SELECT 1 AS "found!"
            FROM mdm_parties
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            FOR SHARE
            "#,
                tenant_id,
                link.entity_id
            )
            .fetch_optional(&mut **transaction)
            .await?
        }
        EntityType::Facility => {
            sqlx::query_scalar!(
                r#"
            SELECT 1 AS "found!"
            FROM mdm_facilities
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            FOR SHARE
            "#,
                tenant_id,
                link.entity_id
            )
            .fetch_optional(&mut **transaction)
            .await?
        }
    };

    Ok(found.is_some())
}
