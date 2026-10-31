//! A stored audit row refuses a change (FR-AUD-003, NFR-SEC-006; [#833], D-139).
//!
//! `audit_events` carried a hash chain and a promise: nothing in the application
//! updates or deletes a row. The chain detects a change, and the promise was
//! nobody's to enforce, so a connection that could write the table could rewrite
//! it. `0052_audit_events_append_only.sql` puts one statement-level trigger in
//! the way.
//!
//! **What these tests hold is the application's connection**, because
//! [`TestApp::pool`] is the connection the application serves on. They do not
//! say the table is safe from its owner: the same pool can disable the trigger,
//! which [`fixtures::write_past_the_audit_trigger`] does and [#842] addresses.
//!
//! The refusals are written out through `app.pool` on purpose. They are the only
//! `UPDATE`, `DELETE` and `TRUNCATE` on `audit_events` in this test suite that
//! do not go through that helper, and none of them lands.
//!
//! [#833]: https://github.com/sujanto-gaws/kelir/issues/833
//! [#842]: https://github.com/sujanto-gaws/kelir/issues/842

mod common;

use axum::http::StatusCode;
use common::fixtures::{self, AUDIT_APPEND_ONLY_TRIGGER};
use common::TestApp;
use serde_json::json;
use uuid::Uuid;

/// An application with audit rows behind it: the sign-in writes one and the
/// party writes another.
async fn an_app_with_audit_rows() -> TestApp {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    create_a_party(&app, &token).await;
    app
}

/// A party, through the API, so the audit row is the one a request writes.
async fn create_a_party(app: &TestApp, token: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(token),
            json!({
                "partyId": "PARTY-0001",
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": "Acme Supplies" },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("the created party carries an id")
}

/// Every stored row, every column, as one text. Comparing two of these is what
/// "the rows are still there" means: a count would pass a rewritten row.
async fn the_rows(app: &TestApp) -> String {
    let rows: String = sqlx::query_scalar(
        "SELECT coalesce(string_agg(audit::text, E'\\n' ORDER BY audit.id), '')
         FROM audit_events audit",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the audit rows read back");

    assert!(!rows.is_empty(), "no audit rows, so this proves nothing");
    rows
}

/// Runs `sql` through the application's pool, requires the refusal, and
/// requires that no row changed.
async fn assert_refused(app: &TestApp, sql: &str) {
    let before = the_rows(app).await;

    let error = sqlx::query(sql)
        .execute(&app.pool)
        .await
        .expect_err(&format!("`{sql}` landed on the audit table"));

    let message = error.to_string();
    assert!(
        message.contains("audit_events") && message.contains("append-only"),
        "`{sql}` failed, but not by the refusal: {message}"
    );

    assert_eq!(the_rows(app).await, before, "`{sql}` changed a stored row");
}

/// AC1. The trigger's shape, read from the catalogue. Everything else in this
/// file shows what it does; this shows there is one, and that it is not a
/// per-row trigger, which would let a zero-row write and a `TRUNCATE` through.
#[tokio::test]
async fn one_statement_level_trigger_guards_the_table() {
    let app = TestApp::spawn().await;

    let triggers: Vec<(String, String, i16)> = sqlx::query_as(
        "SELECT tgname::text, tgenabled::text, tgtype
         FROM pg_trigger
         WHERE tgrelid = 'audit_events'::regclass AND NOT tgisinternal",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the catalogue reads back");

    // `tgtype` is a bit field: 2 BEFORE, 8 DELETE, 16 UPDATE, 32 TRUNCATE. The
    // bits left clear are 1, which would mean per row, and 4, which would mean
    // the trigger also refused an INSERT.
    assert_eq!(
        triggers,
        vec![(AUDIT_APPEND_ONLY_TRIGGER.to_owned(), "O".to_owned(), 58)]
    );
}

/// AC2, a row that matches.
#[tokio::test]
async fn an_update_is_refused() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "UPDATE audit_events SET reason = 'x'").await;
}

/// AC2, no row that matches: a trigger per row would never have fired.
#[tokio::test]
async fn an_update_that_matches_no_row_is_refused() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "UPDATE audit_events SET reason = 'x' WHERE false").await;
}

/// AC3.
#[tokio::test]
async fn a_delete_is_refused_and_the_rows_stay() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "DELETE FROM audit_events").await;
}

/// AC3, the zero-row half.
#[tokio::test]
async fn a_delete_that_matches_no_row_is_refused() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "DELETE FROM audit_events WHERE false").await;
}

/// AC4.
#[tokio::test]
async fn a_truncate_is_refused_and_the_rows_stay() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "TRUNCATE audit_events").await;
}

/// AC4 by the other road: `audit_events` is not named, and it is reached through
/// its foreign key to `tenants`. A `TRUNCATE` fires the triggers of every table
/// it reaches, so this is refused as well, and the whole statement with it.
#[tokio::test]
async fn a_truncate_that_cascades_into_the_table_is_refused() {
    let app = an_app_with_audit_rows().await;

    assert_refused(&app, "TRUNCATE tenants CASCADE").await;
}

/// AC5. The application's own write is the `INSERT`, and the trigger names no
/// `INSERT`. The row lands and links to the row before it.
#[tokio::test]
async fn a_request_that_writes_an_audit_row_still_succeeds_and_chains() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = create_a_party(&app, &token).await;

    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT object_id, previous_hash, current_hash
         FROM audit_events
         ORDER BY created_at, id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the audit rows read back");

    let written = rows
        .iter()
        .position(|(object_id, ..)| *object_id == party)
        .expect("creating the party wrote an audit row");
    assert!(written > 0, "the sign-in's row should come before it");

    for pair in rows.windows(2) {
        assert_eq!(
            pair[1].1, pair[0].2,
            "the chain does not link: {:?} after {:?}",
            pair[1], pair[0]
        );
    }
}

/// AC7, a write that lands: the helper's write is real, and it leaves the
/// trigger on.
#[tokio::test]
async fn the_trigger_is_back_after_a_write_past_it() {
    let app = an_app_with_audit_rows().await;
    let before = the_rows(&app).await;

    let written = fixtures::write_past_the_audit_trigger(
        &app.pool,
        sqlx::query("UPDATE audit_events SET reason = 'x'"),
    )
    .await
    .expect("the write past the trigger lands");

    assert!(written.rows_affected() > 0, "the write touched no row");
    assert_ne!(the_rows(&app).await, before, "the write changed nothing");

    assert_refused(&app, "UPDATE audit_events SET reason = 'y'").await;
}

/// AC7, a write that fails: the transaction is abandoned before its `ENABLE`
/// runs, so it is the rollback that has to put the trigger back. `id` is the
/// primary key, so the `NULL` fails after the trigger has been passed.
#[tokio::test]
async fn the_trigger_is_back_after_a_failing_write_past_it() {
    let app = an_app_with_audit_rows().await;
    let before = the_rows(&app).await;

    let error = fixtures::write_past_the_audit_trigger(
        &app.pool,
        sqlx::query("UPDATE audit_events SET id = NULL"),
    )
    .await
    .expect_err("a NULL primary key cannot land");

    assert!(
        error.to_string().contains("null value in column"),
        "the write should have failed on its own terms: {error}"
    );
    assert_eq!(the_rows(&app).await, before, "the failed write left a mark");

    assert_refused(&app, "UPDATE audit_events SET reason = 'x'").await;
}
