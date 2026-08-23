//! One party, many roles (FR-MDM-002; issue #81).
//!
//! The claim under test is the Party model's whole point: a party that is both
//! a supplier and a customer is one party with two roles, not two records. A
//! test that only assigned one role would pass against a design that stored a
//! second party behind the scenes, so the two-role case is the first one here.
//!
//! # What the scoping tests here reach (#106)
//!
//! `list_party_roles`, `find_live_party_role`, `soft_delete_party_role` and all
//! four `find_*_profile` queries were mutated on both `tenant_id` and
//! `deleted_at`, and every mutation turned a named test red;
//! `find_role_type_id` likewise on `tenant_id`.
//!
//! `a_party_in_another_tenant_has_no_roles_to_reach` is kept, and now claims
//! only what it covers: mutating `find_party` is the one thing that turns it
//! red, because the gate refuses ahead of every query it appears to exercise.
//!
//! `soft_delete_party_roles` is reached as of #121:
//! `deleting_a_party_closes_this_tenants_roles_and_leaves_anothers_alone` puts
//! another tenant's role row on the party being deleted, so the sweep's tenant
//! predicate is the only thing keeping that row live.
//!
//! `find_party_role` is not probed and cannot be: it reads back by primary key
//! (#121), so there is no scoping left in it to drop.
//! `the_assignment_answered_with_is_the_row_this_call_wrote` covers what
//! replaced it — an ambiguous lookup turns it red.
//!
//! Still not reached: `soft_delete_role_profile`, the other half of the
//! party-delete cascade. The delete tests below exercise it, but its scoping
//! sits behind the same gate and is not isolated from it.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";

fn party_group(party_code: &str, name: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PARTY_GROUP",
        "partyGroup": { "groupName": name },
    })
}

fn person(party_code: &str, first: &str, last: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PERSON",
        "person": { "firstName": first, "lastName": last },
    })
}

fn supplier_profile(number: &str) -> Value {
    json!({
        "fromDate": "2026-01-01T00:00:00Z",
        "profile": {
            "supplier": {
                "supplierNumber": number,
                "supplierCategory": "IT",
                "paymentTermDays": 30,
                "defaultCurrencyUom": "IDR",
                "bankName": "Bank Mandiri",
                "bankAccount": "1234567890",
                "bankAccountName": "Acme Supplies",
                "approvalStatus": "APPROVED",
            }
        },
    })
}

/// [`supplier_profile`] with a chosen `fromDate`.
///
/// Two assignments that differ only in their date are the shape a concurrency
/// test needs: the role's unique index includes `starts_at`, so identical dates
/// collide in the database and hide whatever the service did or did not do.
fn supplier_profile_from(number: &str, from_date: &str) -> Value {
    let mut body = supplier_profile(number);
    body["fromDate"] = json!(from_date);
    body
}

fn customer_profile(number: &str) -> Value {
    json!({
        "fromDate": "2026-01-01T00:00:00Z",
        "profile": {
            "customer": {
                "customerNumber": number,
                "customerCategory": "CORPORATE",
                "customerSince": "2024-06-01",
                "creditLimit": "50000000.00",
                "paymentTermDays": 14,
            }
        },
    })
}

async fn create_party(app: &TestApp, token: &str, body: Value) -> Uuid {
    let response = app.post(PARTIES, Some(token), body).await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "create refused: {}",
        response.body
    );

    response.body["data"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("the created party carries an id")
}

fn role_path(party: Uuid, role_type: &str) -> String {
    format!("{PARTIES}/{party}/roles/{role_type}")
}

// ---------------------------------------------------------------------------
// One party, many roles
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_same_party_is_a_supplier_and_a_customer_without_being_stored_twice() {
    // Acceptance criterion 1, and the reason the Party model exists.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let assigned = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    let second = app
        .put(
            &role_path(party, "CUSTOMER"),
            Some(&token),
            customer_profile("CUS-0001"),
        )
        .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.body);

    let aggregate = app.get(&format!("{PARTIES}/{party}"), Some(&token)).await;
    let data = aggregate.data();

    let roles: Vec<&str> = data["roles"]
        .as_array()
        .expect("roles is a list")
        .iter()
        .filter_map(|role| role["roleTypeId"].as_str())
        .collect();
    assert_eq!(roles, vec!["CUSTOMER", "SUPPLIER"], "{data}");

    assert_eq!(data["profiles"]["supplier"]["supplierNumber"], "SUP-0001");
    assert_eq!(data["profiles"]["customer"]["customerNumber"], "CUS-0001");
    // The profile's partyId is the party's own, on both.
    assert_eq!(data["profiles"]["supplier"]["partyId"], "PARTY-ACME");
    assert_eq!(data["profiles"]["customer"]["partyId"], "PARTY-ACME");

    // And there is exactly one party behind all of it.
    let parties: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mdm_parties WHERE deleted_at IS NULL")
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(parties, 1, "a second party was created behind the roles");
}

#[tokio::test]
async fn a_supplier_profile_survives_the_round_trip_whole() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;

    let supplier = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await
        .data()["profiles"]["supplier"]
        .clone();

    assert_eq!(supplier["supplierNumber"], "SUP-0001");
    assert_eq!(supplier["supplierCategory"], "IT");
    assert_eq!(supplier["paymentTermDays"], 30);
    assert_eq!(supplier["defaultCurrencyUom"], "IDR");
    assert_eq!(supplier["bankName"], "Bank Mandiri");
    assert_eq!(supplier["bankAccount"], "1234567890");
    assert_eq!(supplier["bankAccountName"], "Acme Supplies");
    assert_eq!(supplier["approvalStatus"], "APPROVED");
}

#[tokio::test]
async fn a_credit_limit_keeps_its_precision() {
    // NUMERIC(18,2) through a JSON double would round; the string is why it
    // does not.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "CUSTOMER"),
        Some(&token),
        customer_profile("CUS-0001"),
    )
    .await;

    let customer = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await
        .data()["profiles"]["customer"]
        .clone();

    assert_eq!(customer["creditLimit"], "50000000.00");
    assert_eq!(customer["customerSince"], "2024-06-01");
}

#[tokio::test]
async fn an_employee_profile_links_a_department_and_a_manager() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let manager = create_party(&app, &token, person("PARTY-MGR", "Budi", "Santoso")).await;
    let employee = create_party(&app, &token, person("PARTY-EMP", "Jane", "Doe")).await;

    let department = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, 'DEPT-PROC', 'Procurement')",
    )
    .bind(department)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert a department");

    let assigned = app
        .put(
            &role_path(employee, "EMPLOYEE"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": {
                    "employee": {
                        "employeeNumber": "EMP-0001",
                        "departmentId": department,
                        "managerPartyId": "PARTY-MGR",
                        "position": "Buyer",
                        "employmentType": "FULL_TIME",
                        "joinDate": "2026-01-05",
                    }
                },
            }),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    // The assignment answers with the assignment; the profile is read back
    // under the permission that gates it (#104).
    assert_eq!(assigned.data()["roleTypeId"], "EMPLOYEE");

    let profile = app
        .get(&format!("{PARTIES}/{employee}/roles"), Some(&token))
        .await
        .data()["profiles"]["employee"]
        .clone();
    assert_eq!(profile["employeeNumber"], "EMP-0001");
    assert_eq!(profile["departmentId"], department.to_string());
    // The manager comes back as a partyId, not a UUID: the aggregate speaks in
    // business codes.
    assert_eq!(profile["managerPartyId"], "PARTY-MGR");
    assert_eq!(profile["employmentType"], "FULL_TIME");

    // And the manager party itself is untouched by being referenced.
    let manager_roles = app
        .get(&format!("{PARTIES}/{manager}/roles"), Some(&token))
        .await;
    assert_eq!(
        manager_roles.data()["roles"].as_array().map(Vec::len),
        Some(0)
    );
}

// ---------------------------------------------------------------------------
// Assignment is idempotent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assigning_a_role_the_party_already_holds_updates_it_rather_than_doubling_it() {
    // The unique index covers `starts_at`, so a second assignment with a
    // different fromDate would be accepted by the database — a party holding
    // SUPPLIER twice, which nothing downstream could make sense of.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let first = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(first.status, StatusCode::CREATED);

    let again = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            json!({
                "fromDate": "2026-03-01T00:00:00Z",
                "comments": "renegotiated",
                "profile": { "supplier": { "approvalStatus": "BLACKLISTED" } },
            }),
        )
        .await;
    assert_eq!(
        again.status,
        StatusCode::OK,
        "a repeat assignment must be an update, not a create: {}",
        again.body
    );

    // The response is the assignment that was written, and the collection is
    // read back to prove there is still only one of it.
    assert_eq!(again.data()["roleTypeId"], "SUPPLIER");
    assert_eq!(again.data()["fromDate"], "2026-03-01T00:00:00Z");
    assert_eq!(again.data()["comments"], "renegotiated");

    let held = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await;
    let roles = held.data()["roles"].as_array().expect("roles is a list");
    assert_eq!(
        roles.len(),
        1,
        "the party holds SUPPLIER twice: {}",
        held.body
    );

    // The profile was updated in place: the number it was not asked to change
    // is still there, and the field it was asked to change moved.
    let supplier = &held.data()["profiles"]["supplier"];
    assert_eq!(supplier["approvalStatus"], "BLACKLISTED");
    assert_eq!(
        supplier["supplierNumber"], "SUP-0001",
        "an update that did not mention the number replaced it: {supplier}"
    );
}

#[tokio::test]
async fn two_parties_cannot_share_a_supplier_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let first = create_party(&app, &token, party_group("PARTY-A", "First")).await;
    let second = create_party(&app, &token, party_group("PARTY-B", "Second")).await;

    app.put(
        &role_path(first, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;

    let clash = app
        .put(
            &role_path(second, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;

    assert_eq!(clash.status, StatusCode::CONFLICT, "{}", clash.body);
    assert_eq!(clash.error_code(), Some("CONFLICT"));
}

// ---------------------------------------------------------------------------
// Removal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn removing_a_role_leaves_the_party_and_its_other_roles_alone() {
    // Acceptance criterion 3.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;
    app.put(
        &role_path(party, "CUSTOMER"),
        Some(&token),
        customer_profile("CUS-0001"),
    )
    .await;

    let removed = app
        .delete(&role_path(party, "SUPPLIER"), Some(&token))
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);

    let aggregate = app.get(&format!("{PARTIES}/{party}"), Some(&token)).await;
    let data = aggregate.data();

    assert_eq!(
        aggregate.status,
        StatusCode::OK,
        "the party went with the role"
    );
    assert_eq!(data["partyId"], "PARTY-ACME");

    let roles: Vec<&str> = data["roles"]
        .as_array()
        .expect("roles is a list")
        .iter()
        .filter_map(|role| role["roleTypeId"].as_str())
        .collect();
    assert_eq!(roles, vec!["CUSTOMER"]);

    assert!(
        data["profiles"]["supplier"].is_null(),
        "the supplier profile outlived its role: {data}"
    );
    assert_eq!(data["profiles"]["customer"]["customerNumber"], "CUS-0001");
}

#[tokio::test]
async fn a_removed_role_is_kept_as_history_rather_than_erased() {
    // "Was a supplier until March" is a fact the business needs; a hard delete
    // would lose it, and so would clearing thru_date.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;
    app.delete(&role_path(party, "SUPPLIER"), Some(&token))
        .await;

    let (deleted_at, ends_at, status): (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
        String,
    ) = sqlx::query_as(
        "SELECT r.deleted_at, r.ends_at, r.status
           FROM mdm_party_roles r
           JOIN mdm_role_types t ON t.id = r.role_type_id
          WHERE r.party_id = $1 AND t.role_type_code = 'SUPPLIER'",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("the role row was hard-deleted");

    assert!(deleted_at.is_some(), "the role row is still live");
    assert!(ends_at.is_some(), "the assignment was not closed off");
    assert_eq!(status, "INACTIVE");

    // The profile is soft-deleted with it, not orphaned.
    let profile_deleted: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM mdm_supplier_profiles WHERE party_id = $1")
            .bind(party)
            .fetch_one(&app.pool)
            .await
            .expect("the profile row was hard-deleted");
    assert!(profile_deleted.is_some(), "the profile outlived its role");
}

#[tokio::test]
async fn a_role_can_be_assigned_again_after_it_was_removed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;
    app.delete(&role_path(party, "SUPPLIER"), Some(&token))
        .await;

    // The same supplier number, too: the partial unique index excludes
    // soft-deleted rows, so removing a role has to release it.
    let again = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;

    assert_eq!(again.status, StatusCode::CREATED, "{}", again.body);
    assert_eq!(
        app.get(&format!("{PARTIES}/{party}/roles"), Some(&token))
            .await
            .data()["profiles"]["supplier"]["supplierNumber"],
        "SUP-0001"
    );
}

#[tokio::test]
async fn removing_a_role_the_party_does_not_hold_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let missing = app
        .delete(&role_path(party, "SUPPLIER"), Some(&token))
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let unknown_type = app
        .delete(&role_path(party, "NOT-A-ROLE"), Some(&token))
        .await;
    assert_eq!(unknown_type.status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Role types
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_tenant_role_type_can_be_added_without_a_migration_and_assigned() {
    // Acceptance criterion 4. `mdm_role_types` is an ordinary table; there is
    // no endpoint managing it yet, and none is needed for the claim to hold.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, person("PARTY-AUD", "Sri", "Wahyuni")).await;

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, 'AUDITOR', 'Auditor', false)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert a tenant role type");

    let assigned = app
        .put(
            &role_path(party, "AUDITOR"),
            Some(&token),
            json!({ "fromDate": "2026-01-01T00:00:00Z" }),
        )
        .await;

    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);
    assert_eq!(assigned.data()["roleTypeId"], "AUDITOR");

    // A role type with no profile table carries no profile.
    let held = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await;
    assert_eq!(held.data()["roles"][0]["roleTypeId"], "AUDITOR");
    assert!(held.data()["profiles"]
        .as_object()
        .is_some_and(|profiles| profiles.is_empty()));
}

#[tokio::test]
async fn the_four_seeded_role_types_with_profiles_are_all_assignable() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, person("PARTY-ALL", "Multi", "Role")).await;

    let bodies = [
        ("SUPPLIER", supplier_profile("SUP-0001")),
        ("CUSTOMER", customer_profile("CUS-0001")),
        (
            "EMPLOYEE",
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "employee": { "employeeNumber": "EMP-0001" } },
            }),
        ),
        (
            "CONTACT",
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "contact": { "contactType": "PRIMARY", "doNotContact": true } },
            }),
        ),
    ];

    for (role_type, body) in bodies {
        let response = app
            .put(&role_path(party, role_type), Some(&token), body)
            .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "{role_type} was refused: {}",
            response.body
        );
    }

    let profiles = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await
        .data()["profiles"]
        .clone();

    assert_eq!(profiles["supplier"]["supplierNumber"], "SUP-0001");
    assert_eq!(profiles["customer"]["customerNumber"], "CUS-0001");
    assert_eq!(profiles["employee"]["employeeNumber"], "EMP-0001");
    assert_eq!(profiles["contact"]["doNotContact"], true);
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unknown_role_type_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let response = app
        .put(
            &role_path(party, "NOT-A-ROLE"),
            Some(&token),
            json!({ "fromDate": "2026-01-01T00:00:00Z" }),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.body["error"]["details"][0]["path"], "roleTypeId");
}

#[tokio::test]
async fn a_profile_belonging_to_a_different_role_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let response = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            customer_profile("CUS-0001"),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["path"],
        "profile.customer"
    );
}

#[tokio::test]
async fn a_manager_who_does_not_exist_is_refused_before_anything_is_written() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, person("PARTY-EMP", "Jane", "Doe")).await;

    let response = app
        .put(
            &role_path(party, "EMPLOYEE"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": {
                    "employee": { "employeeNumber": "EMP-0001", "managerPartyId": "PARTY-NOBODY" }
                },
            }),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(
        response.body["error"]["details"][0]["path"],
        "profile.employee.managerPartyId"
    );

    let roles: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_party_roles")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(roles, 0, "a refused assignment left a role behind");
}

#[tokio::test]
async fn assigning_a_role_to_a_party_that_does_not_exist_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .put(
            &role_path(Uuid::now_v7(), "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;

    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_party_in_another_tenant_has_no_roles_to_reach() {
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

    assert_eq!(
        app.get(&format!("{PARTIES}/{foreign}/roles"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.put(
            &role_path(foreign, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-1")
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.delete(&role_path(foreign, "SUPPLIER"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Scoping probed past the party gate (#106 F7)
// ---------------------------------------------------------------------------
//
// `a_party_in_another_tenant_has_no_roles_to_reach` above states something
// true, and covers one query while appearing to cover six: every route it
// drives refuses at `find_party`, so nothing below the gate ever runs and every
// mutation beneath it is absorbed. The tests here put the party in the caller's
// own tenant, so the request gets past the gate and the child query's own
// filters are the only thing left standing.

/// The four profile tables, and the aggregate member each is read back into.
const PROFILE_TABLES: [(&str, &str); 4] = [
    ("mdm_supplier_profiles", "supplier"),
    ("mdm_customer_profiles", "customer"),
    ("mdm_employee_profiles", "employee"),
    ("mdm_contact_profiles", "contact"),
];

/// A party of the caller's own holding all four profiled roles, with the
/// business numbers suffixed so two of these can coexist in one tenant.
async fn party_holding_every_profiled_role(app: &TestApp, token: &str, suffix: &str) -> Uuid {
    let party = create_party(
        app,
        token,
        person(&format!("PARTY-{suffix}"), "Multi", "Role"),
    )
    .await;

    let bodies = [
        ("SUPPLIER", supplier_profile(&format!("SUP-{suffix}"))),
        ("CUSTOMER", customer_profile(&format!("CUS-{suffix}"))),
        (
            "EMPLOYEE",
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "employee": { "employeeNumber": format!("EMP-{suffix}") } },
            }),
        ),
        (
            "CONTACT",
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "contact": { "contactType": "PRIMARY", "doNotContact": true } },
            }),
        ),
    ];

    for (role_type, body) in bodies {
        let response = app
            .put(&role_path(party, role_type), Some(token), body)
            .await;
        assert_eq!(
            response.status,
            StatusCode::CREATED,
            "{role_type} was refused: {}",
            response.body
        );
    }

    party
}

/// The role type this tenant knows by `code`.
async fn role_type_id(app: &TestApp, code: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM mdm_role_types WHERE tenant_id = $1 AND role_type_code = $2")
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("the tenant has no {code} role type: {error}"))
}

/// Whether a role row has been soft-deleted.
async fn is_closed(app: &TestApp, role: Uuid) -> bool {
    sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "SELECT deleted_at FROM mdm_party_roles WHERE id = $1",
    )
    .bind(role)
    .fetch_one(&app.pool)
    .await
    .expect("query runs")
    .is_some()
}

/// The role codes a party's roles route lists.
async fn listed_roles(app: &TestApp, token: &str, party: Uuid) -> Vec<String> {
    let response = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(token))
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.data()["roles"]
        .as_array()
        .unwrap_or_else(|| panic!("roles is not a list: {}", response.body))
        .iter()
        .map(|role| {
            role["roleTypeId"]
                .as_str()
                .unwrap_or_else(|| panic!("a role carries no roleTypeId: {}", response.body))
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn a_role_row_stamped_with_another_tenant_is_not_mine_to_read_or_remove() {
    // The row differs from one this tenant could hold only by its `tenant_id`:
    // same party, same role type. Nothing can write that row today - role types
    // and parties are both tenant-scoped - which is exactly why the filters
    // that would exclude it need a probe. Defence in depth that nothing tests
    // is indistinguishable from defence in depth that does not work.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let supplier = role_type_id(&app, "SUPPLIER").await;
    let foreign_role = Uuid::now_v7();

    // A `starts_at` of its own: `uq_mdm_party_roles_party_id_role_type_id_starts_at`
    // does not include the tenant, so a row sharing this party's date would
    // collide with the assignment below and hide what the service did.
    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at)
         VALUES ($1, $2, $3, $4, TIMESTAMPTZ '2025-01-01T00:00:00Z')",
    )
    .bind(foreign_role)
    .bind(other_tenant)
    .bind(party)
    .bind(supplier)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role row");

    // `list_party_roles`: the party holds nothing, and that row is not a role
    // of its.
    assert!(
        listed_roles(&app, &token, party).await.is_empty(),
        "another tenant's role row was listed as this party's"
    );

    // `soft_delete_party_role`: there is nothing here to remove, and the other
    // tenant's row is not what the close may reach for instead.
    assert_eq!(
        app.delete(&role_path(party, "SUPPLIER"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert!(
        !is_closed(&app, foreign_role).await,
        "removing a role this party does not hold closed another tenant's row"
    );

    // `find_live_party_role`: assigning is a create, because what exists is not
    // this tenant's. A 200 here would mean the service had updated their row.
    let assigned = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(
        assigned.status,
        StatusCode::CREATED,
        "the assignment updated another tenant's role row: {}",
        assigned.body
    );
    assert_eq!(listed_roles(&app, &token, party).await, vec!["SUPPLIER"]);
    assert!(
        !is_closed(&app, foreign_role).await,
        "the assignment rewrote another tenant's role row"
    );
}

#[tokio::test]
async fn removing_a_role_twice_is_a_404_the_second_time() {
    // `soft_delete_party_role` counts the rows it closed and the service turns
    // zero into the 404. Without its `deleted_at` filter the second call would
    // close an already-closed row, report one, and answer 204.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;

    assert_eq!(
        app.delete(&role_path(party, "SUPPLIER"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&role_path(party, "SUPPLIER"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_profile_row_stamped_with_another_tenant_is_not_read_back() {
    // The role stays this tenant's, so `load_roles` asks for all four profiles
    // and each profile query answers for itself. Restamping the role instead
    // would take `holds()` to false and the profile would be absent because it
    // was never asked for - which is the shape this issue is about.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = party_holding_every_profiled_role(&app, &token, "0001").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;

    for (table, _) in PROFILE_TABLES {
        // Table names interpolated from the constant above, never from data;
        // the values are still bound (coding standard §2.5).
        let moved = sqlx::query(&format!(
            "UPDATE {table} SET tenant_id = $1 WHERE party_id = $2"
        ))
        .bind(other_tenant)
        .bind(party)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("restamp {table}: {error}"))
        .rows_affected();

        assert_eq!(moved, 1, "{table} had no profile row to restamp");
    }

    let response = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await;
    let data = response.data();

    assert_eq!(
        listed_roles(&app, &token, party).await,
        vec!["CONTACT", "CUSTOMER", "EMPLOYEE", "SUPPLIER"],
        "the roles went with the profiles: {data}"
    );

    for (table, member) in PROFILE_TABLES {
        assert!(
            data["profiles"][member].is_null(),
            "{table} was read across a tenant boundary: {data}"
        );
    }
}

#[tokio::test]
async fn a_soft_deleted_profile_row_is_not_read_back() {
    // The mirror of the test above, for the other filter each profile query
    // carries. Removing the role closes both rows together, so this closes the
    // profile alone: it is the profile query's own filter under test.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = party_holding_every_profiled_role(&app, &token, "0001").await;

    for (table, _) in PROFILE_TABLES {
        let closed = sqlx::query(&format!(
            "UPDATE {table} SET deleted_at = now() WHERE party_id = $1"
        ))
        .bind(party)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("soft-delete {table}: {error}"))
        .rows_affected();

        assert_eq!(closed, 1, "{table} had no profile row to soft-delete");
    }

    let response = app
        .get(&format!("{PARTIES}/{party}/roles"), Some(&token))
        .await;
    let data = response.data();

    assert_eq!(
        listed_roles(&app, &token, party).await,
        vec!["CONTACT", "CUSTOMER", "EMPLOYEE", "SUPPLIER"],
        "the roles went with the profiles: {data}"
    );

    for (table, member) in PROFILE_TABLES {
        assert!(
            data["profiles"][member].is_null(),
            "a soft-deleted {table} row was read: {data}"
        );
    }
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assigning_and_removing_a_role_are_audited_under_the_business_name() {
    // Acceptance criterion 6. Naming convention §7: a master-data event uses
    // the role's domain name — `Supplier.Created` — even though the storage is
    // party-based, because "a supplier was created" is what happened.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;
    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        json!({ "fromDate": "2026-02-01T00:00:00Z" }),
    )
    .await;
    app.delete(&role_path(party, "SUPPLIER"), Some(&token))
        .await;

    let events: Vec<(String, String)> = sqlx::query_as(
        "SELECT event_type, action
           FROM audit_events
          WHERE object_id = $1 AND action LIKE 'ROLE_%'
          ORDER BY created_at, id",
    )
    .bind(party)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(
        events,
        vec![
            ("Supplier.Created".to_owned(), "ROLE_ASSIGNED".to_owned()),
            ("Supplier.Updated".to_owned(), "ROLE_UPDATED".to_owned()),
            ("Supplier.Removed".to_owned(), "ROLE_REMOVED".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_tenant_role_type_is_audited_under_the_name_the_tenant_gave_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, person("PARTY-AUD", "Sri", "Wahyuni")).await;

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, 'ORGANIZATION_UNIT_LEAD', 'Unit Lead', false)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("insert a tenant role type");

    app.put(
        &role_path(party, "ORGANIZATION_UNIT_LEAD"),
        Some(&token),
        json!({ "fromDate": "2026-01-01T00:00:00Z" }),
    )
    .await;

    let event_type: String = sqlx::query_scalar(
        "SELECT event_type FROM audit_events WHERE object_id = $1 AND action = 'ROLE_ASSIGNED'",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("the assignment was not audited");

    // Dotted PascalCase, per naming convention §7 — not the SCREAMING_SNAKE the
    // code column holds.
    assert_eq!(event_type, "OrganizationUnitLead.Created");
}

#[tokio::test]
async fn a_role_type_from_another_tenant_does_not_resolve() {
    // Separate from the party-scoping case above, which this does not cover:
    // there, all three routes refuse at the party lookup and never reach the
    // role type. Here the party is the caller's own, so the only thing standing
    // between it and another tenant's vocabulary is the scoping on
    // `find_role_type_id` — and a role type that crossed the boundary would be
    // a foreign key from this tenant's data into that one's.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, 'FOREIGN_ROLE', 'Foreign Role', false)",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role type");

    let assigned = app
        .put(
            &role_path(party, "FOREIGN_ROLE"),
            Some(&token),
            json!({ "fromDate": "2026-01-01T00:00:00Z" }),
        )
        .await;

    assert_eq!(
        assigned.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another tenant's role type resolved: {}",
        assigned.body
    );
    assert_eq!(assigned.body["error"]["details"][0]["path"], "roleTypeId");

    let roles: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_party_roles")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(roles, 0, "the role was assigned across a tenant boundary");
}

// ---------------------------------------------------------------------------
// Deleting the party (#103)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn deleting_a_party_releases_the_business_numbers_it_held() {
    // The five steps from #103, which succeeded through step 4 and failed at
    // step 5 before this was fixed. `uq_mdm_supplier_profiles_tenant_id_supplier_number`
    // is partial on `deleted_at IS NULL`, so a profile left live behind a
    // deleted party kept the number — and no route could reach it to release
    // it, because `remove_role` 404s at the party lookup.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = create_party(&app, &token, party_group("PARTY-A", "Acme Supplies")).await;
    let assigned = app
        .put(
            &role_path(first, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    let deleted = app
        .delete(&format!("{PARTIES}/{first}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    // The party code is released by the same kind of partial index, so this
    // half already worked and is asserted to keep the scenario honest.
    let second = create_party(&app, &token, party_group("PARTY-A", "Acme Supplies Again")).await;

    let reassigned = app
        .put(
            &role_path(second, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(
        reassigned.status,
        StatusCode::CREATED,
        "the supplier number is still held by the deleted party: {}",
        reassigned.body
    );

    let live_for_first: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mdm_supplier_profiles WHERE party_id = $1 AND deleted_at IS NULL",
    )
    .bind(first)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");
    assert_eq!(live_for_first, 0, "the deleted party kept a live profile");
}

#[tokio::test]
async fn deleting_a_party_leaves_no_live_role_or_profile_behind_it() {
    // #103 acceptance criterion 1, across every role the party held rather
    // than only the first — the close is one statement over all of them, and a
    // version that closed one would pass a single-role test.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme Trading")).await;
    for (role, body) in [
        ("SUPPLIER", supplier_profile("SUP-0001")),
        ("CUSTOMER", customer_profile("CUS-0001")),
    ] {
        let assigned = app.put(&role_path(party, role), Some(&token), body).await;
        assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);
    }

    let deleted = app
        .delete(&format!("{PARTIES}/{party}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    for table in [
        "mdm_party_roles",
        "mdm_supplier_profiles",
        "mdm_customer_profiles",
    ] {
        let live: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE party_id = $1 AND deleted_at IS NULL"
        ))
        .bind(party)
        .fetch_one(&app.pool)
        .await
        .expect("query runs");

        assert_eq!(live, 0, "{table} kept a live row behind a deleted party");
    }

    // Closed, not erased: the history the removal path is careful to keep is
    // kept here too, and `ends_at` says when it stopped.
    let closed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mdm_party_roles
          WHERE party_id = $1 AND deleted_at IS NOT NULL AND ends_at IS NOT NULL",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");
    assert_eq!(
        closed, 2,
        "the delete erased the role history it should keep"
    );
}

#[tokio::test]
async fn deleting_one_party_does_not_close_another_partys_roles() {
    // The close is aimed at one party. A version scoped to the tenant rather
    // than to the party would take every supplier in the tenant out with the
    // one being deleted, and the request would still answer 204.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // The survivor exists *before* the delete, which is what makes this test
    // able to fail: created afterwards, it could not be affected by it.
    let survivor = create_party(&app, &token, party_group("PARTY-LIVE", "Still Trading")).await;
    let kept = app
        .put(
            &role_path(survivor, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0002"),
        )
        .await;
    assert_eq!(kept.status, StatusCode::CREATED, "{}", kept.body);

    let doomed = create_party(&app, &token, party_group("PARTY-ACME", "Acme Trading")).await;
    let assigned = app
        .put(
            &role_path(doomed, "CUSTOMER"),
            Some(&token),
            customer_profile("CUS-0001"),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    let deleted = app
        .delete(&format!("{PARTIES}/{doomed}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    for (table, party, label) in [
        ("mdm_party_roles", survivor, "role"),
        ("mdm_supplier_profiles", survivor, "supplier profile"),
    ] {
        let live: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE party_id = $1 AND deleted_at IS NULL"
        ))
        .bind(party)
        .fetch_one(&app.pool)
        .await
        .expect("query runs");

        assert_eq!(live, 1, "deleting one party closed another party's {label}");
    }

    // And the survivor is still reachable as the supplier it is.
    let aggregate = app
        .get(&format!("{PARTIES}/{survivor}"), Some(&token))
        .await;
    assert_eq!(aggregate.status, StatusCode::OK, "{}", aggregate.body);
    assert_eq!(
        aggregate.data()["profiles"]["supplier"]["supplierNumber"],
        "SUP-0002"
    );
}

#[tokio::test]
async fn a_delete_that_finds_nothing_writes_nothing() {
    // The party lookup refuses before the close runs, so a 404 leaves the
    // tenant exactly as it was — including the second delete of a party that
    // was already deleted.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme Trading")).await;
    let assigned = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            supplier_profile("SUP-0001"),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    let first = app
        .delete(&format!("{PARTIES}/{party}"), Some(&token))
        .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.body);

    let closed_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM mdm_party_roles WHERE party_id = $1")
            .bind(party)
            .fetch_one(&app.pool)
            .await
            .expect("query runs");

    let again = app
        .delete(&format!("{PARTIES}/{party}"), Some(&token))
        .await;
    assert_eq!(again.status, StatusCode::NOT_FOUND, "{}", again.body);

    let unchanged: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM mdm_party_roles WHERE party_id = $1")
            .bind(party)
            .fetch_one(&app.pool)
            .await
            .expect("query runs");
    assert_eq!(
        closed_at, unchanged,
        "a refused delete rewrote the history of an already-closed role"
    );

    let missing = app
        .delete(&format!("{PARTIES}/{}", Uuid::now_v7()), Some(&token))
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.body);
}

// ---------------------------------------------------------------------------
// Concurrency (#105)
// ---------------------------------------------------------------------------

/// A role type this tenant defines, with **no profile table behind it**.
///
/// That is deliberate and is #105 acceptance criterion 2. For the four profiled
/// roles the duplicate is masked into a different symptom: the profile's
/// `uq_…_party_id` index rejects the second insert, so the race surfaces as a
/// spurious 409 rather than as two live roles. A concurrency test written
/// against SUPPLIER would be watching the mask, not the defect.
async fn given_role_type_without_a_profile(app: &TestApp, code: &str) {
    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, $3, $4, false)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert the tenant role type");
}

async fn live_roles(app: &TestApp, party: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM mdm_party_roles WHERE party_id = $1 AND deleted_at IS NULL",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("query runs")
}

#[tokio::test]
async fn two_concurrent_assignments_leave_the_party_holding_the_role_once() {
    // #105, and the reason it is written as a loop rather than as one attempt:
    // the verifier's first single-shot probe passed. The defect surfaced on the
    // thirtieth run. **A concurrency test that runs once and goes green is not
    // evidence**, so this runs the race twenty times and every round has to
    // hold.
    //
    // The two requests carry different `fromDate` values on purpose. The unique
    // index includes `starts_at`, so identical dates would collide in the
    // database and the service would look correct for a reason that is not the
    // one under test.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_role_type_without_a_profile(&app, "DISTRIBUTOR").await;

    const ROUNDS: usize = 20;

    for round in 0..ROUNDS {
        let party = create_party(
            &app,
            &token,
            party_group(&format!("PARTY-{round:04}"), "Acme Distribution"),
        )
        .await;
        let path = role_path(party, "DISTRIBUTOR");

        let (first, second) = tokio::join!(
            app.put(
                &path,
                Some(&token),
                json!({ "fromDate": "2026-01-01T00:00:00Z" }),
            ),
            app.put(
                &path,
                Some(&token),
                json!({ "fromDate": "2026-02-01T00:00:00Z" }),
            ),
        );

        assert_eq!(
            live_roles(&app, party).await,
            1,
            "round {round}: the party holds DISTRIBUTOR twice — {} and {}",
            first.body,
            second.body
        );

        // Both requests succeeded: one created the assignment and the other
        // updated it. Serialising the two must not turn a legitimate request
        // into an error.
        for (label, response) in [("first", &first), ("second", &second)] {
            assert!(
                response.status == StatusCode::CREATED || response.status == StatusCode::OK,
                "round {round}: the {label} request failed with {} — {}",
                response.status,
                response.body
            );
        }

        // Exactly one of them created it. Two 201s would mean two inserts even
        // if one was later cleaned up, and two 200s would mean neither reported
        // the create that must have happened.
        let created = [&first, &second]
            .iter()
            .filter(|response| response.status == StatusCode::CREATED)
            .count();
        assert_eq!(
            created, 1,
            "round {round}: {created} of the two requests reported a create — {} and {}",
            first.body, second.body
        );
    }
}

#[tokio::test]
async fn concurrent_assignment_of_a_profiled_role_is_not_a_conflict() {
    // #105 acceptance criterion 3. Before the fix the profile's unique index
    // caught what the service missed, and the loser got
    // `409 That profile number is already in use` — a misleading answer to a
    // request that did nothing wrong, about a number it was entitled to send.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    const ROUNDS: usize = 10;

    for round in 0..ROUNDS {
        let party = create_party(
            &app,
            &token,
            party_group(&format!("PARTY-{round:04}"), "Acme Supplies"),
        )
        .await;
        let path = role_path(party, "SUPPLIER");
        let number = format!("SUP-{round:04}");

        // Different `fromDate`s, for the same reason as the test above: with
        // identical ones the *role* index collides and the profile index never
        // gets the chance to produce the spurious 409 this is about. The first
        // version of this test sent the same date twice and could not fail —
        // the mutation that restores the defect left it green.
        let (first, second) = tokio::join!(
            app.put(
                &path,
                Some(&token),
                supplier_profile_from(&number, "2026-01-01T00:00:00Z"),
            ),
            app.put(
                &path,
                Some(&token),
                supplier_profile_from(&number, "2026-02-01T00:00:00Z"),
            ),
        );

        for (label, response) in [("first", &first), ("second", &second)] {
            assert!(
                response.status == StatusCode::CREATED || response.status == StatusCode::OK,
                "round {round}: the {label} request answered {} — {}",
                response.status,
                response.body
            );
        }

        let created = [&first, &second]
            .iter()
            .filter(|response| response.status == StatusCode::CREATED)
            .count();
        assert_eq!(
            created, 1,
            "round {round}: {created} of the two requests reported a create — {} and {}",
            first.body, second.body
        );

        assert_eq!(
            live_roles(&app, party).await,
            1,
            "round {round}: the party holds SUPPLIER twice"
        );

        let profiles: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM mdm_supplier_profiles
              WHERE party_id = $1 AND deleted_at IS NULL",
        )
        .bind(party)
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
        assert_eq!(
            profiles, 1,
            "round {round}: the party has two live profiles"
        );
    }
}

#[tokio::test]
async fn a_party_deleted_mid_assignment_does_not_get_the_role() {
    // The second race the lock closes. The party is read inside the
    // transaction that writes, so a delete that commits first is seen: the
    // assignment answers 404 rather than writing a live role onto a party that
    // no longer exists — which #103 has just finished cleaning up after.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_role_type_without_a_profile(&app, "DISTRIBUTOR").await;

    for round in 0..10 {
        let party = create_party(
            &app,
            &token,
            party_group(&format!("PARTY-{round:04}"), "Acme Distribution"),
        )
        .await;

        let delete_path = format!("{PARTIES}/{party}");
        let assign_path = role_path(party, "DISTRIBUTOR");

        let (deleted, assigned) = tokio::join!(
            app.delete(&delete_path, Some(&token)),
            app.put(
                &assign_path,
                Some(&token),
                json!({ "fromDate": "2026-01-01T00:00:00Z" }),
            ),
        );

        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

        // Either order is legal — the assignment may win the race and be
        // closed by the delete, or lose it and be refused. What must not
        // happen is a live role on a deleted party.
        assert!(
            assigned.status == StatusCode::CREATED || assigned.status == StatusCode::NOT_FOUND,
            "round {round}: unexpected {} — {}",
            assigned.status,
            assigned.body
        );
        assert_eq!(
            live_roles(&app, party).await,
            0,
            "round {round}: a deleted party kept a live role, assigned answered {}",
            assigned.status
        );
    }
}

// ---------------------------------------------------------------------------
// One connection at a time (#118)
// ---------------------------------------------------------------------------

/// A department for a profile to name, so that resolving the profile has a
/// query to run. Without one, `resolve_profile_references` returns before it
/// touches the database and the test below cannot fail.
async fn given_department(app: &TestApp, code: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, $3, 'Procurement')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert a department");

    id
}

#[tokio::test]
async fn assigning_a_role_takes_one_connection_at_a_time() {
    // #118. `assign_role` opened its transaction — one connection, held until
    // commit — and then called `resolve_profile_references`, which runs on the
    // pool and takes a second. Ten concurrent profiled assignments against the
    // production ceiling of ten therefore waited on connections held by each
    // other, stalled for the acquire timeout and answered 500. A self-deadlock,
    // not contention: the requests were not waiting on the database.
    //
    // Written by holding connections rather than by racing requests, because
    // racing cannot express it here. Two concurrent requests need four
    // connections and `TEST_POOL_MAX_CONNECTIONS` is five, which is why the two
    // races #116 added stayed green through the whole of #118's lifetime.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let department = given_department(&app, "DEPT-ONECONN").await;
    let employee = create_party(&app, &token, person("PARTY-ONECONN", "Ada", "Byron")).await;

    // Every connection but one. What is left is enough for a request that needs
    // one at a time and not enough for one that holds a transaction open while
    // acquiring a second.
    let mut held = Vec::new();
    for _ in 1..common::TEST_POOL_MAX_CONNECTIONS {
        held.push(
            app.pool
                .acquire()
                .await
                .expect("the harness pool has a connection to hold"),
        );
    }

    let assigned = app
        .put(
            &role_path(employee, "EMPLOYEE"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": {
                    "employee": {
                        "employeeNumber": "EMP-ONECONN",
                        "departmentId": department,
                    }
                },
            }),
        )
        .await;

    drop(held);

    assert_eq!(
        assigned.status,
        StatusCode::CREATED,
        "an assignment answered {} with one connection free — it took two: {}",
        assigned.status,
        assigned.body
    );
}

#[tokio::test]
async fn a_request_aimed_at_no_party_is_a_404_before_its_profile_is_resolved() {
    // The ordering #118's fix had to keep. Hoisting `resolve_profile_references`
    // out of the transaction moves it ahead of the locked party lookup, so
    // without the read that precedes it a request aimed at nothing would be
    // answered `422 no such department` instead of `404 no such party` — a
    // silent contract change inside an availability fix.
    //
    // It is also the ordering `list_role_view` argues for: refuse on the
    // resource before reading the request.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .put(
            &role_path(Uuid::now_v7(), "EMPLOYEE"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": {
                    "employee": {
                        "employeeNumber": "EMP-NOWHERE",
                        "departmentId": Uuid::now_v7(),
                    }
                },
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "a request aimed at no party was answered about its profile: {}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// The predicates the fixes' own queries carry (#121)
// ---------------------------------------------------------------------------

/// A role type with this code in `tenant`, so that a role row stamped with that
/// tenant is reachable by a query that has stopped scoping.
///
/// `a_role_row_stamped_with_another_tenant_is_not_mine_to_read_or_remove` points
/// its foreign row at *this* tenant's role type, which is enough for the three
/// queries it probes because none of them joins `mdm_role_types` on the tenant.
/// The read-back below needs a role type of the other tenant's own, so that the
/// foreign row is a genuine second candidate rather than one the join drops.
async fn given_foreign_role_type(app: &TestApp, tenant: Uuid, code: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, $3, $4, false)",
    )
    .bind(id)
    .bind(tenant)
    .bind(code)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role type");

    id
}

#[tokio::test]
async fn the_assignment_answered_with_is_the_row_this_call_wrote() {
    // #121, and the reason the read-back is by primary key.
    //
    // `find_party_role` looked the row up again by
    // `(tenant_id, party_id, role_type_code)`, which matches one row only
    // because of the tenant predicate — so the predicate could not be pinned by
    // a test: dropping it made the query match two rows and `fetch_optional`
    // return an unspecified one, and asserting which would have been asserting
    // on undefined behaviour. Under the mutation this test passed, which is
    // exactly the shape #106 was about.
    //
    // The read-back is now by the assignment's own id, so a second candidate
    // cannot exist. This test is what says so: another tenant holding the same
    // role code on the same party is present, and the answer is still ours.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let their_supplier = given_foreign_role_type(&app, other_tenant, "SUPPLIER").await;

    // Their SUPPLIER assignment on our party: the same party, the same role
    // code, and a `starts_at` of its own so it cannot collide with ours.
    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at, comments)
         VALUES ($1, $2, $3, $4, TIMESTAMPTZ '2025-01-01T00:00:00Z', 'the other tenant''s row')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(party)
    .bind(their_supplier)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role row");

    let mut assignment = supplier_profile("SUP-0001");
    assignment["comments"] = json!("ours");

    let assigned = app
        .put(&role_path(party, "SUPPLIER"), Some(&token), assignment)
        .await;

    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);
    assert_eq!(
        assigned.data()["comments"],
        "ours",
        "the assign route answered with another tenant's role row: {}",
        assigned.body
    );
    assert_eq!(
        assigned.data()["fromDate"],
        "2026-01-01T00:00:00Z",
        "the assign route answered with another tenant's dates: {}",
        assigned.body
    );

    // And the update path, which reads back the row it updated rather than the
    // one it inserted.
    let restated = app
        .put(
            &role_path(party, "SUPPLIER"),
            Some(&token),
            json!({ "fromDate": "2026-03-01T00:00:00Z" }),
        )
        .await;

    assert_eq!(restated.status, StatusCode::OK, "{}", restated.body);
    assert_eq!(
        restated.data()["fromDate"],
        "2026-03-01T00:00:00Z",
        "the restatement answered with a row it did not write: {}",
        restated.body
    );
}

#[tokio::test]
async fn deleting_a_party_closes_this_tenants_roles_and_leaves_anothers_alone() {
    // #121. `soft_delete_party_roles` was added by the fix for #103 to stop a
    // delete from leaving a live role behind. It closes by party rather than by
    // role type, so its tenant predicate is the only thing keeping the sweep
    // inside the caller's tenant — and nothing exercised it, for the same
    // reason as above.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let party = create_party(&app, &token, party_group("PARTY-ACME", "Acme")).await;

    app.put(
        &role_path(party, "SUPPLIER"),
        Some(&token),
        supplier_profile("SUP-0001"),
    )
    .await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let supplier = role_type_id(&app, "SUPPLIER").await;
    let foreign_role = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at)
         VALUES ($1, $2, $3, $4, TIMESTAMPTZ '2025-01-01T00:00:00Z')",
    )
    .bind(foreign_role)
    .bind(other_tenant)
    .bind(party)
    .bind(supplier)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role row");

    let deleted = app
        .delete(&format!("{PARTIES}/{party}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    // Ours closed — that is #103's fix, and it still holds.
    assert_eq!(
        live_roles(&app, party).await,
        1,
        "the delete left one of this party's roles live, or closed the wrong count"
    );

    // Theirs did not.
    assert!(
        !is_closed(&app, foreign_role).await,
        "deleting a party closed another tenant's role row"
    );
}
