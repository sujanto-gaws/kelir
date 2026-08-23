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

    let auditor = caller_holding(&app, &[AUDIT_READ, "master-data:party:read"], 1).await;
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

    let auditor = caller_holding(&app, &[AUDIT_READ, ROLE_READ], 2).await;
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
        &[
            "master-data:party:read",
            "master-data:party:update",
            ROLE_READ,
        ],
        3,
    )
    .await;

    assert_eq!(
        app.get(&format!("{PARTIES}/{party}/audit"), Some(&everything_else))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let auditor = caller_holding(&app, &[AUDIT_READ], 4).await;
    assert_eq!(
        app.get(&format!("{PARTIES}/{party}/audit"), Some(&auditor))
            .await
            .status,
        StatusCode::OK
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
