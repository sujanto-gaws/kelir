//! Every party route is bound to its own permission string (#80 acceptance
//! criterion 3), in the shape #58 established for identity.
//!
//! A sweep that only proves "a caller holding nothing is refused" passes
//! whatever permission each route actually requires: swap `create_party`'s
//! `master-data:party:create` for `master-data:party:read` and such a sweep
//! stays green — the caller with no permissions still gets 403, the
//! administrator holding everything still succeeds — while a read-only account
//! quietly gains the ability to create master data.
//!
//! The difference here is a caller holding exactly one permission. For each of
//! the four, one user is granted that permission and nothing else and then
//! driven at all five routes: the routes it opens must not be refused, and
//! **every other route must be**. A wrong permission string on any route breaks
//! one of the two halves.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The master-data catalogue, seeded by `0008_master_data.sql`.
///
/// Written out rather than read from the database: this list is the claim under
/// test. Reading it back from `permissions` would make the test agree with
/// whatever the catalogue happens to hold, including a permission nothing
/// enforces.
const PARTY_PERMISSIONS: [&str; 4] = [
    "master-data:party:create",
    "master-data:party:read",
    "master-data:party:update",
    "master-data:party:delete",
];

const PASSWORD: &str = "single-permission-user-password";

struct Route {
    method: Method,
    path: String,
    permission: &'static str,
    /// A structurally valid body. Extraction runs before the service does, so a
    /// malformed body answers 422 and never reaches the permission check — a
    /// test asserting 403 against a body the router rejects would be asserting
    /// nothing about authorization.
    body: Option<Value>,
}

impl Route {
    fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// Every route `master_data::handlers::routes()` mounts, with the permission
/// its service function requires.
fn party_routes(target: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: "/api/v1/master-data/parties".into(),
            permission: "master-data:party:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/master-data/parties".into(),
            permission: "master-data:party:create",
            body: Some(json!({
                "partyId": format!("PARTY-MADE-{nonce}"),
                "partyTypeId": "PERSON",
                "person": { "firstName": "Made", "lastName": "ByTest" },
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/master-data/parties/{target}"),
            permission: "master-data:party:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/master-data/parties/{target}"),
            permission: "master-data:party:update",
            body: Some(json!({ "description": "Renamed by test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/master-data/parties/{target}"),
            permission: "master-data:party:delete",
            body: None,
        },
    ]
}

/// A user holding exactly `permission` and nothing else, signed in.
async fn caller_holding_only(app: &TestApp, permission: &str, nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-MDM-ONLY-{nonce}"),
        &[permission],
    )
    .await;

    let username = format!("user.mdm{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("mdm{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A party for requests to be aimed at, created directly so that seeding it
/// does not itself depend on the permission under test.
async fn target_party(app: &TestApp, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, $3, 'PARTY_GROUP')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(format!("PARTY-TARGET-{nonce}"))
    .execute(&app.pool)
    .await
    .expect("insert the target party");

    sqlx::query(
        "INSERT INTO mdm_party_groups (id, tenant_id, party_id, group_name)
         VALUES ($1, $2, $3, 'Target Group')",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(id)
    .execute(&app.pool)
    .await
    .expect("insert the target party group");

    id
}

#[tokio::test]
async fn each_party_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in PARTY_PERMISSIONS.iter().enumerate() {
        let token = caller_holding_only(&app, permission, nonce).await;
        let target = target_party(&app, nonce).await;

        for route in party_routes(target, nonce) {
            let response = app
                .send(
                    route.method.clone(),
                    &route.path,
                    Some(&token),
                    route.body.clone(),
                )
                .await;

            if route.permission == *permission {
                // Not "200": these answer 200, 201 or 204 depending on the
                // route, and none of that is what this test is about. What it
                // asserts is that authorization did not refuse — a route whose
                // permission string no longer matches the one it documents
                // shows up here as a 403.
                assert_ne!(
                    response.status,
                    StatusCode::FORBIDDEN,
                    "{} should be open to a caller holding {permission}, body {}",
                    route.label(),
                    response.body
                );
                assert_ne!(
                    response.status,
                    StatusCode::UNAUTHORIZED,
                    "{} refused the token itself, so this proves nothing about permissions",
                    route.label()
                );
            } else {
                // The other half: holding *a* permission is not holding *this*
                // one.
                assert_eq!(
                    response.status,
                    StatusCode::FORBIDDEN,
                    "{} should be closed to a caller holding only {permission}, body {}",
                    route.label(),
                    response.body
                );
            }
        }
    }
}

#[tokio::test]
async fn a_reader_may_list_parties_but_may_not_create_one() {
    // The same claim as the table above, in the form a person can check at a
    // glance. If the table ever stops failing for the right reason, this is the
    // case that still says what the product is supposed to do.
    let app = TestApp::spawn().await;
    let token = caller_holding_only(&app, "master-data:party:read", 100).await;

    let listed = app.get("/api/v1/master-data/parties", Some(&token)).await;
    assert_eq!(listed.status, StatusCode::OK, "body {}", listed.body);

    let created = app
        .post(
            "/api/v1/master-data/parties",
            Some(&token),
            json!({
                "partyId": "PARTY-SHOULD-NOT-EXIST",
                "partyTypeId": "PERSON",
                "person": { "firstName": "Should", "lastName": "NotExist" },
            }),
        )
        .await;
    assert_eq!(
        created.status,
        StatusCode::FORBIDDEN,
        "reading parties must not let a caller create master data, body {}",
        created.body
    );

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_parties")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(count, 0, "the refused create wrote a party anyway");
}

#[tokio::test]
async fn every_party_route_refuses_a_request_with_no_token() {
    // FR-API-008 claims this of all of them, and a route that answered without
    // a token would never reach the permission check the table above drives.
    let app = TestApp::spawn().await;
    let target = target_party(&app, 200).await;

    for route in party_routes(target, 200) {
        let response = app
            .send(route.method.clone(), &route.path, None, route.body.clone())
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{} answered without a token, body {}",
            route.label(),
            response.body
        );
    }
}

#[tokio::test]
async fn the_administrator_role_holds_the_new_permissions() {
    // `0008` seeds four rows and grants them to ROLE-ADMIN. A grant that
    // silently failed would leave the bootstrap administrator unable to reach
    // any of these routes on a fresh deployment — the worst place to find out.
    // Scoped to `master-data:party:` so the party-role rows `0009` adds are
    // this file's neighbour's business, not a reason for this test to churn.
    let app = TestApp::spawn().await;

    let granted: Vec<String> = sqlx::query_scalar(
        "SELECT p.permission_code
           FROM role_permissions rp
           JOIN permissions p ON p.id = rp.permission_id
          WHERE rp.role_id = $1 AND p.permission_code LIKE 'master-data:party:%'
          ORDER BY p.permission_code",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    let mut expected = PARTY_PERMISSIONS.map(str::to_owned).to_vec();
    expected.sort();

    assert_eq!(granted, expected);
}
