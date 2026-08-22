//! One party, many roles (FR-MDM-002; issue #81).
//!
//! The claim under test is the Party model's whole point: a party that is both
//! a supplier and a customer is one party with two roles, not two records. A
//! test that only assigned one role would pass against a design that stored a
//! second party behind the scenes, so the two-role case is the first one here.

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

    let profile = assigned.data()["profiles"]["employee"].clone();
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

    let roles = again.data()["roles"].as_array().expect("roles is a list");
    assert_eq!(
        roles.len(),
        1,
        "the party holds SUPPLIER twice: {}",
        again.body
    );
    assert_eq!(roles[0]["fromDate"], "2026-03-01T00:00:00Z");
    assert_eq!(roles[0]["comments"], "renegotiated");

    // The profile was updated in place: the number it was not asked to change
    // is still there, and the field it was asked to change moved.
    let supplier = &again.data()["profiles"]["supplier"];
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
        again.data()["profiles"]["supplier"]["supplierNumber"],
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
    assert_eq!(assigned.data()["roles"][0]["roleTypeId"], "AUDITOR");
    // A role type with no profile table carries no profile.
    assert!(assigned.data()["profiles"]
        .as_object()
        .is_some_and(|p| p.is_empty()));
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
