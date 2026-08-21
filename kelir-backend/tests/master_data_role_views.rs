//! The role views — `/suppliers`, `/customers`, `/employees` (#97).
//!
//! A supplier is a party holding the SUPPLIER role, so these endpoints are
//! projections rather than tables. Two things follow, and both are what this
//! file is aimed at.
//!
//! **The rows are the assertion, never `meta.total` alone.** The party surface
//! verification (#106) found four tests whose only claim about a list was its
//! total, which a different query produces: the count agreed while the page was
//! never exercised. Every test here that says a party is or is not in a view
//! reads the party codes out of `data`.
//!
//! **The view must not be a way around either permission it is made of.** A row
//! is a party summary with a supplier number on it. `master-data:party:read`
//! gates the first half on `/parties` and `master-data:party-role:read` gates
//! the second on the aggregate, so the view requires both — a view needing only
//! one would hand a caller the half the other permission was withholding.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const SUPPLIERS: &str = "/api/v1/master-data/suppliers";
const CUSTOMERS: &str = "/api/v1/master-data/customers";
const EMPLOYEES: &str = "/api/v1/master-data/employees";
const VIEWS: [&str; 3] = [SUPPLIERS, CUSTOMERS, EMPLOYEES];

const PASSWORD: &str = "role-view-caller-password";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A party group, through the API, so the rows under test are the rows the
/// product creates rather than ones a test invented.
async fn given_party(app: &TestApp, token: &str, code: &str, name: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/master-data/parties",
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

/// A person party, for the party-type filter.
async fn given_person(app: &TestApp, token: &str, code: &str, first: &str, last: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(token),
            json!({
                "partyId": code,
                "partyTypeId": "PERSON",
                "person": { "firstName": first, "lastName": last },
            }),
        )
        .await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "the fixture person {code} was refused: {}",
        created.body
    );

    created.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the created party carries an id: {}", created.body))
}

/// Gives a party a role with the profile that role carries.
async fn assign(app: &TestApp, token: &str, party: Uuid, role: &str, profile: Value) {
    assign_with(app, token, party, role, profile, None).await;
}

/// As [`assign`], with a role status — `INACTIVE` is a role the party still
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
            &format!("/api/v1/master-data/parties/{party}/roles/{role}"),
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

fn customer(number: &str) -> Value {
    json!({ "customer": { "customerNumber": number } })
}

fn employee(number: &str) -> Value {
    json!({ "employee": { "employeeNumber": number } })
}

/// A caller holding exactly the permissions named, signed in.
async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-VIEW-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.view{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("view{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// The `partyId` of every row in a list response, in the order the API returned
/// them.
///
/// Reading the rows rather than `meta.total` is the point: a total is produced
/// by the count query and says nothing about what the page contains.
fn codes(response: &TestResponse) -> Vec<String> {
    response
        .data()
        .as_array()
        .unwrap_or_else(|| panic!("a list response carries a data array: {}", response.body))
        .iter()
        .map(|row| {
            row["partyId"]
                .as_str()
                .unwrap_or_else(|| panic!("every row carries a partyId: {row}"))
                .to_owned()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// What each view holds (#97 AC1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_view_lists_the_parties_holding_its_role_and_no_others() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let sup = given_party(&app, &token, "PARTY-SUP", "Acme Supplies").await;
    let cus = given_party(&app, &token, "PARTY-CUS", "Beta Buyers").await;
    let emp = given_person(&app, &token, "PARTY-EMP", "Ella", "Employee").await;
    given_party(&app, &token, "PARTY-NONE", "Gamma Holdings").await;

    assign(&app, &token, sup, "SUPPLIER", supplier("SUP-0001")).await;
    assign(&app, &token, cus, "CUSTOMER", customer("CUS-0001")).await;
    assign(&app, &token, emp, "EMPLOYEE", employee("EMP-0001")).await;

    for (view, expected) in [
        (SUPPLIERS, "PARTY-SUP"),
        (CUSTOMERS, "PARTY-CUS"),
        (EMPLOYEES, "PARTY-EMP"),
    ] {
        let listed = app.get(view, Some(&token)).await;

        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
        assert_eq!(
            codes(&listed),
            vec![expected.to_owned()],
            "{view} listed the wrong parties: {}",
            listed.body
        );
        assert_eq!(listed.body["meta"]["total"], 1, "{}", listed.body);
    }
}

#[tokio::test]
async fn a_row_carries_the_number_that_makes_it_that_role() {
    // "A supplier list that does not show supplier numbers is a party list with
    // a filter on it" — #97. The number is the whole reason the view exists.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-0001", "Acme Supplies").await;
    assign(&app, &token, party, "SUPPLIER", supplier("SUP-0042")).await;

    let listed = app.get(SUPPLIERS, Some(&token)).await;
    let row = &listed.data()[0];

    assert_eq!(row["partyId"], "PARTY-0001");
    assert_eq!(row["name"], "Acme Supplies");
    assert_eq!(row["roleTypeId"], "SUPPLIER");
    assert_eq!(row["roleNumber"], "SUP-0042", "{}", listed.body);
    assert_eq!(row["roleStatusId"], "ACTIVE");
    assert_eq!(row["partyTypeId"], "PARTY_GROUP");
    assert_eq!(row["statusId"], "PARTY_ENABLED");
    assert!(row["id"].is_string(), "a row is addressable: {row}");
}

#[tokio::test]
async fn a_party_holding_two_roles_is_in_both_views() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-BOTH", "Acme Trading").await;
    assign(&app, &token, party, "SUPPLIER", supplier("SUP-0002")).await;
    assign(&app, &token, party, "CUSTOMER", customer("CUS-0002")).await;

    let suppliers = app.get(SUPPLIERS, Some(&token)).await;
    let customers = app.get(CUSTOMERS, Some(&token)).await;

    assert_eq!(codes(&suppliers), vec!["PARTY-BOTH".to_owned()]);
    assert_eq!(codes(&customers), vec!["PARTY-BOTH".to_owned()]);

    // The same party, and each view shows it the number that view is about.
    assert_eq!(suppliers.data()[0]["roleNumber"], "SUP-0002");
    assert_eq!(customers.data()[0]["roleNumber"], "CUS-0002");
}

#[tokio::test]
async fn a_removed_role_leaves_the_view_while_its_history_stays() {
    // #97 AC1: removal is not deletion. The assignment survives as history —
    // and a list that still showed it would report a supplier the business no
    // longer has.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-GONE", "Former Supplier").await;
    assign(&app, &token, party, "SUPPLIER", supplier("SUP-0003")).await;

    assert_eq!(
        codes(&app.get(SUPPLIERS, Some(&token)).await),
        vec!["PARTY-GONE".to_owned()],
        "the fixture never reached the view, so removing it would prove nothing"
    );

    let removed = app
        .delete(
            &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            Some(&token),
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);

    let listed = app.get(SUPPLIERS, Some(&token)).await;
    assert!(
        codes(&listed).is_empty(),
        "a removed supplier is still listed: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);

    let kept: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mdm_party_roles WHERE party_id = $1 AND deleted_at IS NOT NULL",
    )
    .bind(party)
    .fetch_one(&app.pool)
    .await
    .expect("query runs");
    assert_eq!(kept, 1, "the removal destroyed the history it should keep");
}

#[tokio::test]
async fn a_soft_deleted_party_leaves_every_view_it_was_in() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-DEL", "Deleted Supplier").await;
    assign(&app, &token, party, "SUPPLIER", supplier("SUP-0004")).await;

    assert_eq!(
        codes(&app.get(SUPPLIERS, Some(&token)).await).len(),
        1,
        "the fixture never reached the view"
    );

    let deleted = app
        .delete(
            &format!("/api/v1/master-data/parties/{party}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let listed = app.get(SUPPLIERS, Some(&token)).await;
    assert!(
        codes(&listed).is_empty(),
        "a deleted party is still listed as a supplier: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);
}

#[tokio::test]
async fn a_party_holding_the_role_without_a_profile_is_listed_without_a_number() {
    // Built directly, because the API refuses it: `validate_assign_role` makes
    // the profile required for the three profiled roles. The join is a LEFT
    // JOIN anyway — a party that holds SUPPLIER and vanished from `/suppliers`
    // would be a worse answer than one shown with a blank number.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-BARE", "Bare Supplier").await;

    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at)
         SELECT $1, $2, $3, t.id, now()
           FROM mdm_role_types t
          WHERE t.tenant_id = $2 AND t.role_type_code = 'SUPPLIER'",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(party)
    .execute(&app.pool)
    .await
    .expect("insert a role with no profile");

    let listed = app.get(SUPPLIERS, Some(&token)).await;

    assert_eq!(codes(&listed), vec!["PARTY-BARE".to_owned()]);
    assert!(
        listed.data()[0]["roleNumber"].is_null(),
        "a party with no profile reported a number: {}",
        listed.body
    );
}

#[tokio::test]
async fn a_view_with_nothing_in_it_is_an_empty_page_rather_than_an_error() {
    // An empty list and a failed request must not look the same to a client —
    // the distinction #101 acceptance criterion 2 is written around.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for view in VIEWS {
        let listed = app.get(view, Some(&token)).await;

        assert_eq!(listed.status, StatusCode::OK, "{view}: {}", listed.body);
        assert_eq!(listed.body["success"], true, "{view}: {}", listed.body);
        assert_eq!(
            listed.data().as_array().map(Vec::len),
            Some(0),
            "{view}: {}",
            listed.body
        );
        assert_eq!(listed.body["meta"]["total"], 0, "{view}: {}", listed.body);
    }
}

// ---------------------------------------------------------------------------
// Authorization (#97 AC3, AC4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_view_needs_both_the_party_read_and_the_role_read_permission() {
    let app = TestApp::spawn().await;
    let administrator = app.administrator_token().await;

    let party = given_party(&app, &administrator, "PARTY-0001", "Acme Supplies").await;
    assign(
        &app,
        &administrator,
        party,
        "SUPPLIER",
        supplier("SUP-0001"),
    )
    .await;

    // Half of the pair is not the pair. Each of these callers holds a
    // permission the view is made of, and neither may read it: the first would
    // be reading the supplier number the aggregate withholds from them, the
    // second would be enumerating parties, which needs the other permission.
    let party_only = caller_holding(&app, &["master-data:party:read"], 1).await;
    let role_only = caller_holding(&app, &["master-data:party-role:read"], 2).await;

    // And a caller holding neither, but holding *something* — the case a sweep
    // over a caller with no permissions at all cannot distinguish.
    let wrong = caller_holding(
        &app,
        &[
            "master-data:party:create",
            "master-data:party:update",
            "master-data:party:delete",
            "master-data:party-role:assign",
            "master-data:party-role:remove",
        ],
        3,
    )
    .await;

    for (label, token) in [
        ("master-data:party:read alone", &party_only),
        ("master-data:party-role:read alone", &role_only),
        ("every other master-data permission", &wrong),
    ] {
        for view in VIEWS {
            let refused = app.get(view, Some(token)).await;

            assert_eq!(
                refused.status,
                StatusCode::FORBIDDEN,
                "{view} was open to a caller holding {label}: {}",
                refused.body
            );
            assert!(
                !refused.body.to_string().contains("SUP-0001"),
                "{view} leaked a supplier number to a caller holding {label}: {}",
                refused.body
            );
        }
    }

    // Both together, and the view answers.
    let permitted = caller_holding(
        &app,
        &["master-data:party:read", "master-data:party-role:read"],
        4,
    )
    .await;

    for view in VIEWS {
        let allowed = app.get(view, Some(&permitted)).await;

        assert_eq!(
            allowed.status,
            StatusCode::OK,
            "{view} refused a caller holding both permissions: {}",
            allowed.body
        );
    }

    let suppliers = app.get(SUPPLIERS, Some(&permitted)).await;
    assert_eq!(suppliers.data()[0]["roleNumber"], "SUP-0001");
}

#[tokio::test]
async fn every_view_refuses_a_request_with_no_token() {
    // FR-API-008. A route that answered without a token would never reach the
    // permission check the test above drives.
    let app = TestApp::spawn().await;

    for view in VIEWS {
        let response = app.send(Method::GET, view, None, None).await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{view} answered without a token: {}",
            response.body
        );
    }
}

// ---------------------------------------------------------------------------
// Tenant scope (#97 AC5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_view_does_not_show_another_tenants_parties() {
    // The other tenant's data is actually present, down to its own role type
    // row: a foreign party whose tenant has no SUPPLIER role type would be
    // absent from this view whether or not anything filtered by tenant, and the
    // test would pass while proving nothing (#106).
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mine = given_party(&app, &token, "PARTY-MINE", "My Supplier").await;
    assign(&app, &token, mine, "SUPPLIER", supplier("SUP-MINE")).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Other").await;
    let foreign = Uuid::now_v7();
    let foreign_role_type = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name)
         VALUES ($1, $2, 'SUPPLIER', 'Supplier')",
    )
    .bind(foreign_role_type)
    .bind(other_tenant)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role type");

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
        "INSERT INTO mdm_party_groups (id, tenant_id, party_id, group_name)
         VALUES ($1, $2, $3, 'Foreign Supplier')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(foreign)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's group");

    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at)
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(foreign)
    .bind(foreign_role_type)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role");

    sqlx::query(
        "INSERT INTO mdm_supplier_profiles (id, tenant_id, party_id, supplier_number)
         VALUES ($1, $2, $3, 'SUP-FOREIGN')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(foreign)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's supplier profile");

    let listed = app.get(SUPPLIERS, Some(&token)).await;

    assert_eq!(
        codes(&listed),
        vec!["PARTY-MINE".to_owned()],
        "the other tenant's supplier is in the list: {}",
        listed.body
    );
    assert_eq!(
        listed.body["meta"]["total"], 1,
        "the total counts the other tenant's supplier: {}",
        listed.body
    );
    assert!(
        !listed.body.to_string().contains("SUP-FOREIGN"),
        "the other tenant's supplier number leaked: {}",
        listed.body
    );

    // And it cannot be reached by searching for it either — the search runs
    // inside the scoped query, not over it.
    let searched = app
        .get(&format!("{SUPPLIERS}?search=FOREIGN"), Some(&token))
        .await;
    assert!(
        codes(&searched).is_empty(),
        "searching reached the other tenant: {}",
        searched.body
    );
}

// ---------------------------------------------------------------------------
// Paging (#97 AC2)
// ---------------------------------------------------------------------------

/// `count` suppliers, coded so their order is their number.
async fn given_suppliers(app: &TestApp, token: &str, count: usize) {
    for index in 1..=count {
        let party = given_party(
            app,
            token,
            &format!("PARTY-{index:04}"),
            &format!("Supplier {index}"),
        )
        .await;
        assign(
            app,
            token,
            party,
            "SUPPLIER",
            supplier(&format!("SUP-{index:04}")),
        )
        .await;
    }
}

#[tokio::test]
async fn the_page_size_is_clamped_rather_than_rejected() {
    // NFR-PERF-002: a caller cannot ask for an unbounded scan, and asking is
    // not an error — `response::MAX_PAGE_SIZE` is a ceiling, not a validation
    // rule. #101 acceptance criterion 4 is written against this.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 3).await;

    let listed = app
        .get(&format!("{SUPPLIERS}?pageSize=5000"), Some(&token))
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["meta"]["pageSize"], 100, "{}", listed.body);
    assert_eq!(listed.body["meta"]["page"], 1, "{}", listed.body);
    assert_eq!(codes(&listed).len(), 3, "{}", listed.body);
}

#[tokio::test]
async fn the_total_counts_the_same_rows_the_page_shows() {
    // The divergence this is aimed at: `meta.total` comes from the count query
    // and the rows from the page query, and the two carry the same filters
    // written twice. A total that described a different population would be a
    // pager that runs off the end of the list.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 5).await;

    let mut seen = Vec::new();
    for page in 1..=5 {
        let listed = app
            .get(&format!("{SUPPLIERS}?pageSize=1&page={page}"), Some(&token))
            .await;

        assert_eq!(
            listed.body["meta"]["total"], 5,
            "page {page} reported a different total: {}",
            listed.body
        );
        assert_eq!(listed.body["meta"]["page"], page, "{}", listed.body);
        seen.extend(codes(&listed));
    }

    assert_eq!(
        seen,
        (1..=5)
            .map(|index| format!("PARTY-{index:04}"))
            .collect::<Vec<String>>(),
        "paging the view skipped or repeated a supplier"
    );

    // And the same, with a filter on: the count must narrow with the rows.
    let filtered = app
        .get(&format!("{SUPPLIERS}?search=SUP-0003"), Some(&token))
        .await;
    assert_eq!(codes(&filtered), vec!["PARTY-0003".to_owned()]);
    assert_eq!(
        filtered.body["meta"]["total"], 1,
        "the total ignored the search the rows obeyed: {}",
        filtered.body
    );
}

#[tokio::test]
async fn a_page_past_the_end_is_empty_and_still_reports_the_total() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 3).await;

    let listed = app.get(&format!("{SUPPLIERS}?page=99"), Some(&token)).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(codes(&listed).is_empty(), "{}", listed.body);
    assert_eq!(
        listed.body["meta"]["total"], 3,
        "an empty page forgot how many rows there are: {}",
        listed.body
    );
}

// ---------------------------------------------------------------------------
// Search and filter (#97 AC2, FR-MDM-008)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_matches_the_party_code_the_name_and_the_role_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let acme = given_party(&app, &token, "PARTY-0001", "Acme Supplies").await;
    let beta = given_party(&app, &token, "PARTY-0002", "Beta Industries").await;
    assign(&app, &token, acme, "SUPPLIER", supplier("SUP-ACME")).await;
    assign(&app, &token, beta, "SUPPLIER", supplier("SUP-BETA")).await;

    for (search, expected) in [
        ("PARTY-0001", "PARTY-0001"), // the code
        ("Acme", "PARTY-0001"),       // the name
        ("SUP-BETA", "PARTY-0002"),   // the role number
        ("industries", "PARTY-0002"), // case-insensitive
        // A substring rather than a prefix, and `%20` is the space a client
        // would have encoded.
        ("eta%20Indus", "PARTY-0002"),
    ] {
        let listed = app
            .get(&format!("{SUPPLIERS}?search={search}"), Some(&token))
            .await;

        assert_eq!(
            codes(&listed),
            vec![expected.to_owned()],
            "search={search} matched the wrong rows: {}",
            listed.body
        );
    }
}

#[tokio::test]
async fn a_search_that_matches_nothing_is_an_empty_page() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 2).await;

    let listed = app
        .get(
            &format!("{SUPPLIERS}?search=nothing-matches-this"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(codes(&listed).is_empty(), "{}", listed.body);
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);
}

#[tokio::test]
async fn a_wildcard_in_a_search_matches_itself() {
    // `%` is `LIKE`'s "any run of characters". Unescaped, a search for `100%`
    // returns every name beginning `100` — and the caller reads the extra rows
    // as matches rather than as a bug.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let cotton = given_party(&app, &token, "PARTY-0001", "Acme 100% Cotton").await;
    let thousand = given_party(&app, &token, "PARTY-0002", "Acme 1000 Threads").await;
    assign(&app, &token, cotton, "SUPPLIER", supplier("SUP-0001")).await;
    assign(&app, &token, thousand, "SUPPLIER", supplier("SUP-0002")).await;

    // `%25` is `%` percent-encoded, which is how a client sends it.
    let listed = app
        .get(&format!("{SUPPLIERS}?search=100%25"), Some(&token))
        .await;

    assert_eq!(
        codes(&listed),
        vec!["PARTY-0001".to_owned()],
        "the percent sign was read as a wildcard: {}",
        listed.body
    );

    // `_` is the single-character wildcard, and the same applies.
    let underscore = app
        .get(&format!("{SUPPLIERS}?search=Acme_1"), Some(&token))
        .await;
    assert!(
        codes(&underscore).is_empty(),
        "the underscore was read as a wildcard: {}",
        underscore.body
    );
}

#[tokio::test]
async fn filters_narrow_the_view_by_party_status_type_and_role_status() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let group = given_party(&app, &token, "PARTY-0001", "Acme Supplies").await;
    let person = given_person(&app, &token, "PARTY-0002", "Sole", "Trader").await;
    let disabled = given_party(&app, &token, "PARTY-0003", "Dormant Supplies").await;
    let inactive = given_party(&app, &token, "PARTY-0004", "Paused Supplies").await;

    assign(&app, &token, group, "SUPPLIER", supplier("SUP-0001")).await;
    assign(&app, &token, person, "SUPPLIER", supplier("SUP-0002")).await;
    assign(&app, &token, disabled, "SUPPLIER", supplier("SUP-0003")).await;
    assign_with(
        &app,
        &token,
        inactive,
        "SUPPLIER",
        supplier("SUP-0004"),
        Some("INACTIVE"),
    )
    .await;

    let updated = app
        .put(
            &format!("/api/v1/master-data/parties/{disabled}"),
            Some(&token),
            json!({ "statusId": "PARTY_DISABLED" }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    for (query, expected) in [
        ("partyTypeId=PERSON", vec!["PARTY-0002"]),
        (
            "partyTypeId=PARTY_GROUP",
            vec!["PARTY-0001", "PARTY-0003", "PARTY-0004"],
        ),
        ("statusId=PARTY_DISABLED", vec!["PARTY-0003"]),
        ("roleStatusId=INACTIVE", vec!["PARTY-0004"]),
        (
            "roleStatusId=ACTIVE",
            vec!["PARTY-0001", "PARTY-0002", "PARTY-0003"],
        ),
        // Filters combine rather than replace one another.
        (
            "partyTypeId=PARTY_GROUP&statusId=PARTY_ENABLED&roleStatusId=ACTIVE",
            vec!["PARTY-0001"],
        ),
    ] {
        let listed = app.get(&format!("{SUPPLIERS}?{query}"), Some(&token)).await;

        assert_eq!(
            codes(&listed),
            expected
                .iter()
                .map(|code| (*code).to_owned())
                .collect::<Vec<String>>(),
            "?{query} matched the wrong rows: {}",
            listed.body
        );
        assert_eq!(
            listed.body["meta"]["total"],
            expected.len(),
            "?{query} counted a different population than it listed: {}",
            listed.body
        );
    }
}

#[tokio::test]
async fn an_inactive_role_is_still_a_role_the_party_holds() {
    // A role can be INACTIVE and still be held; only removal takes it out of
    // the view. Defaulting the view to active roles would hide a supplier the
    // business has, which is a different claim from the one #97 AC1 makes.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let party = given_party(&app, &token, "PARTY-0001", "Paused Supplies").await;
    assign_with(
        &app,
        &token,
        party,
        "SUPPLIER",
        supplier("SUP-0001"),
        Some("INACTIVE"),
    )
    .await;

    let listed = app.get(SUPPLIERS, Some(&token)).await;

    assert_eq!(
        codes(&listed),
        vec!["PARTY-0001".to_owned()],
        "{}",
        listed.body
    );
    assert_eq!(listed.data()[0]["roleStatusId"], "INACTIVE");
}

#[tokio::test]
async fn a_filter_value_outside_its_vocabulary_is_refused_by_name() {
    // The alternative is worse than an error: a filter that silently does
    // nothing returns the whole population, and the caller reads it as the
    // answer to the question they asked.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 2).await;

    for (query, parameter) in [
        ("statusId=ENABLED", "statusId"),
        ("partyTypeId=ORGANISATION", "partyTypeId"),
        ("roleStatusId=REMOVED", "roleStatusId"),
    ] {
        let refused = app.get(&format!("{SUPPLIERS}?{query}"), Some(&token)).await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "?{query} was accepted: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["path"], parameter,
            "?{query} was refused without naming the parameter: {}",
            refused.body
        );
    }
}

#[tokio::test]
async fn an_unreadable_view_is_refused_before_its_filters_are_read() {
    // A caller who may not see this list learns that, rather than which of
    // their filters was misspelled — a 422 here would be an existence oracle
    // for a surface the caller has no permission on.
    let app = TestApp::spawn().await;
    let reader = caller_holding(&app, &["master-data:party:read"], 90).await;

    let refused = app
        .get(&format!("{SUPPLIERS}?statusId=NONSENSE"), Some(&reader))
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

#[tokio::test]
async fn an_over_long_search_is_refused_rather_than_run() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = app
        .get(
            &format!("{SUPPLIERS}?search={}", "a".repeat(201)),
            Some(&token),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "search");
}

#[tokio::test]
async fn a_blank_search_is_the_whole_view() {
    // `?search=` is what a UI sends when its box is empty.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    given_suppliers(&app, &token, 3).await;

    let listed = app.get(&format!("{SUPPLIERS}?search="), Some(&token)).await;

    assert_eq!(codes(&listed).len(), 3, "{}", listed.body);
}
