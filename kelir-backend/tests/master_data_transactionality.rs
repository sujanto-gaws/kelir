//! The party aggregate's writes are all-or-nothing.
//!
//! **Why this file exists, and why it exists now.** A party write touches nine
//! tables in one transaction — the party, its person or group detail, its
//! identifications, both directions of its relationships, its classifications,
//! its contact mechanisms, the mechanisms those links create, and the opening
//! status-history row. Nothing covered the failure half of that: every test
//! asserted what a *successful* write stored.
//!
//! The gap was invisible because it had no injector. Making the ninth insert
//! fail needs a value that passes validation and is refused by the database,
//! and #109 was exactly that — an unbounded `purposeTypeId` reaching a
//! `VARCHAR(64)` column. That defect is the subject of the fix landing beside
//! this file, and closing it closes the injector with it. #109 says so in as
//! many words: write these tests **before** the fix, using the defect, or find
//! another way to fail mid-transaction.
//!
//! This is the other way, and it is the better one: a `BEFORE INSERT` trigger,
//! installed by the test into its own database, that raises when it sees a
//! marker purpose. It reaches the same statement the defect reached, it does
//! not depend on any validation rule holding or not holding, and it will still
//! work after every field on the surface is bounded. Each test owns its
//! database (see `common`), so arming it is local to the test that arms it.
//!
//! `the_injector_is_what_fails_the_write` is the control. Without it these
//! tests would pass just as well against a router that refused the payload for
//! some unrelated reason, and would be reporting on nothing (coding standard
//! §2.9).

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{json, Value};
use sqlx::PgPool;

/// The purpose code the injected trigger refuses. Any value the column accepts
/// works; this one names itself in a failure message.
const MARKER: &str = "INJECTED_FAILURE";

/// Installs a trigger that fails the insert of a party↔mechanism link carrying
/// [`MARKER`].
///
/// `BEFORE INSERT` on `mdm_party_contact_mechs` puts it on the last statement
/// of `replace_contact_mechs`'s loop, which is where the write is deepest: the
/// party row, both extension rows, and the mechanism row this link points at
/// are all already inserted in the same transaction.
async fn arm_injector(pool: &PgPool) {
    // Two calls, not one: `sqlx::query` speaks the extended protocol, which
    // carries one statement per round trip.
    sqlx::query(
        r#"
        CREATE FUNCTION kelir_test_injected_failure() RETURNS trigger AS $$
        BEGIN
            IF NEW.purpose_type = 'INJECTED_FAILURE' THEN
                RAISE EXCEPTION 'injected mid-transaction failure';
            END IF;
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        "#,
    )
    .execute(pool)
    .await
    .expect("the injector's function installs");

    sqlx::query(
        r#"
        CREATE TRIGGER kelir_test_injected_failure
            BEFORE INSERT ON mdm_party_contact_mechs
            FOR EACH ROW EXECUTE FUNCTION kelir_test_injected_failure()
        "#,
    )
    .execute(pool)
    .await
    .expect("the injector's trigger installs");
}

/// A party with two contact mechanisms; the second carries `purpose` so a
/// caller can decide whether it trips the injector.
fn party_with(party_code: &str, purpose: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PERSON",
        "person": { "firstName": "Ana", "lastName": "Prawira" },
        "contactMechanisms": [
            {
                "contactMechTypeId": "EMAIL_ADDRESS",
                "purposeTypeId": "PRIMARY_OFFICE",
                "fromDate": "2026-01-01T00:00:00Z",
                "detail": { "emailAddress": "ana@acme.example" },
            },
            {
                "contactMechTypeId": "PHONE_NUMBER",
                "purposeTypeId": purpose,
                "fromDate": "2026-01-01T00:00:00Z",
                "detail": { "telecomNumber": { "contactNumber": "555 0100" } },
            },
        ],
    })
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(pool)
        .await
        .expect("the count runs")
}

#[tokio::test]
async fn the_injector_is_what_fails_the_write() {
    // The control. The same payload, one purpose code apart: without the marker
    // the party is created, with it the request fails. Anything else these
    // tests observe is therefore caused by the injected failure and not by the
    // payload being unacceptable for some other reason.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    arm_injector(&app.pool).await;

    let accepted = app
        .post(
            "/api/v1/master-data/parties",
            Some(&token),
            party_with("PARTY-CONTROL", "BILLING"),
        )
        .await;
    let refused = app
        .post(
            "/api/v1/master-data/parties",
            Some(&token),
            party_with("PARTY-INJECTED", MARKER),
        )
        .await;

    assert_eq!(
        accepted.status,
        StatusCode::CREATED,
        "the unmarked payload should be accepted: {}",
        accepted.body
    );
    assert_eq!(
        refused.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the marked payload should fail in the database: {}",
        refused.body
    );
}

#[tokio::test]
async fn a_failed_create_leaves_no_party_at_all() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    arm_injector(&app.pool).await;

    let response = app
        .post(
            "/api/v1/master-data/parties",
            Some(&token),
            party_with("PARTY-ROLLBACK", MARKER),
        )
        .await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);

    // Nine tables took part in that transaction. These are the four a partial
    // write would leave a trace in: the aggregate root, its extension row, the
    // mechanism rows created for the links, and the opening status row.
    assert_eq!(
        count(
            &app.pool,
            "SELECT count(*) FROM mdm_parties WHERE party_code = 'PARTY-ROLLBACK'"
        )
        .await,
        0,
        "the party row survived a failed create"
    );
    assert_eq!(
        count(&app.pool, "SELECT count(*) FROM mdm_persons").await,
        0,
        "the person detail survived a failed create"
    );
    // This is the load-bearing one, and the reason the injector is on the
    // *second* mechanism. By the time the trigger fires, the first mechanism's
    // `mdm_contact_mechs` row has already been inserted in this transaction —
    // so a zero here is a row that existed being taken back, not a row that was
    // never written. Without it the other three assertions would hold just as
    // well against a failure that happened before anything was written, which
    // is the gate §2.9 warns about.
    assert_eq!(
        count(&app.pool, "SELECT count(*) FROM mdm_contact_mechs").await,
        0,
        "a contact mechanism survived the failed create that made it"
    );
    assert_eq!(
        count(&app.pool, "SELECT count(*) FROM mdm_party_statuses").await,
        0,
        "the opening status row survived a failed create"
    );
}

#[tokio::test]
async fn a_failed_update_leaves_the_previous_mechanisms_exactly_intact() {
    // The sharper of the two. `replace_contact_mechs` deletes the party's
    // existing links before it inserts the new ones, so a failure after the
    // delete and before the last insert is the shape that loses data. What must
    // hold is not "some mechanisms remain" but "the set is unchanged".
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(&token),
            party_with("PARTY-KEEP", "BILLING"),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.data()["id"].as_str().expect("id").to_owned();

    let before = app
        .get(&format!("/api/v1/master-data/parties/{id}"), Some(&token))
        .await;
    let before_mechanisms = before.data()["contactMechanisms"].clone();
    assert_eq!(
        before_mechanisms.as_array().map(Vec::len),
        Some(2),
        "the fixture should start with two mechanisms: {}",
        before.body
    );

    arm_injector(&app.pool).await;

    let response = app
        .put(
            &format!("/api/v1/master-data/parties/{id}"),
            Some(&token),
            json!({
                "contactMechanisms": [
                    {
                        "contactMechTypeId": "WEB_ADDRESS",
                        "purposeTypeId": "PUBLIC_SITE",
                        "fromDate": "2026-02-01T00:00:00Z",
                        "detail": { "url": "https://acme.example" },
                    },
                    {
                        "contactMechTypeId": "MOBILE_NUMBER",
                        "purposeTypeId": MARKER,
                        "fromDate": "2026-02-01T00:00:00Z",
                        "detail": { "telecomNumber": { "contactNumber": "555 0200" } },
                    },
                ],
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);

    let after = app
        .get(&format!("/api/v1/master-data/parties/{id}"), Some(&token))
        .await;

    assert_eq!(
        after.data()["contactMechanisms"],
        before_mechanisms,
        "the mechanism set changed across a failed update"
    );
    // And the replacement's own rows are not sitting in the table unlinked.
    assert_eq!(
        count(
            &app.pool,
            "SELECT count(*) FROM mdm_contact_mechs WHERE display_value LIKE '%acme.example%' \
             AND display_value NOT LIKE '%@%'"
        )
        .await,
        0,
        "the mechanism the failed update created was left behind"
    );
}
