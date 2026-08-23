//! Facility master data (FR-MDM-004; issue #98).
//!
//! The claim under test is that a facility hierarchy is a **tree**. Everything
//! else here — create, read, update, soft delete, tenant scoping — is the shape
//! the party surface already established; the hierarchy is what is new, and it
//! is the part no database constraint can hold up.
//!
//! `parent_facility_id` is a self-reference, so PostgreSQL can say *points at a
//! facility* and cannot say *and not at one of its own descendants*. A cycle in
//! storage is not a wrong answer, it is a traversal that never ends, so the
//! rule needs a test rather than a comment.
//!
//! # What the scoping tests here reach
//!
//! `count_facilities`, `list_facilities`, `find_facility`,
//! `find_facility_id_by_code` and `soft_delete_facility` were each mutated on
//! `tenant_id`, and every mutation turns
//! `another_tenants_facility_is_out_of_reach_on_every_route` red.
//!
//! **Not reached, and not reachable: five tenant predicates.** Saying so is
//! better than implying otherwise (#121), and better than a later reader
//! filing the gap again (#139).
//!
//! `update_facility_fields`, `children_of`, both joins in `list_facilities`,
//! and both terms of `facility_ancestors` each filter on `tenant_id` after a
//! preceding tenant-scoped read has already established the row — and each
//! joins or filters on a **UUID primary key**. No fixture can make the id match
//! and the tenant not, so these are defence in depth rather than controls, and
//! a test claiming to cover one would be covering the read before it.
//! `update_facility` is the clearest case: it reads the facility first, for the
//! audit record's *before* values and for the self-parent rule, scoped on the
//! same table by the same `(tenant_id, id)`.
//!
//! **The soft-delete predicates are not in that category** and each now has a
//! test — see the section at the foot of this file. Two of them guard a state
//! nothing can reach through the API, which is an argument for writing the
//! state directly rather than for leaving them untested.
//!
//! **With one exception, and it changed category while #139 was being closed.**
//! `find_facility_id_by_code`'s `deleted_at IS NULL` is no longer isolable:
//! `resolve_parent` is its only caller, and since #137 every caller re-reads the
//! parent under the hierarchy lock through `facility_is_live` before pointing at
//! it. Dropping the predicate now produces the same 422 naming the same field,
//! one guard later. `a_retired_facility_cannot_be_named_as_a_parent` covers the
//! behaviour and is red when *both* guards are removed; it is not evidence about
//! that predicate alone, and saying so here is the point of this section.
//!
//! That is the shape [record 02](../../projects/verifications/02.%20Role%20View%20and%20Fix%20Verification.md)
//! named as F5 — a test masked by a later fix rather than by its own shape — met
//! from the other side: a fix made a predicate redundant, and the mutation that
//! used to prove the predicate now proves the fix.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const FACILITIES: &str = "/api/v1/master-data/facilities";
const PARTIES: &str = "/api/v1/master-data/parties";
const PASSWORD: &str = "facility-caller-password";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn building(code: &str, name: &str) -> Value {
    json!({
        "facilityId": code,
        "name": name,
        "facilityTypeId": "BUILDING",
    })
}

async fn create(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    let response = app.post(FACILITIES, Some(token), body).await;

    (response.status, response.body)
}

/// Creates a facility and returns its surrogate id, failing here rather than at
/// the next assertion if the create itself was refused.
async fn create_ok(app: &TestApp, token: &str, body: Value) -> Uuid {
    let (status, body) = create(app, token, body).await;

    assert_eq!(status, StatusCode::CREATED, "create refused: {body}");

    body["data"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the created facility carries no id: {body}"))
}

/// A caller holding exactly the permissions named, signed in.
async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-FACILITY-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.facility{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("facility{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// The parent a facility actually points at, read from the column.
///
/// Not from the API: `find_facility` joins the parent on `deleted_at IS NULL`
/// and reports a dangling reference as no parent at all, so a test asking the
/// route what the hierarchy looks like cannot see the shapes these tests are
/// about.
async fn parent_of(app: &TestApp, id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT parent_facility_id FROM mdm_facilities WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the parent column")
}

/// The `facilityId` of every row a list response actually returned.
///
/// The rows, not `meta.total` — they come from different statements, and a test
/// that only reads the total reports on `count_facilities` while claiming to
/// cover `list_facilities` (#106).
fn codes(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("data is not a list: {body}"))
        .iter()
        .map(|row| {
            row["facilityId"]
                .as_str()
                .unwrap_or_else(|| panic!("a list row carries no facilityId: {body}"))
                .to_owned()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Create, read, update, delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_facility_is_created_read_back_updated_and_retired() {
    // Acceptance criterion 1, end to end, so that the four routes are known to
    // agree about the same row rather than each being right on its own.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            FACILITIES,
            Some(&token),
            json!({
                "facilityId": "FAC-HQ",
                "name": "Head Office",
                "facilityTypeId": "BUILDING",
                "address": {
                    "address1": "1 Jalan Merdeka",
                    "city": "Jakarta",
                    "postalCode": "10110",
                    "countryGeoId": "IDN",
                },
                "additionalAttributes": { "floors": 12 },
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("id");

    let fetched = app.get(&format!("{FACILITIES}/{id}"), Some(&token)).await;
    assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.body);
    assert_eq!(fetched.data()["facilityId"], "FAC-HQ");
    assert_eq!(fetched.data()["name"], "Head Office");
    assert_eq!(fetched.data()["facilityTypeId"], "BUILDING");
    assert_eq!(fetched.data()["address"]["city"], "Jakarta");
    assert_eq!(fetched.data()["additionalAttributes"]["floors"], 12);
    assert!(
        fetched.data()["parentFacilityId"].is_null(),
        "{}",
        fetched.body
    );

    let updated = app
        .put(
            &format!("{FACILITIES}/{id}"),
            Some(&token),
            json!({ "name": "Head Office (North)" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);
    assert_eq!(updated.data()["name"], "Head Office (North)");
    // An omitted field is left alone, not blanked.
    assert_eq!(updated.data()["facilityTypeId"], "BUILDING");
    assert_eq!(updated.data()["address"]["city"], "Jakarta");

    let deleted = app
        .delete(&format!("{FACILITIES}/{id}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    assert_eq!(
        app.get(&format!("{FACILITIES}/{id}"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(codes(&app.get(FACILITIES, Some(&token)).await.body).is_empty());
}

#[tokio::test]
async fn a_facility_code_is_unique_among_live_rows_and_released_by_a_delete() {
    // The partial unique index is on `deleted_at IS NULL`, so retiring a
    // facility must release its code — the failure #103 was about, one entity
    // over.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;

    let (status, body) = create(&app, &token, building("FAC-HQ", "Another")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    app.delete(&format!("{FACILITIES}/{id}"), Some(&token))
        .await;

    let (status, body) = create(&app, &token, building("FAC-HQ", "The replacement")).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a retired facility kept its code: {body}"
    );
}

#[tokio::test]
async fn an_owner_that_does_not_resolve_is_refused_by_name() {
    // Acceptance criterion 3, and the shape #81 uses for `managerPartyId`: a
    // reference that names nothing is the caller's mistake, not a foreign-key
    // violation surfacing as a 500.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut request = building("FAC-HQ", "Head Office");
    request["ownerPartyId"] = json!("PARTY-NOBODY");

    let (status, body) = create(&app, &token, request).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["path"] == "ownerPartyId"),
        "the refusal did not name the field: {body}"
    );
}

#[tokio::test]
async fn an_owner_that_resolves_comes_back_as_the_party_code_it_was_sent_as() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    app.post(
        PARTIES,
        Some(&token),
        json!({
            "partyId": "PARTY-ACME",
            "partyTypeId": "PARTY_GROUP",
            "partyGroup": { "groupName": "Acme Property" },
        }),
    )
    .await;

    let mut request = building("FAC-HQ", "Head Office");
    request["ownerPartyId"] = json!("PARTY-ACME");

    let (status, body) = create(&app, &token, request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["data"]["ownerPartyId"], "PARTY-ACME",
        "the owner came back as something other than the code it was sent as: {body}"
    );
}

#[tokio::test]
async fn an_unknown_facility_type_is_refused_rather_than_stored() {
    // Acceptance criterion 4. `facility_type` is VARCHAR(64) with no CHECK, so
    // the vocabulary is enforced in code or not at all — the database would
    // store HANGAR happily.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut request = building("FAC-HQ", "Head Office");
    request["facilityTypeId"] = json!("HANGAR");

    let (status, body) = create(&app, &token, request).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_facilities")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(stored, 0, "the refused facility was stored anyway");
}

// ---------------------------------------------------------------------------
// The hierarchy is a tree
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_facility_nests_under_another_and_reads_back_its_parents_code() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;

    let (status, body) = create(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ-L3",
            "name": "Third floor",
            "facilityTypeId": "FLOOR",
            "parentFacilityId": "FAC-HQ",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["data"]["parentFacilityId"], "FAC-HQ", "{body}");
}

#[tokio::test]
async fn a_parent_that_does_not_resolve_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut request = building("FAC-HQ-L3", "Third floor");
    request["parentFacilityId"] = json!("FAC-NOWHERE");

    let (status, body) = create(&app, &token, request).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["path"] == "parentFacilityId"),
        "{body}"
    );
}

#[tokio::test]
async fn a_facility_cannot_be_made_its_own_parent() {
    // The shortest cycle, and the one a client hits by accident.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;

    let refused = app
        .put(
            &format!("{FACILITIES}/{id}"),
            Some(&token),
            json!({ "parentFacilityId": "FAC-HQ" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_stored_parent_is(&app, id, None).await;
}

#[tokio::test]
async fn a_facility_cannot_be_moved_under_its_own_descendant() {
    // Acceptance criterion 2, and the case a self-parent check does not cover.
    // Building → Floor → Room, then Building is asked to move under Room. The
    // database would accept it: every row would still point at a real facility.
    //
    // What makes this worth its own test rather than a comment is the failure
    // mode. A cycle is not a wrong answer that shows up in a response — it is a
    // traversal that never terminates.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let building_id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;
    create_ok(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ-L3",
            "name": "Third floor",
            "facilityTypeId": "FLOOR",
            "parentFacilityId": "FAC-HQ",
        }),
    )
    .await;
    create_ok(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ-L3-01",
            "name": "Room 301",
            "facilityTypeId": "ROOM",
            "parentFacilityId": "FAC-HQ-L3",
        }),
    )
    .await;

    let refused = app
        .put(
            &format!("{FACILITIES}/{building_id}"),
            Some(&token),
            json!({ "parentFacilityId": "FAC-HQ-L3-01" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the building was moved under its own room: {}",
        refused.body
    );
    assert!(
        refused.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["path"] == "parentFacilityId" && detail["code"] == "CYCLE"),
        "{}",
        refused.body
    );

    // And nothing was written: a refusal that left the row changed would be a
    // cycle with a 422 on top of it.
    assert_stored_parent_is(&app, building_id, None).await;
}

#[tokio::test]
async fn a_parent_can_be_cleared_but_not_by_omitting_it() {
    // `null` detaches, absent leaves it alone. Without the distinction a
    // facility could be put under a parent and never taken out from under it.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;
    let floor = create_ok(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ-L3",
            "name": "Third floor",
            "facilityTypeId": "FLOOR",
            "parentFacilityId": "FAC-HQ",
        }),
    )
    .await;

    let renamed = app
        .put(
            &format!("{FACILITIES}/{floor}"),
            Some(&token),
            json!({ "name": "Third floor (east)" }),
        )
        .await;
    assert_eq!(
        renamed.data()["parentFacilityId"],
        "FAC-HQ",
        "an omitted parentFacilityId detached the parent: {}",
        renamed.body
    );

    let detached = app
        .put(
            &format!("{FACILITIES}/{floor}"),
            Some(&token),
            json!({ "parentFacilityId": null }),
        )
        .await;
    assert_eq!(detached.status, StatusCode::OK, "{}", detached.body);
    assert!(
        detached.data()["parentFacilityId"].is_null(),
        "an explicit null did not detach the parent: {}",
        detached.body
    );
}

#[tokio::test]
async fn deleting_a_facility_with_children_is_refused_rather_than_cascaded() {
    // A cascade would let one call delete a hundred rows. Refusing names the
    // count so the caller can re-parent or delete deliberately.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let building_id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;
    let floor = create_ok(
        &app,
        &token,
        json!({
            "facilityId": "FAC-HQ-L3",
            "name": "Third floor",
            "facilityTypeId": "FLOOR",
            "parentFacilityId": "FAC-HQ",
        }),
    )
    .await;

    let refused = app
        .delete(&format!("{FACILITIES}/{building_id}"), Some(&token))
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);

    // The floor is still there, which is the point of refusing.
    assert_eq!(
        app.get(&format!("{FACILITIES}/{floor}"), Some(&token))
            .await
            .status,
        StatusCode::OK
    );

    // Retire the child first and the parent goes.
    app.delete(&format!("{FACILITIES}/{floor}"), Some(&token))
        .await;
    assert_eq!(
        app.delete(&format!("{FACILITIES}/{building_id}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

// ---------------------------------------------------------------------------
// Permissions, tenancy and audit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_facility_route_requires_its_own_permission() {
    // Acceptance criterion 5, in the shape #58 established: holding every
    // *other* permission in the module is not enough, so a route bound to the
    // wrong string is caught rather than passing because the caller happened to
    // be an administrator.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = create_ok(&app, &token, building("FAC-TARGET", "Target")).await;

    let all = [
        "master-data:facility:create",
        "master-data:facility:read",
        "master-data:facility:update",
        "master-data:facility:delete",
    ];

    let routes: Vec<(Method, String, &str, Option<Value>)> = vec![
        (
            Method::GET,
            FACILITIES.to_owned(),
            "master-data:facility:read",
            None,
        ),
        (
            Method::GET,
            format!("{FACILITIES}/{id}"),
            "master-data:facility:read",
            None,
        ),
        (
            Method::POST,
            FACILITIES.to_owned(),
            "master-data:facility:create",
            Some(building("FAC-NEW", "New")),
        ),
        (
            Method::PUT,
            format!("{FACILITIES}/{id}"),
            "master-data:facility:update",
            Some(json!({ "name": "Renamed" })),
        ),
        (
            Method::DELETE,
            format!("{FACILITIES}/{id}"),
            "master-data:facility:delete",
            None,
        ),
    ];

    for (nonce, (method, path, permission, body)) in routes.into_iter().enumerate() {
        let others: Vec<&str> = all
            .iter()
            .copied()
            .filter(|code| *code != permission)
            .collect();
        let caller = caller_holding(&app, &others, nonce).await;

        let response = app
            .send_from(
                common::TEST_PEER,
                method.clone(),
                &path,
                Some(&caller),
                body.clone(),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{method} {path} answered without {permission}: {}",
            response.body
        );
    }
}

#[tokio::test]
async fn another_tenants_facility_is_out_of_reach_on_every_route() {
    // Acceptance criterion 7, with the other tenant's row actually present
    // rather than assumed absent — and on the writes as well as the reads,
    // which is the gap #121 found on the party surface.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let foreign = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO mdm_facilities (id, tenant_id, facility_code, name)
         VALUES ($1, $2, 'FAC-FOREIGN', 'Their warehouse')",
    )
    .bind(foreign)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's facility");

    let listed = app.get(FACILITIES, Some(&token)).await;
    assert!(codes(&listed.body).is_empty(), "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);

    assert_eq!(
        app.get(&format!("{FACILITIES}/{foreign}"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.put(
            &format!("{FACILITIES}/{foreign}"),
            Some(&token),
            json!({ "name": "Ours now" })
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&format!("{FACILITIES}/{foreign}"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // The answers, and then the row: a 404 produced after the write would still
    // be a 404.
    let untouched: (String, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT name, deleted_at FROM mdm_facilities WHERE id = $1")
            .bind(foreign)
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(untouched.0, "Their warehouse");
    assert!(
        untouched.1.is_none(),
        "another tenant's facility was closed"
    );

    // And its code is not ours to reference either.
    let mut request = building("FAC-HQ-L3", "Third floor");
    request["parentFacilityId"] = json!("FAC-FOREIGN");
    let (status, body) = create(&app, &token, request).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another tenant's facility resolved as a parent: {body}"
    );
}

#[tokio::test]
async fn creating_updating_and_retiring_a_facility_is_on_the_record() {
    // Acceptance criterion 6. The audit chain is what answers "who moved this
    // building" later, and a write that is not on it cannot be answered for.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;
    app.put(
        &format!("{FACILITIES}/{id}"),
        Some(&token),
        json!({ "name": "Head Office (North)" }),
    )
    .await;
    app.delete(&format!("{FACILITIES}/{id}"), Some(&token))
        .await;

    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM audit_events
         WHERE object_type = 'FACILITY' AND object_id = $1
         ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(
        events,
        vec![
            "Facility.Created".to_owned(),
            "Facility.Updated".to_owned(),
            "Facility.Deleted".to_owned()
        ],
        "the facility's history is not on the record"
    );

    let old_and_new: (Option<Value>, Option<Value>) = sqlx::query_as(
        "SELECT old_value_json, new_value_json FROM audit_events
         WHERE object_id = $1 AND event_type = 'Facility.Updated'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(
        old_and_new.0.expect("an old value")["name"],
        "Head Office",
        "the update recorded no before"
    );
    assert_eq!(
        old_and_new.1.expect("a new value")["name"],
        "Head Office (North)",
        "the update recorded no after"
    );
}

#[tokio::test]
async fn a_new_facility_starts_at_draft_and_no_route_moves_it() {
    // Acceptance criterion 8. `record_status` is storage until #99 gives it a
    // transition; what this asserts is the half that is true today — the column
    // defaults to DRAFT and nothing here writes it — so that #99 has a starting
    // point it can rely on rather than assume.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, building("FAC-HQ", "Head Office")).await;

    let status: String =
        sqlx::query_scalar("SELECT record_status FROM mdm_facilities WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(status, "DRAFT");

    // And it is off the wire, for the same reason it is on the party: a field
    // nothing can change reads as a control that exists.
    let fetched = app.get(&format!("{FACILITIES}/{id}"), Some(&token)).await;
    assert!(
        fetched.data().get("recordStatus").is_none(),
        "recordStatus is published before anything can move it: {}",
        fetched.body
    );
}

#[tokio::test]
async fn the_list_pages_and_reports_a_total_that_counts_the_same_rows() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for n in 0..5 {
        create_ok(&app, &token, building(&format!("FAC-{n:04}"), "Site")).await;
    }

    let first = app
        .get(&format!("{FACILITIES}?page=1&pageSize=2"), Some(&token))
        .await;
    assert_eq!(codes(&first.body).len(), 2, "{}", first.body);
    assert_eq!(first.body["meta"]["total"], 5, "{}", first.body);

    let last = app
        .get(&format!("{FACILITIES}?page=3&pageSize=2"), Some(&token))
        .await;
    assert_eq!(codes(&last.body).len(), 1, "{}", last.body);
    assert_eq!(last.body["meta"]["total"], 5, "{}", last.body);
}

#[tokio::test]
async fn every_facility_route_refuses_a_request_with_no_token() {
    let app = TestApp::spawn().await;

    for (method, path) in [
        (Method::GET, FACILITIES.to_owned()),
        (Method::POST, FACILITIES.to_owned()),
        (Method::GET, format!("{FACILITIES}/{}", Uuid::now_v7())),
        (Method::PUT, format!("{FACILITIES}/{}", Uuid::now_v7())),
        (Method::DELETE, format!("{FACILITIES}/{}", Uuid::now_v7())),
    ] {
        let response = app
            .send_from(common::TEST_PEER, method.clone(), &path, None, None)
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{method} {path} answered without a token: {}",
            response.body
        );
    }
}

/// What the row itself says its parent is, by `facility_code` rather than id.
async fn assert_stored_parent_is(app: &TestApp, id: Uuid, expected: Option<&str>) {
    let parent: Option<String> = sqlx::query_scalar(
        "SELECT parent.facility_code
         FROM mdm_facilities f
         LEFT JOIN mdm_facilities parent ON parent.id = f.parent_facility_id
         WHERE f.id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(parent.as_deref(), expected, "the stored parent is wrong");
}

#[tokio::test]
async fn the_ancestor_walk_stops_at_its_bound_against_a_cycle_already_in_storage() {
    // Why `MAX_FACILITY_DEPTH` exists, and the one case that makes it more than
    // a decorative constant.
    //
    // `refuse_cycle` is what stops a cycle being written *through this module*.
    // The recursive walk it runs is not protected by that: a cycle that reached
    // the table some other way — a migration, a repair script, a defect in a
    // later feature — would make an unbounded `WITH RECURSIVE` generate rows
    // until the connection died.
    //
    // Asserted against the repository rather than through the router, and on
    // the *count* rather than on elapsed time. A test of "this terminates" can
    // only fail by not terminating, which hangs a suite instead of failing it;
    // asserting that the walk returns exactly `max_depth` rows fails fast
    // against a bound that is present and wrong, and the `timeout` below is
    // what turns a bound that is missing into an assertion rather than a hang.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = create_ok(&app, &token, building("FAC-A", "A")).await;
    let second = create_ok(&app, &token, building("FAC-B", "B")).await;

    // Straight into the table: the API refuses this, which is the point. It did
    // not always — until #133 two concurrent re-parentings could write exactly
    // this pair — so the claim is now carried by
    // `two_concurrent_reparentings_cannot_close_a_loop` as well as by this
    // comment.
    for (child, parent) in [(first, second), (second, first)] {
        sqlx::query("UPDATE mdm_facilities SET parent_facility_id = $2 WHERE id = $1")
            .bind(child)
            .bind(parent)
            .execute(&app.pool)
            .await
            .expect("write the cycle directly");
    }

    let walked = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        kelir_backend::modules::master_data::repository::facility_ancestors(
            &app.pool,
            fixtures::SYSTEM_TENANT_ID,
            first,
            8,
        ),
    )
    .await
    .expect("the ancestor walk did not terminate against a cycle in storage")
    .expect("the query runs");

    assert_eq!(
        walked.ids.len(),
        8,
        "the walk did not stop at the bound it was given"
    );

    // Stopping is half of it. The walk must also say that it stopped early,
    // because a caller that cannot tell a prefix from a whole path treats
    // "not on it" as "not an ancestor" — which is how the bound came to allow
    // the cycle it exists to survive (#134).
    assert!(
        walked.truncated,
        "the walk hit its bound and reported a complete path"
    );
}

/// #133. The check-then-act shape #105 was about, on the hierarchy.
///
/// `refuse_cycle` read the ancestor path and `update_facility_fields` wrote,
/// both on the pool. Two callers each walked a path the other was about to
/// change, both were told the move was legal, and the pair closed a loop
/// neither could see alone. The verification pass reproduced it in 18 of 20
/// rounds.
///
/// A loop rather than one attempt, for the reason #105 records and #118
/// repeated: a race that needs an interleaving is not reliably reproduced by a
/// single shot.
#[tokio::test]
async fn two_concurrent_reparentings_cannot_close_a_loop() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let a_code = format!("LOOP-A-{round:04}");
        let b_code = format!("LOOP-B-{round:04}");
        let a = create_ok(&app, &token, building(&a_code, "A")).await;
        let b = create_ok(&app, &token, building(&b_code, "B")).await;

        let a_path = format!("{FACILITIES}/{a}");
        let b_path = format!("{FACILITIES}/{b}");
        let (first, second) = tokio::join!(
            app.put(&a_path, Some(&token), json!({ "parentFacilityId": b_code })),
            app.put(&b_path, Some(&token), json!({ "parentFacilityId": a_code })),
        );

        // Not "one of them fails": whichever runs first is a legal move and
        // must succeed. What must never happen is both, because together they
        // are a loop.
        assert!(
            !(parent_of(&app, a).await == Some(b) && parent_of(&app, b).await == Some(a)),
            "round {round}: a loop reached storage — {} / {}",
            first.body,
            second.body
        );

        for response in [&first, &second] {
            assert!(
                response.status == StatusCode::OK
                    || response.status == StatusCode::UNPROCESSABLE_ENTITY,
                "round {round}: a re-parenting answered {} — {}",
                response.status,
                response.body
            );
        }
    }
}

/// #133, the case that decides how the serialisation has to work.
///
/// Two re-parentings can close a loop while touching four different facilities,
/// so locking each caller's own row and its proposed parent locks two disjoint
/// pairs and serialises nothing. With `B → C` and `D → A` stored, moving `A`
/// under `B` and `C` under `D` at once yields `A → B → C → D → A`.
///
/// This is here as a test rather than only as a comment on
/// `lock_facility_hierarchy`, because the tempting cheaper fix passes the test
/// above and fails this one.
#[tokio::test]
async fn two_re_parentings_that_share_no_row_cannot_close_a_loop() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let codes: Vec<String> = (0..4).map(|n| format!("FOUR-{round:04}-{n}")).collect();
        let mut ids = Vec::new();
        for (n, code) in codes.iter().enumerate() {
            ids.push(create_ok(&app, &token, building(code, &format!("node {n}"))).await);
        }
        let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);

        // B -> C and D -> A, written directly so the round starts from the
        // shape the race needs rather than from four more requests.
        for (child, parent) in [(b, c), (d, a)] {
            sqlx::query("UPDATE mdm_facilities SET parent_facility_id = $2 WHERE id = $1")
                .bind(child)
                .bind(parent)
                .execute(&app.pool)
                .await
                .expect("seed the two existing edges");
        }

        let a_path = format!("{FACILITIES}/{a}");
        let c_path = format!("{FACILITIES}/{c}");
        let (first, second) = tokio::join!(
            app.put(
                &a_path,
                Some(&token),
                json!({ "parentFacilityId": codes[1] })
            ),
            app.put(
                &c_path,
                Some(&token),
                json!({ "parentFacilityId": codes[3] })
            ),
        );

        let closed = parent_of(&app, a).await == Some(b)
            && parent_of(&app, b).await == Some(c)
            && parent_of(&app, c).await == Some(d)
            && parent_of(&app, d).await == Some(a);

        assert!(
            !closed,
            "round {round}: a four-node loop reached storage — {} / {}",
            first.body, second.body
        );
    }
}

/// #134. A hierarchy deeper than the walk's bound.
///
/// Nothing limits how deep a tree may be built, so past `MAX_FACILITY_DEPTH`
/// the walk up from the proposed parent returns a prefix that does not contain
/// the root. `contains(&id)` then answers "not an ancestor" about a facility
/// that is one, and the check that exists to refuse a cycle allows it — one
/// caller, one request, no race.
#[tokio::test]
async fn a_move_the_depth_bound_cannot_verify_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // Comfortably past the bound, and cheap: these are 70 inserts.
    const DEPTH: usize = 70;

    let mut codes: Vec<String> = Vec::new();
    let mut root = None;
    for level in 0..DEPTH {
        let code = format!("DEEP-{level:04}");
        let mut body = building(&code, &format!("level {level}"));
        if level > 0 {
            body["parentFacilityId"] = json!(codes[level - 1]);
        }
        let id = create_ok(&app, &token, body).await;
        root.get_or_insert(id);
        codes.push(code);
    }
    let root = root.expect("a root");

    let response = app
        .put(
            &format!("{FACILITIES}/{root}"),
            Some(&token),
            json!({ "parentFacilityId": codes[DEPTH - 1] }),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the root was moved under its own descendant: {}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["code"], "TOO_DEEP",
        "refused, but not for the reason the caller needs: {}",
        response.body
    );

    assert_eq!(
        parent_of(&app, root).await,
        None,
        "the root was re-parented despite the refusal"
    );
}

/// #137. The no-cascade refusal only held against children that already existed.
///
/// `children_of` counted on the pool and `soft_delete_facility` wrote
/// afterwards, while a create resolved its parent on the pool and inserted
/// afterwards. A create that resolved the parent a moment before the delete
/// landed produced a live facility under a deleted one — 19 of 20 rounds — and
/// nobody decided it: the delete answered 204, the create answered 201, and the
/// decision the refusal exists to force was never put to anyone.
///
/// The assertion is on the column rather than on the route, because
/// `find_facility` joins the parent on `deleted_at IS NULL` and reports the
/// dangling reference as no parent at all.
#[tokio::test]
async fn deleting_a_facility_cannot_race_a_child_being_created_under_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let parent_code = format!("RACE-P-{round:04}");
        let child_code = format!("RACE-C-{round:04}");
        let parent = create_ok(&app, &token, building(&parent_code, "parent")).await;

        let parent_path = format!("{FACILITIES}/{parent}");
        let child_body = json!({
            "facilityId": child_code,
            "name": "child",
            "facilityTypeId": "ROOM",
            "parentFacilityId": parent_code,
        });
        let (deleted, created) = tokio::join!(
            app.delete(&parent_path, Some(&token)),
            app.post(FACILITIES, Some(&token), child_body),
        );

        // Either order is legitimate. What must not happen is both: a live
        // facility whose parent column names a row that has been retired.
        if deleted.status == StatusCode::NO_CONTENT && created.status == StatusCode::CREATED {
            let child = created.data()["id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .unwrap_or_else(|| panic!("the created child carries no id: {}", created.body));

            assert_ne!(
                parent_of(&app, child).await,
                Some(parent),
                "round {round}: a live facility was left under a deleted one"
            );
        }

        for response in [&deleted.status, &created.status] {
            assert!(
                matches!(
                    *response,
                    StatusCode::NO_CONTENT
                        | StatusCode::CREATED
                        | StatusCode::CONFLICT
                        | StatusCode::UNPROCESSABLE_ENTITY
                ),
                "round {round}: the pair answered {} / {} — {} / {}",
                deleted.status,
                created.status,
                deleted.body,
                created.body
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Soft-delete predicates the scoping tests did not reach (#139)
// ---------------------------------------------------------------------------
//
// Every tenant predicate on this table is either covered by
// `another_tenants_facility_is_out_of_reach_on_every_route` or is redundancy no
// fixture can isolate — the list of the second kind is in the module
// documentation at the top of this file. The soft-delete predicates are
// different: each of the four below is a control, each was still green under
// mutation, and two of them guard a state only #137's race could produce.

/// #139. A retired facility leaves the total, not just the page.
///
/// `count_facilities` and `list_facilities` are different statements, and
/// `the_list_pages_and_reports_a_total_that_counts_the_same_rows` compares them
/// against each other without a deleted row in play — so removing
/// `deleted_at IS NULL` from the count alone changed nothing it asserts.
#[tokio::test]
async fn a_retired_facility_leaves_the_total_as_well_as_the_page() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for code in ["TOT-A", "TOT-B", "TOT-C"] {
        create_ok(&app, &token, building(code, code)).await;
    }
    let retired = create_ok(&app, &token, building("TOT-D", "TOT-D")).await;
    assert_eq!(
        app.delete(&format!("{FACILITIES}/{retired}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let listed = app.get(FACILITIES, Some(&token)).await;
    let rows = codes(&listed.body);

    assert_eq!(rows.len(), 3, "{}", listed.body);
    assert_eq!(
        listed.body["meta"]["total"], 3,
        "the total counts a retired facility the page does not show: {}",
        listed.body
    );
}

/// #139. A retired facility cannot be named as a parent.
///
/// A delete releases the code (`a_facility_code_is_unique_among_live_rows_...`),
/// so the code of a retired facility is a string a caller may well still have.
/// `find_facility_id_by_code` filters `deleted_at IS NULL` and nothing asked it
/// to: without the predicate the create succeeds and puts a live facility under
/// a retired one, which is #137's outcome by a different route.
#[tokio::test]
async fn a_retired_facility_cannot_be_named_as_a_parent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parent = create_ok(&app, &token, building("PAR-GONE", "Parent")).await;
    assert_eq!(
        app.delete(&format!("{FACILITIES}/{parent}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let mut child = building("CHILD-1", "Child");
    child["parentFacilityId"] = json!("PAR-GONE");
    let (status, body) = create(&app, &token, child).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["details"][0]["path"], "parentFacilityId",
        "{body}"
    );
}

/// #139. A read never shows a retired facility as somebody's parent.
///
/// The `parent.deleted_at IS NULL` predicate on both reads guards a row whose
/// `parent_facility_id` names a retired facility. Nothing can create that state
/// through the API — the delete refuses while children exist, and #137 closed
/// the race that got around it — so the state is written directly here, the way
/// `the_ancestor_walk_stops_at_its_bound_against_a_cycle_already_in_storage`
/// writes the cycle it is about. A predicate that only defends against a
/// defect elsewhere is still a predicate, and this is the shape of test that
/// can reach one.
#[tokio::test]
async fn a_retired_parent_is_not_shown_as_a_parent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parent = create_ok(&app, &token, building("HID-P", "Parent")).await;
    let mut child_body = building("HID-C", "Child");
    child_body["parentFacilityId"] = json!("HID-P");
    let child = create_ok(&app, &token, child_body).await;

    // Straight into the table: the API refuses this, which is the point.
    sqlx::query("UPDATE mdm_facilities SET deleted_at = now() WHERE id = $1")
        .bind(parent)
        .execute(&app.pool)
        .await
        .expect("retire the parent behind the API's back");

    let fetched = app
        .get(&format!("{FACILITIES}/{child}"), Some(&token))
        .await;
    assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.body);
    assert!(
        fetched.data()["parentFacilityId"].is_null(),
        "a retired facility is still shown as a parent: {}",
        fetched.body
    );

    let listed = app.get(FACILITIES, Some(&token)).await;
    let row = listed.body["data"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["facilityId"] == "HID-C"))
        .unwrap_or_else(|| panic!("the child is not in the list: {}", listed.body));
    assert!(
        row["parentFacilityId"].is_null(),
        "the list shows a retired facility as a parent: {}",
        listed.body
    );
}

/// #139. A read never shows a retired party as somebody's owner.
///
/// The owner's soft-delete predicate, and unlike the parent's this state is
/// reachable through the API alone: nothing stops a party being deleted while a
/// facility names it as owner.
#[tokio::test]
async fn a_retired_owner_is_not_shown_as_an_owner() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .post(
            PARTIES,
            Some(&token),
            json!({
                "partyId": "OWN-GONE",
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": "Owner" },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let owner = created.data()["id"].as_str().expect("id").to_owned();

    let mut body = building("OWNED-1", "Owned");
    body["ownerPartyId"] = json!("OWN-GONE");
    let facility = create_ok(&app, &token, body).await;

    assert_eq!(
        app.delete(&format!("{PARTIES}/{owner}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    let fetched = app
        .get(&format!("{FACILITIES}/{facility}"), Some(&token))
        .await;
    assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.body);
    assert!(
        fetched.data()["ownerPartyId"].is_null(),
        "a retired party is still shown as an owner: {}",
        fetched.body
    );

    let listed = app.get(FACILITIES, Some(&token)).await;
    let row = listed.body["data"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["facilityId"] == "OWNED-1"))
        .unwrap_or_else(|| panic!("the facility is not in the list: {}", listed.body));
    assert!(
        row["ownerPartyId"].is_null(),
        "the list shows a retired party as an owner: {}",
        listed.body
    );
}
