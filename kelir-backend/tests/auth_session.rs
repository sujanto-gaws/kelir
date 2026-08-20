//! The session lifecycle: sign in, refresh, sign out, and the ways a token is
//! refused (issue #59).
//!
//! Sprint 3 recorded these flows as hand-verified. Four of them had no
//! automated coverage of any kind — `/api/v1/auth/refresh` had none at all,
//! and that is the endpoint deciding how long a stolen credential stays useful.
//! What follows drives each of them through the router.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery";

/// Signs in and returns `(access token, refresh token)`.
///
/// `TestApp::sign_in` drops the refresh half, which is what most of this file
/// is about.
async fn session_for(app: &TestApp, username: &str) -> (String, String) {
    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": username, "password": PASSWORD }),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let access = response.data()["accessToken"]
        .as_str()
        .expect("accessToken is a string")
        .to_owned();
    let refresh = response.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();

    (access, refresh)
}

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

#[tokio::test]
async fn signing_in_with_a_wrong_password_is_unauthorized() {
    let app = TestApp::spawn().await;
    user(&app, "wrong.password").await;

    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "wrong.password", "password": "not the password" }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.error_code(), Some("UNAUTHORIZED"));

    // No token, by any name. A body that leaked one here would be the whole
    // authentication boundary.
    assert!(
        response.data()["accessToken"].is_null(),
        "a refused sign-in returned a token: {}",
        response.body
    );
}

#[tokio::test]
async fn signing_in_with_an_unknown_username_is_unauthorized_not_not_found() {
    // Deliberately the same answer as a wrong password: a 404 here would let an
    // unauthenticated caller enumerate accounts.
    let app = TestApp::spawn().await;

    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "nobody.here", "password": PASSWORD }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.error_code(), Some("UNAUTHORIZED"));
}

#[tokio::test]
async fn a_refresh_token_rotates_and_the_old_one_is_refused_on_replay() {
    let app = TestApp::spawn().await;
    user(&app, "rotating.user").await;

    let (_, first_refresh) = session_for(&app, "rotating.user").await;

    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": first_refresh }),
        )
        .await;

    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);

    let second_refresh = rotated.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();

    assert_ne!(
        second_refresh, first_refresh,
        "refresh must rotate; reissuing the same token makes theft undetectable"
    );

    // The replay. The first token is spent, and presenting it again is the
    // signal that either the client or an attacker holds a copy.
    let replayed = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": first_refresh }),
        )
        .await;

    assert_eq!(replayed.status, StatusCode::UNAUTHORIZED);

    // The answer to that ambiguity is to end the whole family, so the token the
    // legitimate client is holding stops working too.
    let after_replay = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": second_refresh }),
        )
        .await;

    assert_eq!(
        after_replay.status,
        StatusCode::UNAUTHORIZED,
        "a replay must end every session for the user, not only the replayed one"
    );
}

#[tokio::test]
async fn a_deactivated_users_refresh_token_is_rejected_immediately() {
    // The access token still lives out its fifteen minutes — that is the
    // documented trade of carrying permissions in the token. The refresh is the
    // half that must not survive, because it is what would extend the session
    // beyond that window.
    let app = TestApp::spawn().await;
    let id = user(&app, "soon.deactivated").await;
    let (_, refresh) = session_for(&app, "soon.deactivated").await;

    let admin = app.administrator_token().await;
    let deactivated = app
        .delete(&format!("/api/v1/identity/users/{id}"), Some(&admin))
        .await;

    assert_eq!(deactivated.status, StatusCode::NO_CONTENT);

    let response = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn signing_out_invalidates_the_refresh_token() {
    let app = TestApp::spawn().await;
    user(&app, "signing.out").await;
    let (access, refresh) = session_for(&app, "signing.out").await;

    let signed_out = app
        .post(
            "/api/v1/auth/logout",
            Some(&access),
            json!({ "refreshToken": refresh }),
        )
        .await;

    assert_eq!(
        signed_out.status,
        StatusCode::NO_CONTENT,
        "{}",
        signed_out.body
    );

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
        "sign-out must revoke the refresh token, not merely forget it client-side"
    );
}

#[tokio::test]
async fn an_expired_access_token_is_unauthorized() {
    // `verify_access_token` sets `validate_exp`, and nothing proved it was
    // enforced: every token any test had ever held was minutes old. This one is
    // signed with the real secret and correct in every way except its expiry.
    let app = TestApp::spawn().await;
    let id = user(&app, "expired.token").await;

    let expired = signed_token(&json!({
        "sub": id,
        "tenant_id": fixtures::SYSTEM_TENANT_ID,
        "username": "expired.token",
        "roles": [],
        "permissions": ["identity:user:read"],
        "exp": 1_600_000_000_i64,
        "iat": 1_599_999_000_i64,
    }));

    let response = app.get("/api/v1/identity/users", Some(&expired)).await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_signed_with_the_right_secret_and_a_live_expiry_is_accepted() {
    // The control for the test above. Without it, `an_expired_access_token_is_
    // unauthorized` would also pass if hand-minted tokens were refused for some
    // unrelated reason — and would then be proving nothing about expiry.
    let app = TestApp::spawn().await;
    let id = user(&app, "live.token").await;

    let live = signed_token(&json!({
        "sub": id,
        "tenant_id": fixtures::SYSTEM_TENANT_ID,
        "username": "live.token",
        "roles": [],
        "permissions": ["identity:user:read"],
        "exp": 4_102_444_800_i64,
        "iat": 1_750_000_000_i64,
    }));

    let response = app.get("/api/v1/identity/users", Some(&live)).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
}

#[tokio::test]
async fn a_token_with_alg_none_is_refused() {
    // The classic JWT bypass: declare the token unsigned and hope the verifier
    // agrees. `a_forged_token_is_refused` covers a wrong signature, which is a
    // different failure — this one has no signature to be wrong.
    let app = TestApp::spawn().await;

    let claims = json!({
        "sub": Uuid::now_v7(),
        "tenant_id": fixtures::SYSTEM_TENANT_ID,
        "username": "unsigned",
        "roles": ["ROLE-ADMIN"],
        "permissions": ["identity:user:read"],
        "exp": 4_102_444_800_i64,
        "iat": 1_750_000_000_i64,
    });

    let token = format!(
        "{}.{}.",
        base64url(br#"{"alg":"none","typ":"JWT"}"#),
        base64url(claims.to_string().as_bytes())
    );

    let response = app.get("/api/v1/identity/users", Some(&token)).await;

    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

/// Signs claims with the secret the test application runs on.
fn signed_token(claims: &Value) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(common::JWT_SECRET.as_bytes()),
    )
    .expect("claims sign")
}

/// Base64url without padding, as JWT requires.
///
/// Hand-rolled because no encoder is a dependency, and adding one for the
/// single `alg: none` token below would cost more than the twelve lines.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut encoded = String::new();

    for chunk in bytes.chunks(3) {
        let padded = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits =
            (u32::from(padded[0]) << 16) | (u32::from(padded[1]) << 8) | u32::from(padded[2]);

        // A 3-byte group encodes to 4 characters, a 2-byte group to 3, and a
        // 1-byte group to 2 — the rest would encode only padding, which
        // base64url omits.
        for position in 0..=chunk.len() {
            let index = (bits >> (18 - 6 * position)) & 63;
            encoded.push(char::from(ALPHABET[index as usize]));
        }
    }

    encoded
}
