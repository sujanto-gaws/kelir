//! The audit chain covers what a record says, not only who wrote it (#145).
//!
//! `chain_hash` covered ten inputs and neither payload column was among them.
//! `record` writes `old_value_json` and `new_value_json`, `GET /parties/{id}/audit`
//! publishes both, and `created_at` came from the column default and was
//! published as `occurredAt` — so all three could be rewritten without
//! disturbing that row's `current_hash`, or any hash after it. The chain still
//! verified.
//!
//! `old_value` and `new_value` are the part of an audit record that answers the
//! question the record exists for. The metadata says *somebody changed this
//! party at this time*; only the payload says *from what, to what*. A control
//! that protects who and when but not what protects the half nobody would
//! bother to forge.
//!
//! # Why this file is not enough on its own, and what it adds
//!
//! `src/modules/audit/mod.rs` holds the unit tests: they pin the format, and
//! `rewriting_what_a_record_says_it_changed_breaks_the_chain` is the
//! reproduction #145 was filed on. What they cannot reach is the round trip.
//!
//! Verification recomputes a hash from the **stored** row, and the value that
//! comes back out of a `JSONB` column is not the value that went in —
//! PostgreSQL sorts object keys, discards whitespace, drops duplicate keys and
//! normalises numbers. A hash that is only correct before the row is written is
//! not a hash anything can verify, and no unit test would ever say so. That is
//! what these tests are for, and why they go through a real database.

mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

use kelir_backend::modules::audit::{canonical_json, chain_hash, AuditEntry};

/// One `audit_events` row, as a verifier would read it.
struct StoredRow {
    id: Uuid,
    tenant_id: Uuid,
    event_type: String,
    action: String,
    object_type: String,
    object_id: Uuid,
    actor_user_id: Option<Uuid>,
    ip_address: Option<String>,
    reason: Option<String>,
    old_value: Option<Value>,
    new_value: Option<Value>,
    previous_hash: String,
    current_hash: String,
    created_at: DateTime<Utc>,
}

impl StoredRow {
    /// The hash this row should carry, recomputed from the row itself.
    fn recomputed(&self) -> String {
        chain_hash(
            &self.previous_hash,
            &self.id,
            self.created_at,
            &AuditEntry {
                tenant_id: self.tenant_id,
                event_type: &self.event_type,
                action: &self.action,
                object_type: &self.object_type,
                object_id: self.object_id,
                actor_user_id: self.actor_user_id,
                ip_address: self.ip_address.as_deref(),
                reason: self.reason.as_deref(),
                old_value: self.old_value.clone(),
                new_value: self.new_value.clone(),
            },
        )
    }
}

/// Every audit row of the tenant, oldest first — the order a verifier walks.
async fn stored_rows(app: &TestApp) -> Vec<StoredRow> {
    let rows = sqlx::query!(
        r#"
        SELECT id, tenant_id, event_type, action, object_type, object_id,
               actor_user_id, ip_address, reason, old_value_json, new_value_json,
               previous_hash, current_hash, created_at
        FROM audit_events
        ORDER BY created_at, id
        "#
    )
    .fetch_all(&app.pool)
    .await
    .expect("the audit rows read back");

    rows.into_iter()
        .map(|row| StoredRow {
            id: row.id,
            tenant_id: row.tenant_id,
            event_type: row.event_type,
            action: row.action,
            object_type: row.object_type,
            object_id: row.object_id,
            actor_user_id: row.actor_user_id,
            ip_address: row.ip_address,
            reason: row.reason,
            old_value: row.old_value_json,
            new_value: row.new_value_json,
            previous_hash: row.previous_hash,
            current_hash: row.current_hash,
            created_at: row.created_at,
        })
        .collect()
}

/// A party, then an edit to it — the edit is what produces a record carrying
/// both payload halves.
async fn a_party_with_a_change_behind_it(app: &TestApp, token: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(token),
            json!({
                "partyId": "PARTY-0001",
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": "Acme Supplies" },
                // Keys deliberately out of order, and a nested object, so the
                // stored value is one PostgreSQL has genuinely reordered.
                "additionalAttributes": {
                    "zone": "north",
                    "a": 1,
                    "nested": { "second": true, "b": "x" }
                },
                "externalId": "SAP-000123",
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("the created party carries an id");

    let updated = app
        .put(
            &format!("/api/v1/master-data/parties/{id}"),
            Some(token),
            json!({
                "description": "Primary supplier",
                "externalId": "SAP-999999",
                // The shapes the round trip is actually hard for, so the
                // stored payload exercises them rather than only strings:
                // keys whose length order differs from their alphabetical
                // order, an exponent literal `numeric` prints as an integer,
                // a fraction, a nested object, an array, and a non-ASCII
                // string.
                "additionalAttributes": {
                    "zone": "utara — Jakarta",
                    "a": 1e2,
                    "ratio": 1.5,
                    // Large and small enough that the shortest-round-trip
                    // formatter reaches for exponent notation, which `numeric`
                    // never prints. Without these two the number rule looks
                    // covered and is not: every other literal here is a fixed
                    // point of both formatters.
                    "big": 1e30,
                    "small": 1e-9,
                    "nested": { "second": true, "b": [1, "two", null] }
                },
            }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    id
}

#[tokio::test]
async fn the_stored_row_recomputes_to_the_hash_stored_with_it() {
    // Acceptance criterion 2 of #145, and the one no unit test can stand in for.
    // Every row written by every operation this test performs — sign-in
    // included — is read back out of PostgreSQL and rehashed from what the
    // database returned, not from what the application held.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    a_party_with_a_change_behind_it(&app, &token).await;

    let rows = stored_rows(&app).await;
    assert!(
        rows.len() >= 2,
        "the fixture should have written several audit rows, got {}",
        rows.len()
    );

    let with_payloads = rows
        .iter()
        .filter(|row| row.old_value.is_some() || row.new_value.is_some())
        .count();
    assert!(
        with_payloads > 0,
        "no row carries a payload, so this test would prove nothing about them"
    );

    for row in &rows {
        assert_eq!(
            row.recomputed(),
            row.current_hash,
            "the stored row does not recompute to its own hash: {} / {} with old={:?} new={:?}",
            row.event_type,
            row.action,
            row.old_value,
            row.new_value
        );
    }
}

#[tokio::test]
async fn the_chain_links_every_row_to_the_one_before_it() {
    // The other property a verifier checks, and the reason the round trip
    // matters at all: a chain that does not link is a chain that cannot detect
    // a removed row.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    a_party_with_a_change_behind_it(&app, &token).await;

    let rows = stored_rows(&app).await;
    let genesis = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    let mut expected_previous = genesis.to_owned();
    for row in &rows {
        assert_eq!(
            row.previous_hash, expected_previous,
            "the chain is broken at {} / {}",
            row.event_type, row.action
        );
        expected_previous = row.current_hash.clone();
    }
}

#[tokio::test]
async fn rewriting_what_a_stored_record_says_it_changed_no_longer_verifies() {
    // The defect, driven end to end against the database rather than against
    // `AuditEntry` alone. Before #145 this rewrite left `current_hash` intact
    // and every hash after it intact: a record naming a real actor at a real
    // time, misstating the change, and verifying.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    a_party_with_a_change_behind_it(&app, &token).await;

    let target = stored_rows(&app)
        .await
        .into_iter()
        .find(|row| row.new_value.is_some())
        .expect("an update recorded what it changed");

    assert_eq!(
        target.recomputed(),
        target.current_hash,
        "the row should verify before it is tampered with"
    );

    sqlx::query("UPDATE audit_events SET new_value_json = $2 WHERE id = $1")
        .bind(target.id)
        .bind(json!({ "externalId": "SAP-000000" }))
        .execute(&app.pool)
        .await
        .expect("the tampering write lands");

    let tampered = stored_rows(&app)
        .await
        .into_iter()
        .find(|row| row.id == target.id)
        .expect("the row is still there");

    assert_ne!(
        tampered.recomputed(),
        tampered.current_hash,
        "rewriting what the record says it changed left the chain intact"
    );
}

#[tokio::test]
async fn moving_a_stored_record_in_time_no_longer_verifies() {
    // `created_at` came from the column default and was outside the hash, so a
    // record could be back-dated — the `occurredAt` a reader trusts — without
    // disturbing anything.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    a_party_with_a_change_behind_it(&app, &token).await;

    let target = stored_rows(&app)
        .await
        .into_iter()
        .next()
        .expect("at least one audit row");

    sqlx::query("UPDATE audit_events SET created_at = created_at - interval '1 day' WHERE id = $1")
        .bind(target.id)
        .execute(&app.pool)
        .await
        .expect("the tampering write lands");

    let tampered = stored_rows(&app)
        .await
        .into_iter()
        .find(|row| row.id == target.id)
        .expect("the row is still there");

    assert_ne!(
        tampered.recomputed(),
        tampered.current_hash,
        "back-dating a record left the chain intact"
    );
}

#[tokio::test]
async fn a_payload_survives_the_jsonb_round_trip_byte_for_byte() {
    // The narrow property the canonical form exists for, isolated from the
    // hash. If PostgreSQL renders the stored value differently from the way the
    // application canonicalised it, every row fails verification and the reason
    // is one layer below anything the other tests report.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    a_party_with_a_change_behind_it(&app, &token).await;

    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT new_value_json::text FROM audit_events WHERE new_value_json IS NOT NULL",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the payloads read back as text");

    assert!(!stored.is_empty(), "no payload to check");

    for text in stored {
        let value: Value = serde_json::from_str(&text).expect("the stored text is JSON");
        // Re-canonicalising what came back must reproduce it exactly. The only
        // way this holds is if what went in was already in PostgreSQL's own
        // form — which is what `record` stores.
        let recanonicalised = canonical_json(&value);

        assert_eq!(
            recanonicalised, text,
            "the canonical form is not a fixed point of a JSONB round trip"
        );
    }
}
