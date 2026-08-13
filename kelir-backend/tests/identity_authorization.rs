//! End-to-end proof of the Sprint 3 authorisation rule.
//!
//! Sprint 3 shipped `Authenticated` (FR-API-008) and a `require()` call in every
//! identity service function (FR-IDM-005), verified only by unit tests over
//! hand-constructed claims and by manual clicking. What those cannot show is
//! that the rule survives the whole stack: a token issued by the real sign-in
//! path, carrying permissions read from the real database, checked by the real
//! service behind the real router.
//!
//! The distinction under test is between three outcomes that are easy to
//! confuse and very different in consequence:
//!
//! * no token at all              → 401 Unauthorized
//! * a valid token, no permission → 403 Forbidden
//! * a valid token with it        → 200 OK
//!
//! A frontend that hides a button proves none of this.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const NO_ROLE_USERNAME: &str = "user.norole";
const NO_ROLE_PASSWORD: &str = "no-role-user-password";

/// Signs in a user who holds no roles at all, and therefore no permissions.
async fn token_for_a_user_without_roles(app: &TestApp) -> String {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        NO_ROLE_USERNAME,
        "norole@kelir.test",
        NO_ROLE_PASSWORD,
        &[],
    )
    .await;

    app.sign_in(NO_ROLE_USERNAME, NO_ROLE_PASSWORD).await
}

// ---------------------------------------------------------------------------
// The rule
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_user_without_roles_is_authenticated_but_forbidden_on_an_identity_route() {
    let app = TestApp::spawn().await;
    let token = token_for_a_user_without_roles(&app).await;

    // Authenticated: the token is accepted, and the session it describes is
    // real. If this failed, the 403 below would prove nothing — a rejected
    // token is refused before any permission is consulted.
    let session = app.get("/api/v1/auth/me", Some(&token)).await;

    assert_eq!(
        session.status,
        StatusCode::OK,
        "the token must authenticate; body was {}",
        session.body
    );
    assert_eq!(session.data()["username"], NO_ROLE_USERNAME);
    assert_eq!(
        session.data()["permissions"],
        json!([]),
        "a user with no roles must carry no permissions"
    );

    // Authorised: no. FR-IDM-005.
    let listing = app.get("/api/v1/identity/users", Some(&token)).await;

    assert_eq!(
        listing.status,
        StatusCode::FORBIDDEN,
        "expected 403 without identity:user:read; body was {}",
        listing.body
    );
    assert_eq!(listing.error_code(), Some("FORBIDDEN"));
    assert_eq!(listing.body["success"], false);
}

#[tokio::test]
async fn an_administrator_may_list_users() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let listing = app.get("/api/v1/identity/users", Some(&token)).await;

    assert_eq!(
        listing.status,
        StatusCode::OK,
        "the bootstrap administrator holds every permission; body was {}",
        listing.body
    );
    assert_eq!(listing.body["success"], true);

    // The same route, the same request, a different answer — so the 403 above is
    // the permission check and not a broken route.
    let usernames: Vec<String> = listing
        .data()
        .as_array()
        .expect("the list envelope carries an array in `data`")
        .iter()
        .filter_map(|user| user["username"].as_str().map(str::to_owned))
        .collect();

    assert!(
        usernames.contains(&common::ADMIN_USERNAME.to_owned()),
        "the administrator should see itself; got {usernames:?}"
    );
}

#[tokio::test]
async fn an_identity_route_without_a_token_is_unauthorized_not_forbidden() {
    // 401 and 403 are answers to different questions, and a client that cannot
    // tell them apart cannot decide whether to re-authenticate or give up.
    let app = TestApp::spawn().await;

    let response = app.get("/api/v1/identity/users", None).await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.error_code(), Some("UNAUTHORIZED"));
}

#[tokio::test]
async fn a_forged_token_is_refused() {
    // The permission list lives inside the token, so a token the server did not
    // sign would be a complete authorisation bypass.
    let app = TestApp::spawn().await;

    for token in [
        "not-a-token",
        // A well-formed JWT signed with a different secret, claiming everything.
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIwMDAwMDAwMC0wMDAwLTAwMDAtMDAwMC0wMDAwMDAwMDAwMDEiLCJ0ZW5hbnRfaWQiOiIwMDAwMDAwMC0wMDAwLTAwMDAtMDAwMC0wMDAwMDAwMDAwMDEiLCJ1c2VybmFtZSI6ImF0dGFja2VyIiwicm9sZXMiOlsiUk9MRS1BRE1JTiJdLCJwZXJtaXNzaW9ucyI6WyJpZGVudGl0eTp1c2VyOnJlYWQiXSwiZXhwIjo0MTAyNDQ0ODAwLCJpYXQiOjE3NTAwMDAwMDB9.0000000000000000000000000000000000000000000",
    ] {
        let response = app.get("/api/v1/identity/users", Some(token)).await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "token {token} was not refused"
        );
    }
}

/// The whole identity surface, not one representative route.
///
/// A permission check placed in the service is easy to add to the handler that
/// prompted the review and forget on its neighbours; the missing one is
/// invisible from the frontend, which simply never renders the control.
#[tokio::test]
async fn every_identity_route_refuses_a_caller_holding_no_permission() {
    let app = TestApp::spawn().await;
    let token = token_for_a_user_without_roles(&app).await;
    let some_id = Uuid::now_v7();

    let routes: Vec<(&str, common::TestResponse)> = vec![
        (
            "GET /identity/users",
            app.get("/api/v1/identity/users", Some(&token)).await,
        ),
        (
            "GET /identity/users/{id}",
            app.get(&format!("/api/v1/identity/users/{some_id}"), Some(&token))
                .await,
        ),
        (
            "POST /identity/users",
            app.post(
                "/api/v1/identity/users",
                Some(&token),
                json!({
                    "username": "user.escalated",
                    "email": "escalated@kelir.test",
                    "password": "escalation-attempt-password",
                    "displayName": "Escalated",
                    "roleIds": [fixtures::ADMIN_ROLE_ID],
                }),
            )
            .await,
        ),
        (
            "PUT /identity/users/{id}",
            app.put(
                &format!("/api/v1/identity/users/{some_id}"),
                Some(&token),
                json!({ "displayName": "Renamed" }),
            )
            .await,
        ),
        (
            "DELETE /identity/users/{id}",
            app.delete(&format!("/api/v1/identity/users/{some_id}"), Some(&token))
                .await,
        ),
        (
            "POST /identity/users/{id}/password",
            app.post(
                &format!("/api/v1/identity/users/{some_id}/password"),
                Some(&token),
                json!({ "password": "someone-elses-new-password" }),
            )
            .await,
        ),
        (
            "GET /identity/roles",
            app.get("/api/v1/identity/roles", Some(&token)).await,
        ),
        (
            "GET /identity/roles/{id}",
            app.get(&format!("/api/v1/identity/roles/{some_id}"), Some(&token))
                .await,
        ),
        (
            "POST /identity/roles",
            app.post(
                "/api/v1/identity/roles",
                Some(&token),
                json!({ "roleCode": "ROLE-ESCALATED", "name": "Escalated", "permissionIds": [] }),
            )
            .await,
        ),
        (
            "PUT /identity/roles/{id}",
            app.put(
                &format!("/api/v1/identity/roles/{some_id}"),
                Some(&token),
                json!({ "name": "Renamed" }),
            )
            .await,
        ),
        (
            "DELETE /identity/roles/{id}",
            app.delete(&format!("/api/v1/identity/roles/{some_id}"), Some(&token))
                .await,
        ),
        (
            "GET /identity/permissions",
            app.get("/api/v1/identity/permissions", Some(&token)).await,
        ),
    ];

    let leaked: Vec<&str> = routes
        .iter()
        .filter(|(_, response)| response.status != StatusCode::FORBIDDEN)
        .map(|(route, _)| *route)
        .collect();

    assert!(
        leaked.is_empty(),
        "these identity routes did not answer 403 to a caller with no permissions: {:?}\nstatuses: {:?}",
        leaked,
        routes
            .iter()
            .map(|(route, response)| (*route, response.status))
            .collect::<Vec<_>>()
    );

    // And nothing was written along the way.
    let escalated: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'user.escalated'")
            .fetch_one(&app.pool)
            .await
            .expect("queryable");
    let roles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM roles WHERE role_code = 'ROLE-ESCALATED'")
            .fetch_one(&app.pool)
            .await
            .expect("queryable");

    assert_eq!(escalated, 0, "a refused request still created a user");
    assert_eq!(roles, 0, "a refused request still created a role");
}

// ---------------------------------------------------------------------------
// Tenant scoping (database schema §1.5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn listing_users_returns_only_the_callers_tenant() {
    // Probed with another tenant's rows actually present: a filter that is
    // missing and a filter that is present look identical when there is only
    // ever one tenant in the database.
    let app = TestApp::spawn().await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-001", "Another Customer").await;
    fixtures::create_user(
        &app.pool,
        other_tenant,
        "user.othertenant",
        "other@tenant.test",
        "other-tenant-user-password",
        &[],
    )
    .await;

    let token = app.administrator_token().await;
    let listing = app.get("/api/v1/identity/users", Some(&token)).await;

    assert_eq!(listing.status, StatusCode::OK);

    let body = listing.body.to_string();
    assert!(
        !body.contains("user.othertenant"),
        "another tenant's user appeared in the listing: {body}"
    );

    // The row exists — so the absence above is a filter doing its job, not an
    // insert that silently failed.
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'user.othertenant'")
            .fetch_one(&app.pool)
            .await
            .expect("queryable");

    assert_eq!(stored, 1);
    assert_eq!(
        listing.body["meta"]["total"], 1,
        "the total must count only the caller's tenant, or pagination leaks the other's size"
    );
}
