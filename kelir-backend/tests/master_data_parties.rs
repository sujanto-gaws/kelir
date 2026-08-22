//! Party master data through the API (FR-MDM-001, FR-MDM-003; issue #80).
//!
//! The aggregate is the contract, so these tests drive it as a client would:
//! a party is created with its identifications, classifications, contact
//! mechanisms and relationships in one payload, and every one of them has to
//! come back off `GET`. A create path that silently dropped a collection would
//! pass a test that only checked the status code.
//!
//! # What the scoping tests here reach (#106)
//!
//! Stated because two of them used to reach almost nothing. Both filters of
//! every query the aggregate reads through were mutated, and each mutation
//! turned a named test red: `find_party`, `list_parties`, `find_person`,
//! `find_party_group`, `list_identifications`, `list_classifications`,
//! `list_contact_mechs` and `list_relationships` on `tenant_id` and on
//! `deleted_at`; `list_statuses` on `tenant_id` alone, because
//! `mdm_party_statuses` is append-only and has no `deleted_at` to filter.
//! `count_parties` was probed on both filters too. It was the *only* thing the
//! two list tests reached before this, which is what #106 is about.
//!
//! Not reached, and left so knowingly: the write-path queries — `insert_*`,
//! `update_*`, `replace_*`, `soft_delete_party` — and `find_party_id_by_code`.
//! The relationship half of `a_party_in_another_tenant_is_not_visible` asserts
//! that a foreign party code does not resolve, but that assertion was not
//! mutation-probed here. Isolating any of them from the `find_party` gate that
//! refuses ahead of them needs a party whose own tenant stamp disagrees with
//! the caller's, which is a different fixture from the ones below.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";

/// A person party with one of everything the aggregate carries, so a dropped
/// collection shows up as a missing element rather than as nothing at all.
fn full_person(party_code: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PERSON",
        "description": "Procurement contact",
        "externalId": "SAP-000123",
        "person": {
            "firstName": "Jane",
            "middleName": "Quill",
            "lastName": "Doe",
            "personalTitle": "Ms",
            "gender": "F",
            "birthDate": "1988-04-17",
        },
        "identifications": [{
            "partyIdentificationTypeId": "PASSPORT_NUMBER",
            "idValue": "X1234567",
            "issuedBy": "Direktorat Jenderal Imigrasi",
            "issueDate": "2021-01-05",
            "expireDate": "2031-01-04",
        }],
        "classifications": [{
            "partyClassTypeId": "CONTACT_TIER",
            "partyClassificationId": "TIER_1",
            "fromDate": "2026-01-01T00:00:00Z",
        }],
        "contactMechanisms": [
            {
                "contactMechTypeId": "EMAIL_ADDRESS",
                "purposeTypeId": "PRIMARY_OFFICE",
                "fromDate": "2026-01-01T00:00:00Z",
                "isPrimary": true,
                "detail": { "emailAddress": "jane@acme.example" },
            },
            {
                "contactMechTypeId": "POSTAL_ADDRESS",
                "fromDate": "2026-01-01T00:00:00Z",
                "detail": {
                    "postalAddress": {
                        "address1": "1 Jalan Merdeka",
                        "city": "Jakarta",
                        "postalCode": "10110",
                        "countryGeoId": "IDN",
                    }
                },
            },
        ],
        "additionalAttributes": { "preferredLanguage": "id" },
    })
}

fn party_group(party_code: &str, name: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PARTY_GROUP",
        "partyGroup": { "groupName": name, "annualRevenue": "1234567.89", "numEmployees": 42 },
    })
}

/// The `partyId` of every row a list response actually returned.
///
/// The rows, not `meta.total` — they come from different statements, and a test
/// that only reads the total is reporting on `count_parties` while claiming to
/// cover `list_parties` (#106 F6).
fn party_codes(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap_or_else(|| panic!("data is not a list: {body}"))
        .iter()
        .map(|row| {
            row["partyId"]
                .as_str()
                .unwrap_or_else(|| panic!("a list row carries no partyId: {body}"))
                .to_owned()
        })
        .collect()
}

async fn create(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    let response = app.post(PARTIES, Some(token), body).await;

    (response.status, response.body)
}

/// Creates a party and returns its surrogate id, failing the test rather than
/// the next assertion if the create itself was refused.
async fn create_ok(app: &TestApp, token: &str, body: Value) -> Uuid {
    let (status, body) = create(app, token, body).await;

    assert_eq!(status, StatusCode::CREATED, "create refused: {body}");

    body["data"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the created party carries no id: {body}"))
}

// ---------------------------------------------------------------------------
// Create and read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creates_a_person_party_and_reads_the_whole_aggregate_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    let response = app.get(&format!("{PARTIES}/{id}"), Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "body {}", response.body);

    let party = response.data();

    assert_eq!(party["partyId"], "PARTY-0001");
    assert_eq!(party["partyTypeId"], "PERSON");
    assert_eq!(party["statusId"], "PARTY_ENABLED");
    assert_eq!(party["externalId"], "SAP-000123");
    assert_eq!(party["person"]["firstName"], "Jane");
    assert_eq!(party["person"]["lastName"], "Doe");
    assert_eq!(party["person"]["gender"], "F");
    assert_eq!(party["person"]["birthDate"], "1988-04-17");
    // The aggregate repeats the party's code inside `person`; a client
    // round-tripping the document needs it to be the party's own.
    assert_eq!(party["person"]["partyId"], "PARTY-0001");
    assert!(
        party["partyGroup"].is_null(),
        "a PERSON party must not carry group detail: {party}"
    );

    assert_eq!(party["identifications"][0]["idValue"], "X1234567");
    assert_eq!(
        party["identifications"][0]["partyIdentificationTypeId"],
        "PASSPORT_NUMBER"
    );
    assert_eq!(
        party["classifications"][0]["partyClassificationId"],
        "TIER_1"
    );
    assert_eq!(party["additionalAttributes"]["preferredLanguage"], "id");

    let mechanisms = party["contactMechanisms"]
        .as_array()
        .expect("contactMechanisms is a list");
    assert_eq!(mechanisms.len(), 2, "{party}");
    // Primary first, which is the order the list ordering promises.
    assert_eq!(mechanisms[0]["contactMechTypeId"], "EMAIL_ADDRESS");
    assert_eq!(mechanisms[0]["isPrimary"], true);
    assert_eq!(mechanisms[0]["detail"]["emailAddress"], "jane@acme.example");
    assert_eq!(mechanisms[0]["purposeTypeId"], "PRIMARY_OFFICE");
    assert_eq!(
        mechanisms[1]["detail"]["postalAddress"]["city"], "Jakarta",
        "the postal detail did not survive the round trip: {party}"
    );

    // The creation status is history from the first moment, not from the first
    // change: FR-MDM-003 asks for statuses, and an empty list until someone
    // edits the party would not be one.
    assert_eq!(party["statuses"][0]["statusId"], "PARTY_ENABLED");
    assert_eq!(
        party["statuses"][0]["changedByUserLogin"],
        common::ADMIN_USERNAME
    );
}

#[tokio::test]
async fn the_display_value_is_derived_from_the_detail_rather_than_taken_from_the_client() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    let values: Vec<String> = sqlx::query_scalar(
        "SELECT m.display_value
           FROM mdm_party_contact_mechs l
           JOIN mdm_contact_mechs m ON m.id = l.contact_mech_id
          WHERE l.party_id = $1
          ORDER BY m.contact_mech_type",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(
        values,
        vec![
            "jane@acme.example".to_owned(),
            "1 Jalan Merdeka, Jakarta, 10110, IDN".to_owned(),
        ],
        "display_value is the one-line projection lists read (§4.10)"
    );
}

#[tokio::test]
async fn creates_a_party_group() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let party = app.get(&format!("{PARTIES}/{id}"), Some(&token)).await.body["data"].clone();

    assert_eq!(party["partyTypeId"], "PARTY_GROUP");
    assert_eq!(party["partyGroup"]["groupName"], "Acme Supplies");
    assert_eq!(party["partyGroup"]["numEmployees"], 42);
    // NUMERIC(18,2) travels as a string: a JSON double cannot hold every value
    // the column can.
    assert_eq!(party["partyGroup"]["annualRevenue"], "1234567.89");
    assert!(party["person"].is_null(), "{party}");
}

#[tokio::test]
async fn the_lifecycle_columns_arrive_at_their_documented_defaults() {
    // Acceptance criterion 5 of #80: `record_status` defaults to DRAFT and the
    // document references stay null until Phase 4. Nothing on the API can move
    // them, so this is asserted where they live.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    let (record_status, created_by_document, last_updated_by_document): (
        String,
        Option<Uuid>,
        Option<Uuid>,
    ) = sqlx::query_as(
        "SELECT record_status, created_by_document_id, last_updated_by_document_id
           FROM mdm_parties WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(record_status, "DRAFT");
    assert_eq!(created_by_document, None);
    assert_eq!(last_updated_by_document, None);
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refuses_a_person_party_with_no_person_detail() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (status, body) = create(
        &app,
        &token,
        json!({ "partyId": "PARTY-0001", "partyTypeId": "PERSON" }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"]["details"][0]["path"], "person");
}

#[tokio::test]
async fn refuses_a_second_party_with_the_same_party_id() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_ok(&app, &token, full_person("PARTY-0001")).await;
    let (status, body) = create(&app, &token, full_person("PARTY-0001")).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "CONFLICT");
}

#[tokio::test]
async fn refuses_a_field_the_contract_does_not_have() {
    // #62: a misspelled field must be named, not discarded. `partyID` for
    // `partyId` would otherwise create a party with an empty code.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (status, body) = create(
        &app,
        &token,
        json!({
            "partyId": "PARTY-0001",
            "partyTypeId": "PERSON",
            "person": { "firstName": "Jane", "lastName": "Doe" },
            "partyRoles": [],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body.to_string().contains("partyRoles"),
        "the response does not name the field that was rejected: {body}"
    );
}

// ---------------------------------------------------------------------------
// Relationships
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_relationship_is_readable_from_both_of_its_ends() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let employer = create_ok(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let mut employee = full_person("PARTY-0001");
    employee["relationshipsFrom"] = json!([{
        "partyIdFrom": "PARTY-0001",
        "partyIdTo": "PARTY-ACME",
        "partyRelationshipTypeId": "EMPLOYMENT",
        "roleTypeIdFrom": "EMPLOYEE",
        "fromDate": "2026-01-01T00:00:00Z",
    }]);
    let employee = create_ok(&app, &token, employee).await;

    let from_side = app
        .get(&format!("{PARTIES}/{employee}"), Some(&token))
        .await
        .body["data"]["relationshipsFrom"]
        .clone();
    assert_eq!(from_side[0]["partyIdTo"], "PARTY-ACME");
    assert_eq!(from_side[0]["partyRelationshipTypeId"], "EMPLOYMENT");
    assert_eq!(
        from_side[0]["roleTypeIdFrom"], "EMPLOYEE",
        "the role type came back as something other than the code that was sent: {from_side}"
    );

    // The same row, projected from the other party. One table, two directions
    // (§4.8) — so the employer sees the relationship without anyone writing it
    // twice.
    let to_side = app
        .get(&format!("{PARTIES}/{employer}"), Some(&token))
        .await
        .body["data"]["relationshipsTo"]
        .clone();
    assert_eq!(to_side[0]["partyIdFrom"], "PARTY-0001");
    assert_eq!(
        from_side[0]["partyRelationshipId"], to_side[0]["partyRelationshipId"],
        "both ends must be the same row, not two"
    );
}

#[tokio::test]
async fn a_relationship_to_a_party_that_does_not_exist_is_refused_by_name() {
    // A foreign-key violation would be a 500 saying nothing a client can act
    // on. The counterparty is resolved before anything is written.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut party = full_person("PARTY-0001");
    party["relationshipsFrom"] = json!([{
        "partyIdFrom": "PARTY-0001",
        "partyIdTo": "PARTY-NOBODY",
        "partyRelationshipTypeId": "EMPLOYMENT",
        "fromDate": "2026-01-01T00:00:00Z",
    }]);

    let (status, body) = create(&app, &token, party).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["details"][0]["path"],
        "relationshipsFrom[0].partyIdTo"
    );

    // And nothing was stored: the party the payload described must not exist.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_parties WHERE party_code = $1")
        .bind("PARTY-0001")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(count, 0, "a refused create left a party behind");
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_update_replaces_the_collections_it_sends_and_leaves_the_rest() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    let response = app
        .put(
            &format!("{PARTIES}/{id}"),
            Some(&token),
            json!({
                "person": { "lastName": "Roe" },
                "identifications": [{
                    "partyIdentificationTypeId": "TAX_ID",
                    "idValue": "99.888.777.6-000",
                }],
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "body {}", response.body);
    let party = response.data();

    // Sent: replaced wholesale.
    assert_eq!(party["identifications"].as_array().map(Vec::len), Some(1));
    assert_eq!(party["identifications"][0]["idValue"], "99.888.777.6-000");

    // Sent, but only one field of it: the rest of the person is untouched.
    assert_eq!(party["person"]["lastName"], "Roe");
    assert_eq!(party["person"]["firstName"], "Jane");

    // Not sent: left alone, rather than emptied.
    assert_eq!(
        party["contactMechanisms"].as_array().map(Vec::len),
        Some(2),
        "an update that did not mention contact mechanisms deleted them: {party}"
    );
    assert_eq!(party["classifications"].as_array().map(Vec::len), Some(1));
    assert_eq!(party["description"], "Procurement contact");
}

#[tokio::test]
async fn replacing_contact_mechanisms_does_not_leave_orphan_rows_behind() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    app.put(
        &format!("{PARTIES}/{id}"),
        Some(&token),
        json!({
            "contactMechanisms": [{
                "contactMechTypeId": "MOBILE_NUMBER",
                "fromDate": "2026-02-01T00:00:00Z",
                "detail": { "telecomNumber": { "countryCode": "+62", "contactNumber": "811 2233" } },
            }],
        }),
    )
    .await;

    let mechanisms: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_contact_mechs")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");

    assert_eq!(
        mechanisms, 1,
        "the two replaced mechanisms are still in the table with nothing linking to them"
    );
}

#[tokio::test]
async fn a_status_change_appends_to_the_history() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    let party = app
        .put(
            &format!("{PARTIES}/{id}"),
            Some(&token),
            json!({ "statusId": "PARTY_DISABLED", "statusComments": "left the company" }),
        )
        .await
        .data()
        .clone();

    assert_eq!(party["statusId"], "PARTY_DISABLED");

    let history = party["statuses"].as_array().expect("statuses is a list");
    assert_eq!(history.len(), 2, "{party}");
    assert_eq!(history[0]["statusId"], "PARTY_ENABLED");
    assert_eq!(history[1]["statusId"], "PARTY_DISABLED");
    assert_eq!(history[1]["comments"], "left the company");

    // Setting the same status again is not a change and must not add a row —
    // otherwise a client that PUTs the whole document repeatedly grows the
    // history without anything having happened.
    let again = app
        .put(
            &format!("{PARTIES}/{id}"),
            Some(&token),
            json!({ "statusId": "PARTY_DISABLED" }),
        )
        .await;
    assert_eq!(
        again.data()["statuses"].as_array().map(Vec::len),
        Some(2),
        "re-sending the current status appended to the history"
    );
}

#[tokio::test]
async fn updating_a_party_that_does_not_exist_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .put(
            &format!("{PARTIES}/{}", Uuid::now_v7()),
            Some(&token),
            json!({ "description": "nothing" }),
        )
        .await;

    assert_eq!(response.status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// List and delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_list_projects_the_name_each_kind_of_party_is_known_by() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    create_ok(&app, &token, full_person("PARTY-0001")).await;
    create_ok(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let response = app.get(PARTIES, Some(&token)).await;
    assert_eq!(response.status, StatusCode::OK, "body {}", response.body);

    let rows = response.body["data"].as_array().expect("data is a list");
    assert_eq!(rows.len(), 2);
    assert_eq!(response.body["meta"]["total"], 2);

    // Ordered by partyId, so PARTY-0001 comes first.
    assert_eq!(rows[0]["name"], "Jane Quill Doe");
    assert_eq!(rows[1]["name"], "Acme Supplies");
}

#[tokio::test]
async fn a_deleted_party_leaves_the_list_and_the_detail_route() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;
    // A survivor, so "the deleted party is gone" is distinguishable from "the
    // list returns nothing at all" (#106 F6).
    create_ok(&app, &token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let deleted = app.delete(&format!("{PARTIES}/{id}"), Some(&token)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let fetched = app.get(&format!("{PARTIES}/{id}"), Some(&token)).await;
    assert_eq!(fetched.status, StatusCode::NOT_FOUND);

    // Both the rows and the count, because they are separate statements:
    // `meta.total` comes from `count_parties` and the rows from `list_parties`,
    // and asserting only the total left the list query with no soft-delete
    // coverage at all (#106 F6).
    let listed = app.get(PARTIES, Some(&token)).await;
    assert_eq!(
        party_codes(&listed.body),
        vec!["PARTY-ACME".to_owned()],
        "the deleted party is still listed: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);

    // Soft delete, not a hard one: the row is still there for a restore and for
    // the audit trail to point at (§1.2).
    let (deleted_at, status): (Option<chrono::DateTime<chrono::Utc>>, String) =
        sqlx::query_as("SELECT deleted_at, status FROM mdm_parties WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row was hard-deleted");

    assert!(deleted_at.is_some());
    assert_eq!(status, "PARTY_DISABLED");
}

#[tokio::test]
async fn deleting_a_party_twice_is_a_404_the_second_time() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;

    assert_eq!(
        app.delete(&format!("{PARTIES}/{id}"), Some(&token))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.delete(&format!("{PARTIES}/{id}"), Some(&token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// Tenancy and audit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_party_in_another_tenant_is_not_visible() {
    // Master data is the first module where a second tenant has rows of its
    // own (decision D-7), so the scoping every query carries is checked with
    // the other tenant's data actually present rather than assumed absent.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let foreign_party = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, 'PARTY-FOREIGN', 'PARTY_GROUP')",
    )
    .bind(foreign_party)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's party");

    // Rows as well as the count: `list_parties` is the read every user hits,
    // and `meta.total` alone could not tell whether it was scoped (#106 F6).
    let listed = app.get(PARTIES, Some(&token)).await;
    assert!(
        party_codes(&listed.body).is_empty(),
        "the other tenant's party is in the list: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);

    let fetched = app
        .get(&format!("{PARTIES}/{foreign_party}"), Some(&token))
        .await;
    assert_eq!(fetched.status, StatusCode::NOT_FOUND);

    // And its code must not resolve as a relationship counterparty either,
    // which is the one place a party is addressed by code rather than by id.
    let mut party = full_person("PARTY-0001");
    party["relationshipsFrom"] = json!([{
        "partyIdFrom": "PARTY-0001",
        "partyIdTo": "PARTY-FOREIGN",
        "partyRelationshipTypeId": "ORGANIZATION_ROLLUP",
        "fromDate": "2026-01-01T00:00:00Z",
    }]);

    let (status, body) = create(&app, &token, party).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another tenant's party resolved as a counterparty: {body}"
    );
}

/// The child tables the aggregate reads back, and the column each is keyed on.
///
/// `mdm_party_statuses` is here for the tenant sweep only: it is append-only
/// (§4.7) and has no `deleted_at` column, so `list_statuses` has no soft-delete
/// filter to probe. `mdm_contact_mechs` is absent because it is reached through
/// its link row rather than by `party_id`; `list_contact_mechs` joins it on
/// `m.deleted_at IS NULL`, and the link row covers the same reach.
const CHILD_TABLES: [(&str, &str); 7] = [
    ("mdm_persons", "party_id"),
    ("mdm_party_groups", "party_id"),
    ("mdm_party_identifications", "party_id"),
    ("mdm_party_statuses", "party_id"),
    ("mdm_party_classifications", "party_id"),
    ("mdm_party_contact_mechs", "party_id"),
    ("mdm_party_relationships", "from_party_id"),
];

/// The subject of the two child-row probes: a person and a group which between
/// them carry a row in every table [`CHILD_TABLES`] names, related to each
/// other so the relationship is read from both directions.
async fn a_person_and_a_group_with_one_of_every_child(app: &TestApp, token: &str) -> [Uuid; 2] {
    let group = create_ok(app, token, party_group("PARTY-ACME", "Acme Supplies")).await;

    let mut person = full_person("PARTY-0001");
    person["relationshipsFrom"] = json!([{
        "partyIdFrom": "PARTY-0001",
        "partyIdTo": "PARTY-ACME",
        "partyRelationshipTypeId": "ORGANIZATION_ROLLUP",
        "fromDate": "2026-01-01T00:00:00Z",
    }]);

    [create_ok(app, token, person).await, group]
}

/// Restamps every child row of `parties` with `tenant`, and fails unless each
/// table had a row to restamp.
///
/// That last part is what stops the assertions downstream from being vacuous: a
/// collection that comes back empty because nothing was ever written proves
/// nothing about the query that would have read it.
async fn move_child_rows_to(app: &TestApp, parties: &[Uuid], tenant: Uuid) {
    for (table, key) in CHILD_TABLES {
        // The table and column names are interpolated because neither can be a
        // bind parameter. They come from the constant above, never from data,
        // and the values are still bound (coding standard §2.5).
        let moved = sqlx::query(&format!(
            "UPDATE {table} SET tenant_id = $1 WHERE {key} = ANY($2)"
        ))
        .bind(tenant)
        .bind(parties)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("restamp {table}: {error}"))
        .rows_affected();

        assert!(moved > 0, "{table} had no row to restamp");
    }
}

/// Soft-deletes every child row of `parties`, on the same terms.
///
/// `mdm_party_statuses` is skipped rather than failing the `rewritten > 0`
/// check: the table is append-only and has no `deleted_at` at all.
async fn close_child_rows(app: &TestApp, parties: &[Uuid]) {
    for (table, key) in CHILD_TABLES {
        if table == "mdm_party_statuses" {
            continue;
        }

        let closed = sqlx::query(&format!(
            "UPDATE {table} SET deleted_at = now() WHERE {key} = ANY($1)"
        ))
        .bind(parties)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("soft-delete {table}: {error}"))
        .rows_affected();

        assert!(closed > 0, "{table} had no row to soft-delete");
    }
}

/// Fails unless the aggregate came back with every child collection empty.
///
/// Applied to both parties, and the two carry different subsets — the person
/// has the identifications and contact mechanisms, the group has the group row
/// and the incoming relationship. [`move_child_rows_to`] is what proves the
/// union covers every table.
fn assert_no_children(party: &Value) {
    assert!(
        party["person"].is_null(),
        "the person row was read: {party}"
    );
    assert!(
        party["partyGroup"].is_null(),
        "the group row was read: {party}"
    );

    for collection in [
        "identifications",
        "statuses",
        "classifications",
        "contactMechanisms",
        "relationshipsFrom",
        "relationshipsTo",
    ] {
        assert_eq!(
            party[collection].as_array().map(Vec::len),
            Some(0),
            "{collection} was read: {party}"
        );
    }
}

#[tokio::test]
async fn a_child_row_stamped_with_another_tenant_is_not_read_back() {
    // The party is the caller's own, which is the whole point (#106 F7). A
    // party in *another* tenant is refused at the `find_party` gate and nothing
    // below it ever runs, so a test written that way absorbs every mutation
    // beneath the gate and covers exactly one query no matter how many it
    // appears to. Here the gate lets the request through and the child query's
    // own `tenant_id` filter is the only thing left standing.
    //
    // That filter is defence in depth — parties are themselves tenant-scoped,
    // so a child row whose stamp disagrees with its party's is a row no route
    // can currently write. This is the probe that says whether the filter is
    // load-bearing or decorative, and the answer has to be the first one.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parties = a_person_and_a_group_with_one_of_every_child(&app, &token).await;
    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;

    move_child_rows_to(&app, &parties, other_tenant).await;

    for party in parties {
        let fetched = app.get(&format!("{PARTIES}/{party}"), Some(&token)).await;
        assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.body);
        assert_no_children(fetched.data());
    }
}

#[tokio::test]
async fn a_soft_deleted_child_row_is_not_read_back() {
    // The mirror of the test above, for the other filter every child query
    // carries. `mdm_party_statuses` keeps its rows: the table is append-only
    // and has no `deleted_at`, so `statuses` is the one collection that stays
    // populated here.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parties = a_person_and_a_group_with_one_of_every_child(&app, &token).await;

    close_child_rows(&app, &parties).await;

    for party in parties {
        let fetched = app.get(&format!("{PARTIES}/{party}"), Some(&token)).await;
        assert_eq!(fetched.status, StatusCode::OK, "{}", fetched.body);

        let data = fetched.data();
        assert!(data["person"].is_null(), "the person row was read: {data}");
        assert!(
            data["partyGroup"].is_null(),
            "the group row was read: {data}"
        );

        for collection in [
            "identifications",
            "classifications",
            "contactMechanisms",
            "relationshipsFrom",
            "relationshipsTo",
        ] {
            assert_eq!(
                data[collection].as_array().map(Vec::len),
                Some(0),
                "{collection} was read: {data}"
            );
        }
    }
}

#[tokio::test]
async fn creating_updating_and_deleting_a_party_are_audited() {
    // Acceptance criterion 4 of #80. The change-record surface of FR-MDM-009 is
    // Sprint 6; the write path is here from the first endpoint.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;
    app.put(
        &format!("{PARTIES}/{id}"),
        Some(&token),
        json!({ "description": "Renamed" }),
    )
    .await;
    app.delete(&format!("{PARTIES}/{id}"), Some(&token)).await;

    let events: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT event_type, action, object_type
           FROM audit_events
          WHERE object_id = $1
          ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    assert_eq!(
        events,
        vec![
            (
                "Party.Created".to_owned(),
                "CREATE".to_owned(),
                "PARTY".to_owned()
            ),
            (
                "Party.Updated".to_owned(),
                "UPDATE".to_owned(),
                "PARTY".to_owned()
            ),
            (
                "Party.Deleted".to_owned(),
                "DELETE".to_owned(),
                "PARTY".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn a_status_change_is_audited_as_one() {
    // A status transition is a different fact from a field edit (SRS
    // FR-AUD-002 names status transitions specifically), and the trail has to
    // be able to tell them apart.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = create_ok(&app, &token, full_person("PARTY-0001")).await;
    app.put(
        &format!("{PARTIES}/{id}"),
        Some(&token),
        json!({ "statusId": "PARTY_DISABLED", "statusComments": "supplier blacklisted" }),
    )
    .await;

    let (action, reason, old_status): (String, Option<String>, Value) = sqlx::query_as(
        "SELECT action, reason, old_value_json -> 'statusId'
           FROM audit_events
          WHERE object_id = $1 AND event_type = 'Party.Updated'
          ORDER BY created_at DESC, id DESC
          LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the status change was not audited");

    assert_eq!(action, "STATUS_CHANGE");
    assert_eq!(reason.as_deref(), Some("supplier blacklisted"));
    assert_eq!(old_status, json!("PARTY_ENABLED"));
}
