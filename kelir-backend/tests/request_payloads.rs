//! A misspelled field is refused, not discarded (issue #62).
//!
//! `CreateUserRequest` and friends carried `rename_all = "camelCase"`, their
//! collection fields carried `#[serde(default)]`, and nothing denied unknown
//! fields. The three together produced silent data loss: a client posting
//! `role_ids` instead of `roleIds` got **201 Created, a user with no roles, and
//! no error**. The unknown field was discarded, the missing one defaulted to
//! empty, and every layer reported success — the damage surfaced later as a
//! permissions problem, a long way from its cause.
//!
//! The unit tests in `src/extract.rs` pin the extractor's behaviour. This file
//! pins the thing that actually matters: **on the real routes, with a real
//! database, the row is not written.** A 422 that still created the user would
//! satisfy the extractor tests and none of the point.
//!
//! Every case here was run against the pre-fix code and seen to pass — which is
//! why the assertions are on the database rather than only on the status.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::{json, Value};

/// The first validation detail, or `Value::Null` if the envelope carries none.
fn first_detail(response: &common::TestResponse) -> &Value {
    &response.body["error"]["details"][0]
}

#[tokio::test]
async fn creating_a_user_with_a_snake_case_role_field_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "silently.roleless",
                "email": "silently.roleless@example.com",
                "password": "correct horse battery",
                "displayName": "Silently Roleless",
                // The defect, exactly as reported: snake_case where the API is
                // camelCase.
                "role_ids": [],
            }),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "expected 422, got {} with {}",
        response.status,
        response.body
    );
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));
    assert_eq!(first_detail(&response)["path"], "role_ids");
    assert_eq!(first_detail(&response)["code"], "UNKNOWN_FIELD");

    // The half that the status code alone would not prove.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = $1")
        .bind("silently.roleless")
        .fetch_one(&app.pool)
        .await
        .expect("count runs");

    assert_eq!(count, 0, "the refused request must not have written a user");
}

#[tokio::test]
async fn creating_a_role_with_a_snake_case_permission_field_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({
                "roleCode": "ROLE-SILENT",
                "name": "Silently Powerless",
                "permission_ids": [],
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_detail(&response)["path"], "permission_ids");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM roles WHERE role_code = $1")
        .bind("ROLE-SILENT")
        .fetch_one(&app.pool)
        .await
        .expect("count runs");

    assert_eq!(count, 0, "the refused request must not have written a role");
}

#[tokio::test]
async fn a_correctly_spelled_payload_still_succeeds() {
    // The regression guard on the fix itself: `deny_unknown_fields` is one
    // attribute away from refusing every request the frontend sends.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-READER",
        &[],
    )
    .await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "properly.named",
                "email": "properly.named@example.com",
                "password": "correct horse battery",
                "displayName": "Properly Named",
                "departmentId": null,
                "roleIds": [role],
            }),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "the well-formed payload was refused: {}",
        response.body
    );
    assert_eq!(response.data()["roles"][0]["roleCode"], "ROLE-READER");
}

#[tokio::test]
async fn an_omitted_optional_field_is_still_optional() {
    // `deny_unknown_fields` denies unknown fields; it must not quietly make
    // optional ones required. The frontend omits `departmentId` and `roleIds`.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "minimal.payload",
                "email": "minimal.payload@example.com",
                "password": "correct horse battery",
                "displayName": "Minimal Payload",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
}

#[tokio::test]
async fn the_unauthenticated_sign_in_route_is_covered_too() {
    // #62 was found on identity, but the sweep was the point: `/auth/login`
    // takes a body before any token exists, and a client misspelling
    // `tenantCode` there would have been told nothing at all.
    let app = TestApp::spawn().await;

    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": common::ADMIN_USERNAME,
                "password": common::ADMIN_PASSWORD,
                "tenant_code": "SYSTEM",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_detail(&response)["path"], "tenant_code");
    assert_eq!(first_detail(&response)["code"], "UNKNOWN_FIELD");
}

#[tokio::test]
async fn an_update_with_an_unknown_field_leaves_the_row_untouched() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "unchanged.user",
        "unchanged.user@kelir.test",
        "correct horse battery",
        &[],
    )
    .await;

    let response = app
        .put(
            &format!("/api/v1/identity/users/{user}"),
            Some(&token),
            json!({
                "displayName": "Renamed",
                "display_name": "Renamed Again",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let display_name: String = sqlx::query_scalar("SELECT display_name FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .expect("select runs");

    assert_ne!(
        display_name, "Renamed",
        "a rejected update must not have applied its recognised fields"
    );
}
