//! Every identity route is bound to its own permission string (issue #58).
//!
//! Sprint 3's authorization sweep proves that a caller holding *no* permission
//! is refused, and nothing more. That check passes whatever permission each
//! route actually requires: swap `create_user`'s `identity:user:create` for
//! `identity:user:read` and the suite stays green — the caller with no
//! permissions still gets 403, the administrator holding all eight still
//! succeeds — while a read-only account quietly gains the ability to mint
//! administrators. FR-IDM-005 and FR-API-008 have rested on that check since
//! Sprint 3, and #22 and #29 closed on it.
//!
//! The difference here is a caller holding exactly one permission. For each of
//! the eight identity permissions, one user is granted that permission and
//! nothing else, and then driven at all twelve identity routes: the routes that
//! permission opens must not be refused, and **every other route must be**. A
//! wrong permission string on any route breaks one of the two halves.
//!
//! Two further things this file pins down, both named in #58:
//!
//! * `POST /api/v1/identity/users` is the highest-privilege write on the
//!   surface — it can mint an account holding `ROLE-ADMIN` — and had no
//!   success-path test at any level. Replacing its body with
//!   `Err(AppError::Forbidden)` kept the whole suite green.
//! * Authentication was spot-checked on one protected route out of fifteen,
//!   while FR-API-008 claims all of them.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The identity catalogue, seeded by `0002_identity.sql`.
///
/// Written out rather than read from the database: this list is the claim under
/// test. Reading it back from `permissions` would make the test agree with
/// whatever the catalogue happens to hold, including a permission nothing
/// enforces.
const IDENTITY_PERMISSIONS: [&str; 11] = [
    "identity:user:create",
    "identity:user:read",
    "identity:user:update",
    "identity:user:delete",
    "identity:role:create",
    "identity:role:read",
    "identity:role:update",
    "identity:role:delete",
    // The delegation slice (FR-IDM-006, #184). Its three permissions were
    // seeded by `0005_delegation_tenant_permissions.sql` four sprints before
    // anything checked them, which is exactly the shape this list exists to
    // catch: a permission in the catalogue that nothing enforces.
    "identity:delegation:create",
    "identity:delegation:read",
    "identity:delegation:delete",
];

const PASSWORD: &str = "single-permission-user-password";

/// One route, and the one permission that opens it.
struct Route {
    method: Method,
    path: String,
    /// The permission this route requires, as the service asks for it.
    permission: &'static str,
    /// A structurally valid body.
    ///
    /// Extraction runs before the service does, so a malformed body answers 422
    /// and never reaches the permission check. A test asserting 403 against a
    /// body the router rejects would be asserting nothing about authorization.
    body: Option<Value>,
}

impl Route {
    fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// Every route `identity::handlers::routes()` mounts, with the permission its
/// service function requires.
///
/// `nonce` keeps the created username and role code unique, so the same table
/// can be driven once per permission without colliding on the unique indexes.
fn identity_routes(target_user: Uuid, target_role: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: "/api/v1/identity/users".into(),
            permission: "identity:user:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/identity/users".into(),
            permission: "identity:user:create",
            body: Some(json!({
                "username": format!("user.made{nonce}"),
                "email": format!("made{nonce}@kelir.test"),
                "password": "a-created-account-password",
                "displayName": "Created By Test",
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/identity/users/{target_user}"),
            permission: "identity:user:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/identity/users/{target_user}"),
            permission: "identity:user:update",
            body: Some(json!({ "displayName": "Renamed By Test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/identity/users/{target_user}"),
            permission: "identity:user:delete",
            body: None,
        },
        Route {
            method: Method::POST,
            path: format!("/api/v1/identity/users/{target_user}/password"),
            permission: "identity:user:update",
            body: Some(json!({ "password": "a-reset-account-password" })),
        },
        Route {
            method: Method::GET,
            path: "/api/v1/identity/roles".into(),
            permission: "identity:role:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/identity/roles".into(),
            permission: "identity:role:create",
            body: Some(json!({
                "roleCode": format!("ROLE-MADE-{nonce}"),
                "name": "Created By Test",
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/identity/roles/{target_role}"),
            permission: "identity:role:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/identity/roles/{target_role}"),
            permission: "identity:role:update",
            body: Some(json!({ "name": "Renamed By Test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/identity/roles/{target_role}"),
            permission: "identity:role:delete",
            body: None,
        },
        Route {
            method: Method::GET,
            path: "/api/v1/identity/delegations".into(),
            permission: "identity:delegation:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/identity/delegations".into(),
            permission: "identity:delegation:create",
            // The window is opened in the *caller's* name — the request type has
            // no delegator field — so `target_user` is the delegate, and the
            // caller is whoever this row is being driven as. A far-future end
            // keeps the body valid however long this suite lives: the service
            // refuses a window that is already over.
            body: Some(json!({
                "delegateUserId": target_user,
                "startsAt": "2099-01-01T00:00:00Z",
                "endsAt": "2099-02-01T00:00:00Z",
            })),
        },
        Route {
            method: Method::DELETE,
            // No delegation is created for this row to aim at, and none is
            // needed: what is under test is whether authorization runs before
            // the row is looked for. A caller holding the permission gets 404
            // and everybody else gets 403, which is the distinction the assert
            // below is written in terms of.
            path: format!("/api/v1/identity/delegations/{}", Uuid::now_v7()),
            permission: "identity:delegation:delete",
            body: None,
        },
        Route {
            method: Method::GET,
            path: "/api/v1/identity/permissions".into(),
            // The catalogue is read under the role permission, not a permission
            // of its own: FR-IDM-004 as narrowed by decision D-6 makes the
            // catalogue system-defined, so reading it is part of seeing what a
            // role may hold.
            permission: "identity:role:read",
            body: None,
        },
    ]
}

/// A user holding exactly `permission` and nothing else, signed in.
async fn caller_holding_only(app: &TestApp, permission: &str, nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-ONLY-{nonce}"),
        &[permission],
    )
    .await;

    let username = format!("user.only{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("only{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A user and a role for requests to be aimed at, so no test ever mutates its
/// own caller — `deactivate_user` refuses self-deletion, and a caller who
/// deleted themselves would fail the next assertion for the wrong reason.
async fn targets(app: &TestApp, nonce: usize) -> (Uuid, Uuid) {
    let user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("user.target{nonce}"),
        &format!("target{nonce}@kelir.test"),
        PASSWORD,
        &[],
    )
    .await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-TARGET-{nonce}"),
        &[],
    )
    .await;

    (user, role)
}

// ---------------------------------------------------------------------------
// The binding
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_identity_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in IDENTITY_PERMISSIONS.iter().enumerate() {
        let token = caller_holding_only(&app, permission, nonce).await;
        let (target_user, target_role) = targets(&app, nonce).await;

        for route in identity_routes(target_user, target_role, nonce) {
            let response = app
                .send(
                    route.method.clone(),
                    &route.path,
                    Some(&token),
                    route.body.clone(),
                )
                .await;

            if route.permission == *permission {
                // Not "200": several of these answer 204, 201 or 404 depending
                // on the target's state, and none of that is what this test is
                // about. What it asserts is that authorization did not refuse —
                // a route whose permission string no longer matches the one it
                // documents shows up here as a 403.
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
                // The other half, and the one Sprint 3's sweep could not make:
                // holding *a* permission is not holding *this* one.
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
async fn a_reader_may_list_users_but_may_not_create_one() {
    let app = TestApp::spawn().await;
    let token = caller_holding_only(&app, "identity:user:read", 100).await;

    // The same claim as the table above, in the form a person can check at a
    // glance. If the table ever stops failing for the right reason, this is the
    // case that still says what the product is supposed to do.
    let listed = app.get("/api/v1/identity/users", Some(&token)).await;
    assert_eq!(listed.status, StatusCode::OK, "body {}", listed.body);

    let created = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "user.escalated",
                "email": "escalated@kelir.test",
                "password": "an-escalated-account-password",
                "displayName": "Should Not Exist",
                "roleIds": [fixtures::ADMIN_ROLE_ID],
            }),
        )
        .await;
    assert_eq!(
        created.status,
        StatusCode::FORBIDDEN,
        "reading users must not let a caller mint an administrator, body {}",
        created.body
    );

    let count =
        sqlx::query_scalar!("SELECT count(*) FROM users WHERE username = 'user.escalated'",)
            .fetch_one(&app.pool)
            .await
            .expect("count users");
    assert_eq!(
        count,
        Some(0),
        "the refusal must also mean no row was written"
    );
}

// ---------------------------------------------------------------------------
// The success path that was never asserted
// ---------------------------------------------------------------------------

#[tokio::test]
async fn creating_a_user_grants_the_roles_it_names() {
    let app = TestApp::spawn().await;
    let token = caller_holding_only(&app, "identity:user:create", 200).await;

    let created = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "user.newadmin",
                "email": "newadmin@kelir.test",
                "password": "a-new-administrator-password",
                "displayName": "New Administrator",
                "roleIds": [fixtures::ADMIN_ROLE_ID],
            }),
        )
        .await;

    // Until now, replacing this handler's body with a bare `Forbidden` kept the
    // whole suite green: no test anywhere drove the highest-privilege write on
    // the surface to a success.
    assert_eq!(created.status, StatusCode::CREATED, "body {}", created.body);

    let granted = sqlx::query_scalar!(
        r#"
        SELECT count(*) FROM user_roles ur
        JOIN users u ON u.id = ur.user_id
        WHERE u.username = 'user.newadmin' AND ur.role_id = $1
        "#,
        fixtures::ADMIN_ROLE_ID
    )
    .fetch_one(&app.pool)
    .await
    .expect("count granted roles");

    // What the response said and what was stored are different claims. A
    // handler that answered 201 and dropped `roleIds` would pass the assertion
    // above and leave an administrator who is not one.
    assert_eq!(
        granted,
        Some(1),
        "the role named in the request must actually be granted"
    );
}

// ---------------------------------------------------------------------------
// Authentication, on every route rather than one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_protected_route_refuses_a_request_without_a_token() {
    let app = TestApp::spawn().await;
    let (target_user, target_role) = targets(&app, 300).await;

    let mut protected: Vec<Route> = identity_routes(target_user, target_role, 300);

    // The authenticated half of `/auth`. `/login` and `/refresh` are open by
    // definition — they are how a caller gets a token.
    //
    // `/logout` is open too, and deliberately: it identifies the caller from the
    // refresh token being revoked, because signing out has to work once the
    // access token has expired, which is when a client is most likely to try.
    // #58 lists it among the protected routes; that is the one place the issue
    // is wrong about the product, and `signing_out_needs_no_access_token`
    // below pins the contract it actually has.
    protected.extend([
        Route {
            method: Method::GET,
            path: "/api/v1/auth/me".into(),
            permission: "",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/auth/change-password".into(),
            permission: "",
            body: Some(json!({
                "currentPassword": PASSWORD,
                "newPassword": "a-changed-account-password",
            })),
        },
    ]);

    assert_eq!(
        protected.len(),
        17,
        "every protected route belongs in this list; FR-API-008 claims all of them"
    );

    for route in protected {
        let response = app
            .send(route.method.clone(), &route.path, None, route.body.clone())
            .await;

        // 401 rather than 403: with no token there is no caller to have
        // permissions, and answering 403 would tell an anonymous caller that the
        // route exists and that they merely lack a grant.
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{} must refuse an untokened request, body {}",
            route.label(),
            response.body
        );
    }
}

#[tokio::test]
async fn signing_out_needs_no_access_token_but_revokes_the_refresh_token() {
    let app = TestApp::spawn().await;

    let session = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": common::ADMIN_USERNAME, "password": common::ADMIN_PASSWORD }),
        )
        .await;
    assert_eq!(session.status, StatusCode::OK, "body {}", session.body);
    let refresh_token = session.data()["refreshToken"]
        .as_str()
        .expect("a sign-in returns a refresh token")
        .to_owned();

    // No bearer token, and that is the contract: an expired access token is the
    // normal reason to be signing out. Requiring one here would leave a client
    // holding a live refresh token it cannot revoke.
    let signed_out = app
        .post(
            "/api/v1/auth/logout",
            None,
            json!({ "refreshToken": refresh_token }),
        )
        .await;
    assert_eq!(
        signed_out.status,
        StatusCode::NO_CONTENT,
        "body {}",
        signed_out.body
    );

    // Open is not the same as inert. The route's authority is the refresh token
    // it was given, so the proof that it did its job is that the token is dead.
    let reused = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh_token }),
        )
        .await;
    assert_eq!(
        reused.status,
        StatusCode::UNAUTHORIZED,
        "the revoked refresh token must not mint a new session, body {}",
        reused.body
    );
}
