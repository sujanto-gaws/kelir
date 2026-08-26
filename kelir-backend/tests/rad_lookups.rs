//! A lookup field resolves its options through the API, and cannot become a way
//! to read master data the caller could not read directly (FR-RAD-007, #161).
//!
//! **The permission question is the point of this file.** A lookup binds a form
//! field to a master-data query, so a caller who may render a form is one URL
//! away from enumerating suppliers, and the only thing that stops it is that the
//! lookup requires what the master-data endpoint requires. That is a security
//! control, so every test below asserting one was accepted only after the defect
//! it names was reintroduced and the test seen to fail (coding standard §2.9).
//! Each names its own mutation.
//!
//! **The scoping tests reach the query, rather than asserting around it** — the
//! #106 and #121 lesson. The caller in
//! `a_lookup_does_not_offer_another_tenants_master_data` **holds every
//! permission the lookup requires**, so nothing refuses before the SQL runs and
//! the tenant predicate is the only thing between the foreign row and the
//! response. A fixture that put the caller outside the permission instead would
//! be covered by the permission check and would stay green through any mutation
//! below it.
//!
//! **Boundary.** What is covered here is the lookup surface: its two permission
//! families, its tenant scope, its paging, its server-side filters and its
//! refusals. The queries underneath belong to the master-data module and are
//! covered by `master_data_role_views.rs` and `master_data_facilities.rs`; this
//! file does not re-cover them, it covers that the lookup reaches them and adds
//! nothing of its own.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";
const FACILITIES: &str = "/api/v1/master-data/facilities";
const FORMS: &str = "/api/v1/rad/forms";

const PARTY_READ: &str = "master-data:party:read";
const ROLE_READ: &str = "master-data:party-role:read";
const FACILITY_READ: &str = "master-data:facility:read";
const FORM_READ: &str = "rad:form:read";

const PASSWORD: &str = "lookup-caller-password";

fn options(source: &str) -> String {
    format!("/api/v1/rad/lookups/{source}/options")
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A party group, through the API, so the rows under test are rows the product
/// creates rather than ones a test invented.
async fn given_party(app: &TestApp, token: &str, code: &str, name: &str) -> Uuid {
    let created = app
        .post(
            PARTIES,
            Some(token),
            json!({
                "partyId": code,
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": name },
            }),
        )
        .await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "the fixture party {code} was refused: {}",
        created.body
    );

    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the created party carries an id: {}", created.body))
}

/// Gives a party a role, with the number that makes it that role.
async fn assign(app: &TestApp, token: &str, party: Uuid, role: &str, profile: Value) {
    assign_with(app, token, party, role, profile, None).await;
}

/// As [`assign`], with a role status. `INACTIVE` is a role the party still
/// holds, which is not the same as one that was removed.
async fn assign_with(
    app: &TestApp,
    token: &str,
    party: Uuid,
    role: &str,
    profile: Value,
    status: Option<&str>,
) {
    let mut body = json!({
        "fromDate": "2026-01-01T00:00:00Z",
        "profile": profile,
    });

    if let Some(status) = status {
        body["statusId"] = json!(status);
    }

    let assigned = app
        .put(
            &format!("{PARTIES}/{party}/roles/{role}"),
            Some(token),
            body,
        )
        .await;

    assert_eq!(
        assigned.status,
        StatusCode::CREATED,
        "the fixture {role} assignment was refused: {}",
        assigned.body
    );
}

fn supplier(number: &str) -> Value {
    json!({ "supplier": { "supplierNumber": number } })
}

/// A party holding the SUPPLIER role — the fixture most of this file needs.
async fn given_supplier(app: &TestApp, token: &str, code: &str, name: &str, number: &str) -> Uuid {
    let party = given_party(app, token, code, name).await;

    assign(app, token, party, "SUPPLIER", supplier(number)).await;
    party
}

async fn given_facility(app: &TestApp, token: &str, code: &str, name: &str) -> Uuid {
    let created = app
        .post(
            FACILITIES,
            Some(token),
            json!({ "facilityId": code, "name": name, "facilityTypeId": "BUILDING" }),
        )
        .await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "the fixture facility {code} was refused: {}",
        created.body
    );

    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the created facility carries an id: {}", created.body))
}

/// A caller holding exactly the permissions named, signed in.
async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-LOOKUP-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.lookup{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("lookup{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// The `label` of every option in a response, in the order the API returned
/// them.
///
/// The rows, never `meta.total` alone: a total comes from the count query and
/// says nothing about what the page holds (#106).
fn labels(response: &TestResponse) -> Vec<String> {
    response
        .data()
        .as_array()
        .unwrap_or_else(|| panic!("a list response carries a data array: {}", response.body))
        .iter()
        .map(|row| {
            row["label"]
                .as_str()
                .unwrap_or_else(|| panic!("every option carries a label: {row}"))
                .to_owned()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// A form definition declares a lookup, and it resolves (#161 AC1)
// ---------------------------------------------------------------------------

/// A JFSS v2.0.1 document with one lookup field bound to `source`.
///
/// **No property outside the frozen meta-schema.** JFSS is a `Final Standard`,
/// so the binding lives in `settings`, the one object the specification leaves
/// open to an implementation — see `rad::domain::jfss`.
fn definition_with_lookup(form_id: &str, source: &str) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "title": "Purchase requisition",
        "settings": { "lookups": { "supplier_field": source } },
        "components": [{
            "id": "supplier_field",
            "role": "data",
            "type": "lookup",
            "key": "supplier_id",
            "label": "Supplier",
            "validation": { "type": "string" }
        }]
    })
}

#[tokio::test]
async fn a_definition_declaring_a_lookup_is_stored_and_names_a_source_the_api_serves() {
    // AC1, both halves in one flow: the definition is accepted, and the source
    // it names resolves through the API. A definition that stored happily and
    // named a source nobody serves would pass the first half and fail a user.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_supplier(&app, &token, "PARTY-SUP", "Acme Supplies", "SUP-0001").await;

    let created = app
        .post(
            FORMS,
            Some(&token),
            json!({
                "formKey": "purchase-requisition",
                "title": "Purchase requisition",
                "definition": definition_with_lookup("purchase-requisition", "supplier"),
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let source = created.data()["definition"]["settings"]["lookups"]["supplier_field"]
        .as_str()
        .unwrap_or_else(|| panic!("the stored definition keeps its binding: {}", created.body));

    let resolved = app.get(&options(source), Some(&token)).await;

    assert_eq!(resolved.status, StatusCode::OK, "{}", resolved.body);
    assert_eq!(labels(&resolved), vec!["Acme Supplies".to_owned()]);
}

#[tokio::test]
async fn a_definition_binding_a_lookup_to_a_source_nobody_serves_is_refused_at_save() {
    // Refused at save rather than at render: a definition is written once and
    // rendered thousands of times, and at render the failure is a chooser that
    // opens empty, which reads as master data holding nothing.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .post(
            FORMS,
            Some(&token),
            json!({
                "formKey": "bad-binding",
                "title": "Bad binding",
                "definition": definition_with_lookup("bad-binding", "parties"),
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "LOOKUP_SOURCE_UNKNOWN",
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn every_source_the_validator_accepts_is_a_source_the_api_serves() {
    // The two allow-lists are the same list, and this is what keeps them so: a
    // source added to the enum without a branch in `list_options` would store
    // happily and 404 at render, which is the failure the save-time check exists
    // to prevent.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for source in ["supplier", "customer", "employee", "facility"] {
        let created = app
            .post(
                FORMS,
                Some(&token),
                json!({
                    "formKey": format!("form-{source}"),
                    "title": "Bound",
                    "definition": definition_with_lookup(&format!("form-{source}"), source),
                }),
            )
            .await;

        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "{source} was refused at save: {}",
            created.body
        );

        let resolved = app.get(&options(source), Some(&token)).await;

        assert_eq!(
            resolved.status,
            StatusCode::OK,
            "{source} saves but does not resolve: {}",
            resolved.body
        );
    }
}

#[tokio::test]
async fn an_unknown_source_is_not_a_url_this_api_serves() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app.get(&options("parties"), Some(&token)).await;

    assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
}

// ---------------------------------------------------------------------------
// The permission for the underlying entity is required (#161 AC2, AC3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_caller_who_may_read_forms_cannot_enumerate_parties_through_a_lookup() {
    // **#161 AC3, and the defect this whole item exists to prevent.** Rendering
    // a form and reading master data are different permissions; a lookup that
    // required only the first would hand every form-renderer the supplier list.
    //
    // Seen to fail (coding standard §2.9) by deleting **both** `caller.require`
    // lines from `master_data::service::role_view::list_role_view_options`: the
    // caller then reads Acme Supplies out of the response holding no master-data
    // permission at all, and this goes red on the status assertion.
    //
    // **Both, and that is a boundary rather than a convenience.** Deleting only
    // `require(PARTY_READ)` leaves this test green, because a caller holding
    // `rad:form:read` alone is still refused by the `ROLE_READ` line below it —
    // the gate §2.9 describes, where one check absorbs a mutation aimed at
    // another. So what this test covers is the claim #161 AC3 makes, that
    // rendering a form does not open master data; which of the two permissions
    // does the refusing is isolated by
    // `a_supplier_lookup_needs_both_permissions_the_role_view_is_made_of`, whose
    // callers hold exactly one each. Neither test covers the other's line.
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    given_supplier(&app, &admin, "PARTY-SUP", "Acme Supplies", "SUP-0001").await;

    let renderer = caller_holding(&app, &[FORM_READ], 1).await;
    let refused = app.get(&options("supplier"), Some(&renderer)).await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a form-read caller must not enumerate parties through a lookup: {}",
        refused.body
    );

    // Refused, not emptied — the decision `service::lookup` records. An empty
    // list would be a false statement about the data, and the person filling in
    // the form could not tell it from a tenant with no suppliers yet.
    assert!(
        refused.data().as_array().is_none(),
        "a refusal must not arrive as an empty page: {}",
        refused.body
    );
}

#[tokio::test]
async fn a_supplier_lookup_needs_both_permissions_the_role_view_is_made_of() {
    // A row is a party summary with a supplier number on it, so it is made of
    // two surfaces (#97). A lookup requiring only one would be a way around the
    // other, in whichever direction the missing one points.
    //
    // Seen to fail by deleting `caller.require(ROLE_READ)?;` from
    // `list_role_view_options`: the party-read-only caller then gets 200 and the
    // first case goes red.
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    given_supplier(&app, &admin, "PARTY-SUP", "Acme Supplies", "SUP-0001").await;

    for (nonce, held) in [(2, vec![PARTY_READ]), (3, vec![ROLE_READ])] {
        let caller = caller_holding(&app, &held, nonce).await;
        let refused = app.get(&options("supplier"), Some(&caller)).await;

        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN,
            "holding only {held:?} must not open the supplier lookup: {}",
            refused.body
        );
    }

    let both = caller_holding(&app, &[PARTY_READ, ROLE_READ], 4).await;
    let allowed = app.get(&options("supplier"), Some(&both)).await;

    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.body);
    assert_eq!(labels(&allowed), vec!["Acme Supplies".to_owned()]);
}

#[tokio::test]
async fn each_source_needs_its_own_entitys_permission_and_not_another_sources() {
    // The other half of the same claim, across permission families. A sweep
    // asserting only "a caller holding nothing is refused" would stay green with
    // every source checking the same string — and a facility lookup gated on
    // `master-data:party:read` is a facility list handed to somebody who may not
    // read facilities.
    //
    // Seen to fail by swapping `FACILITY_READ` for `PARTY_READ` in
    // `master_data::service::facility::list_facility_options`: the party caller
    // then reads the facility list and the first case goes red.
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    given_supplier(&app, &admin, "PARTY-SUP", "Acme Supplies", "SUP-0001").await;
    given_facility(&app, &admin, "FAC-001", "Head Office").await;

    let party_caller = caller_holding(&app, &[PARTY_READ, ROLE_READ], 5).await;
    let facility_caller = caller_holding(&app, &[FACILITY_READ], 6).await;

    let refused = app.get(&options("facility"), Some(&party_caller)).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the party permissions must not open the facility lookup: {}",
        refused.body
    );

    let refused = app.get(&options("supplier"), Some(&facility_caller)).await;
    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "the facility permission must not open the supplier lookup: {}",
        refused.body
    );

    let allowed = app.get(&options("facility"), Some(&facility_caller)).await;
    assert_eq!(allowed.status, StatusCode::OK, "{}", allowed.body);
    assert_eq!(labels(&allowed), vec!["Head Office".to_owned()]);
}

#[tokio::test]
async fn a_lookup_refuses_a_request_carrying_no_token_at_all() {
    let app = TestApp::spawn().await;

    let refused = app.get(&options("supplier"), None).await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// Tenant scope (#161 AC5)
// ---------------------------------------------------------------------------

/// Another tenant, holding a party with a SUPPLIER role and a facility.
///
/// Seeded with direct SQL because the API cannot produce it: every write path
/// takes its tenant from the caller's claims, so there is no request that puts a
/// row in a tenant the caller is not in. That is what made the #108 family of
/// defects latent, and it is why a fixture rather than a route is the only way
/// to cover the predicate.
async fn given_another_tenants_master_data(app: &TestApp) {
    let tenant = fixtures::create_tenant(&app.pool, "TNT-OTHER", "Other").await;
    let party = Uuid::now_v7();
    let role_type = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type, status)
         VALUES ($1, $2, 'PARTY-THEIRS', 'PARTY_GROUP', 'PARTY_ENABLED')",
    )
    .bind(party)
    .bind(tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's party");

    sqlx::query(
        "INSERT INTO mdm_party_groups (id, tenant_id, party_id, group_name)
         VALUES ($1, $2, $3, 'Foreign Competitor')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(party)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's party group");

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name)
         VALUES ($1, $2, 'SUPPLIER', 'Supplier')",
    )
    .bind(role_type)
    .bind(tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role type");

    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at, status)
         VALUES ($1, $2, $3, $4, now(), 'ACTIVE')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(party)
    .bind(role_type)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role assignment");

    sqlx::query(
        "INSERT INTO mdm_facilities (id, tenant_id, facility_code, name)
         VALUES ($1, $2, 'FAC-THEIRS', 'Foreign Warehouse')",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's facility");
}

#[tokio::test]
async fn a_lookup_does_not_offer_another_tenants_master_data() {
    // **The caller holds every permission the lookup requires**, so nothing
    // refuses before the query runs and the tenant predicate is the only thing
    // between the foreign row and the response. A fixture that put the caller
    // outside the permission would be covered by the permission check and would
    // stay green through any mutation below it — the #106 gate.
    //
    // Seen to fail by dropping `WHERE r.tenant_id = $1` from `list_role_view`
    // and `WHERE tenant_id = $1` from `list_facility_options`: each mutation
    // puts the named foreign row in the caller's own chooser.
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    given_another_tenants_master_data(&app).await;
    given_supplier(&app, &admin, "PARTY-MINE", "My Supplier", "SUP-0001").await;
    given_facility(&app, &admin, "FAC-MINE", "My Office").await;

    let supplier_options = app.get(&options("supplier"), Some(&admin)).await;

    assert_eq!(
        supplier_options.status,
        StatusCode::OK,
        "{}",
        supplier_options.body
    );
    assert_eq!(labels(&supplier_options), vec!["My Supplier".to_owned()]);
    assert!(
        !supplier_options.body.to_string().contains("Foreign"),
        "another tenant's supplier reached the chooser: {}",
        supplier_options.body
    );

    let facility_options = app.get(&options("facility"), Some(&admin)).await;

    assert_eq!(labels(&facility_options), vec!["My Office".to_owned()]);
    assert!(
        !facility_options.body.to_string().contains("Foreign"),
        "another tenant's facility reached the chooser: {}",
        facility_options.body
    );
}

// ---------------------------------------------------------------------------
// Paged and filtered server-side (#161 AC4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_lookup_pages_rather_than_returning_every_row() {
    // "A lookup that returns every party is a lookup that stops working at the
    // size where it matters." The page is what the caller asked for and the
    // total is the population behind it.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for index in 1..=5 {
        given_supplier(
            &app,
            &token,
            &format!("PARTY-{index:03}"),
            &format!("Supplier {index}"),
            &format!("SUP-{index:04}"),
        )
        .await;
    }

    let first = app
        .get(&format!("{}?pageSize=2", options("supplier")), Some(&token))
        .await;

    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(
        labels(&first),
        vec!["Supplier 1".to_owned(), "Supplier 2".to_owned()]
    );
    assert_eq!(first.body["meta"]["total"], 5, "{}", first.body);

    let third = app
        .get(
            &format!("{}?pageSize=2&page=3", options("supplier")),
            Some(&token),
        )
        .await;

    assert_eq!(labels(&third), vec!["Supplier 5".to_owned()]);
}

#[tokio::test]
async fn a_lookup_filters_on_the_server_rather_than_handing_over_the_population() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_supplier(&app, &token, "PARTY-ACME", "Acme Supplies", "SUP-0001").await;
    given_supplier(&app, &token, "PARTY-BETA", "Beta Trading", "SUP-0002").await;

    let by_name = app
        .get(
            &format!("{}?search=Beta", options("supplier")),
            Some(&token),
        )
        .await;

    assert_eq!(by_name.status, StatusCode::OK, "{}", by_name.body);
    assert_eq!(labels(&by_name), vec!["Beta Trading".to_owned()]);
    assert_eq!(
        by_name.body["meta"]["total"], 1,
        "the total must count the same rows the page shows: {}",
        by_name.body
    );

    // The business identifier is searchable too, because it is what a person
    // reads off a paper requisition.
    let by_number = app
        .get(
            &format!("{}?search=SUP-0001", options("supplier")),
            Some(&token),
        )
        .await;

    assert_eq!(labels(&by_number), vec!["Acme Supplies".to_owned()]);
}

#[tokio::test]
async fn an_option_carries_the_identifier_that_tells_two_of_the_same_name_apart() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_supplier(&app, &token, "PARTY-ACME", "Acme Supplies", "SUP-0001").await;

    let listed = app.get(&options("supplier"), Some(&token)).await;
    let option = &listed.data()[0];

    assert_eq!(
        option["value"],
        json!(party.to_string()),
        "the value is what a document stores to point at the record: {}",
        listed.body
    );
    assert_eq!(option["label"], "Acme Supplies", "{}", listed.body);
    assert_eq!(option["description"], "SUP-0001", "{}", listed.body);
}

#[tokio::test]
async fn a_lookup_does_not_offer_a_party_the_business_no_longer_deals_with() {
    // Two filters that are the server's and not the caller's: a disabled party
    // and an inactive role assignment are not things a requisition may name, and
    // a renderer that had to remember that is a renderer that could forget it.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_supplier(&app, &token, "PARTY-LIVE", "Live Supplier", "SUP-0001").await;

    let disabled = given_party(&app, &token, "PARTY-OFF", "Disabled Supplier").await;
    assign(&app, &token, disabled, "SUPPLIER", supplier("SUP-0002")).await;
    let updated = app
        .put(
            &format!("{PARTIES}/{disabled}"),
            Some(&token),
            json!({ "statusId": "PARTY_DISABLED" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let lapsed = given_party(&app, &token, "PARTY-EX", "Former Supplier").await;
    assign_with(
        &app,
        &token,
        lapsed,
        "SUPPLIER",
        supplier("SUP-0003"),
        Some("INACTIVE"),
    )
    .await;

    let listed = app.get(&options("supplier"), Some(&token)).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        labels(&listed),
        vec!["Live Supplier".to_owned()],
        "a lookup offers what the business currently deals with: {}",
        listed.body
    );
}

#[tokio::test]
async fn a_facility_lookup_does_not_offer_an_archived_facility() {
    // `ARCHIVED` is the one status the lifecycle calls terminal — a record that
    // must live again is re-created — so offering one on a form would be
    // offering a record the tenant has finished with. The other four are
    // reversible and are deliberately still offered; every facility sits at
    // `DRAFT` until somebody transitions it, so a filter demanding `ACTIVE`
    // would empty every lookup in the product.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    given_facility(&app, &token, "FAC-LIVE", "Head Office").await;
    let archived = given_facility(&app, &token, "FAC-OLD", "Old Depot").await;

    // `DRAFT -> INACTIVE -> ARCHIVED` is the path the lifecycle allows; there is
    // no direct route from a draft to the archive (`RecordStatus::may_move_to`).
    for status in ["INACTIVE", "ARCHIVED"] {
        let moved = app
            .post(
                &format!("{FACILITIES}/{archived}/transition"),
                Some(&token),
                json!({ "recordStatusId": status }),
            )
            .await;

        assert_eq!(
            moved.status,
            StatusCode::OK,
            "the fixture transition to {status} was refused: {}",
            moved.body
        );
    }

    let listed = app.get(&options("facility"), Some(&token)).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        labels(&listed),
        vec!["Head Office".to_owned()],
        "{}",
        listed.body
    );
}

#[tokio::test]
async fn an_over_long_search_is_refused_inside_the_error_envelope() {
    // #122's shape: a bad parameter that answered outside the envelope was the
    // one refusal a client written against the envelope cannot read. This list
    // must not become a fourth instance of it.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .get(
            &format!("{}?search={}", options("supplier"), "a".repeat(201)),
            Some(&token),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.error_code().is_some(),
        "the refusal must carry an error code: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"], "search",
        "{}",
        refused.body
    );
}
