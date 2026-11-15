//! A stored audit row refuses a change (FR-AUD-003, NFR-SEC-006; [#833], D-139).
//!
//! `audit_events` carried a hash chain and a promise: nothing in the application
//! updates or deletes a row. The chain detects a change, and the promise was
//! nobody's to enforce, so a connection that could write the table could rewrite
//! it. `0052_audit_events_append_only.sql` puts one statement-level trigger in
//! the way.
//!
//! **The trigger is held against the owner's pool**, [`TestApp::pool`], which
//! keeps every privilege on the table, so those tests prove the trigger alone.
//! The refusals are written out through `app.pool` on purpose: they are the
//! only `UPDATE`, `DELETE` and `TRUNCATE` on `audit_events` in this suite that
//! do not go through [`fixtures::write_past_the_audit_trigger`], and none of
//! them lands.
//!
//! **The serving role is held after them** ([#842], ADR-0051 §5), through
//! [`TestApp::serving_pool`], the pool the router answers requests on. It is
//! refused the three verbs on privilege, before the trigger, and every change
//! to the trigger, its function or its table that #833's campaign landed as
//! the owner. The owner can still disable the trigger, which is what the
//! fixture does and what ADR-0051 §4 leaves to the owner's credential.
//!
//! [#833]: https://github.com/sujanto-gaws/kelir/issues/833
//! [#842]: https://github.com/sujanto-gaws/kelir/issues/842

mod common;

use axum::http::StatusCode;
use common::fixtures::{self, AUDIT_APPEND_ONLY_TRIGGER};
use common::TestApp;
use kelir_backend::db;
use serde_json::json;
use sqlx::PgPool;
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

/// AC1, what `tgtype` does not hold. A `WHEN` clause leaves the bit field at 58
/// and every refusal in this file intact, and it is a way past the trigger for
/// whatever it tests: a session setting, a role, an `application_name`.
#[tokio::test]
async fn the_trigger_fires_on_no_condition() {
    let app = TestApp::spawn().await;

    let unconditional: bool = sqlx::query_scalar(
        "SELECT tgqual IS NULL
         FROM pg_trigger
         WHERE tgrelid = 'audit_events'::regclass AND tgname = $1",
    )
    .bind(AUDIT_APPEND_ONLY_TRIGGER)
    .fetch_one(&app.pool)
    .await
    .expect("the catalogue reads back");

    assert!(unconditional, "the trigger carries a WHEN clause");
}

/// AC2's refusal, whole. The function is the one `RAISE`: any branch in it is a
/// way past it, and a setting it honours is no control, so no refusal below can
/// show its absence, because none of them knows which setting to try. The text
/// also holds the verb, which a fixed message would get wrong for two of the
/// three, and leaves `RAISE`'s own `SQLSTATE`.
#[tokio::test]
async fn the_refusal_is_one_statement_with_no_way_around_it() {
    let app = TestApp::spawn().await;

    let source: String = sqlx::query_scalar(
        "SELECT prosrc FROM pg_proc WHERE oid = 'audit_events_refuse_change'::regproc",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the function reads back");

    assert_eq!(
        source.split_whitespace().collect::<Vec<_>>().join(" "),
        "BEGIN RAISE EXCEPTION 'audit_events is append-only: % is refused', TG_OP; END"
    );
}

/// AC2, every column: `UPDATE OF reason` would refuse the write above and let
/// a rewritten `current_hash` through. The columns come from the catalogue, so
/// one added later is held as well.
#[tokio::test]
async fn an_update_of_any_column_is_refused() {
    let app = an_app_with_audit_rows().await;

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT attname::text
         FROM pg_attribute
         WHERE attrelid = 'audit_events'::regclass AND attnum > 0 AND NOT attisdropped",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the columns read back");
    assert!(columns.len() > 1, "no columns, so this proves nothing");

    for column in columns {
        assert_refused(
            &app,
            &format!("UPDATE audit_events SET {column} = {column}"),
        )
        .await;
    }
}

/// AC2 and AC3 by the roads that do not open with the verb. Each fires the
/// statement-level trigger, on the versions CI and a laptop run, and nothing
/// but PostgreSQL's own rules says so: an upgrade that changed one is what
/// this would catch. `COPY` is not here because it only appends, which is the
/// forged row the migration's header says it does not stop.
#[tokio::test]
async fn a_write_by_another_road_is_refused() {
    let app = an_app_with_audit_rows().await;

    for sql in [
        "INSERT INTO audit_events
             (id, tenant_id, event_type, action, object_type, object_id,
              previous_hash, current_hash)
         SELECT id, tenant_id, event_type, action, object_type, object_id,
                previous_hash, current_hash
         FROM audit_events
         LIMIT 1
         ON CONFLICT (id) DO UPDATE SET reason = 'x'",
        "WITH changed AS (UPDATE audit_events SET reason = 'x' RETURNING id)
         SELECT count(*) FROM changed",
        "WITH removed AS (DELETE FROM audit_events RETURNING id)
         SELECT count(*) FROM removed",
        "MERGE INTO audit_events audit
         USING (SELECT id FROM audit_events) stored ON audit.id = stored.id
         WHEN MATCHED THEN UPDATE SET reason = 'x'",
        "MERGE INTO audit_events audit
         USING (SELECT id FROM audit_events) stored ON audit.id = stored.id
         WHEN MATCHED THEN DELETE",
    ] {
        assert_refused(&app, sql).await;
    }
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

// ---------------------------------------------------------------------------
// The serving role (ADR-0051 §5, #842)
//
// Everything above holds the trigger against the owner's pool, which keeps
// every privilege, so it proves the trigger alone. What follows holds the
// serving role, the one the router answers requests through: it may read and
// append to `audit_events`, and nothing else, by privilege and by ownership.
// Each of the two stops the three verbs on its own.
// ---------------------------------------------------------------------------

/// What PostgreSQL answers a statement the role holds no privilege or
/// ownership for: `insufficient_privilege`.
const INSUFFICIENT_PRIVILEGE: &str = "42501";

/// What the trigger's `RAISE` answers: `raise_exception`.
const RAISED_BY_THE_TRIGGER: &str = "P0001";

/// Runs `sql` as the serving role in a transaction that is always rolled back,
/// and returns the refusal's SQLSTATE and text, or `None` when it landed.
///
/// Rolled back whichever way it went: a statement that lands is a red probe,
/// and it must not leave the next case a different table.
async fn as_the_serving_role(app: &TestApp, sql: &str) -> Option<(String, String)> {
    as_role(&app.serving_pool, sql).await
}

/// [`as_the_serving_role`], on any role's pool.
async fn as_role(pool: &PgPool, sql: &str) -> Option<(String, String)> {
    let mut transaction = pool
        .begin()
        .await
        .expect("the role's pool opens a transaction");

    let outcome = sqlx::query(sql).execute(&mut *transaction).await;
    let _ = transaction.rollback().await;

    match outcome {
        Ok(_) => None,
        Err(error) => {
            let code = error
                .as_database_error()
                .and_then(|database| database.code())
                .map(|code| code.into_owned())
                .unwrap_or_default();

            Some((code, error.to_string()))
        }
    }
}

/// The serving role's name, as the serving pool reports it.
async fn the_serving_role(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&app.serving_pool)
        .await
        .expect("the serving role names itself")
}

/// The trigger's row, which every case below must leave as it found it.
async fn the_trigger(app: &TestApp) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT tgname::text, tgenabled::text
         FROM pg_trigger
         WHERE tgrelid = 'audit_events'::regclass AND NOT tgisinternal",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the catalogue reads back")
}

/// The probe's three verbs, each as a statement that would change or remove
/// a stored row.
const THE_THREE_VERBS: [&str; 4] = [
    "UPDATE audit_events SET reason = 'x'",
    "UPDATE audit_events SET reason = 'x' WHERE false",
    "DELETE FROM audit_events",
    "TRUNCATE audit_events",
];

/// The probe itself: each of [`THE_THREE_VERBS`] that the serving role is
/// **not** refused on privilege, with what happened instead. Empty is green.
/// The test below and its red control both read this, so the control proves
/// the probe and not a copy of it.
async fn the_verbs_not_refused_on_privilege(app: &TestApp) -> Vec<String> {
    let mut wrong = Vec::new();

    for sql in THE_THREE_VERBS {
        match as_the_serving_role(app, sql).await {
            None => wrong.push(format!("landed: {sql}")),
            Some((code, _)) if code == INSUFFICIENT_PRIVILEGE => {}
            Some((code, message)) => {
                wrong.push(format!(
                    "refused, not on privilege ({code} {message}): {sql}"
                ));
            }
        }
    }

    wrong
}

/// ADR-0051 §5, the first bullet. The three verbs are refused **on privilege**,
/// before the trigger is reached: the probe asserts which of the two refused.
#[tokio::test]
async fn the_serving_role_is_refused_the_three_verbs_on_privilege() {
    let app = an_app_with_audit_rows().await;
    let before = the_rows(&app).await;

    let wrong = the_verbs_not_refused_on_privilege(&app).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    assert_eq!(the_rows(&app).await, before, "a refused verb changed a row");
}

/// The probe above, **red with the grants widened** (plan 22 §7, row 2's
/// gate; ADR-0051 §5's first *Red*). Each verb is granted back alone, to the
/// role and then to `PUBLIC`, which the startup's narrowing does not revoke
/// from: the probe reports exactly that verb's statements, and the posture
/// check refuses the role. Revoked again, the probe is green, so each round
/// starts from the table as provisioned.
#[tokio::test]
async fn the_privilege_probe_goes_red_with_the_grants_widened() {
    let app = an_app_with_audit_rows().await;
    let role = the_serving_role(&app).await;

    assert!(
        the_verbs_not_refused_on_privilege(&app).await.is_empty(),
        "the probe is not green to begin with"
    );

    for grantee in [format!(r#""{role}""#), "PUBLIC".to_owned()] {
        for verb in ["UPDATE", "DELETE", "TRUNCATE"] {
            sqlx::query(&format!("GRANT {verb} ON audit_events TO {grantee}"))
                .execute(&app.pool)
                .await
                .expect("the owner widens the grant");

            let expected: Vec<String> = THE_THREE_VERBS
                .iter()
                .filter(|sql| sql.starts_with(verb))
                .map(|sql| (*sql).to_owned())
                .collect();
            let reported: Vec<String> = the_verbs_not_refused_on_privilege(&app)
                .await
                .into_iter()
                .filter_map(|wrong| wrong.rsplit_once(": ").map(|(_, sql)| sql.to_owned()))
                .collect();
            assert_eq!(
                reported, expected,
                "{verb} granted to {grantee}: the probe did not report it alone"
            );

            let refused = db::check_serving_posture(&app.serving_pool)
                .await
                .expect_err("the posture check passes a widened grant");
            assert!(
                refused.to_string().contains(verb),
                "{verb} granted to {grantee}: the refusal does not name it: {refused}"
            );

            sqlx::query(&format!("REVOKE {verb} ON audit_events FROM {grantee}"))
                .execute(&app.pool)
                .await
                .expect("the owner narrows the grant again");
        }
    }

    assert!(
        the_verbs_not_refused_on_privilege(&app).await.is_empty(),
        "the probe is not green again with the grants revoked"
    );
}

/// ADR-0051 §4, *either alone still holds*. With the three verbs granted back,
/// the serving role reaches the trigger, and the trigger refuses it.
#[tokio::test]
async fn with_the_verbs_granted_back_the_trigger_still_refuses_the_serving_role() {
    let app = an_app_with_audit_rows().await;
    let role = the_serving_role(&app).await;

    sqlx::query(&format!(
        r#"GRANT UPDATE, DELETE, TRUNCATE ON audit_events TO "{role}""#
    ))
    .execute(&app.pool)
    .await
    .expect("the owner widens the grant");

    for sql in [
        "UPDATE audit_events SET reason = 'x'",
        "DELETE FROM audit_events",
        "TRUNCATE audit_events",
    ] {
        let (code, message) = as_the_serving_role(&app, sql)
            .await
            .unwrap_or_else(|| panic!("`{sql}` landed past the trigger"));

        assert!(
            code == RAISED_BY_THE_TRIGGER && message.contains("append-only"),
            "`{sql}` was refused, but not by the trigger: {code} {message}"
        );
    }
}

/// #833's campaign, every statement that landed as the owner on PG17, and
/// the neighbours of each. **They need the parents
/// [`the_campaign_parents`] makes.**
fn the_campaign() -> Vec<String> {
    let name = AUDIT_APPEND_ONLY_TRIGGER;

    vec![
        format!("ALTER TABLE audit_events DISABLE TRIGGER {name}"),
        "ALTER TABLE audit_events DISABLE TRIGGER ALL".to_owned(),
        format!("DROP TRIGGER {name} ON audit_events"),
        format!("ALTER TABLE audit_events ENABLE REPLICA TRIGGER {name}"),
        "CREATE OR REPLACE FUNCTION audit_events_refuse_change() RETURNS trigger
         LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$"
            .to_owned(),
        "DROP FUNCTION audit_events_refuse_change() CASCADE".to_owned(),
        "ALTER TABLE audit_events INHERIT audit_events_parent".to_owned(),
        "ALTER TABLE audit_events_parted ATTACH PARTITION audit_events
         FOR VALUES WITH (MODULUS 1, REMAINDER 0)"
            .to_owned(),
        "ALTER TABLE audit_events DROP COLUMN reason".to_owned(),
        "ALTER TABLE audit_events RENAME TO audit_events_original".to_owned(),
        "ALTER TABLE audit_events RENAME COLUMN reason TO reason_original".to_owned(),
        "CREATE TABLE audit_events_lookalike (LIKE audit_events)".to_owned(),
        "ALTER TABLE audit_events OWNER TO CURRENT_USER".to_owned(),
        "DROP TABLE audit_events CASCADE".to_owned(),
        "SET session_replication_role = replica".to_owned(),
    ]
}

/// The owner's parents for the campaign's `INHERIT` and `ATTACH PARTITION`,
/// because the serving role cannot create one; a role that could would fail
/// the `CREATE TABLE` case instead.
async fn the_campaign_parents(app: &TestApp) {
    for setup in [
        "CREATE TABLE audit_events_parent (LIKE audit_events)",
        "CREATE TABLE audit_events_parted (LIKE audit_events) PARTITION BY HASH (id)",
    ] {
        sqlx::query(setup)
            .execute(&app.pool)
            .await
            .unwrap_or_else(|error| panic!("the owner's `{setup}`: {error}"));
    }
}

/// The probe for the campaign: each statement of [`the_campaign`] that
/// `pool`'s role is **not** refused on privilege. Empty is green. Each runs in
/// a transaction that is rolled back.
async fn the_campaign_not_refused_on_privilege(pool: &PgPool) -> Vec<String> {
    let mut landed = Vec::new();

    for sql in the_campaign() {
        match as_role(pool, &sql).await {
            None => landed.push(format!("landed: {sql}")),
            Some((code, _)) if code == INSUFFICIENT_PRIVILEGE => {}
            Some((code, message)) => {
                landed.push(format!(
                    "refused, not on privilege ({code} {message}): {sql}"
                ));
            }
        }
    }

    landed
}

/// ADR-0051 §5, the second bullet, and #833's campaign, which #842's comments
/// read into its *Done when*. Each of these landed **as the owner** on PG17,
/// several with the trigger still enabled. As the serving role every one is
/// refused for want of ownership or of `CREATE`, and the trigger and the rows
/// are as they were.
///
/// The parents for `INHERIT` and `ATTACH PARTITION` are the owner's, because
/// the serving role cannot create one; a role that could would fail the
/// `CREATE TABLE` case instead.
#[tokio::test]
async fn the_serving_role_cannot_change_the_trigger_its_function_or_its_table() {
    let app = an_app_with_audit_rows().await;
    the_campaign_parents(&app).await;

    let rows = the_rows(&app).await;
    let trigger = the_trigger(&app).await;

    let landed = the_campaign_not_refused_on_privilege(&app.serving_pool).await;

    assert!(
        landed.is_empty(),
        "the serving role is not refused:\n{}",
        landed.join("\n")
    );
    assert_eq!(the_trigger(&app).await, trigger, "the trigger changed");
    assert_eq!(the_rows(&app).await, rows, "a stored row changed");
}

/// The campaign above, **red with the serving role made a member of the
/// owner** (ADR-0051 §5's second *Red*). A scratch role, provisioned and
/// narrowed as the serving role is, is green first; granted the owner, it
/// inherits ownership, and the trigger is disabled, dropped and replaced
/// through it. Every statement runs in a rolled-back transaction, so the
/// trigger is as it was at the end.
#[tokio::test]
async fn the_campaign_goes_red_with_the_serving_role_a_member_of_the_owner() {
    let app = an_app_with_audit_rows().await;
    the_campaign_parents(&app).await;
    let trigger = the_trigger(&app).await;

    let owner: String = sqlx::query_scalar("SELECT quote_ident(current_user)")
        .fetch_one(&app.pool)
        .await
        .expect("the owner names itself");
    let scratch = app.scratch_serving_role().await;

    let green = the_campaign_not_refused_on_privilege(&scratch.pool).await;
    assert!(
        green.is_empty(),
        "not green to begin with:\n{}",
        green.join("\n")
    );

    sqlx::query(&format!("GRANT {owner} TO {}", scratch.name))
        .execute(&app.pool)
        .await
        .expect("the owner admits the scratch role");

    let landed = the_campaign_not_refused_on_privilege(&scratch.pool).await;
    let name = AUDIT_APPEND_ONLY_TRIGGER;

    for sql in [
        format!("landed: ALTER TABLE audit_events DISABLE TRIGGER {name}"),
        format!("landed: DROP TRIGGER {name} ON audit_events"),
        "landed: DROP FUNCTION audit_events_refuse_change() CASCADE".to_owned(),
        "landed: ALTER TABLE audit_events DROP COLUMN reason".to_owned(),
    ] {
        assert!(
            landed.contains(&sql),
            "the campaign did not go red at `{sql}`:\n{}",
            landed.join("\n")
        );
    }

    scratch.drop_from(&app).await;
    assert_eq!(the_trigger(&app).await, trigger, "the trigger changed");
}

/// One table's effective privileges, as the serving role holds them.
#[derive(Debug, sqlx::FromRow)]
struct TablePrivileges {
    table_name: String,
    can_select: bool,
    can_insert: bool,
    can_update: bool,
    can_delete: bool,
    can_truncate: bool,
    can_reference: bool,
    can_trigger: bool,
}

impl TablePrivileges {
    /// The privileges held, named as `GRANT` names them, in a fixed order.
    fn held(&self) -> Vec<&'static str> {
        [
            ("SELECT", self.can_select),
            ("INSERT", self.can_insert),
            ("UPDATE", self.can_update),
            ("DELETE", self.can_delete),
            ("TRUNCATE", self.can_truncate),
            ("REFERENCES", self.can_reference),
            ("TRIGGER", self.can_trigger),
        ]
        .into_iter()
        .filter_map(|(privilege, granted)| granted.then_some(privilege))
        .collect()
    }
}

/// One sequence's effective privileges, as the serving role holds them.
#[derive(sqlx::FromRow)]
struct SequencePrivileges {
    sequence_name: String,
    can_use: bool,
    can_select: bool,
    can_update: bool,
}

/// ADR-0051 §5, the third bullet. Every table but the two is the serving
/// role's to read and write, and no table gives it `TRUNCATE`, `REFERENCES` or
/// `TRIGGER`. Read as effective privilege, so a grant through `PUBLIC` or a
/// membership counts as well as a grant to the role.
#[tokio::test]
async fn the_serving_role_holds_exactly_its_grants_on_every_table() {
    let app = TestApp::spawn().await;

    let tables: Vec<TablePrivileges> = sqlx::query_as(
        "SELECT c.relname::text AS table_name,
                has_table_privilege(c.oid, 'SELECT') AS can_select,
                has_table_privilege(c.oid, 'INSERT') AS can_insert,
                has_table_privilege(c.oid, 'UPDATE') AS can_update,
                has_table_privilege(c.oid, 'DELETE') AS can_delete,
                has_table_privilege(c.oid, 'TRUNCATE') AS can_truncate,
                has_table_privilege(c.oid, 'REFERENCES') AS can_reference,
                has_table_privilege(c.oid, 'TRIGGER') AS can_trigger
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace AND c.relkind IN ('r', 'p')
         ORDER BY 1",
    )
    .fetch_all(&app.serving_pool)
    .await
    .expect("the serving role reads its own privileges");
    assert!(tables.len() > 2, "no tables, so this proves nothing");

    let mut wrong = Vec::new();

    for table in tables {
        let held = table.held();
        let expected: &[&str] = match table.table_name.as_str() {
            "audit_events" => &["SELECT", "INSERT"],
            "_sqlx_migrations" => &["SELECT"],
            _ => &["SELECT", "INSERT", "UPDATE", "DELETE"],
        };

        if held != expected {
            wrong.push(format!(
                "{}: holds {held:?}, expected {expected:?}",
                table.table_name
            ));
        }
    }

    let sequences: Vec<SequencePrivileges> = sqlx::query_as(
        "SELECT c.relname::text AS sequence_name,
                has_sequence_privilege(c.oid, 'USAGE') AS can_use,
                has_sequence_privilege(c.oid, 'SELECT') AS can_select,
                has_sequence_privilege(c.oid, 'UPDATE') AS can_update
         FROM pg_class c
         WHERE c.relnamespace = 'public'::regnamespace AND c.relkind = 'S'
         ORDER BY 1",
    )
    .fetch_all(&app.serving_pool)
    .await
    .expect("the serving role reads its sequence privileges");

    for sequence in sequences {
        if !(sequence.can_use && sequence.can_select) || sequence.can_update {
            wrong.push(format!(
                "sequence {}: USAGE {}, SELECT {}, UPDATE {}",
                sequence.sequence_name, sequence.can_use, sequence.can_select, sequence.can_update
            ));
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What the serving role is, beyond its grants. Every field is `false` for a
/// role that may serve.
#[derive(Debug, Default, PartialEq, sqlx::FromRow)]
struct RolePosture {
    superuser: bool,
    member_of_the_owner: bool,
    member_of_a_role: bool,
    creates_in_public: bool,
    creates_in_the_database: bool,
    owns_an_object: bool,
}

/// ADR-0051 §2, the serving role itself: not a superuser, owner of nothing,
/// member of no role, with no `CREATE` on the database or on schema `public`.
#[tokio::test]
async fn the_serving_role_owns_nothing_and_may_create_nothing() {
    let app = TestApp::spawn().await;

    let posture: RolePosture = sqlx::query_as(
        "SELECT r.rolsuper AS superuser,
                pg_has_role(current_user, c.relowner, 'MEMBER') AS member_of_the_owner,
                EXISTS (SELECT 1 FROM pg_auth_members m WHERE m.member = r.oid)
                    AS member_of_a_role,
                has_schema_privilege('public', 'CREATE') AS creates_in_public,
                has_database_privilege(current_database(), 'CREATE') AS creates_in_the_database,
                EXISTS (SELECT 1 FROM pg_class o WHERE o.relowner = r.oid)
                    OR EXISTS (SELECT 1 FROM pg_proc p WHERE p.proowner = r.oid)
                    OR EXISTS (SELECT 1 FROM pg_namespace n WHERE n.nspowner = r.oid)
                    OR EXISTS (SELECT 1 FROM pg_type t WHERE t.typowner = r.oid)
                    OR EXISTS (
                        SELECT 1 FROM pg_database d
                        WHERE d.datname = current_database() AND d.datdba = r.oid
                    ) AS owns_an_object
         FROM pg_roles r, pg_class c
         WHERE r.rolname = current_user AND c.oid = 'audit_events'::regclass",
    )
    .fetch_one(&app.serving_pool)
    .await
    .expect("the serving role reads its own posture");

    assert_eq!(posture, RolePosture::default());
}

/// ADR-0051 §2, *future tables are granted by default privileges*: a table the
/// owner creates later, as a later migration would, is the serving role's to
/// read and write without anybody naming the role.
#[tokio::test]
async fn a_table_the_owner_creates_later_is_the_serving_roles_to_read_and_write() {
    let app = TestApp::spawn().await;

    sqlx::query("CREATE TABLE a_later_table (id integer PRIMARY KEY)")
        .execute(&app.pool)
        .await
        .expect("the owner creates a table");

    let table: TablePrivileges = sqlx::query_as(
        "SELECT c.relname::text AS table_name,
                has_table_privilege(c.oid, 'SELECT') AS can_select,
                has_table_privilege(c.oid, 'INSERT') AS can_insert,
                has_table_privilege(c.oid, 'UPDATE') AS can_update,
                has_table_privilege(c.oid, 'DELETE') AS can_delete,
                has_table_privilege(c.oid, 'TRUNCATE') AS can_truncate,
                has_table_privilege(c.oid, 'REFERENCES') AS can_reference,
                has_table_privilege(c.oid, 'TRIGGER') AS can_trigger
         FROM pg_class c
         WHERE c.oid = 'a_later_table'::regclass",
    )
    .fetch_one(&app.serving_pool)
    .await
    .expect("the serving role reads its privileges");

    assert_eq!(table.held(), ["SELECT", "INSERT", "UPDATE", "DELETE"]);
}
