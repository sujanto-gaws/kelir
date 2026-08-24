//! Every RAD route is bound to its own permission string, and every read is
//! tenant-scoped (#156 AC1, AC5).
//!
//! The sweep is the one `master_data_permissions.rs` established, and its
//! reasoning is worth restating because it is what makes the test worth having:
//! a sweep that only proves "a caller holding nothing is refused" passes
//! whatever permission each route actually requires. Swap `create_form`'s
//! `rad:form:create` for `rad:form:read` and such a sweep stays green — the
//! caller with nothing still gets 403, the administrator holding everything
//! still succeeds — while a read-only account quietly gains the ability to
//! store a form definition that every document will render.
//!
//! So each caller here holds **exactly one** permission and is driven at every
//! route: the routes it opens must not be refused, and every other route must
//! be. A wrong permission string on any route breaks one of the two halves.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The RAD catalogue, seeded by `0014_rad.sql`.
///
/// Written out rather than read from `permissions`: this list is the claim
/// under test, and reading it back would make the test agree with whatever the
/// catalogue happens to hold — including a permission nothing enforces.
const RAD_PERMISSIONS: [&str; 9] = [
    "rad:form:create",
    "rad:form:read",
    "rad:form:update",
    "rad:form:publish",
    "rad:form:delete",
    "rad:list:create",
    "rad:list:read",
    "rad:list:update",
    "rad:list:delete",
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

fn definition(key: &str) -> Value {
    json!({
        "formId": key,
        "version": "2.0.1",
        "components": [{
            "id": "quantity",
            "role": "data",
            "type": "number",
            "key": "quantity",
            "label": "Quantity",
            "validation": { "type": "number" }
        }]
    })
}

/// Every route `rad::handlers::routes()` mounts, with the permission its
/// service function requires.
fn rad_routes(form: Uuid, list: Uuid, nonce: usize) -> Vec<Route> {
    vec![
        Route {
            method: Method::GET,
            path: "/api/v1/rad/forms".into(),
            permission: "rad:form:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/rad/forms".into(),
            permission: "rad:form:create",
            body: Some(json!({
                "formKey": format!("made-by-test-{nonce}"),
                "title": "Made by test",
                "definition": definition(&format!("made-by-test-{nonce}")),
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/rad/forms/{form}"),
            permission: "rad:form:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/rad/forms/{form}"),
            permission: "rad:form:update",
            body: Some(json!({ "title": "Renamed by test" })),
        },
        Route {
            method: Method::POST,
            path: format!("/api/v1/rad/forms/{form}/publish"),
            permission: "rad:form:publish",
            body: None,
        },
        // A new revision is a new row, so it takes the create permission rather
        // than update — asserted here because that is exactly the kind of
        // choice a sweep of "does it 403" would not notice was wrong.
        Route {
            method: Method::POST,
            path: format!("/api/v1/rad/forms/{form}/revisions"),
            permission: "rad:form:create",
            body: Some(json!({ "title": "Next revision" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/rad/forms/{form}"),
            permission: "rad:form:delete",
            body: None,
        },
        Route {
            method: Method::GET,
            path: "/api/v1/rad/lists".into(),
            permission: "rad:list:read",
            body: None,
        },
        Route {
            method: Method::POST,
            path: "/api/v1/rad/lists".into(),
            permission: "rad:list:create",
            body: Some(json!({
                "listKey": format!("made-by-test-{nonce}"),
                "title": "Made by test",
            })),
        },
        Route {
            method: Method::GET,
            path: format!("/api/v1/rad/lists/{list}"),
            permission: "rad:list:read",
            body: None,
        },
        Route {
            method: Method::PUT,
            path: format!("/api/v1/rad/lists/{list}"),
            permission: "rad:list:update",
            body: Some(json!({ "title": "Renamed by test" })),
        },
        Route {
            method: Method::DELETE,
            path: format!("/api/v1/rad/lists/{list}"),
            permission: "rad:list:delete",
            body: None,
        },
    ]
}

/// A user holding exactly `permission` and nothing else, signed in.
async fn caller_holding_only(app: &TestApp, permission: &str, nonce: usize) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-RAD-ONLY-{nonce}"),
        &[permission],
    )
    .await;

    let username = format!("user.rad{nonce}");
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("rad{nonce}@kelir.test"),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

/// A form to aim requests at, inserted directly so that seeding it does not
/// itself depend on the permission under test.
async fn target_form(app: &TestApp, tenant_id: Uuid, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO rad_forms (id, tenant_id, form_key, title, jfss_version, definition_json)
         VALUES ($1, $2, $3, 'Target form', '2.0.1', $4)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(format!("target-form-{nonce}"))
    .bind(definition(&format!("target-form-{nonce}")))
    .execute(&app.pool)
    .await
    .expect("insert the target form");

    id
}

async fn target_list(app: &TestApp, tenant_id: Uuid, nonce: usize) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO rad_lists (id, tenant_id, list_key, title)
         VALUES ($1, $2, $3, 'Target list')",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(format!("target-list-{nonce}"))
    .execute(&app.pool)
    .await
    .expect("insert the target list");

    id
}

#[tokio::test]
async fn each_rad_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;

    for (nonce, permission) in RAD_PERMISSIONS.iter().enumerate() {
        let token = caller_holding_only(&app, permission, nonce).await;
        let form = target_form(&app, fixtures::SYSTEM_TENANT_ID, nonce).await;
        let list = target_list(&app, fixtures::SYSTEM_TENANT_ID, nonce).await;

        for route in rad_routes(form, list, nonce) {
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
                // asserts is that authorization did not refuse.
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
                    "{} was reachable by a caller holding only {permission}, body {}",
                    route.label(),
                    response.body
                );
            }
        }
    }
}

#[tokio::test]
async fn every_rad_route_refuses_a_request_without_a_token() {
    let app = TestApp::spawn().await;
    let form = target_form(&app, fixtures::SYSTEM_TENANT_ID, 900).await;
    let list = target_list(&app, fixtures::SYSTEM_TENANT_ID, 900).await;

    for route in rad_routes(form, list, 900) {
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

/// A form belonging to another tenant is invisible, not forbidden.
///
/// **404 rather than 403 is the point.** A 403 would confirm the row exists,
/// which is a disclosure across a tenant boundary — the caller learns that some
/// other tenant has a form with that id. The repository's `tenant_id` predicate
/// is what makes the read return nothing at all.
#[tokio::test]
async fn a_form_in_another_tenant_is_not_found() {
    let app = TestApp::spawn().await;
    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-RAD-OTHER", "Other tenant").await;
    let hidden = target_form(&app, other_tenant, 901).await;

    let token = app.administrator_token().await;
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/forms/{hidden}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "an administrator holding every permission must still not see another \
         tenant's form; body {}",
        response.body
    );
}

/// The same for a list, and the same reasoning.
#[tokio::test]
async fn a_list_in_another_tenant_is_not_found() {
    let app = TestApp::spawn().await;
    let other_tenant =
        fixtures::create_tenant(&app.pool, "TNT-RAD-OTHER-2", "Other tenant 2").await;
    let hidden = target_list(&app, other_tenant, 902).await;

    let token = app.administrator_token().await;
    let response = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/lists/{hidden}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "body {}",
        response.body
    );
}

/// A page of forms holds this tenant's forms and no others.
///
/// Distinct from the single read above: a list endpoint that forgot its tenant
/// predicate would still 404 on a direct read of another tenant's row if that
/// read has its own predicate, and would leak the whole page. Both are checked
/// because both have been the bug somewhere.
#[tokio::test]
async fn a_page_of_forms_holds_no_other_tenants_forms() {
    let app = TestApp::spawn().await;
    let other_tenant =
        fixtures::create_tenant(&app.pool, "TNT-RAD-OTHER-3", "Other tenant 3").await;
    let hidden = target_form(&app, other_tenant, 903).await;
    let mine = target_form(&app, fixtures::SYSTEM_TENANT_ID, 904).await;

    let token = app.administrator_token().await;
    let response = app
        .send(
            Method::GET,
            "/api/v1/rad/forms?pageSize=100",
            Some(&token),
            None,
        )
        .await;

    let ids: Vec<String> = response.body["data"]
        .as_array()
        .expect("a page of forms")
        .iter()
        .map(|row| row["id"].as_str().unwrap_or_default().to_owned())
        .collect();

    assert!(
        ids.contains(&mine.to_string()),
        "this tenant's own form is missing from the page: {ids:?}"
    );
    assert!(
        !ids.contains(&hidden.to_string()),
        "another tenant's form reached the page: {ids:?}"
    );
}
