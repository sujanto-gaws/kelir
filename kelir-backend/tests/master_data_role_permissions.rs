//! Every party-role route is bound to its own permission string (#81
//! acceptance criterion 5), in the shape #58 established for identity — and the
//! read permission is checked where it actually bites, which is not a route.
//!
//! `master-data:party-role:read` would be a decorative permission if the party
//! aggregate handed its roles and profiles to anyone holding
//! `master-data:party:read`: the dedicated collection route would gate data
//! already reachable one URL away. A supplier profile carries a bank account
//! number and a customer profile a credit limit, so the aggregate omits both
//! members entirely without it, and `the_aggregate_hides_roles_and_profiles_*`
//! is the test that makes the permission mean something.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// Seeded by `0009_party_role_permissions.sql`.
///
/// Written out rather than read from the database: this list is the claim under
/// test.
const ROLE_PERMISSIONS: [&str; 3] = [
    "master-data:party-role:assign",
    "master-data:party-role:remove",
    "master-data:party-role:read",
];

const PASSWORD: &str = "single-permission-user-password";

struct Route {
    method: Method,
    path: String,
    permission: &'static str,
    body: Option<Value>,
}

impl Route {
    fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// Every route the role sub-resource mounts, with the permission its service
/// function requires.
fn role_routes(party: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: format!("/api/v1/master-data/parties/{party}/roles"),
            permission: "master-data:party-role:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            permission: "master-data:party-role:assign",
            body: Some(json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "supplier": { "supplierNumber": format!("SUP-{nonce:04}") } },
            })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            permission: "master-data:party-role:remove",
            body: None,
        },
    ]
}

async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-MDMROLE-{nonce}"),
        permissions,
    )
    .await;

    let username = format!("user.mdmrole{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("mdmrole{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A party for requests to be aimed at, inserted directly so that seeding it
/// does not depend on the permissions under test.
async fn target_party(app: &TestApp, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, $3, 'PARTY_GROUP')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(format!("PARTY-ROLETARGET-{nonce}"))
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
    .expect("insert the target group");

    id
}

// ---------------------------------------------------------------------------
// The binding
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_role_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in ROLE_PERMISSIONS.iter().enumerate() {
        let token = caller_holding(&app, &[permission], nonce).await;
        let party = target_party(&app, nonce).await;

        for route in role_routes(party, nonce) {
            let response = app
                .send(
                    route.method.clone(),
                    &route.path,
                    Some(&token),
                    route.body.clone(),
                )
                .await;

            if route.permission == *permission {
                // Not "200": these answer 200, 201, 204 or 404 depending on
                // what the target holds, and none of that is what this test is
                // about. What it asserts is that authorization did not refuse.
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
async fn assigning_a_role_is_not_something_the_party_permissions_grant() {
    // The party surface and the role surface are separate authorization
    // surfaces. A caller who may create and update parties must not be able to
    // make one a supplier — that is what `master-data:party-role:assign` is
    // for, and if the routes ever fell back to the party permissions this is
    // where it would show.
    let app = TestApp::spawn().await;
    let token = caller_holding(
        &app,
        &[
            "master-data:party:create",
            "master-data:party:read",
            "master-data:party:update",
            "master-data:party:delete",
        ],
        50,
    )
    .await;
    let party = target_party(&app, 50).await;

    let assigned = app
        .put(
            &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "supplier": { "supplierNumber": "SUP-0050" } },
            }),
        )
        .await;

    assert_eq!(
        assigned.status,
        StatusCode::FORBIDDEN,
        "the party permissions let a caller assign a role, body {}",
        assigned.body
    );

    let roles: i64 = sqlx::query_scalar("SELECT count(*) FROM mdm_party_roles")
        .fetch_one(&app.pool)
        .await
        .expect("query runs");
    assert_eq!(roles, 0, "the refused assignment wrote a role anyway");
}

// ---------------------------------------------------------------------------
// The read permission, where it actually bites
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_aggregate_hides_roles_and_profiles_without_the_read_permission() {
    let app = TestApp::spawn().await;
    let administrator = app.administrator_token().await;
    let party = target_party(&app, 60).await;

    app.put(
        &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
        Some(&administrator),
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "profile": {
                "supplier": { "supplierNumber": "SUP-0060", "bankAccount": "1234567890" }
            },
        }),
    )
    .await;

    // A caller who may read parties and nothing else.
    let reader = caller_holding(&app, &["master-data:party:read"], 60).await;
    let restricted = app
        .get(
            &format!("/api/v1/master-data/parties/{party}"),
            Some(&reader),
        )
        .await;

    assert_eq!(restricted.status, StatusCode::OK, "{}", restricted.body);
    assert_eq!(restricted.data()["partyId"], "PARTY-ROLETARGET-60");
    assert!(
        restricted.data().get("roles").is_none(),
        "the roles were handed to a caller without master-data:party-role:read: {}",
        restricted.body
    );
    assert!(
        restricted.data().get("profiles").is_none(),
        "the bank account was handed to a caller without master-data:party-role:read: {}",
        restricted.body
    );
    assert!(
        !restricted.body.to_string().contains("1234567890"),
        "the bank account leaked into the response: {}",
        restricted.body
    );

    // The same party, to a caller who holds both.
    let both = caller_holding(
        &app,
        &["master-data:party:read", "master-data:party-role:read"],
        61,
    )
    .await;
    let full = app
        .get(&format!("/api/v1/master-data/parties/{party}"), Some(&both))
        .await;

    assert_eq!(full.data()["roles"][0]["roleTypeId"], "SUPPLIER");
    assert_eq!(
        full.data()["profiles"]["supplier"]["bankAccount"],
        "1234567890"
    );
}

#[tokio::test]
async fn assigning_a_role_does_not_hand_back_the_profiles() {
    // #104. `the_aggregate_hides_roles_and_profiles_without_the_read_permission`
    // covers `GET` and provably does not cover `PUT`: the assign route
    // answered with every role and every profile the party held while
    // requiring only `master-data:party-role:assign`, so a caller who could
    // write a role could read a bank account and a credit limit — the exact
    // data `master-data:party-role:read` was introduced to gate.
    //
    // A caller restating a role the party already holds sends no profile and
    // needs none back to know the write happened.
    //
    // **#119 is the two assertions this test was missing.** #104 narrowed the
    // answer to `PartyRole`, which still carries `comments` and
    // `additionalAttributes` — and `update_party_role` merges both, so a
    // restatement that sent neither got back what the administrator had put
    // there. The same leak one field over, and this test did not see it because
    // it only ever asserted about the profile secrets. The administrator's write
    // below therefore carries a comment and an attribute worth not leaking.
    let app = TestApp::spawn().await;
    let administrator = app.administrator_token().await;
    let party = target_party(&app, 65).await;

    app.put(
        &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
        Some(&administrator),
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "comments": "renegotiating terms, do not pay",
            "additionalAttributes": { "internalRating": "D", "watchlist": true },
            "profile": {
                "supplier": { "supplierNumber": "SUP-0065", "bankAccount": "1234567890" }
            },
        }),
    )
    .await;
    app.put(
        &format!("/api/v1/master-data/parties/{party}/roles/CUSTOMER"),
        Some(&administrator),
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "profile": {
                "customer": { "customerNumber": "CUS-0065", "creditLimit": "50000000.00" }
            },
        }),
    )
    .await;

    // A caller who may assign a role and nothing else.
    let assigner = caller_holding(&app, &["master-data:party-role:assign"], 65).await;
    let restated = app
        .put(
            &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            Some(&assigner),
            json!({ "fromDate": "2026-01-01T00:00:00Z" }),
        )
        .await;

    assert_eq!(restated.status, StatusCode::OK, "{}", restated.body);

    // The write happened, and the response says which assignment it was.
    assert_eq!(
        restated.data()["roleTypeId"],
        "SUPPLIER",
        "{}",
        restated.body
    );

    for secret in [
        "1234567890",
        "SUP-0065",
        "50000000.00",
        "CUS-0065",
        // #119: written by the administrator, not by this caller.
        "renegotiating terms, do not pay",
        "internalRating",
    ] {
        assert!(
            !restated.body.to_string().contains(secret),
            "assigning a role handed back {secret} to a caller without              master-data:party-role:read: {}",
            restated.body
        );
    }

    // Stated as absence of the members rather than only as absence of the
    // strings, so a future response that carried them under different values
    // would still fail here.
    assert!(
        restated.data().get("comments").is_none(),
        "the assign response carries comments this call did not send: {}",
        restated.body
    );
    assert!(
        restated.data().get("additionalAttributes").is_none(),
        "the assign response carries additionalAttributes this call did not send: {}",
        restated.body
    );

    // And the administrator's values are still stored — the response withholds
    // them, it does not clear them. Merging is the documented behaviour (#120)
    // and this is the assertion that keeps the two facts from being confused.
    let stored = app
        .get(
            &format!("/api/v1/master-data/parties/{party}/roles"),
            Some(&administrator),
        )
        .await;
    let supplier = stored.data()["roles"]
        .as_array()
        .expect("roles is a list")
        .iter()
        .find(|role| role["roleTypeId"] == "SUPPLIER")
        .expect("the party still holds SUPPLIER")
        .clone();
    assert_eq!(supplier["comments"], "renegotiating terms, do not pay");
    assert_eq!(supplier["additionalAttributes"]["internalRating"], "D");

    assert!(
        restated.data().get("profiles").is_none(),
        "the assign response still carries a profiles member: {}",
        restated.body
    );
    assert!(
        restated.data().get("roles").is_none(),
        "the assign response still carries every role the party holds: {}",
        restated.body
    );

    // And the route that does gate them still refuses this caller outright,
    // so there is no second way round.
    let refused = app
        .get(
            &format!("/api/v1/master-data/parties/{party}/roles"),
            Some(&assigner),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

#[tokio::test]
async fn a_first_assignment_does_not_hand_back_the_profiles_either() {
    // The 201 path, not only the 200 one. A caller assigning a role for the
    // first time sends the profile, so echoing it back leaks nothing it did
    // not already know — but it would still carry the *other* roles' profiles,
    // which is the half that matters.
    let app = TestApp::spawn().await;
    let administrator = app.administrator_token().await;
    let party = target_party(&app, 66).await;

    app.put(
        &format!("/api/v1/master-data/parties/{party}/roles/CUSTOMER"),
        Some(&administrator),
        json!({
            "fromDate": "2026-01-01T00:00:00Z",
            "profile": {
                "customer": { "customerNumber": "CUS-0066", "creditLimit": "90000000.00" }
            },
        }),
    )
    .await;

    let assigner = caller_holding(&app, &["master-data:party-role:assign"], 66).await;
    let created = app
        .put(
            &format!("/api/v1/master-data/parties/{party}/roles/SUPPLIER"),
            Some(&assigner),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "supplier": { "supplierNumber": "SUP-0066" } },
            }),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    for secret in ["CUS-0066", "90000000.00"] {
        assert!(
            !created.body.to_string().contains(secret),
            "a first assignment handed back another role's {secret}: {}",
            created.body
        );
    }
}

#[tokio::test]
async fn an_empty_roles_list_is_not_the_same_as_a_hidden_one() {
    // `[]` says this party holds no roles; absence says you cannot see. A
    // client that could not tell them apart would render "no roles" to someone
    // who is simply not allowed to know.
    let app = TestApp::spawn().await;
    let party = target_party(&app, 70).await;

    let permitted = caller_holding(
        &app,
        &["master-data:party:read", "master-data:party-role:read"],
        70,
    )
    .await;

    let response = app
        .get(
            &format!("/api/v1/master-data/parties/{party}"),
            Some(&permitted),
        )
        .await;

    assert_eq!(
        response.data()["roles"].as_array().map(Vec::len),
        Some(0),
        "a party with no roles must report an empty list, not omit the member: {}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// Authentication and the catalogue
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_role_route_refuses_a_request_with_no_token() {
    let app = TestApp::spawn().await;
    let party = target_party(&app, 80).await;

    for route in role_routes(party, 80) {
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
async fn the_administrator_role_holds_the_party_role_permissions() {
    let app = TestApp::spawn().await;

    let granted: Vec<String> = sqlx::query_scalar(
        "SELECT p.permission_code
           FROM role_permissions rp
           JOIN permissions p ON p.id = rp.permission_id
          WHERE rp.role_id = $1 AND p.permission_code LIKE 'master-data:party-role:%'
          ORDER BY p.permission_code",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .fetch_all(&app.pool)
    .await
    .expect("query runs");

    let mut expected = ROLE_PERMISSIONS.map(str::to_owned).to_vec();
    expected.sort();

    assert_eq!(granted, expected);
}
