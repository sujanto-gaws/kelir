//! Searching the audit trail (FR-AUD-004; [#252]).
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Every mutation below was run against this file and the reddened test named,
//! on 2026-09-01:
//!
//! - **M1** — `redact_for` returns the row unchanged, which is **D-12**
//!   ungeneralized. Red: *a caller who may not read the object may not read its
//!   values*.
//! - **M2** — `readable_by`'s `_ => return None` arm changed to return
//!   `Some("audit:read")`, so an unplaceable type opens on the surface's own
//!   permission. Red: *an object type nobody has placed withholds*.
//! - **M3** — `tenant_id = $1` dropped from `search`. Red: *another tenant's
//!   trail is not in this one*.
//! - **M4** — `caller.require(AUDIT_READ)?` deleted from `search_audit`. Red:
//!   *searching the trail needs its own permission*.
//! - **M5** — `ORDER BY created_at DESC, id DESC` reduced to `created_at DESC`.
//!   Red: *a page boundary inside one transaction neither repeats nor skips*.
//! - **M6** — `redact_for` drops the row instead of its values. Red: *a
//!   withheld row is still counted and still returned*.
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A person holding exactly the permissions named.
async fn person(app: &TestApp, username: &str, permissions: &[&str]) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        permissions,
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

/// A document type, which writes a `DOCUMENT_TYPE` audit row with values.
async fn document_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Writes one audit row straight into the table, which is how a test reaches an
/// object type no fixture in this file produces.
async fn audit_row(
    app: &TestApp,
    tenant: Uuid,
    object_type: &str,
    object_id: Uuid,
    new_value: Value,
) {
    sqlx::query(
        "INSERT INTO audit_events \
         (id, tenant_id, event_type, action, object_type, object_id, new_value_json, \
          previous_hash, current_hash) \
         VALUES ($1, $2, $3, 'UPDATE', $4, $5, $6, 'sha256:none', 'sha256:none')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(format!("{object_type}.Updated"))
    .bind(object_type)
    .bind(object_id)
    .bind(new_value)
    .execute(&app.pool)
    .await
    .expect("the audit row");
}

fn rows(body: &Value) -> &Vec<Value> {
    body["data"].as_array().expect("a page")
}

fn row_for<'a>(body: &'a Value, object_type: &str) -> &'a Value {
    rows(body)
        .iter()
        .find(|row| row["objectType"] == object_type)
        .unwrap_or_else(|| panic!("no `{object_type}` row: {body}"))
}

// ---------------------------------------------------------------------------
// AC1 — searchable by actor, object type, object id, event type and date
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_trail_is_searchable_by_each_of_its_axes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let type_id = document_type(&app, &token, "PR_AUDIT_AXES").await;

    let all = app.get("/api/v1/audit", Some(&token)).await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);
    assert!(
        !rows(&all.body).is_empty(),
        "the trail is empty: {}",
        all.body
    );

    // By object id — the narrowest, and the one that proves the rest are
    // filtering rather than the trail happening to hold only this.
    let by_object = app
        .get(
            &format!("/api/v1/audit?objectId={type_id}&objectType=DOCUMENT_TYPE"),
            Some(&token),
        )
        .await;
    assert_eq!(by_object.status, StatusCode::OK, "{}", by_object.body);
    assert!(!rows(&by_object.body).is_empty());
    for row in rows(&by_object.body) {
        assert_eq!(row["objectType"], "DOCUMENT_TYPE");
        assert_eq!(row["objectId"], Value::String(type_id.to_string()));
    }

    // By event type.
    let event_type = row_for(&by_object.body, "DOCUMENT_TYPE")["eventType"]
        .as_str()
        .expect("an event type")
        .to_owned();

    let by_event = app
        .get(
            &format!("/api/v1/audit?eventType={event_type}"),
            Some(&token),
        )
        .await;
    assert!(!rows(&by_event.body).is_empty());
    for row in rows(&by_event.body) {
        assert_eq!(row["eventType"], event_type.as_str());
    }

    // By a range that excludes everything, which is the half that shows the
    // bound is applied rather than ignored.
    let none = app
        .get("/api/v1/audit?to=2000-01-01T00:00:00Z", Some(&token))
        .await;
    assert_eq!(none.status, StatusCode::OK, "{}", none.body);
    assert!(
        rows(&none.body).is_empty(),
        "a date bound in the past matched rows: {}",
        none.body
    );
    assert_eq!(none.body["meta"]["total"], 0);
}

/// A range that ends before it starts selects nothing, and is refused rather
/// than answered with an empty page — the two are different mistakes.
#[tokio::test]
async fn an_inverted_range_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .get(
            "/api/v1/audit?from=2026-06-01T00:00:00Z&to=2026-01-01T00:00:00Z",
            Some(&token),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "RANGE_INVERTED"
    );
}

// ---------------------------------------------------------------------------
// AC2 — D-12's rule, for every object type
// ---------------------------------------------------------------------------

/// **A caller who may not read the object may not read its values** (#252 AC2,
/// **D-49**), and **the row is still there**.
///
/// This is **D-12** generalized: that decision found a party's own field values
/// reachable through its change history by a caller refused `GET /parties/{id}`.
/// The same rule, across every object type, is what this surface owes.
///
/// **Two object types and one caller who can read one of them**, which is the
/// second subject coding standard §2.9 asks for: with one type, *withheld
/// because the caller lacks the permission* and *withheld always* are the same
/// assertion.
///
/// **Seen red (M1)** with `redact_for` returning the row unchanged: the
/// document type's configuration comes back to somebody with no
/// `document-type:read`.
#[tokio::test]
async fn a_caller_who_may_not_read_the_object_may_not_read_its_values() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    document_type(&app, &token, "PR_AUDIT_REDACT").await;
    let party = Uuid::now_v7();
    audit_row(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        "PARTY",
        party,
        json!({ "partyCode": "SUPP-0001", "statusId": "SUSPENDED" }),
    )
    .await;

    // Holds the audit permission and `document-type:read`, and no master-data
    // permission at all.
    let auditor = person(&app, "audit-partial", &["audit:read", "document-type:read"]).await;

    let listed = app.get("/api/v1/audit", Some(&auditor)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    // The type they may read: values served.
    let readable = row_for(&listed.body, "DOCUMENT_TYPE");
    assert_eq!(readable["valuesWithheld"], false);
    assert!(
        !readable["newValue"].is_null(),
        "the values of an object this caller may read were withheld: {readable}"
    );

    // The party they may not: values withheld, row present.
    let withheld = row_for(&listed.body, "PARTY");
    assert_eq!(
        withheld["valuesWithheld"], true,
        "a party's field values reached a caller with no master-data permission: {withheld}"
    );
    assert!(withheld["newValue"].is_null());
    assert!(withheld["oldValue"].is_null());
    assert_eq!(
        withheld["objectId"],
        Value::String(party.to_string()),
        "the row must still say what was acted on"
    );
    assert!(
        !listed.body.to_string().contains("SUPP-0001"),
        "the withheld payload is somewhere else in the response: {}",
        listed.body
    );
}

/// **The row is withheld, never hidden** (#252 AC2), and the count says so too.
///
/// A search that dropped the rows would teach an auditor the trail is shorter
/// than it is — which is worse than one that says *something happened here and
/// you may not see what*.
///
/// **Seen red (M6)** with `redact_for` filtering the row out instead: the page
/// loses the row and disagrees with `meta.total`, which is drawn under the
/// unfiltered predicate.
#[tokio::test]
async fn a_withheld_row_is_still_counted_and_still_returned() {
    let app = TestApp::spawn().await;

    let party = Uuid::now_v7();
    audit_row(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        "PARTY",
        party,
        json!({ "partyCode": "SUPP-0002" }),
    )
    .await;

    let auditor = person(&app, "audit-nothing-else", &["audit:read"]).await;

    let listed = app
        .get(
            &format!("/api/v1/audit?objectId={party}&objectType=PARTY"),
            Some(&auditor),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        rows(&listed.body).len(),
        1,
        "the row was hidden rather than withheld: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 1);
    assert_eq!(rows(&listed.body)[0]["valuesWithheld"], true);
}

/// **An object type nobody has placed withholds** (**D-49**).
///
/// A row written by a later release or a plugin is served as an event with no
/// contents, rather than as contents nobody decided about — the safe direction,
/// and the same choice `activity::domain::disclosable` makes.
///
/// **Seen red (M2)** with `readable_by`'s fallback returning the surface's own
/// permission: an unplaceable type opens to everybody who may search.
#[tokio::test]
async fn an_object_type_nobody_has_placed_withholds() {
    let app = TestApp::spawn().await;

    let object = Uuid::now_v7();
    audit_row(
        &app,
        fixtures::SYSTEM_TENANT_ID,
        "SOMETHING_A_PLUGIN_WROTE",
        object,
        json!({ "secret": "and it stays that way" }),
    )
    .await;

    // The administrator holds every permission in the catalogue, so if anything
    // could open this row it would be this caller.
    let token = app.administrator_token().await;

    let listed = app
        .get(&format!("/api/v1/audit?objectId={object}"), Some(&token))
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let row = &rows(&listed.body)[0];
    assert_eq!(
        row["valuesWithheld"], true,
        "a type this build cannot place served its values: {row}"
    );
    assert!(
        !listed.body.to_string().contains("and it stays that way"),
        "{}",
        listed.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — the permission, and that it is not the master-data one
// ---------------------------------------------------------------------------

/// **`audit:read` opens this surface, and `master-data:audit:read` does not.**
///
/// The two are separate on purpose (#252 AC3, **D-49**): one opens a party's
/// own change history, the other the whole trail. A permission named for one
/// module governing all of them is what AC3 asks to be avoided.
///
/// **Seen red (M4)** with the `require` deleted from `search_audit`.
#[tokio::test]
async fn searching_the_trail_needs_its_own_permission() {
    let app = TestApp::spawn().await;

    let master_data_only = person(
        &app,
        "audit-md-only",
        &["master-data:audit:read", "master-data:party:read"],
    )
    .await;

    for route in ["/api/v1/audit", "/api/v1/audit/object-types"] {
        let refused = app.get(route, Some(&master_data_only)).await;

        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN,
            "{route} opened to `master-data:audit:read`, which is not this \
             surface's permission: {}",
            refused.body
        );
    }

    let auditor = person(&app, "audit-proper", &["audit:read"]).await;
    let allowed = app.get("/api/v1/audit", Some(&auditor)).await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.body);
}

// ---------------------------------------------------------------------------
// AC4 — paginated and totally ordered
// ---------------------------------------------------------------------------

/// **A page boundary neither repeats a row nor skips one** (#252 AC4).
///
/// `created_at` alone is not a total order: rows written in one transaction
/// share a timestamp, and a boundary landing inside such a group is where the
/// defect appears — `workflow_history`'s lesson from #181.
///
/// # The fixture, not the assertion, is what makes the defect visible
///
/// Two earlier versions of this test survived `id DESC` being removed, and both
/// failures were the same mistake: **the data could not tell the two orders
/// apart.**
///
/// 1. Asserting the three ids were *distinct* passes under any ordering that
///    happens to be stable, and a broken one usually is. Distinctness is a
///    property luck has too.
/// 2. Asserting the sequence was `id DESC` **still** passed, because the rows
///    were written with ascending ids: `idx_audit_events_object` is
///    `(object_type, object_id, created_at)`, so a backward scan for
///    `created_at DESC` walks the tied group in reverse insertion order — which
///    for ascending ids *is* `id DESC`. The mutation produced the right answer
///    for the wrong reason.
///
/// So the ids below are **fixed, and written in an order that is not their
/// own**: `C`, then `A`, then `B`. `created_at DESC, id DESC` must answer
/// `C, B, A`; a backward index scan answers `B, A, C`. Now the two orders
/// disagree and the test can see which one it got.
///
/// **Seen red (M5)**, 2026-09-01, with `id DESC` removed.
#[tokio::test]
async fn a_page_boundary_inside_one_transaction_neither_repeats_nor_skips() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let object = Uuid::now_v7();

    // Fixed ids, and **written in the order C, A, B** so that insertion order,
    // heap order and id order are three different things.
    let a: Uuid = "00000000-0000-4000-8000-0000000000aa"
        .parse()
        .expect("a uuid");
    let b: Uuid = "00000000-0000-4000-8000-0000000000bb"
        .parse()
        .expect("a uuid");
    let c: Uuid = "00000000-0000-4000-8000-0000000000cc"
        .parse()
        .expect("a uuid");

    for id in [c, a, b] {
        sqlx::query(
            "INSERT INTO audit_events \
             (id, tenant_id, event_type, action, object_type, object_id, previous_hash, \
              current_hash, created_at) \
             VALUES ($1, $2, 'Same.Instant', 'UPDATE', 'SAME_INSTANT', $3, \
                     'sha256:none', 'sha256:none', '2026-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(object)
        .execute(&app.pool)
        .await
        .expect("a row sharing the instant");
    }

    let mut seen = Vec::new();

    for page in 1..=3 {
        let listed = app
            .get(
                &format!("/api/v1/audit?objectType=SAME_INSTANT&pageSize=1&page={page}"),
                Some(&token),
            )
            .await;

        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

        let page_rows = rows(&listed.body);
        assert_eq!(page_rows.len(), 1, "page {page}: {}", listed.body);
        seen.push(page_rows[0]["id"].as_str().expect("an id").to_owned());
    }

    // Distinct — no row on two pages, none skipped.
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "a row appeared on two pages, so the order is not total: {seen:?}"
    );

    // And in the order the statement promises. Written C, A, B; `id DESC` is
    // C, B, A; a backward index scan would be B, A, C.
    assert_eq!(
        seen,
        vec![c.to_string(), b.to_string(), a.to_string()],
        "tied rows did not come back in `id DESC`, so the tie-break is not \
         being applied and a page boundary is at the planner's discretion"
    );
}

// ---------------------------------------------------------------------------
// AC6 — nothing here can edit or delete a row, asserted over the columns
// ---------------------------------------------------------------------------

/// **Over `information_schema`, not over the router** (#252 AC6).
///
/// A route that does not exist today is one somebody adds tomorrow. What makes
/// this table append-only is that an edit has nothing to stamp and a soft
/// delete nowhere to write — a fact about the columns, which is how #181 AC6
/// and #247 AC4 asserted the same property one table over.
#[tokio::test]
async fn the_trail_is_append_only_by_its_columns() {
    let app = TestApp::spawn().await;

    for column in ["updated_at", "updated_by", "deleted_at"] {
        let present: Option<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'audit_events' AND column_name = $1",
        )
        .bind(column)
        .fetch_optional(&app.pool)
        .await
        .expect("the column list");

        assert!(
            present.is_none(),
            "`audit_events.{column}` exists, so this table is no longer append-only"
        );
    }
}

/// **And the surface offers no way to write one.** The router carries `GET`
/// only, so a `POST`, `PUT`, `PATCH` or `DELETE` is refused by axum before any
/// handler sees it.
#[tokio::test]
async fn the_search_surface_offers_no_write() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
        let refused = app
            .send(
                method.clone(),
                "/api/v1/audit",
                Some(&token),
                Some(json!({})),
            )
            .await;

        assert_eq!(
            refused.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} on the audit search was not refused: {}",
            refused.body
        );
    }
}

// ---------------------------------------------------------------------------
// Tenant scope
// ---------------------------------------------------------------------------

/// **Another tenant's trail is not in this one**, refused by the query.
///
/// **Seen red (M3)** with `tenant_id = $1` dropped from `search`.
#[tokio::test]
async fn another_tenants_trail_is_not_in_this_one() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other = fixtures::create_tenant(&app.pool, "OTHER-AUDIT", "Another tenant").await;
    let object = Uuid::now_v7();

    audit_row(
        &app,
        other,
        "PARTY",
        object,
        json!({ "partyCode": "THEIRS" }),
    )
    .await;

    let listed = app
        .get(&format!("/api/v1/audit?objectId={object}"), Some(&token))
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        rows(&listed.body).is_empty(),
        "another tenant's audit row reached this trail: {}",
        listed.body
    );
    assert_eq!(
        listed.body["meta"]["total"], 0,
        "and the count is drawn under the same rule as the page"
    );
}

/// The object-type control offers what this tenant's trail holds.
#[tokio::test]
async fn the_object_types_come_from_the_rows() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    document_type(&app, &token, "PR_AUDIT_TYPES").await;

    let listed = app.get("/api/v1/audit/object-types", Some(&token)).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let types = listed.body["data"].as_array().expect("the list");
    assert!(
        types.iter().any(|value| value == "DOCUMENT_TYPE"),
        "the trail holds DOCUMENT_TYPE rows and the control does not offer it: {}",
        listed.body
    );
}
