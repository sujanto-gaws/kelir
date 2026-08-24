//! Reading back what happened to a master-data record (FR-MDM-009; issue #100).
//!
//! The write path shipped with #80's first endpoint. What is under test here is
//! the ability to *ask* — and, more than anything else in this item, that
//! asking does not become a way around a permission. #81 keeps a party's roles
//! and profiles from a caller without `master-data:party-role:read`, and a role
//! assignment's audit record names the role type: **the audit surface must not
//! leak what the aggregate withholds** (#100 AC3), which is the single most
//! likely defect in the item and has its own tests below.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";
const FACILITIES: &str = "/api/v1/master-data/facilities";
const AUDIT_READ: &str = "master-data:audit:read";
const ROLE_READ: &str = "master-data:party-role:read";
const PARTY_READ: &str = "master-data:party:read";
const FACILITY_READ: &str = "master-data:facility:read";
const PASSWORD: &str = "audit-caller-password";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

async fn given_party(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .post(
            PARTIES,
            Some(token),
            json!({
                "partyId": code,
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": "Acme" },
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id")
}

async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-AUDIT-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.audit{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("audit{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

fn actions(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("data is not a list: {body}"))
        .iter()
        .map(|row| {
            row["action"]
                .as_str()
                .unwrap_or_else(|| panic!("a record carries no action: {row}"))
                .to_owned()
        })
        .collect()
}

/// A party with one of every kind of change against it: created, edited, its
/// status changed, a role assigned, the role removed, and a lifecycle
/// transition.
async fn a_party_with_a_history(app: &TestApp, token: &str) -> Uuid {
    let party = given_party(app, token, "PARTY-ACME").await;

    app.put(
        &format!("{PARTIES}/{party}"),
        Some(token),
        json!({ "description": "Preferred supplier" }),
    )
    .await;
    app.put(
        &format!("{PARTIES}/{party}"),
        Some(token),
        json!({ "statusId": "PARTY_DISABLED", "statusComments": "under review" }),
    )
    .await;
    app.put(
        &format!("{PARTIES}/{party}/roles/SUPPLIER"),
        Some(token),
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "profile": { "supplier": { "supplierNumber": "SUP-0001", "bankAccount": "1234567890" } },
        }),
    )
    .await;
    app.delete(&format!("{PARTIES}/{party}/roles/SUPPLIER"), Some(token))
        .await;
    app.post(
        &format!("{PARTIES}/{party}/transition"),
        Some(token),
        json!({ "recordStatusId": "ACTIVE", "reason": "onboarding complete" }),
    )
    .await;

    party
}

// ---------------------------------------------------------------------------
// Everything that was recorded reads back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_change_the_module_records_is_readable_against_the_record_it_happened_to() {
    // #100 AC1. One assertion per write path, so a path that stops auditing is
    // caught here rather than discovered when somebody asks.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = a_party_with_a_history(&app, &token).await;

    let history = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&token))
        .await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);

    assert_eq!(
        actions(&history.body),
        vec![
            "CREATE",
            "UPDATE",
            "STATUS_CHANGE",
            "ROLE_ASSIGNED",
            "ROLE_REMOVED",
            "RECORD_STATUS_CHANGE",
        ],
        "the history is not what happened, in the order it happened: {}",
        history.body
    );
    assert_eq!(history.body["meta"]["total"], 6, "{}", history.body);
}

#[tokio::test]
async fn a_record_carries_who_when_and_both_ends_of_what_changed() {
    // #100 AC2. `oldValue` and `newValue` are what make a record answer a
    // question rather than announce an event.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;

    app.put(
        &format!("{PARTIES}/{party}"),
        Some(&token),
        json!({ "description": "Preferred supplier" }),
    )
    .await;

    let history = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&token))
        .await;
    let update = history.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .find(|record| record["action"] == "UPDATE")
        .expect("the update is on the record")
        .clone();

    assert!(update["actorUserId"].is_string(), "{update}");
    assert!(
        update["actorUsername"].is_string(),
        "the actor is not resolved to a name: {update}"
    );
    assert!(update["occurredAt"].is_string(), "{update}");
    assert!(update["oldValue"].is_object(), "no before: {update}");
    assert_eq!(update["newValue"]["description"], "Preferred supplier");
}

#[tokio::test]
async fn a_reason_travels_with_the_change_it_explains() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;

    app.post(
        &format!("{PARTIES}/{party}/transition"),
        Some(&token),
        json!({ "recordStatusId": "ACTIVE", "reason": "onboarding complete" }),
    )
    .await;

    let history = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&token))
        .await;
    let transition = history.body["data"]
        .as_array()
        .expect("data")
        .iter()
        .find(|record| record["action"] == "RECORD_STATUS_CHANGE")
        .expect("the transition is on the record")
        .clone();

    assert_eq!(transition["reason"], "onboarding complete", "{transition}");
}

#[tokio::test]
async fn a_facility_has_a_history_too() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            FACILITIES,
            Some(&token),
            json!({ "facilityId": "FAC-HQ", "name": "Head Office", "facilityTypeId": "BUILDING" }),
        )
        .await;
    let facility = created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id");

    app.put(
        &format!("{FACILITIES}/{facility}"),
        Some(&token),
        json!({ "name": "Head Office (North)" }),
    )
    .await;

    let history = app
        .get(&format!("{FACILITIES}/{facility}/audit"), Some(&token))
        .await;

    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    assert_eq!(actions(&history.body), vec!["CREATE", "UPDATE"]);
}

// ---------------------------------------------------------------------------
// The surface does not leak what the aggregate withholds (#100 AC3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_history_withholds_the_role_records_without_the_role_read_permission() {
    // The single most likely defect in this item. `master-data:audit:read`
    // alone would otherwise put "this party is a supplier" one URL away from
    // the permission #81 introduced to refuse exactly that.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = a_party_with_a_history(&app, &token).await;

    let auditor = caller_holding(&app, &[AUDIT_READ, PARTY_READ], 1).await;
    let history = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&auditor))
        .await;

    assert_eq!(history.status, StatusCode::OK, "{}", history.body);

    let seen = actions(&history.body);
    for withheld in ["ROLE_ASSIGNED", "ROLE_UPDATED", "ROLE_REMOVED"] {
        assert!(
            !seen.contains(&withheld.to_owned()),
            "a caller without {ROLE_READ} was shown {withheld}: {}",
            history.body
        );
    }

    // Not a filtered page with holes in it: the total counts what the caller
    // can see, or the two disagree and the client renders a page that is short.
    assert_eq!(
        history.body["meta"]["total"],
        seen.len(),
        "the total counts rows the caller cannot see: {}",
        history.body
    );

    // And nothing that came back names the role or the profile behind it.
    let body = history.body.to_string();
    for secret in ["SUPPLIER", "SUP-0001", "1234567890"] {
        assert!(
            !body.contains(secret),
            "the history handed back {secret} without {ROLE_READ}: {body}"
        );
    }

    // What it *does* show is the party's own history, which this caller may
    // read. The gate must not swallow the records it is not for.
    assert_eq!(
        seen,
        vec!["CREATE", "UPDATE", "STATUS_CHANGE", "RECORD_STATUS_CHANGE"],
        "{}",
        history.body
    );
}

#[tokio::test]
async fn the_history_shows_the_role_records_to_a_caller_who_may_read_roles() {
    // The other half: the gate is a gate, not a removal.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = a_party_with_a_history(&app, &token).await;

    let auditor = caller_holding(&app, &[AUDIT_READ, PARTY_READ, ROLE_READ], 2).await;
    let history = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&auditor))
        .await;

    let seen = actions(&history.body);
    assert!(
        seen.contains(&"ROLE_ASSIGNED".to_owned()),
        "{}",
        history.body
    );
    assert!(
        seen.contains(&"ROLE_REMOVED".to_owned()),
        "{}",
        history.body
    );
    assert_eq!(history.body["meta"]["total"], 6, "{}", history.body);
}

#[tokio::test]
async fn the_history_never_carries_the_hash_chain() {
    // #100 AC7. `previousHash` and `currentHash` make tampering detectable and
    // nothing verifies them until FR-AUD-003. Publishing them would let a
    // client show "verified" beside a chain nobody checked.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = a_party_with_a_history(&app, &token).await;

    let body = app
        .get(&format!("{PARTIES}/{party}/audit"), Some(&token))
        .await
        .body
        .to_string();

    for absent in ["previousHash", "currentHash", "sha256:"] {
        assert!(
            !body.contains(absent),
            "the history published {absent}: {body}"
        );
    }
}

// ---------------------------------------------------------------------------
// Permission, paging, tenancy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_history_needs_its_own_permission() {
    // #100 AC6, in the shape #58 established: holding every other master-data
    // permission is not enough.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = given_party(&app, &token, "PARTY-ACME").await;

    let everything_else = caller_holding(
        &app,
        &[PARTY_READ, "master-data:party:update", ROLE_READ],
        3,
    )
    .await;

    assert_eq!(
        app.get(&format!("{PARTIES}/{party}/audit"), Some(&everything_else))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Since #136 the surface needs the record's own read permission alongside
    // it, so the caller who is allowed through holds both. That the second one
    // is genuinely required is
    // `the_history_needs_the_records_own_read_permission_too`.
    let auditor = caller_holding(&app, &[AUDIT_READ, PARTY_READ], 4).await;
    assert_eq!(
        app.get(&format!("{PARTIES}/{party}/audit"), Some(&auditor))
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_history_needs_the_records_own_read_permission_too() {
    // #136. A record's history is made of the record's own field values —
    // `Party.Created` carries the party code, its type and its status — so
    // `master-data:audit:read` alone would hand over at the side door what
    // `master-data:party:read` refuses at the front one. The module already
    // applies that rule to the role half of the same list; this is the party
    // half of it.
    //
    // Both routes, because the reasoning is about the record and not about the
    // entity: a facility's history answers the same way to
    // `master-data:facility:read`.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-ACME").await;
    let facility = given_facility(
        &app,
        &token,
        json!({ "facilityId": "FAC-HQ", "name": "Head Office" }),
    )
    .await;

    let party_history = format!("{PARTIES}/{party}/audit");
    let facility_history = format!("{FACILITIES}/{facility}/audit");

    // The permission that opens the surface, and nothing that opens a record.
    let auditor = caller_holding(&app, &[AUDIT_READ], 5).await;
    for path in [&party_history, &facility_history] {
        assert_eq!(
            app.get(path, Some(&auditor)).await.status,
            StatusCode::FORBIDDEN,
            "{AUDIT_READ} alone read a record's own field values: {path}"
        );
    }

    // And it is the record's own permission that is missing rather than any
    // permission at all: each caller below is refused exactly where they may
    // not read.
    let over_parties = caller_holding(&app, &[AUDIT_READ, PARTY_READ], 6).await;
    assert_eq!(
        app.get(&party_history, Some(&over_parties)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.get(&facility_history, Some(&over_parties)).await.status,
        StatusCode::FORBIDDEN,
        "a party reader was given a facility's history"
    );

    let over_facilities = caller_holding(&app, &[AUDIT_READ, FACILITY_READ], 7).await;
    assert_eq!(
        app.get(&facility_history, Some(&over_facilities))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        app.get(&party_history, Some(&over_facilities)).await.status,
        StatusCode::FORBIDDEN,
        "a facility reader was given a party's history"
    );
}

#[tokio::test]
async fn the_history_pages_and_reports_a_total_that_counts_the_same_rows() {
    // #100 AC4. The rows as well as the total: they come from different
    // statements, and a test reading only the total reports on the count while
    // claiming to cover the page (#106 F6).
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = a_party_with_a_history(&app, &token).await;
    let path = format!("{PARTIES}/{party}/audit");

    let first = app
        .get(&format!("{path}?page=1&pageSize=2"), Some(&token))
        .await;
    assert_eq!(
        actions(&first.body),
        vec!["CREATE", "UPDATE"],
        "{}",
        first.body
    );
    assert_eq!(first.body["meta"]["total"], 6, "{}", first.body);

    let last = app
        .get(&format!("{path}?page=3&pageSize=2"), Some(&token))
        .await;
    assert_eq!(
        actions(&last.body),
        vec!["ROLE_REMOVED", "RECORD_STATUS_CHANGE"],
        "{}",
        last.body
    );

    let past_the_end = app
        .get(&format!("{path}?page=9&pageSize=2"), Some(&token))
        .await;
    assert!(
        actions(&past_the_end.body).is_empty(),
        "{}",
        past_the_end.body
    );
    assert_eq!(
        past_the_end.body["meta"]["total"], 6,
        "{}",
        past_the_end.body
    );
}

#[tokio::test]
async fn another_tenants_history_is_out_of_reach() {
    // #100 AC5, with the other tenant's records actually present in the table
    // rather than assumed absent.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let foreign = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, 'PARTY-FOREIGN', 'PARTY_GROUP')",
    )
    .bind(foreign)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's party");

    sqlx::query(
        "INSERT INTO audit_events
             (id, tenant_id, event_type, action, object_type, object_id,
              previous_hash, current_hash)
         VALUES ($1, $2, 'Party.Created', 'CREATE', 'PARTY', $3, 'sha256:none', 'sha256:theirs')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(foreign)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's audit record");

    assert_eq!(
        app.get(&format!("{PARTIES}/{foreign}/audit"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND,
        "another tenant's history answered"
    );

    // And their records do not appear under an id of ours either: a party of
    // ours whose id somehow collided would still not inherit their chain.
    let ours = given_party(&app, &token, "PARTY-OURS").await;
    sqlx::query("UPDATE audit_events SET object_id = $1 WHERE tenant_id = $2")
        .bind(ours)
        .bind(other_tenant)
        .execute(&app.pool)
        .await
        .expect("point their record at our party");

    let history = app
        .get(&format!("{PARTIES}/{ours}/audit"), Some(&token))
        .await;
    assert_eq!(
        actions(&history.body),
        vec!["CREATE"],
        "another tenant's audit record was read as ours: {}",
        history.body
    );
    assert_eq!(history.body["meta"]["total"], 1, "{}", history.body);
}

#[tokio::test]
async fn a_history_for_a_record_that_does_not_exist_is_a_404() {
    // Rather than an empty page: a caller cannot otherwise tell "no such party"
    // from "nothing has happened to it".
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for path in [
        format!("{PARTIES}/{}/audit", Uuid::now_v7()),
        format!("{FACILITIES}/{}/audit", Uuid::now_v7()),
    ] {
        assert_eq!(
            app.get(&path, Some(&token)).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn both_history_routes_refuse_a_request_with_no_token() {
    let app = TestApp::spawn().await;

    for path in [
        format!("{PARTIES}/{}/audit", Uuid::now_v7()),
        format!("{FACILITIES}/{}/audit", Uuid::now_v7()),
    ] {
        let response = app
            .send_from(common::TEST_PEER, Method::GET, &path, None, None)
            .await;

        assert_eq!(response.status, StatusCode::UNAUTHORIZED, "{path}");
    }
}

// ---------------------------------------------------------------------------
// A record is the change, not the request (#135)
// ---------------------------------------------------------------------------

async fn given_facility(app: &TestApp, token: &str, body: Value) -> Uuid {
    let created = app.post(FACILITIES, Some(token), body).await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id")
}

/// Every UPDATE record against one record, oldest first.
async fn update_records(app: &TestApp, token: &str, path: &str) -> Vec<Value> {
    let history = app.get(&format!("{path}/audit"), Some(token)).await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);

    history.body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("data is not a list: {}", history.body))
        .iter()
        .filter(|record| record["action"] == "UPDATE")
        .cloned()
        .collect()
}

/// The field names one half of a record carries, sorted.
///
/// Sorted because the column is `JSONB` and PostgreSQL orders an object's keys
/// by length and then by bytes, which is neither the order they were written in
/// nor the one `serde_json` would produce.
fn fields(half: &Value) -> Vec<String> {
    let mut names: Vec<String> = half
        .as_object()
        .unwrap_or_else(|| panic!("not an object: {half}"))
        .keys()
        .cloned()
        .collect();

    names.sort();
    names
}

#[tokio::test]
async fn an_updates_record_names_the_fields_that_changed_and_no_others() {
    // #135 AC1 and AC2. Every field of an update request is optional — that is
    // what makes a partial update partial — so a field the caller did not
    // mention serialised as `null` on the new side, and the record said the
    // name and the type had been cleared when neither had been touched.
    //
    // `address` and `additionalAttributes` are the other half of the same
    // defect: both are updatable and neither appeared on either side, so the
    // only thing that did change was in no half of the record at all.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let facility = given_facility(
        &app,
        &token,
        json!({ "facilityId": "FAC-HQ", "name": "Head Office", "facilityTypeId": "BUILDING" }),
    )
    .await;
    let path = format!("{FACILITIES}/{facility}");

    let updated = app
        .put(
            &path,
            Some(&token),
            json!({
                "address": { "address1": "1 Dock Road", "city": "Surabaya" },
                "additionalAttributes": { "floors": 3 },
            }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let records = update_records(&app, &token, &path).await;
    let [record] = records.as_slice() else {
        panic!("one update, one record: {records:?}");
    };

    // The two fields that moved, on both sides, and nothing else on either.
    assert_eq!(
        fields(&record["oldValue"]),
        vec!["additionalAttributes", "address"],
        "{record}"
    );
    assert_eq!(
        fields(&record["newValue"]),
        vec!["additionalAttributes", "address"],
        "{record}"
    );

    assert_eq!(record["oldValue"]["address"], Value::Null, "{record}");
    assert_eq!(
        record["oldValue"]["additionalAttributes"],
        json!({}),
        "{record}"
    );
    assert_eq!(
        record["newValue"]["address"]["address1"], "1 Dock Road",
        "{record}"
    );
    assert_eq!(
        record["newValue"]["address"]["city"], "Surabaya",
        "{record}"
    );
    assert_eq!(
        record["newValue"]["additionalAttributes"],
        json!({ "floors": 3 }),
        "{record}"
    );

    // And the facility still holds the name and the type the record does not
    // mention, which is what makes naming them a false statement.
    let facility = app.get(&path, Some(&token)).await;
    assert_eq!(facility.data()["name"], "Head Office", "{}", facility.body);
    assert_eq!(
        facility.data()["facilityTypeId"],
        "BUILDING",
        "{}",
        facility.body
    );
}

#[tokio::test]
async fn both_halves_of_a_record_are_read_off_the_row() {
    // The request is not the change even for a field it does name: the service
    // trims a name before storing it, so a record built from the payload
    // reports a value the column does not hold.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let facility = given_facility(
        &app,
        &token,
        json!({ "facilityId": "FAC-HQ", "name": "Head Office" }),
    )
    .await;
    let path = format!("{FACILITIES}/{facility}");

    app.put(
        &path,
        Some(&token),
        json!({ "name": "  Head Office (North)  " }),
    )
    .await;

    let records = update_records(&app, &token, &path).await;
    let [record] = records.as_slice() else {
        panic!("one update, one record: {records:?}");
    };

    assert_eq!(
        record["oldValue"],
        json!({ "name": "Head Office" }),
        "{record}"
    );
    assert_eq!(
        record["newValue"],
        json!({ "name": "Head Office (North)" }),
        "{record}"
    );
}

#[tokio::test]
async fn clearing_a_reference_and_leaving_it_alone_are_different_records() {
    // #135 AC3. `parentFacilityId` is `Option<Option<String>>` precisely so
    // that *omitted* and *explicitly cleared* are different requests, and
    // `update_facility_fields` goes to some trouble to honour the difference.
    // Both serialised to `null`, so the audit trail could not tell a facility
    // taken out from under its parent from one whose parent was never
    // mentioned.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_facility(
        &app,
        &token,
        json!({ "facilityId": "FAC-SITE", "name": "Dock Site", "facilityTypeId": "SITE" }),
    )
    .await;
    let facility = given_facility(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ",
            "name": "Head Office",
            "facilityTypeId": "BUILDING",
            "parentFacilityId": "FAC-SITE",
        }),
    )
    .await;
    let path = format!("{FACILITIES}/{facility}");

    // Omitted: the parent is not what this request is about.
    app.put(
        &path,
        Some(&token),
        json!({ "name": "Head Office (North)" }),
    )
    .await;

    // Cleared: the parent is exactly what this request is about.
    let detached = app
        .put(&path, Some(&token), json!({ "parentFacilityId": null }))
        .await;
    assert_eq!(detached.status, StatusCode::OK, "{}", detached.body);

    let records = update_records(&app, &token, &path).await;
    let [left_alone, cleared] = records.as_slice() else {
        panic!("two updates, two records: {records:?}");
    };

    assert_eq!(
        fields(&left_alone["oldValue"]),
        vec!["name"],
        "{left_alone}"
    );
    assert_eq!(
        fields(&left_alone["newValue"]),
        vec!["name"],
        "{left_alone}"
    );

    assert_eq!(
        cleared["oldValue"],
        json!({ "parentFacilityId": "FAC-SITE" }),
        "{cleared}"
    );
    assert_eq!(
        cleared["newValue"],
        json!({ "parentFacilityId": null }),
        "{cleared}"
    );
}

#[tokio::test]
async fn a_partys_update_records_the_change_too() {
    // #135 AC4. `update_party` has had the same shape since #80 and the same
    // symptom: changing only the description reported `externalId` and
    // `statusId` as cleared, and both were still there.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            PARTIES,
            Some(&token),
            json!({
                "partyId": "PARTY-ACME",
                "partyTypeId": "PARTY_GROUP",
                "externalId": "EXT-9",
                "description": "original description",
                "partyGroup": { "groupName": "Acme" },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let party = created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id");
    let path = format!("{PARTIES}/{party}");

    app.put(
        &path,
        Some(&token),
        json!({ "description": "a new description" }),
    )
    .await;

    let records = update_records(&app, &token, &path).await;
    let [record] = records.as_slice() else {
        panic!("one update, one record: {records:?}");
    };

    assert_eq!(
        record["oldValue"],
        json!({ "description": "original description" }),
        "{record}"
    );
    assert_eq!(
        record["newValue"],
        json!({ "description": "a new description" }),
        "{record}"
    );

    // The party still holds what the record does not mention.
    let party = app.get(&path, Some(&token)).await;
    assert_eq!(party.data()["externalId"], "EXT-9", "{}", party.body);
    assert_eq!(party.data()["statusId"], "PARTY_ENABLED", "{}", party.body);
}
