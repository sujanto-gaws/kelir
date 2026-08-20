//! Change password (#16, FR-AUTH-005), and what it does to a live session
//! (#60).
//!
//! The endpoint merged in Sprint 4 with no test on either path — neither the
//! success nor the refusal — against coding standard §2.9. Verification also
//! found the contract overstated: the OpenAPI response said "every session for
//! the account ends", while only refresh tokens are revoked.
//!
//! The wording is now narrowed to what the code does, and
//! `an_access_token_issued_before_a_password_change_still_works` pins the
//! residual window so it stays recorded rather than assumed closed. If that
//! test ever fails, someone has closed the gap and the contract can widen
//! again.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";
const NEW_PASSWORD: &str = "a different long password";

async fn user(app: &TestApp, username: &str) -> Uuid {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@kelir.test"),
        PASSWORD,
        &[],
    )
    .await
}

/// Signs in and returns `(access token, refresh token)`.
async fn session_for(app: &TestApp, username: &str, password: &str) -> (String, String) {
    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": username, "password": password }),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    (
        response.data()["accessToken"]
            .as_str()
            .expect("accessToken is a string")
            .to_owned(),
        response.data()["refreshToken"]
            .as_str()
            .expect("refreshToken is a string")
            .to_owned(),
    )
}

async fn change_password(
    app: &TestApp,
    token: &str,
    current: &str,
    new: &str,
) -> common::TestResponse {
    app.post(
        "/api/v1/auth/change-password",
        Some(token),
        json!({ "currentPassword": current, "newPassword": new }),
    )
    .await
}

#[tokio::test]
async fn the_new_password_signs_in_and_the_old_one_does_not() {
    let app = TestApp::spawn().await;
    user(&app, "changer.happy").await;
    let (access, _) = session_for(&app, "changer.happy", PASSWORD).await;

    let changed = change_password(&app, &access, PASSWORD, NEW_PASSWORD).await;

    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);

    let with_new = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "changer.happy", "password": NEW_PASSWORD }),
        )
        .await;

    assert_eq!(with_new.status, StatusCode::OK, "{}", with_new.body);

    let with_old = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "changer.happy", "password": PASSWORD }),
        )
        .await;

    assert_eq!(
        with_old.status,
        StatusCode::UNAUTHORIZED,
        "the old password still works, so nothing was actually replaced"
    );
}

#[tokio::test]
async fn changing_a_password_requires_the_current_one() {
    // Otherwise anyone with a borrowed session can lock the owner out of their
    // own account, which is worse than reading it.
    let app = TestApp::spawn().await;
    user(&app, "changer.wrongcurrent").await;
    let (access, _) = session_for(&app, "changer.wrongcurrent", PASSWORD).await;

    let response = change_password(&app, &access, "not my password", NEW_PASSWORD).await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error_code(), Some("VALIDATION_ERROR"));

    let detail = &response.body["error"]["details"][0];
    assert_eq!(detail["path"], "currentPassword");
    assert_eq!(detail["code"], "INCORRECT_PASSWORD");

    // The refusal has to leave the password alone, not merely answer 422.
    let still_the_old_one = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "changer.wrongcurrent", "password": PASSWORD }),
        )
        .await;

    assert_eq!(still_the_old_one.status, StatusCode::OK);
}

#[tokio::test]
async fn a_new_password_below_the_minimum_is_refused_before_the_current_one_is_checked() {
    // Both halves of this payload are wrong. The new password is rejected
    // first, deliberately: validating the cheap thing before verifying an
    // Argon2 hash keeps a caller from using this endpoint as a password oracle
    // that costs the server a hash per guess.
    let app = TestApp::spawn().await;
    user(&app, "changer.tooshort").await;
    let (access, _) = session_for(&app, "changer.tooshort", PASSWORD).await;

    let response = change_password(&app, &access, "also not my password", "short").await;

    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let paths: Vec<&str> = response.body["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .map(|detail| detail["path"].as_str().expect("path is a string"))
        .collect();

    assert!(
        paths.iter().any(|path| path.contains("assword")),
        "expected a password-length failure, got {paths:?}"
    );
    assert!(
        !paths.contains(&"currentPassword"),
        "the wrong current password was reported, so the hash was verified \
         before the length check: {paths:?}"
    );
}

#[tokio::test]
async fn changing_a_password_revokes_every_refresh_token() {
    // Two sessions, and the one that does not make the change must die too —
    // that is the whole point when the change is prompted by a suspected
    // compromise.
    let app = TestApp::spawn().await;
    user(&app, "changer.sessions").await;

    let (access, first_refresh) = session_for(&app, "changer.sessions", PASSWORD).await;
    let (_, second_refresh) = session_for(&app, "changer.sessions", PASSWORD).await;

    let changed = change_password(&app, &access, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);

    for (label, refresh) in [
        ("the session that made the change", first_refresh),
        ("the other session", second_refresh),
    ] {
        let response = app
            .post(
                "/api/v1/auth/refresh",
                None,
                json!({ "refreshToken": refresh }),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{label} could still refresh after the password changed"
        );
    }
}

#[tokio::test]
async fn an_access_token_issued_before_a_password_change_still_works() {
    // **This pins a gap, not a guarantee.** Access tokens are stateless JWTs
    // checked against no revocation list (architecture 01 §18.1), so one issued
    // before the change stays valid until it expires — up to fifteen minutes.
    //
    // The OpenAPI response used to claim "every session for the account ends",
    // which is what #60 found. The wording now says what happens; this test is
    // what keeps the two from drifting apart again.
    //
    // If it ever fails, the window has been closed and the contract can widen.
    // Read the failure as news, not as a regression.
    let app = TestApp::spawn().await;
    user(&app, "changer.stale").await;
    let (access, _) = session_for(&app, "changer.stale", PASSWORD).await;

    let changed = change_password(&app, &access, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT);

    let response = app.get("/api/v1/auth/me", Some(&access)).await;

    assert_eq!(
        response.status,
        StatusCode::OK,
        "the pre-change access token was refused — the revocation gap is closed, \
         so widen the contract in handlers.rs and delete this test"
    );
}

#[tokio::test]
async fn change_password_without_a_token_is_unauthorized() {
    // Taking `Authenticated` is what enforces this (FR-API-008); an unauthenticated
    // change-password would be an account takeover primitive.
    let app = TestApp::spawn().await;
    user(&app, "changer.anonymous").await;

    let response = app
        .post(
            "/api/v1/auth/change-password",
            None,
            json!({ "currentPassword": PASSWORD, "newPassword": NEW_PASSWORD }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.error_code(), Some("UNAUTHORIZED"));
}

#[tokio::test]
async fn a_failed_change_password_attempt_is_audited() {
    // Only success was recorded. Someone holding a live session and guessing at
    // the password is the shape a hijacked session takes, and it appears
    // nowhere in the login record because no login is happening.
    let app = TestApp::spawn().await;
    let id = user(&app, "changer.audited").await;
    let (access, _) = session_for(&app, "changer.audited", PASSWORD).await;

    let response = change_password(&app, &access, "not my password", NEW_PASSWORD).await;
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);

    let (event_type, action, actor): (String, String, Option<Uuid>) = sqlx::query_as(
        "SELECT event_type, action, actor_user_id
           FROM audit_events
          WHERE object_id = $1 AND event_type = 'Security.PasswordChangeFailed'
          ORDER BY created_at DESC, id DESC
          LIMIT 1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the failed attempt was not audited");

    assert_eq!(event_type, "Security.PasswordChangeFailed");
    assert_eq!(action, "UPDATE_FAILED");
    assert_eq!(actor, Some(id));
}

#[tokio::test]
async fn a_successful_change_is_audited() {
    let app = TestApp::spawn().await;
    let id = user(&app, "changer.auditedok").await;
    let (access, _) = session_for(&app, "changer.auditedok", PASSWORD).await;

    let changed = change_password(&app, &access, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT);

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
          WHERE object_id = $1 AND event_type = 'User.PasswordChanged'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count runs");

    assert_eq!(count, 1);
}
