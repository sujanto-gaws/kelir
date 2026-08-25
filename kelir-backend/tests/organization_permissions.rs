//! Every department route is bound to its own permission string (#28).
//!
//! This file exists because a mutation survived without it: swapping the list
//! route's `organization:department:read` for `:manage` left every other test
//! in the department suite green, since all of them sign in as an administrator
//! who holds both. That is the sweep `master_data_permissions.rs` was written
//! to catch, and it had no counterpart here.
//!
//! **Two permissions, not four.** `0002_identity.sql` seeded
//! `organization:department:read` and `organization:department:manage` in Phase
//! 2 and nothing ever checked them; this module uses those rather than inventing
//! a parallel set and leaving the originals orphaned. So `manage` covers create,
//! update and delete, and the sweep below asserts exactly that split rather than
//! a finer one nobody granted.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const DEPARTMENT_PERMISSIONS: [&str; 2] = [
    "organization:department:read",
    "organization:department:manage",
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

fn department_routes(target: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: "/api/v1/organization/departments".into(),
            permission: "organization:department:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/organization/departments".into(),
            permission: "organization:department:manage",
            body: Some(json!({
                "departmentId": format!("DEPT-MADE-{nonce}"),
                "name": "Made by test",
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/organization/departments/{target}"),
            permission: "organization:department:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/organization/departments/{target}"),
            permission: "organization:department:manage",
            body: Some(json!({ "name": "Renamed by test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/organization/departments/{target}"),
            permission: "organization:department:manage",
            body: None,
        },
    ]
}

async fn caller_holding_only(app: &TestApp, permission: &str, nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-ORG-ONLY-{nonce}"),
        &[permission],
    )
    .await;

    let username = format!("user.org{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("org{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A department to aim requests at, inserted directly so that seeding it does
/// not itself depend on the permission under test.
async fn target_department(app: &TestApp, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name)
         VALUES ($1, $2, $3, 'Target department')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(format!("DEPT-TARGET-{nonce}"))
    .execute(&app.pool)
    .await
    .expect("insert the target department");

    id
}

#[tokio::test]
async fn each_department_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in DEPARTMENT_PERMISSIONS.iter().enumerate() {
        let token = caller_holding_only(&app, permission, nonce).await;
        let target = target_department(&app, nonce).await;

        for route in department_routes(target, nonce) {
            let response = app
                .send(
                    route.method.clone(),
                    &route.path,
                    Some(&token),
                    route.body.clone(),
                )
                .await;

            if route.permission == *permission {
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
                    "{} was reachable by a caller holding only {permission}, body {}",
                    route.label(),
                    response.body
                );
            }
        }
    }
}

#[tokio::test]
async fn every_department_route_refuses_a_request_without_a_token() {
    let app = TestApp::spawn().await;
    let target = target_department(&app, 900).await;

    for route in department_routes(target, 900) {
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

/// FR-IDM-008's edge: a user may only be assigned to a department that exists.
///
/// Before this the reference went straight to the foreign key, so a mistyped id
/// was a 500 naming a constraint rather than a 422 naming the field — and no
/// test noticed, which is why the mutation that removed the check stayed green.
#[tokio::test]
async fn a_user_cannot_be_assigned_to_a_department_that_does_not_exist() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let invented = Uuid::now_v7();

    let response = app
        .send(
            Method::POST,
            "/api/v1/identity/users",
            Some(&token),
            Some(json!({
                "username": "user.nodept",
                "email": "nodept@kelir.test",
                "password": "a-perfectly-good-password",
                "displayName": "No Department",
                "departmentId": invented,
            })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a department that does not exist must be a 422 naming the field, not a \
         500 naming a foreign key; body {}",
        response.body
    );
    assert_eq!(response.body["error"]["details"][0]["path"], "departmentId");
}

/// And a department that does exist is accepted, and can then be cleared.
///
/// The clearing half matters on its own: `COALESCE` reads a missing field and
/// an explicit null identically, so before this the column could be set and
/// never unset — a person who left a department kept it forever.
#[tokio::test]
async fn a_user_is_assigned_to_a_department_and_can_be_removed_from_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = app
        .send(
            Method::POST,
            "/api/v1/organization/departments",
            Some(&token),
            Some(json!({ "departmentId": "DEPT-STAFFED", "name": "Staffed" })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let department_id = created.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();

    let user = app
        .send(
            Method::POST,
            "/api/v1/identity/users",
            Some(&token),
            Some(json!({
                "username": "user.withdept",
                "email": "withdept@kelir.test",
                "password": "a-perfectly-good-password",
                "displayName": "With Department",
                "departmentId": department_id,
            })),
        )
        .await;

    assert_eq!(user.status, StatusCode::CREATED, "{}", user.body);
    assert_eq!(user.body["data"]["departmentId"], department_id);

    let user_id = user.body["data"]["id"].as_str().expect("an id").to_owned();

    let cleared = app
        .send(
            Method::PUT,
            &format!("/api/v1/identity/users/{user_id}"),
            Some(&token),
            Some(json!({ "departmentId": null })),
        )
        .await;

    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert!(
        cleared.body["data"]["departmentId"].is_null(),
        "an explicit null clears the department; body {}",
        cleared.body
    );
}
