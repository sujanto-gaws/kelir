//! The user-administration flows Sprint 3 recorded as hand-verified (#59).
//!
//! Duplicate handling, validation reporting, the self-deactivation guard and
//! the session consequences of an administrative password reset had no
//! automated coverage. Each is a rule that reads as obviously true and holds
//! only because a specific line of code makes it so.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";

/// A well-formed create-user payload, for tests that vary one thing about it.
fn payload(username: &str) -> serde_json::Value {
    json!({
        "username": username,
        "email": format!("{username}@kelir.test"),
        "password": PASSWORD,
        "displayName": "Test Person",
    })
}

async fn user_id_of(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user exists")
}

#[tokio::test]
async fn creating_a_user_with_a_duplicate_username_is_a_conflict() {
    // The unique index is the thing that actually holds, and a 500 from a raw
    // constraint violation would be indistinguishable from a broken server to
    // any client trying to report "that name is taken".
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            payload("duplicate.name"),
        )
        .await;

    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);

    let second = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "duplicate.name",
                // A different address, so the username is unambiguously what
                // collided.
                "email": "someone.else@kelir.test",
                "password": PASSWORD,
                "displayName": "Someone Else",
            }),
        )
        .await;

    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(second.error_code(), Some("CONFLICT"));

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = $1")
        .bind("duplicate.name")
        .fetch_one(&app.pool)
        .await
        .expect("count runs");

    assert_eq!(count, 1, "the refused create must not have written a row");
}

#[tokio::test]
async fn creating_a_user_with_a_duplicate_email_is_a_conflict() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    app.post(
        "/api/v1/identity/users",
        Some(&token),
        payload("first.holder"),
    )
    .await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "second.holder",
                "email": "first.holder@kelir.test",
                "password": PASSWORD,
                "displayName": "Second Holder",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn creating_a_user_reports_every_invalid_field_at_once() {
    // A form marks all its bad fields in one pass (JSON Form Schema S10.3).
    // Stopping at the first would make a four-mistake payload take four
    // round trips to fix, and the validator collects rather than returns early
    // precisely so it does not.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let response = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "",
                "email": "not-an-address",
                "password": "short",
                "displayName": "   ",
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));

    let reported: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .map(|detail| detail["path"].as_str().expect("path is a string"))
        .collect();

    for field in ["username", "email", "password", "displayName"] {
        assert!(
            reported.contains(&field),
            "`{field}` was not reported; got {reported:?}"
        );
    }
}

#[tokio::test]
async fn an_administrator_cannot_deactivate_their_own_account() {
    // The permission check passes first — an administrator holds
    // `identity:user:delete` — and the self-check is what refuses. Without it a
    // single-administrator deployment can be left with no way in, which is the
    // same failure mode the permanent lockout had (#55).
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let admin_id = user_id_of(&app, common::ADMIN_USERNAME).await;

    let response = app
        .delete(&format!("/api/v1/identity/users/{admin_id}"), Some(&token))
        .await;

    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "expected the self-deactivation guard, got {}: {}",
        response.status,
        response.body
    );

    // Still able to sign in, which is the consequence that matters.
    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM users WHERE id = $1")
            .bind(admin_id)
            .fetch_one(&app.pool)
            .await
            .expect("select runs");

    assert!(
        deleted_at.is_none(),
        "the administrator's account was soft-deleted by the refused request"
    );
}

#[tokio::test]
async fn an_administrator_can_deactivate_another_account() {
    // The control for the guard above: it must refuse self-deletion only, not
    // deletion generally.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "removable.user",
        "removable.user@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let response = app
        .delete(&format!("/api/v1/identity/users/{id}"), Some(&token))
        .await;

    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn setting_a_password_ends_every_refresh_token_for_that_user() {
    // What an administrative reset is *for*: the previous holder of the
    // password stops being able to extend their session.
    //
    // It ends the refresh tokens, not the access token already issued — that
    // one lives out its fifteen minutes, which is the documented cost of
    // carrying permissions in the token. #16 and #60 carry the same gap on the
    // self-service path, where the contract claims otherwise.
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "reset.target",
        "reset.target@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let signed_in = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "reset.target", "password": PASSWORD }),
        )
        .await;

    let refresh = signed_in.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();

    let reset = app
        .post(
            &format!("/api/v1/identity/users/{id}/password"),
            Some(&admin),
            json!({ "password": "a different long password" }),
        )
        .await;

    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);

    let refreshed = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;

    assert_eq!(
        refreshed.status,
        StatusCode::UNAUTHORIZED,
        "the pre-reset refresh token still worked"
    );

    // And the new password is the one that works.
    let with_new = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "reset.target", "password": "a different long password" }),
        )
        .await;

    assert_eq!(with_new.status, StatusCode::OK, "{}", with_new.body);
}

/// **The tenant boundary, asserted from outside it** (#204).
///
/// Quarantined from Sprint 3 until 2026-08-27 on decision **D-7**, which
/// deferred per-request tenant resolution: sign-in resolved the configured
/// default tenant for every credential, so no token could carry another
/// tenant and the caller this test needs could not be arranged. **D-18**
/// superseded D-7 on 2026-08-25 — `resolve_for_sign_in` honours a requested
/// code on a multi-tenant deployment — so the condition the `#[ignore]` named
/// is met and the quarantine is gone.
///
/// **The caller holds `identity:user:read` deliberately, and taking that away
/// turns this into a different test.** Without it the route answers `403` at
/// the permission gate, before tenancy is consulted at all, and the assertion
/// below would be green while guarding nothing about tenants. The quarantined
/// body passed `&[]` and had exactly that defect: un-ignored as written it
/// would have read as tenant isolation and asserted the permission check.
///
/// Seen red (coding standard §2.9) against `find_user`'s `tenant_id = $1`
/// weakened to `(tenant_id = $1 OR TRUE)`: the foreign user's record comes
/// back `200` with its username and address in it.
#[tokio::test]
async fn reading_another_tenants_user_by_id_is_not_found() {
    // The tenant code has to reach sign-in, which needs the deployment mode
    // D-7 refused to let anything run in.
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;

    let other_tenant = fixtures::create_tenant(&app.pool, "TNT-002", "Another Customer").await;
    let reader = fixtures::create_role_with_permissions(
        &app.pool,
        other_tenant,
        "OTHER-TENANT-READER",
        &["identity:user:read"],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        other_tenant,
        "tenant.caller",
        "tenant.caller@other.test",
        PASSWORD,
        &[reader],
    )
    .await;

    let system_user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "system.subject",
        "system.subject@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    let token = app.sign_in_to("TNT-002", "tenant.caller", PASSWORD).await;

    let response = app
        .get(
            &format!("/api/v1/identity/users/{system_user}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::NOT_FOUND,
        "another tenant's user must be invisible, not merely unreadable: {}",
        response.body
    );
}
