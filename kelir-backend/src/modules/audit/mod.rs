//! Formal audit record (SRS FR-AUD-001..005).
//!
//! Modules never insert audit rows directly (coding standard §2.8) — they call
//! [`record`], which owns the hash chain.
//!
//! Phase 2 writes the chain; verifying it and exposing audit search land with
//! the rest of §10 in Phase 6.

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

/// What happened, in the event vocabulary of naming convention §7.
pub struct AuditEntry<'a> {
    pub tenant_id: Uuid,
    pub event_type: &'a str,
    pub action: &'a str,
    pub object_type: &'a str,
    pub object_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub ip_address: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,
}

/// What a write actually changed, for the two halves of an [`AuditEntry`].
///
/// An update request carries only the fields it changes — that is what makes a
/// partial update partial — so **the request is not a description of the
/// change**. A field the caller never mentioned deserialises to the same `None`
/// as one they asked to clear, and a record built from the request cannot tell
/// the two apart. It reported the second: an update that touched one field
/// produced a record saying every other field had been cleared, and the field
/// that did change was in neither half (#135).
///
/// So both halves are read off the row — once before the write, once after —
/// and only the fields whose value moved are recorded. A field that did not
/// move is absent from both halves, which is also what removes the ambiguity:
/// *omitted* leaves the value where it was and says nothing here, while
/// *cleared* moves it to `null` and is recorded as such.
///
/// It follows that a request which changes nothing records nothing on either
/// side. The record still exists, with its actor and its time and two empty
/// objects — the update happened and moved no field, which is what the trail
/// should say.
///
/// ```ignore
/// let mut changes = ChangeSet::new();
/// changes.field("name", &before.name, &after.name);
/// changes.field("ownerPartyId", &before.owner_party_id, &after.owner_party_id);
/// let (old_value, new_value) = changes.halves();
/// ```
#[derive(Debug, Default)]
pub struct ChangeSet {
    old: Map<String, Value>,
    new: Map<String, Value>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `name` if the row's value for it moved, and nothing otherwise.
    ///
    /// Both sides are serialised the way the API publishes them, so a record
    /// reads in the caller's vocabulary — `"BUILDING"` rather than the enum,
    /// under the `facilityTypeId` the caller sent. `name` is therefore the
    /// wire name of the field and not the column's.
    ///
    /// Serialisation cannot fail for what a caller passes here: a string, a
    /// plain enum, an `Option` of one, or a `Value` already read out of a JSONB
    /// column. `Value::Null` stands in if one ever did, and a field that
    /// serialises to null on both sides is a field that did not move.
    pub fn field<T: Serialize + ?Sized>(&mut self, name: &str, before: &T, after: &T) {
        let before = serde_json::to_value(before).unwrap_or(Value::Null);
        let after = serde_json::to_value(after).unwrap_or(Value::Null);

        if before == after {
            return;
        }

        self.old.insert(name.to_owned(), before);
        self.new.insert(name.to_owned(), after);
    }

    /// The `old_value` and `new_value` of the entry, in that order.
    pub fn halves(self) -> (Value, Value) {
        (Value::Object(self.old), Value::Object(self.new))
    }
}

/// Appends an audit event, chaining it to the tenant's previous row.
///
/// Each row's `current_hash` covers its own content *and* the previous row's
/// hash, so altering or removing any row breaks every hash after it. That makes
/// tampering detectable (FR-AUD-003) without preventing writes.
///
/// A failure here must not fail the operation being audited — losing a login
/// because the audit insert failed would be worse than the missing row — so the
/// caller decides. [`record_or_warn`] is the usual choice.
pub async fn record(pool: &PgPool, entry: AuditEntry<'_>) -> Result<Uuid, AppError> {
    let previous_hash = sqlx::query_scalar!(
        r#"
        SELECT current_hash
        FROM audit_events
        WHERE tenant_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
        entry.tenant_id
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or_else(genesis_hash);

    let id = Uuid::now_v7();
    let current_hash = chain_hash(&previous_hash, &id, &entry);

    sqlx::query!(
        r#"
        INSERT INTO audit_events (
            id, tenant_id, event_type, action, object_type, object_id,
            old_value_json, new_value_json, reason, actor_user_id, ip_address,
            previous_hash, current_hash
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        id,
        entry.tenant_id,
        entry.event_type,
        entry.action,
        entry.object_type,
        entry.object_id,
        entry.old_value,
        entry.new_value,
        entry.reason,
        entry.actor_user_id,
        entry.ip_address,
        previous_hash,
        current_hash
    )
    .execute(pool)
    .await?;

    Ok(id)
}

/// Records an event, logging rather than propagating a failure.
///
/// For events that accompany an operation which has already succeeded: the user
/// is signed in whether or not the audit row was written, and refusing the
/// response at that point would be a worse outcome than a gap in the trail. The
/// gap is logged at error level so it is not silent.
pub async fn record_or_warn(pool: &PgPool, entry: AuditEntry<'_>) {
    let event_type = entry.event_type.to_owned();

    if let Err(error) = record(pool, entry).await {
        tracing::error!(error = ?error, event_type, "failed to write audit event");
    }
}

/// First link in a tenant's chain.
fn genesis_hash() -> String {
    "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned()
}

fn chain_hash(previous_hash: &str, id: &Uuid, entry: &AuditEntry<'_>) -> String {
    let mut hasher = Sha256::new();

    // Field order is part of the format: changing it invalidates every existing
    // chain, so it must not be reordered casually.
    hasher.update(previous_hash.as_bytes());
    hasher.update(id.as_bytes());
    hasher.update(entry.tenant_id.as_bytes());
    hasher.update(entry.event_type.as_bytes());
    hasher.update(entry.action.as_bytes());
    hasher.update(entry.object_type.as_bytes());
    hasher.update(entry.object_id.as_bytes());
    hasher.update(entry.actor_user_id.unwrap_or_default().as_bytes());
    hasher.update(entry.ip_address.unwrap_or_default().as_bytes());
    hasher.update(entry.reason.unwrap_or_default().as_bytes());

    format!("sha256:{}", hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tenant_id: Uuid, action: &'static str) -> AuditEntry<'static> {
        AuditEntry {
            tenant_id,
            event_type: "Security.SignedIn",
            action,
            object_type: "USER",
            object_id: Uuid::nil(),
            actor_user_id: None,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        }
    }

    #[test]
    fn a_change_set_records_only_the_fields_that_moved() {
        let mut changes = ChangeSet::new();
        changes.field("name", "Head Office", "Head Office (North)");
        changes.field("facilityTypeId", "BUILDING", "BUILDING");

        let (old_value, new_value) = changes.halves();

        assert_eq!(old_value, serde_json::json!({ "name": "Head Office" }));
        assert_eq!(
            new_value,
            serde_json::json!({ "name": "Head Office (North)" })
        );
    }

    #[test]
    fn clearing_a_field_is_not_the_same_as_leaving_it_alone() {
        // The distinction #135 is about. `Option<Option<T>>` exists in the
        // update requests so that *omitted* and *set to null* are different
        // requests; recording the request lost that, and recording the row
        // keeps it — an omitted field never moves, so it is never written.
        let mut cleared = ChangeSet::new();
        cleared.field("parentFacilityId", &Some("FAC-SITE"), &None);

        let mut left_alone = ChangeSet::new();
        left_alone.field("parentFacilityId", &Some("FAC-SITE"), &Some("FAC-SITE"));

        let (old_value, new_value) = cleared.halves();
        assert_eq!(
            old_value,
            serde_json::json!({ "parentFacilityId": "FAC-SITE" })
        );
        assert_eq!(new_value, serde_json::json!({ "parentFacilityId": null }));

        let (old_value, new_value) = left_alone.halves();
        assert_eq!(old_value, serde_json::json!({}));
        assert_eq!(new_value, serde_json::json!({}));
    }

    #[test]
    fn a_write_that_moved_nothing_records_two_empty_halves() {
        // Not `null`: a DELETE has no halves at all, and the difference between
        // "not applicable" and "nothing moved" is worth keeping in the trail.
        let (old_value, new_value) = ChangeSet::new().halves();

        assert_eq!(old_value, serde_json::json!({}));
        assert_eq!(new_value, serde_json::json!({}));
    }

    #[test]
    fn the_chain_depends_on_the_previous_hash() {
        // The same content at a different chain position must hash differently,
        // or a row could be moved without detection.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let first = chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN"));
        let second = chain_hash("sha256:deadbeef", &id, &entry(tenant, "LOGIN"));

        assert_ne!(first, second);
    }

    #[test]
    fn the_chain_depends_on_the_content() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let previous = genesis_hash();

        let login = chain_hash(&previous, &id, &entry(tenant, "LOGIN"));
        let failed = chain_hash(&previous, &id, &entry(tenant, "LOGIN_FAILED"));

        assert_ne!(
            login, failed,
            "changing the action must change the hash, or an event could be rewritten"
        );
    }

    #[test]
    fn hashing_is_deterministic() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        assert_eq!(
            chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN")),
            chain_hash(&genesis_hash(), &id, &entry(tenant, "LOGIN"))
        );
    }

    #[test]
    fn hashes_are_prefixed_and_full_length() {
        let hash = chain_hash(
            &genesis_hash(),
            &Uuid::now_v7(),
            &entry(Uuid::now_v7(), "LOGIN"),
        );

        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), "sha256:".len() + 64);
    }
}
