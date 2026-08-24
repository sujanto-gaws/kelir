//! Every document-type route is bound to its own permission string (#157 AC1).
//!
//! The sweep `master_data_permissions.rs` established: each caller holds
//! **exactly one** permission and is driven at every route, so a route carrying
//! the wrong permission string breaks one of the two halves. A sweep that only
//! proved "a caller holding nothing is refused" would pass whatever permission
//! each route actually requires.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The document-type catalogue, seeded by `0015_document.sql`. Written out
/// rather than read from `permissions`: this list is the claim under test.
const TYPE_PERMISSIONS: [&str; 4] = [
    "document-type:create",
    "document-type:read",
    "document-type:update",
    "document-type:delete",
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

fn type_routes(target: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: "/api/v1/document-types".into(),
            permission: "document-type:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/document-types".into(),
            permission: "document-type:create",
            body: Some(json!({
                "typeCode": format!("MADE_BY_TEST_{nonce}"),
                "name": "Made by test",
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/document-types/{target}"),
            permission: "document-type:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/document-types/{target}"),
            permission: "document-type:update",
            body: Some(json!({ "name": "Renamed by test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/document-types/{target}"),
            permission: "document-type:delete",
            body: None,
        },
    ]
}

async fn caller_holding_only(app: &TestApp, permission: &str, nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-DT-ONLY-{nonce}"),
        &[permission],
    )
    .await;

    let username = format!("user.dt{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("dt{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A type to aim requests at, inserted directly so that seeding it does not
/// itself depend on the permission under test.
async fn target_type(app: &TestApp, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO document_types (id, tenant_id, type_code, name)
         VALUES ($1, $2, $3, 'Target type')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(format!("TARGET_TYPE_{nonce}"))
    .execute(&app.pool)
    .await
    .expect("insert the target document type");

    id
}

#[tokio::test]
async fn each_document_type_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in TYPE_PERMISSIONS.iter().enumerate() {
        let token = caller_holding_only(&app, permission, nonce).await;
        let target = target_type(&app, nonce).await;

        for route in type_routes(target, nonce) {
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
async fn every_document_type_route_refuses_a_request_without_a_token() {
    let app = TestApp::spawn().await;
    let target = target_type(&app, 900).await;

    for route in type_routes(target, 900) {
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
