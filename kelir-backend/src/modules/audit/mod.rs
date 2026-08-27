//! Formal audit record (SRS FR-AUD-001..005).
//!
//! Modules never insert audit rows directly (coding standard §2.8) — they call
//! [`record`], which owns the hash chain.
//!
//! Phase 2 writes the chain; verifying it and exposing audit search land with
//! the rest of §10 in Phase 6.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Map, Number, Value};
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
/// Each row's `current_hash` covers **every column this function writes** — the
/// metadata, the two payload halves, and the row's own `created_at` — *and* the
/// previous row's hash, so altering or removing any row breaks every hash after
/// it. That makes tampering detectable (FR-AUD-003) without preventing writes.
///
/// The columns outside the hash are the ones nothing writes: `document_id`,
/// `workflow_instance_id`, `entity_type`, `entity_id`, `actor_role_id` and
/// `digital_signature_ref` are declared in `0003_audit.sql` for later phases and
/// are `NULL` on every row this function has ever produced. They join the hash
/// when something starts filling them, which is another format change and
/// another re-chaining decision — see [`chain_hash`].
///
/// **`created_at` is taken here rather than by the column default.** Hashing a
/// value the database chooses would mean either reading it back after the insert
/// or hashing something other than what is stored. Taking it in the application
/// makes the hash a function of the entry alone, at the cost of the row's time
/// being the API's clock rather than the database's — which is the honest one
/// anyway, since it is the moment the event happened.
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
    // Truncated to microseconds because that is `TIMESTAMPTZ`'s resolution: a
    // nanosecond the column cannot hold is a nanosecond the hash would cover and
    // no verifier could read back.
    let created_at = truncate_to_microseconds(Utc::now());
    let current_hash = chain_hash(&previous_hash, &id, created_at, &entry);

    // The payloads go in as the canonical text the hash was taken over, not as
    // `Value`. Binding the `Value` lets `sqlx` render it with `serde_json`, and
    // `serde_json`'s rendering is not `jsonb`'s: an exponent literal reaches
    // PostgreSQL as `100.0` and is stored and printed with that scale, while the
    // canonical form of the same value is `100`. The hash verifies either way —
    // both sides canonicalise the `Value` — but the stored bytes would then be
    // bytes no hash describes, and "the row is what was hashed" is the property
    // a verifier and an auditor should not have to qualify.
    //
    // `a_payload_survives_the_jsonb_round_trip_byte_for_byte` is what holds this
    // to it: it compares the canonical form against PostgreSQL's own `::text`.
    sqlx::query!(
        r#"
        INSERT INTO audit_events (
            id, tenant_id, event_type, action, object_type, object_id,
            old_value_json, new_value_json, reason, actor_user_id, ip_address,
            previous_hash, current_hash, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb, $8::text::jsonb,
                $9, $10, $11, $12, $13, $14)
        "#,
        id,
        entry.tenant_id,
        entry.event_type,
        entry.action,
        entry.object_type,
        entry.object_id,
        entry.old_value.as_ref().map(canonical_json),
        entry.new_value.as_ref().map(canonical_json),
        entry.reason,
        entry.actor_user_id,
        entry.ip_address,
        previous_hash,
        current_hash,
        created_at
    )
    .execute(pool)
    .await?;

    Ok(id)
}

/// `TIMESTAMPTZ` holds microseconds; `Utc::now()` offers nanoseconds.
fn truncate_to_microseconds(at: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(at.timestamp_micros()).unwrap_or(at)
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

/// The chain format. Public because verifying a chain (FR-AUD-003) means
/// recomputing this over a row read back out of the database, and a verifier
/// that reimplemented it would be checking its own copy of the rule.
///
/// **Field order is part of the format**: changing it, or adding a field,
/// invalidates every chain computed without it. No row carries a version stamp,
/// so a format change today is a code change, and a format change after anything
/// verifies a chain is a re-chaining migration plus a decision about what a row
/// predating the version means to a verifier.
///
/// **What the encoding guarantees.** Every present field is hashed as eight
/// bytes of big-endian length followed by its bytes, and every absent one as
/// the [`ABSENT`] sentinel length and no bytes. That is two properties, and
/// each answers a way a rewrite could go unnoticed:
///
/// * **A field boundary cannot be moved.** Concatenating raw bytes leaves the
///   boundaries floating: an `event_type` of `Party.Cre` with an `action` of
///   `atedX` hashed identically to `Party.Created` with `X`.
/// * **Absent is not empty.** A `NULL` `ip_address` and an `''` one are
///   different rows and hash differently, as are a `NULL` `reason` and an empty
///   one, and an absent payload and a JSON `null` one. The sentinel is what
///   makes that true of *every* optional field rather than only of the two
///   whose empty form is already four bytes long.
///
/// It has been changed twice, both times while that was still free — nothing
/// has ever verified a chain (FR-AUD-003 is Phase 6), so no stored value has
/// ever been relied on:
///
/// * **2026-08-26, #145.** `old_value`, `new_value` and `created_at` joined the
///   hash and the length prefixes arrived. The three had never been in it —
///   `record` writes all three and `GET /parties/{id}/audit` publishes all
///   three, and each could be rewritten without disturbing this row's hash or
///   any hash after it. A control that covers who and when but not *what*
///   protects the half nobody would bother to forge.
/// * **2026-08-27, #203.** Absent stopped being hashed as zero bytes. The
///   change above claimed the length prefix already told absent from
///   present-but-empty; it did not, and the two `Option<&str>` columns were the
///   proof.
pub fn chain_hash(
    previous_hash: &str,
    id: &Uuid,
    created_at: DateTime<Utc>,
    entry: &AuditEntry<'_>,
) -> String {
    let old_value = entry.old_value.as_ref().map(canonical_json);
    let new_value = entry.new_value.as_ref().map(canonical_json);

    let mut hasher = Sha256::new();
    let mut field = |bytes: Option<&[u8]>| match bytes {
        Some(bytes) => {
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        None => hasher.update(ABSENT.to_be_bytes()),
    };

    field(Some(previous_hash.as_bytes()));
    field(Some(id.as_bytes()));
    field(Some(entry.tenant_id.as_bytes()));
    field(Some(
        created_at
            .to_rfc3339_opts(SecondsFormat::Micros, true)
            .as_bytes(),
    ));
    field(Some(entry.event_type.as_bytes()));
    field(Some(entry.action.as_bytes()));
    field(Some(entry.object_type.as_bytes()));
    field(Some(entry.object_id.as_bytes()));
    field(
        entry
            .actor_user_id
            .as_ref()
            .map(|actor| actor.as_bytes().as_slice()),
    );
    field(entry.ip_address.map(str::as_bytes));
    field(entry.reason.map(str::as_bytes));
    field(old_value.as_deref().map(str::as_bytes));
    field(new_value.as_deref().map(str::as_bytes));

    format!("sha256:{}", hex(&hasher.finalize()))
}

/// The length prefix an **absent** field is hashed as, in place of its bytes.
///
/// A present field is hashed as its own length followed by its bytes, so a
/// present-but-empty one is a prefix of `0` and nothing. Absent needs a prefix
/// no present field can produce, and `u64::MAX` is that: a field of that length
/// is sixteen exabytes and cannot be constructed, let alone stored in the
/// column this is hashing.
///
/// **The guarantee is the sentinel's, not the length prefix's.** It replaced a
/// helper that hashed `None` as zero bytes and a doc comment claiming the
/// prefix kept the two apart; it did not — `ip_address` `NULL` and `''` hashed
/// identically, and so did `reason`, so either column could be rewritten either
/// way without breaking the chain that exists to make a rewrite detectable
/// (#203). The claim was true of *payloads* only, and for a different reason:
/// `None` is SQL `NULL` while `Some(Value::Null)` canonicalises to the four
/// bytes `null`, which is a present field of length four.
const ABSENT: u64 = u64::MAX;

/// The payload rendered the way PostgreSQL renders `jsonb`.
///
/// **Why this exists rather than `serde_json::to_string`.** Verification
/// recomputes the hash from the stored row, and the value that comes back out of
/// a `JSONB` column is not the value that went in: PostgreSQL sorts object keys,
/// discards whitespace, drops duplicate keys and normalises numbers. The hash
/// therefore has to be taken over a form that both sides of that round trip
/// agree on — a hash that is only correct before the row is written is not a
/// hash anything can verify.
///
/// Two things make that agreement hold, and only one of them is obvious:
///
/// * **Key order is pinned here rather than inherited.** `serde_json`'s `Map` is
///   a `BTreeMap` by default and would order keys consistently on its own — but
///   `preserve_order` is a *feature*, and a feature any crate in the dependency
///   graph can turn on for the whole build. A chain whose validity depends on
///   which features unified is a chain that breaks on an unrelated `cargo
///   update`. This function does not care what `Map` is backed by.
/// * **Numbers are normalised the way `numeric` normalises them.** This is the
///   half that is not merely defensive: `1e2` goes in as an `f64` and comes back
///   out of `jsonb` as the integer `100`, so a formatter that preserved the
///   input form would hash two different strings for the same stored value.
///
/// `record` also stores this rather than the `Value`, so the bytes in the column
/// are the bytes that were hashed. That is not what makes verification work —
/// both sides canonicalise the `Value`, so the hash holds either way — but
/// "the row is what was hashed" is a property a verifier and an auditor should
/// not have to qualify. It was very nearly dropped as unobservable: the mutation
/// that reverts it comes back green against a payload of strings and only goes
/// red once one carries a number, which is why the round-trip fixture carries
/// `1e2`.
///
/// The rules, each matching `jsonb`:
///
/// * **Object keys sorted by length, then bytewise** — `jsonb`'s storage order,
///   and the reason application-side key ordering is pinned here rather than
///   inherited from whichever `serde_json` features the dependency graph happens
///   to enable.
/// * **Numbers in plain decimal, never exponent form.** `numeric` prints no
///   exponent, so `1e2` has to be written `100` for the round trip to hold.
///   Rust's `Display` for `f64` is already exponent-free and shortest
///   round-trip; `-0.0` is the one value it renders in a form `numeric` would
///   not return, and is normalised to `0`.
/// * **A space after `:` and after `,`**, which is what `jsonb` prints.
/// * **`serde_json`'s string escaping**, which is `jsonb`'s: quote and
///   backslash, the five short control escapes, `\uXXXX` below those, and raw
///   UTF-8 above.
///
/// Public for the same reason [`chain_hash`] is: it is part of the published
/// chain format, and anything verifying a chain has to agree with it rather than
/// with its own copy of the rule.
///
/// `the_stored_row_recomputes_to_the_hash_stored_with_it` is what keeps this
/// honest — it goes through PostgreSQL rather than reasoning about it.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&canonical_number(number)),
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|left, right| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left.as_bytes().cmp(right.as_bytes()))
            });

            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_json_string(key, out);
                out.push_str(": ");
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

/// A JSON string literal, escaping and all, borrowed from `serde_json` rather
/// than hand-rolled — the escape set is the part worth not reimplementing.
fn write_json_string(text: &str, out: &mut String) {
    out.push_str(&Value::String(text.to_owned()).to_string());
}

fn canonical_number(number: &Number) -> String {
    if let Some(value) = number.as_i64() {
        return value.to_string();
    }
    if let Some(value) = number.as_u64() {
        return value.to_string();
    }

    match number.as_f64() {
        Some(value) => {
            if value == 0.0 {
                // `numeric` has no signed zero, so `-0` would come back as `0`.
                "0".to_owned()
            } else {
                value.to_string()
            }
        }
        // Unreachable: a `Number` is one of the three.
        None => "0".to_owned(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant, so a unit test's hash is a function of its entry alone.
    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp_micros(1_772_000_000_000_000).expect("a valid instant")
    }

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

        let first = chain_hash(&genesis_hash(), &id, at(), &entry(tenant, "LOGIN"));
        let second = chain_hash("sha256:deadbeef", &id, at(), &entry(tenant, "LOGIN"));

        assert_ne!(first, second);
    }

    #[test]
    fn the_chain_depends_on_the_content() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let previous = genesis_hash();

        let login = chain_hash(&previous, &id, at(), &entry(tenant, "LOGIN"));
        let failed = chain_hash(&previous, &id, at(), &entry(tenant, "LOGIN_FAILED"));

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
            chain_hash(&genesis_hash(), &id, at(), &entry(tenant, "LOGIN")),
            chain_hash(&genesis_hash(), &id, at(), &entry(tenant, "LOGIN"))
        );
    }

    #[test]
    fn hashes_are_prefixed_and_full_length() {
        let hash = chain_hash(
            &genesis_hash(),
            &Uuid::now_v7(),
            at(),
            &entry(Uuid::now_v7(), "LOGIN"),
        );

        assert!(hash.starts_with("sha256:"));
        assert_eq!(hash.len(), "sha256:".len() + 64);
    }

    // -----------------------------------------------------------------------
    // What the chain covers (#145)
    // -----------------------------------------------------------------------

    #[test]
    fn rewriting_what_a_record_says_it_changed_breaks_the_chain() {
        // The reproduction #145 was filed on, kept as the regression test its
        // third acceptance criterion asks for. Against the previous format these
        // two hashed identically: `old_value` and `new_value` were on the entry,
        // were written to the row, were published by `GET /parties/{id}/audit` --
        // and were not in the hash. Nine call sites across `identity` and
        // `master-data` write payloads.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let previous = genesis_hash();

        let mut recorded = entry(tenant, "UPDATE");
        recorded.new_value = Some(serde_json::json!({ "statusId": "PARTY_ENABLED" }));

        let mut altered = entry(tenant, "UPDATE");
        altered.new_value = Some(serde_json::json!({ "statusId": "PARTY_DISABLED" }));

        assert_ne!(
            chain_hash(&previous, &id, at(), &recorded),
            chain_hash(&previous, &id, at(), &altered),
            "rewriting what a record says it changed left the chain intact"
        );
    }

    #[test]
    fn rewriting_what_a_record_says_it_changed_from_breaks_the_chain() {
        // The other half. A forged `old_value` misstates what the change was
        // *from*, which is the half an approval trail is read for.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let previous = genesis_hash();

        let mut recorded = entry(tenant, "UPDATE");
        recorded.old_value = Some(serde_json::json!({ "creditLimit": "1000.00" }));

        let mut altered = entry(tenant, "UPDATE");
        altered.old_value = Some(serde_json::json!({ "creditLimit": "9000000.00" }));

        assert_ne!(
            chain_hash(&previous, &id, at(), &recorded),
            chain_hash(&previous, &id, at(), &altered)
        );
    }

    #[test]
    fn moving_a_record_in_time_breaks_the_chain() {
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();
        let later = DateTime::from_timestamp_micros(1_772_000_000_000_001).expect("valid");

        assert_ne!(
            chain_hash(&genesis_hash(), &id, at(), &entry(tenant, "LOGIN")),
            chain_hash(&genesis_hash(), &id, later, &entry(tenant, "LOGIN"))
        );
    }

    #[test]
    fn an_absent_payload_is_not_the_same_as_a_null_one() {
        // What the length prefix buys. A `None` payload writes SQL NULL and a
        // `Some(Value::Null)` writes JSON null; the two are different rows and
        // must be different hashes.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let absent = entry(tenant, "DELETE");
        let mut null = entry(tenant, "DELETE");
        null.new_value = Some(Value::Null);

        assert_ne!(
            chain_hash(&genesis_hash(), &id, at(), &absent),
            chain_hash(&genesis_hash(), &id, at(), &null)
        );
    }

    #[test]
    fn an_absent_optional_field_is_not_the_same_as_an_empty_one() {
        // #203: four entries differing only in whether `ip_address` and
        // `reason` are absent or present-but-empty hashed to one value, so
        // either column could be rewritten either way and the chain still
        // verified. The four hashes below must be four.
        //
        // Seen red (coding standard §2.9) against `ABSENT` restored to `0`,
        // which is what the previous `optional` helper hashed a `None` as: all
        // four collapse to the same digest.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let bare = entry(tenant, "LOGIN");

        let mut empty_ip = entry(tenant, "LOGIN");
        empty_ip.ip_address = Some("");

        let mut empty_reason = entry(tenant, "LOGIN");
        empty_reason.reason = Some("");

        let mut both_empty = entry(tenant, "LOGIN");
        both_empty.ip_address = Some("");
        both_empty.reason = Some("");

        let hashes: Vec<String> = [&bare, &empty_ip, &empty_reason, &both_empty]
            .iter()
            .map(|entry| chain_hash(&genesis_hash(), &id, at(), entry))
            .collect();

        let distinct: std::collections::BTreeSet<&String> = hashes.iter().collect();

        assert_eq!(
            distinct.len(),
            hashes.len(),
            "absent and present-but-empty hash alike: {hashes:?}"
        );
    }

    #[test]
    fn an_absent_actor_is_not_the_same_as_the_nil_actor() {
        // The other `Option` in the entry, and the one where "empty" has a
        // natural spelling: an unauthenticated event writes no actor, and
        // `Uuid::nil()` is sixteen zero bytes somebody could write instead.
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let absent = entry(tenant, "LOGIN_FAILED");
        let mut nil_actor = entry(tenant, "LOGIN_FAILED");
        nil_actor.actor_user_id = Some(Uuid::nil());

        assert_ne!(
            chain_hash(&genesis_hash(), &id, at(), &absent),
            chain_hash(&genesis_hash(), &id, at(), &nil_actor)
        );
    }

    #[test]
    fn a_field_boundary_cannot_be_moved() {
        // Without length prefixes these hashed the same: the concatenation
        // "Party.Cre" + "atedX" is byte-for-byte "Party.Created" + "X".
        let tenant = Uuid::now_v7();
        let id = Uuid::now_v7();

        let mut left = entry(tenant, "atedX");
        left.event_type = "Party.Cre";

        let mut right = entry(tenant, "X");
        right.event_type = "Party.Created";

        assert_ne!(
            chain_hash(&genesis_hash(), &id, at(), &left),
            chain_hash(&genesis_hash(), &id, at(), &right)
        );
    }

    // -----------------------------------------------------------------------
    // The canonical payload form
    // -----------------------------------------------------------------------

    #[test]
    fn object_keys_are_ordered_the_way_jsonb_orders_them() {
        // Shorter keys first, then bytewise -- not lexicographic. `bb` before
        // `aaa` is the case that tells the two rules apart.
        let value = serde_json::json!({ "aaa": 1, "bb": 2, "a": 3 });

        assert_eq!(canonical_json(&value), "{\"a\": 3, \"bb\": 2, \"aaa\": 1}");
    }

    #[test]
    fn the_canonical_form_does_not_depend_on_insertion_order() {
        let one = serde_json::json!({ "b": 1, "a": 2 });
        let other = serde_json::json!({ "a": 2, "b": 1 });

        assert_eq!(canonical_json(&one), canonical_json(&other));
    }

    #[test]
    fn numbers_are_written_in_the_form_numeric_prints() {
        // `1e2` stored as `numeric` prints `100`, so the canonical form has to
        // be `100` for a row to recompute to its own hash.
        assert_eq!(canonical_json(&serde_json::json!(1e2)), "100");
        assert_eq!(canonical_json(&serde_json::json!(1.5)), "1.5");
        assert_eq!(canonical_json(&serde_json::json!(-0.0)), "0");
        assert_eq!(canonical_json(&serde_json::json!(42)), "42");
        assert_eq!(canonical_json(&serde_json::json!(1e-7)), "0.0000001");
        // The two that separate this rule from `serde_json`'s own formatter,
        // which reaches for exponent notation here and nowhere below it.
        assert_eq!(
            canonical_json(&serde_json::json!(1e30)),
            "1000000000000000000000000000000"
        );
        assert_eq!(canonical_json(&serde_json::json!(1e-9)), "0.000000001");
    }

    #[test]
    fn nesting_and_arrays_carry_the_same_rules_down() {
        let value = serde_json::json!({ "z": [ { "bb": 1, "a": 2 } ], "y": null });

        assert_eq!(
            canonical_json(&value),
            "{\"y\": null, \"z\": [{\"a\": 2, \"bb\": 1}]}"
        );
    }
}
