//! The session lifecycle: sign in, refresh, sign out, and the ways a token is
//! refused (issue #59).
//!
//! Sprint 3 recorded these flows as hand-verified. Four of them had no
//! automated coverage of any kind — `/api/v1/auth/refresh` had none at all,
//! and that is the endpoint deciding how long a stolen credential stays useful.
//! What follows drives each of them through the router.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use kelir_backend::modules::auth::token::hash_refresh_token;
use serde_json::{json, Value};
use sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;
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

/// A user who may read and create users, so a token of theirs has one read
/// route and one write route to be tried on. Returns the user's id.
async fn user_administering_users(app: &TestApp, username: &str) -> Uuid {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase().replace('.', "-")),
        &["identity:user:read", "identity:user:create"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@kelir.test"),
        PASSWORD,
        &[role],
    )
    .await
}

#[tokio::test]
async fn a_deleted_users_access_token_and_refresh_token_are_both_refused_at_once() {
    // The user list's Deactivate button is this route: `DELETE`, a soft
    // delete. It is decision D-105's case and not D-104's, so the account is
    // signed out at once (#650): the access token is refused on its next
    // request, and the refresh token cannot start the session again.
    //
    // Until D-105 this test was `a_deactivated_users_refresh_token_is_
    // rejected_immediately` and asserted the access token still got 200.
    let app = TestApp::spawn().await;
    let id = user_administering_users(&app, "soon.deleted").await;
    let (access, refresh) = session_for(&app, "soon.deleted").await;

    // Control: the token reads while its user is there.
    let before = app.get("/api/v1/identity/users", Some(&access)).await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.body);

    let admin = app.administrator_token().await;
    let deleted = app
        .delete(&format!("/api/v1/identity/users/{id}"), Some(&admin))
        .await;

    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let listed = app.get("/api/v1/identity/users", Some(&access)).await;
    assert_eq!(
        listed.status,
        StatusCode::UNAUTHORIZED,
        "D-105: a deleted user's access token is refused at once: {}",
        listed.body
    );
    assert_eq!(listed.error_code(), Some("UNAUTHORIZED"));

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
async fn by_decision_d_104_a_deactivated_users_access_token_works_until_it_expires() {
    // **Intended, and bounded** (D-104, answered A by the product owner on
    // 2026-10-01): an access token outlives the deactivation of its user by at
    // most its lifetime, `ACCESS_TOKEN_TTL_MINUTES` from issue: 15 minutes,
    // with no leeway. Sign-in and refresh are refused at once; for the token
    // already issued, `middleware::auth` checks signature and expiry, and
    // that the user and the tenant are not *deleted* (D-105). It does not
    // read `status`.
    //
    // Do not "fix" this test. A change that makes the middleware read the
    // user's status turns it red, and that is it working: such a change
    // reverses D-104 and needs the decision reopened, not these assertions
    // edited.
    //
    // The test above removes the account by `DELETE`, which is D-105's case
    // and is refused at once; this one sets `status`, the other way an
    // account is taken out of use, and it tries a write as well.
    use kelir_backend::modules::auth::token::ACCESS_TOKEN_TTL_MINUTES;

    let app = TestApp::spawn().await;
    let id = user_administering_users(&app, "status.inactive").await;
    let (access, refresh) = session_for(&app, "status.inactive").await;

    let admin = app.administrator_token().await;
    let deactivated = app
        .put(
            &format!("/api/v1/identity/users/{id}"),
            Some(&admin),
            json!({ "status": "INACTIVE" }),
        )
        .await;
    assert_eq!(deactivated.status, StatusCode::OK, "{}", deactivated.body);
    assert_eq!(deactivated.data()["status"], "INACTIVE");

    // Inside the window: it reads...
    let listed = app.get("/api/v1/identity/users", Some(&access)).await;
    assert_eq!(
        listed.status,
        StatusCode::OK,
        "D-104 (A): a deactivated user's access token is good until it expires: {}",
        listed.body
    );

    // ...and writes. Not merely answered 201: the administrator sees the row.
    let written = app
        .post(
            "/api/v1/identity/users",
            Some(&access),
            json!({
                "username": "made.afterwards",
                "email": "made.afterwards@kelir.test",
                "password": PASSWORD,
                "displayName": "Made Afterwards",
            }),
        )
        .await;
    assert_eq!(
        written.status,
        StatusCode::CREATED,
        "D-104 (A): the limit covers writes as well as reads: {}",
        written.body
    );
    let made = written.data()["id"]
        .as_str()
        .expect("a created user has an id");
    let seen = app
        .get(&format!("/api/v1/identity/users/{made}"), Some(&admin))
        .await;
    assert_eq!(seen.status, StatusCode::OK, "{}", seen.body);
    assert_eq!(seen.data()["username"], "made.afterwards");

    // The bound. Nothing starts the session again or extends it...
    let signed_in = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "status.inactive", "password": PASSWORD }),
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::UNAUTHORIZED);

    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;
    assert_eq!(rotated.status, StatusCode::UNAUTHORIZED);

    // ...and the token itself ends. Its own claims under the same signature,
    // as they will be one second after its lifetime: `verify_access_token`
    // allows no leeway on `exp`.
    let past_expiry = ACCESS_TOKEN_TTL_MINUTES * 60 + 1;

    // Control: re-signed with its expiry untouched it is still accepted, so
    // what refuses the aged one is its age and not that this test minted it.
    let live = app
        .get("/api/v1/identity/users", Some(&aged(&access, 0)))
        .await;
    assert_eq!(live.status, StatusCode::OK, "{}", live.body);

    let expired = app
        .get("/api/v1/identity/users", Some(&aged(&access, past_expiry)))
        .await;
    assert_eq!(
        expired.status,
        StatusCode::UNAUTHORIZED,
        "a deactivated user's token still read after its lifetime: {}",
        expired.body
    );
}

#[tokio::test]
async fn by_decision_d_104_a_locked_users_access_token_works_until_it_expires() {
    // **Intended** (D-104; #650's criteria name this status with `INACTIVE`).
    // The per-request check D-105 added reads `deleted_at` and nothing else,
    // so a status that stops sign-in does not stop a token already issued. A
    // check that read `status` would turn this red, and that is it working.
    //
    // Both ways an account is locked: the status an administrator sets, and
    // the `locked_until` that failed sign-ins set.
    let app = TestApp::spawn().await;
    let id = user_administering_users(&app, "status.locked").await;
    let (access, refresh) = session_for(&app, "status.locked").await;

    let admin = app.administrator_token().await;
    let locked = app
        .put(
            &format!("/api/v1/identity/users/{id}"),
            Some(&admin),
            json!({ "status": "LOCKED" }),
        )
        .await;
    assert_eq!(locked.status, StatusCode::OK, "{}", locked.body);
    assert_eq!(locked.data()["status"], "LOCKED");
    sqlx::query("UPDATE users SET locked_until = now() + interval '1 hour' WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("set locked_until");

    let listed = app.get("/api/v1/identity/users", Some(&access)).await;
    assert_eq!(
        listed.status,
        StatusCode::OK,
        "D-104: a locked user's access token is good until it expires: {}",
        listed.body
    );

    let written = app
        .post(
            "/api/v1/identity/users",
            Some(&access),
            json!({
                "username": "made.while.locked",
                "email": "made.while.locked@kelir.test",
                "password": PASSWORD,
                "displayName": "Made While Locked",
            }),
        )
        .await;
    assert_eq!(written.status, StatusCode::CREATED, "{}", written.body);

    // The bound: nothing starts the session again or extends it.
    let signed_in = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "status.locked", "password": PASSWORD }),
        )
        .await;
    assert_ne!(signed_in.status, StatusCode::OK, "{}", signed_in.body);

    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;
    assert_eq!(rotated.status, StatusCode::UNAUTHORIZED, "{}", rotated.body);
}

/// The same claims under the same signature, as the token would be `seconds`
/// later: `iat` and `exp` both moved back, nothing else touched.
fn aged(access: &str, seconds: i64) -> String {
    let mut claims = jsonwebtoken::decode::<Value>(
        access,
        &jsonwebtoken::DecodingKey::from_secret(common::JWT_SECRET.as_bytes()),
        &jsonwebtoken::Validation::new(Algorithm::HS256),
    )
    .expect("the application's own token verifies under the test secret")
    .claims;

    for claim in ["iat", "exp"] {
        let issued = claims[claim]
            .as_i64()
            .unwrap_or_else(|| panic!("no {claim} in {claims}"));
        claims[claim] = json!(issued - seconds);
    }

    signed_token(&claims)
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

// ---------------------------------------------------------------------------
// #649: a refresh reads its tenant's status itself.
//
// `update_tenant` and `delete_tenant` revoke a tenant's refresh tokens after
// they change its row, in a second statement. Until #649 a refresh read only
// the user's status and relied on that revoke for the tenant's, so a status
// that changed without it left a session that renewed for up to the refresh
// token's 30 days: a revoke that failed after the row changed, a sign-in that
// inserted its token after the revoke ran, or a status set outside the API.
// Each test below sets the status by SQL, so no revoke has run, and asserts
// that none has before it refreshes.
// ---------------------------------------------------------------------------

/// Signs in to the tenant `tenant_code` names and returns the refresh token.
async fn refresh_token_in(app: &TestApp, tenant_code: &str, username: &str) -> String {
    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({
                "username": username,
                "password": PASSWORD,
                "tenantCode": tenant_code,
            }),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned()
}

async fn refreshed(app: &TestApp, refresh_token: &str) -> common::TestResponse {
    app.post(
        "/api/v1/auth/refresh",
        None,
        json!({ "refreshToken": refresh_token }),
    )
    .await
}

/// How many of the user's refresh tokens are not revoked, read past the API.
async fn live_refresh_tokens(app: &TestApp, user_id: Uuid) -> i64 {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM refresh_tokens WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .expect("reads refresh_tokens");

    count
}

/// #649 for one way a tenant leaves `ACTIVE`: `change` is an `UPDATE` of
/// `tenants` whose `$1` is the tenant's id, and `became` names the result.
async fn a_refresh_is_refused_once_its_tenant_left_by_sql(change: &str, became: &str) {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;

    let acme = fixtures::create_tenant(&app.pool, "ACME", "Acme Limited").await;
    let beta = fixtures::create_tenant(&app.pool, "BETA", "Beta Limited").await;
    let leaving = fixtures::create_user(
        &app.pool,
        acme,
        "acme.user",
        "acme.user@kelir.test",
        PASSWORD,
        &[],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        beta,
        "beta.user",
        "beta.user@kelir.test",
        PASSWORD,
        &[],
    )
    .await;

    // Two sessions for the user whose tenant leaves, and one in a tenant that
    // stays.
    let first = refresh_token_in(&app, "ACME", "acme.user").await;
    let second = refresh_token_in(&app, "ACME", "acme.user").await;
    let staying = refresh_token_in(&app, "BETA", "beta.user").await;

    // Control: while its tenant is `ACTIVE`, the token rotates (#649's third
    // criterion), so what refuses it below is the change and not the setup.
    let rotated = refreshed(&app, &first).await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let current = rotated.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();
    assert_ne!(current, first, "the control refresh did not rotate");

    let changed = sqlx::query(change)
        .bind(acme)
        .execute(&app.pool)
        .await
        .expect("the tenant's row changes");
    assert_eq!(changed.rows_affected(), 1, "{change} changed no tenant");

    // What makes this #649's case: the change revoked nothing.
    assert_eq!(
        live_refresh_tokens(&app, leaving).await,
        2,
        "setting the tenant {became} by SQL revoked a refresh token, so this \
         test no longer shows a refresh deciding for itself"
    );

    let refused = refreshed(&app, &current).await;
    assert_eq!(
        refused.status,
        StatusCode::UNAUTHORIZED,
        "a refresh renewed the session of a tenant that became {became}: {}",
        refused.body
    );
    assert_eq!(
        refused.error_code(),
        Some("UNAUTHORIZED"),
        "{}",
        refused.body
    );
    assert!(
        refused.data()["accessToken"].is_null(),
        "a refused refresh returned a token: {}",
        refused.body
    );

    // The refusal ends every session the user holds, as an inactive user's
    // does: the token never presented is revoked as well.
    assert_eq!(
        live_refresh_tokens(&app, leaving).await,
        0,
        "a refresh refused for a {became} tenant left the user's refresh tokens live"
    );
    let other_session = refreshed(&app, &second).await;
    assert_eq!(
        other_session.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        other_session.body
    );

    // Only the token's own tenant is read: another tenant's session renews.
    let other_tenant = refreshed(&app, &staying).await;
    assert_eq!(
        other_tenant.status,
        StatusCode::OK,
        "a {became} tenant stopped another tenant's refresh: {}",
        other_tenant.body
    );
}

#[tokio::test]
async fn a_refresh_is_refused_and_ends_the_users_sessions_when_its_tenant_is_suspended() {
    a_refresh_is_refused_once_its_tenant_left_by_sql(
        "UPDATE tenants SET status = 'SUSPENDED' WHERE id = $1",
        "SUSPENDED",
    )
    .await;
}

#[tokio::test]
async fn a_refresh_is_refused_and_ends_the_users_sessions_when_its_tenant_is_inactive() {
    a_refresh_is_refused_once_its_tenant_left_by_sql(
        "UPDATE tenants SET status = 'INACTIVE' WHERE id = $1",
        "INACTIVE",
    )
    .await;
}

#[tokio::test]
async fn a_refresh_is_refused_and_ends_the_users_sessions_when_its_tenant_is_deleted() {
    // `status` stays `ACTIVE`: a deleted tenant is refused for its
    // `deleted_at`, which the tenant lookup reads as no tenant at all.
    a_refresh_is_refused_once_its_tenant_left_by_sql(
        "UPDATE tenants SET deleted_at = now() WHERE id = $1",
        "deleted",
    )
    .await;
}

#[tokio::test]
async fn a_refresh_is_refused_when_its_tenant_is_both_deleted_and_suspended() {
    // The two ways out at once: neither hides the other.
    a_refresh_is_refused_once_its_tenant_left_by_sql(
        "UPDATE tenants SET deleted_at = now(), status = 'SUSPENDED' WHERE id = $1",
        "deleted and suspended",
    )
    .await;
}

/// Signs in to `tenant_code` and returns `(access token, refresh token)`.
async fn session_in(app: &TestApp, tenant_code: &str, username: &str) -> (String, String) {
    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": username, "password": PASSWORD, "tenantCode": tenant_code }),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let token = |name: &str| {
        response.data()[name]
            .as_str()
            .unwrap_or_else(|| panic!("{name} is a string"))
            .to_owned()
    };
    (token("accessToken"), token("refreshToken"))
}

/// Runs `statement` against the test's private database.
async fn execute(app: &TestApp, statement: &str) {
    sqlx::query(statement)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

/// #649: a refresh that cannot read its tenant is an error, never a refusal.
///
/// An outage must not look like a signed-out session, and must not end one:
/// `admits_session` answers the read's failure as an error, and the refresh
/// revokes nothing. Only the tenant read is made to fail. `find_user` reads
/// `users` alone, so the user check before it still passes.
///
/// Seen red (2026-10-10), and by no test of the builder's: the failed read
/// answered as a refusal, in `admits_session` and again in `refresh`.
#[tokio::test]
async fn a_refresh_that_cannot_read_its_tenant_is_an_error_and_ends_no_session() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "tenant.unread").await;
    let (_, refresh_token) = session_for(&app, "tenant.unread").await;

    execute(
        &app,
        "ALTER TABLE tenants RENAME COLUMN status TO status_away",
    )
    .await;

    let failed = refreshed(&app, &refresh_token).await;
    assert_eq!(
        failed.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a tenant read that failed was answered as a refusal: {}",
        failed.body
    );
    assert_eq!(failed.error_code(), Some("INTERNAL_ERROR"));
    assert_eq!(
        live_refresh_tokens(&app, user_id).await,
        1,
        "a tenant read that failed ended the user's session"
    );

    // With the column back, the same token renews: nothing was revoked.
    execute(
        &app,
        "ALTER TABLE tenants RENAME COLUMN status_away TO status",
    )
    .await;
    let renewed = refreshed(&app, &refresh_token).await;
    assert_eq!(renewed.status, StatusCode::OK, "{}", renewed.body);
}

/// #649: a tenant no row has at all is refused, as a deleted one is.
///
/// The foreign keys keep this out of a real database, so the test lifts them
/// in its own and moves the user and their token to an id no tenant has.
/// Without that, `find_user` would refuse first and the tenant check would
/// never be reached.
#[tokio::test]
async fn a_refresh_is_refused_and_ends_the_users_sessions_when_its_tenant_has_no_row() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let acme = fixtures::create_tenant(&app.pool, "ACME", "Acme Limited").await;
    let orphan = fixtures::create_user(
        &app.pool,
        acme,
        "acme.orphan",
        "acme.orphan@kelir.test",
        PASSWORD,
        &[],
    )
    .await;
    let token = refresh_token_in(&app, "ACME", "acme.orphan").await;

    execute(
        &app,
        "DO $$ DECLARE fk record; BEGIN
           FOR fk IN SELECT conrelid::regclass AS tbl, conname FROM pg_constraint
                     WHERE contype = 'f' AND confrelid = 'tenants'::regclass
                       AND conrelid IN ('users'::regclass, 'refresh_tokens'::regclass)
           LOOP EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', fk.tbl, fk.conname);
           END LOOP;
         END $$",
    )
    .await;
    let nowhere = Uuid::now_v7();
    for table in ["users", "refresh_tokens"] {
        let column = if table == "users" { "id" } else { "user_id" };
        sqlx::query(&format!(
            "UPDATE {table} SET tenant_id = $1 WHERE {column} = $2"
        ))
        .bind(nowhere)
        .bind(orphan)
        .execute(&app.pool)
        .await
        .expect("moves the row to no tenant");
    }

    let refused = refreshed(&app, &token).await;
    assert_eq!(
        refused.status,
        StatusCode::UNAUTHORIZED,
        "a refresh renewed a session whose tenant has no row: {}",
        refused.body
    );
    assert_eq!(live_refresh_tokens(&app, orphan).await, 0);
}

/// #649: a refusal for the tenant reads exactly as one for the user.
///
/// A caller holding a refresh token learns nothing about which of the two
/// stopped it, so the refresh is not a way to read a tenant's status. When
/// both refuse, the user check runs first, and only the revocation's reason
/// in `refresh_tokens` records which one did.
#[tokio::test]
async fn a_refresh_refused_for_its_tenant_reads_as_one_refused_for_its_user() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;

    let mut refusals = Vec::new();
    for (code, tenant_leaves, user_leaves, reason) in [
        ("ACME", true, false, "tenant not active"),
        ("BETA", false, true, "account not active"),
        ("GAMA", true, true, "account not active"),
    ] {
        let tenant = fixtures::create_tenant(&app.pool, code, "Some Limited").await;
        let username = format!("{}.user", code.to_lowercase());
        let user_id = fixtures::create_user(
            &app.pool,
            tenant,
            &username,
            &format!("{username}@kelir.test"),
            PASSWORD,
            &[],
        )
        .await;
        let token = refresh_token_in(&app, code, &username).await;

        if tenant_leaves {
            sqlx::query("UPDATE tenants SET status = 'SUSPENDED' WHERE id = $1")
                .bind(tenant)
                .execute(&app.pool)
                .await
                .expect("suspends the tenant");
        }
        if user_leaves {
            sqlx::query("UPDATE users SET status = 'INACTIVE' WHERE id = $1")
                .bind(user_id)
                .execute(&app.pool)
                .await
                .expect("deactivates the user");
        }

        let refused = refreshed(&app, &token).await;
        assert_eq!(
            refused.status,
            StatusCode::UNAUTHORIZED,
            "{code}: {}",
            refused.body
        );
        refusals.push(refused.body);

        let reasons: Vec<Option<String>> =
            sqlx::query_scalar("SELECT revoked_reason FROM refresh_tokens WHERE user_id = $1")
                .bind(user_id)
                .fetch_all(&app.pool)
                .await
                .expect("reads refresh_tokens");
        assert_eq!(reasons, vec![Some(reason.to_owned())], "{code}");
    }

    assert_eq!(
        refusals[0], refusals[1],
        "a tenant's refusal reads apart from a user's"
    );
    assert_eq!(refusals[1], refusals[2]);
}

/// #649: a session the refresh ended stays ended, and D-104 is untouched.
///
/// The refusal revokes the user's refresh tokens; it does not pause them. So
/// presenting the token again is refused, and still is once the tenant is
/// `ACTIVE` again. The access token issued before the refusal is not reached:
/// it works until it expires, as D-104 states, and #649 does not change that.
#[tokio::test]
async fn a_session_a_refresh_ended_for_its_tenant_stays_ended_when_the_tenant_returns() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let acme = fixtures::create_tenant(&app.pool, "ACME", "Acme Limited").await;
    let user_id = fixtures::create_user(
        &app.pool,
        acme,
        "acme.user",
        "acme.user@kelir.test",
        PASSWORD,
        &[],
    )
    .await;
    let (access, refresh_token) = session_in(&app, "ACME", "acme.user").await;

    let pool = &app.pool;
    let set_status = move |status: &'static str| {
        sqlx::query("UPDATE tenants SET status = $2 WHERE id = $1")
            .bind(acme)
            .bind(status)
            .execute(pool)
    };
    set_status("SUSPENDED").await.expect("suspends the tenant");

    let refused = refreshed(&app, &refresh_token).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);

    // D-104, as it stood before #649.
    let me = app.get("/api/v1/auth/me", Some(&access)).await;
    assert_eq!(
        me.status,
        StatusCode::OK,
        "a refused refresh reached the access token D-104 lets live: {}",
        me.body
    );

    let again = refreshed(&app, &refresh_token).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED, "{}", again.body);

    set_status("ACTIVE").await.expect("reactivates the tenant");
    let revived = refreshed(&app, &refresh_token).await;
    assert_eq!(
        revived.status,
        StatusCode::UNAUTHORIZED,
        "reactivating the tenant revived a session its refresh had ended: {}",
        revived.body
    );
    assert_eq!(live_refresh_tokens(&app, user_id).await, 0);

    // Control: the tenant is back, so a new sign-in is how back in is.
    session_in(&app, "ACME", "acme.user").await;
}

/// #649 in a single-tenant deployment: the `SYSTEM` tenant is not exempt.
#[tokio::test]
async fn a_refresh_is_refused_and_ends_the_users_sessions_when_the_system_tenant_is_suspended() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "system.user").await;
    let (_, refresh_token) = session_for(&app, "system.user").await;

    sqlx::query("UPDATE tenants SET status = 'SUSPENDED' WHERE id = $1")
        .bind(fixtures::SYSTEM_TENANT_ID)
        .execute(&app.pool)
        .await
        .expect("suspends the system tenant");

    let refused = refreshed(&app, &refresh_token).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(live_refresh_tokens(&app, user_id).await, 0);
}

/// #649: a status this code does not know refuses, as `from_db` promises.
///
/// The `CHECK` keeps such a value out today. A later migration that adds a
/// status, run ahead of the code that knows it, is the case: the test drops
/// the `CHECK` in its own database and writes a status no variant names.
#[tokio::test]
async fn a_refresh_is_refused_when_its_tenants_status_is_one_the_code_does_not_know() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "unknown.status").await;
    let (_, refresh_token) = session_for(&app, "unknown.status").await;

    execute(
        &app,
        "DO $$ DECLARE c record; BEGIN
           FOR c IN SELECT conname FROM pg_constraint
                    WHERE contype = 'c' AND conrelid = 'tenants'::regclass
                      AND pg_get_constraintdef(oid) LIKE '%status%'
           LOOP EXECUTE format('ALTER TABLE tenants DROP CONSTRAINT %I', c.conname);
           END LOOP;
         END $$",
    )
    .await;
    sqlx::query("UPDATE tenants SET status = 'ARCHIVED' WHERE id = $1")
        .bind(fixtures::SYSTEM_TENANT_ID)
        .execute(&app.pool)
        .await
        .expect("writes an unknown status");

    let refused = refreshed(&app, &refresh_token).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(live_refresh_tokens(&app, user_id).await, 0);
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

// ---------------------------------------------------------------------------
// #710: a refresh after an administrative revoke is not token reuse.
//
// Until #710 a revoked refresh token took the reuse branch whatever had
// revoked it: the refresh wrote `Security.RefreshTokenReused` and ended every
// session the user held. Rotation, sign-out and the reuse sweep revoke a token
// its client should no longer hold, so presenting it again is evidence that a
// copy exists. A password change, a suspension or a deactivation revokes a
// token its client still holds in good faith, so presenting it is not. The
// stored `revoked_reason` decides which (SDD §11.1). Each administrative case
// below revokes through the route an administrator uses, not by SQL.
// ---------------------------------------------------------------------------

const TENANTS: &str = "/api/v1/organization/tenants";
const NEW_PASSWORD: &str = "a different long password";

/// How many `TOKEN_REUSE` rows name `user_id`.
async fn reuse_rows(app: &TestApp, user_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events \
         WHERE event_type = 'Security.RefreshTokenReused' AND object_id = $1",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .expect("counts the reuse rows")
}

/// An audit row's action, object, actor and reason.
type AuditFields = (String, Uuid, Option<Uuid>, Option<String>);

/// The `TOKEN_REUSE` rows that name `user_id`, oldest first.
async fn reuse_records(app: &TestApp, user_id: Uuid) -> Vec<AuditFields> {
    sqlx::query_as(
        "SELECT action, object_id, actor_user_id, reason FROM audit_events \
         WHERE event_type = 'Security.RefreshTokenReused' AND object_id = $1 \
         ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(&app.pool)
    .await
    .expect("reads the reuse rows")
}

/// The record a replay writes about `user_id`: the user is both the object
/// and the actor, since the token was theirs.
fn replay_record(user_id: Uuid) -> AuditFields {
    (
        "TOKEN_REUSE".to_owned(),
        user_id,
        Some(user_id),
        Some("revoked refresh token presented".to_owned()),
    )
}

/// How many audit rows of any kind name `user_id`.
async fn audit_rows_about(app: &TestApp, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE object_id = $1")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .expect("counts the audit rows")
}

/// Every refresh token's revocation, read past the API: id, when, why.
async fn revocations(app: &TestApp) -> Vec<(Uuid, Option<String>, Option<String>)> {
    sqlx::query_as("SELECT id, revoked_at::text, revoked_reason FROM refresh_tokens ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .expect("reads refresh_tokens")
}

/// Why each of the user's refresh tokens was revoked, oldest first.
async fn revoked_reasons(app: &TestApp, user_id: Uuid) -> Vec<Option<String>> {
    sqlx::query_scalar(
        "SELECT revoked_reason FROM refresh_tokens WHERE user_id = $1 ORDER BY issued_at, id",
    )
    .bind(user_id)
    .fetch_all(&app.pool)
    .await
    .expect("reads refresh_tokens")
}

/// The refusal of a replayed rotated token: rotates `refresh_token`, presents
/// it again, and returns the second answer's body. Every other refusal of a
/// revoked token must read exactly as this one does.
async fn replay_refusal(app: &TestApp, refresh_token: &str) -> Value {
    let rotated = refreshed(app, refresh_token).await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);

    let replayed = refreshed(app, refresh_token).await;
    assert_eq!(
        replayed.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        replayed.body
    );
    replayed.body
}

/// A tenant `code` with one user in it: `(tenant, user, username)`.
async fn tenant_user(app: &TestApp, code: &str) -> (Uuid, Uuid, String) {
    let tenant = fixtures::create_tenant(&app.pool, code, &format!("{code} Limited")).await;
    let username = format!("{}.user", code.to_lowercase());
    let user_id = fixtures::create_user(
        &app.pool,
        tenant,
        &username,
        &format!("{username}@kelir.test"),
        PASSWORD,
        &[],
    )
    .await;

    (tenant, user_id, username)
}

/// Presents `token`, which `case` revoked administratively, and says what was
/// wrong with the answer: anything but a bare 401, a `TOKEN_REUSE` row, any
/// audit row about the user, or any token's revocation changed (#710's AC1
/// and AC6). Returns the body with the problems, so a table reports each case
/// rather than stopping at the first.
async fn quiet_refusal(
    app: &TestApp,
    user_id: Uuid,
    token: &str,
    case: &str,
) -> (Value, Vec<String>) {
    let audited = audit_rows_about(app, user_id).await;
    let before = revocations(app).await;

    let refused = refreshed(app, token).await;

    let mut problems = Vec::new();
    if refused.status != StatusCode::UNAUTHORIZED
        || refused.error_code() != Some("UNAUTHORIZED")
        || !refused.data()["accessToken"].is_null()
    {
        problems.push(format!(
            "{case}: answered {} {}",
            refused.status, refused.body
        ));
    }
    let reused = reuse_rows(app, user_id).await;
    if reused != 0 {
        problems.push(format!(
            "{case}: recorded as token reuse ({reused} TOKEN_REUSE row)"
        ));
    }
    let audited_after = audit_rows_about(app, user_id).await;
    if audited_after != audited {
        problems.push(format!(
            "{case}: the refusal wrote {} audit row(s) about the user",
            audited_after - audited
        ));
    }
    if revocations(app).await != before {
        problems.push(format!("{case}: the refusal revoked a refresh token"));
    }

    (refused.body, problems)
}

#[tokio::test]
async fn a_refresh_after_an_administrative_revoke_is_refused_and_is_not_token_reuse() {
    let mut problems = Vec::new();
    let mut bodies: Vec<(&str, Value)> = Vec::new();

    // The tenant's revokes and #708's refusals, on a multi-tenant deployment.
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let admin = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;

    let (_, _, username) = tenant_user(&app, "ECHO").await;
    let token = refresh_token_in(&app, "ECHO", &username).await;
    let reference = replay_refusal(&app, &token).await;

    {
        let (tenant, user_id, username) = tenant_user(&app, "ACME").await;
        let token = refresh_token_in(&app, "ACME", &username).await;
        let suspended = app
            .put(
                &format!("{TENANTS}/{tenant}"),
                Some(&admin),
                json!({ "status": "SUSPENDED" }),
            )
            .await;
        assert_eq!(suspended.status, StatusCode::OK, "{}", suspended.body);
        assert_eq!(
            revoked_reasons(&app, user_id).await,
            [Some("tenant suspended".to_owned())]
        );

        let (body, found) = quiet_refusal(&app, user_id, &token, "tenant suspended").await;
        bodies.push(("tenant suspended", body));
        problems.extend(found);
    }

    {
        let (tenant, user_id, username) = tenant_user(&app, "BETA").await;
        let token = refresh_token_in(&app, "BETA", &username).await;
        let deleted = app
            .delete(&format!("{TENANTS}/{tenant}"), Some(&admin))
            .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
        assert_eq!(
            revoked_reasons(&app, user_id).await,
            [Some("tenant deleted".to_owned())]
        );

        let (body, found) = quiet_refusal(&app, user_id, &token, "tenant deleted").await;
        bodies.push(("tenant deleted", body));
        problems.extend(found);
    }

    // #708's own refusals revoke, and the retry presents what they revoked,
    // as `a_refresh_refused_for_its_tenant_reads_as_one_refused_for_its_user`
    // does.
    for (code, statement, reason) in [
        (
            "GAMA",
            "UPDATE tenants SET status = 'SUSPENDED' WHERE id = $1",
            "tenant not active",
        ),
        (
            "DLTA",
            "UPDATE users SET status = 'INACTIVE' WHERE tenant_id = $1",
            "account not active",
        ),
    ] {
        let (tenant, user_id, username) = tenant_user(&app, code).await;
        let token = refresh_token_in(&app, code, &username).await;
        sqlx::query(statement)
            .bind(tenant)
            .execute(&app.pool)
            .await
            .expect("takes the session out of use");

        let first = refreshed(&app, &token).await;
        assert_eq!(first.status, StatusCode::UNAUTHORIZED, "{}", first.body);
        assert_eq!(
            revoked_reasons(&app, user_id).await,
            [Some(reason.to_owned())]
        );

        let (body, found) = quiet_refusal(&app, user_id, &token, reason).await;
        bodies.push((reason, body));
        problems.extend(found);
    }

    // The account's revokes, on a single-tenant deployment: a reset link
    // needs no tenant code there.
    let single = TestApp::spawn().await;
    let admin = single.administrator_token().await;

    user(&single, "replay.reference").await;
    let (_, token) = session_for(&single, "replay.reference").await;
    let single_reference = replay_refusal(&single, &token).await;

    {
        // First, so the reset link is the only message delivered.
        let user_id = user(&single, "reset.user").await;
        let (_, token) = session_for(&single, "reset.user").await;
        let asked = single
            .post(
                "/api/v1/auth/forgot-password",
                None,
                json!({ "username": "reset.user" }),
            )
            .await;
        assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
        let link = reset_token_from_mail(&single).await;
        let reset = single
            .post(
                "/api/v1/auth/reset-password",
                None,
                json!({ "token": link, "newPassword": NEW_PASSWORD }),
            )
            .await;
        assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);
        assert_eq!(
            revoked_reasons(&single, user_id).await,
            [Some("password reset".to_owned())]
        );

        let (body, found) = quiet_refusal(&single, user_id, &token, "password reset").await;
        bodies.push(("password reset", body));
        problems.extend(found);
    }

    {
        let user_id = user(&single, "deactivated.user").await;
        let (_, token) = session_for(&single, "deactivated.user").await;
        let deactivated = single
            .put(
                &format!("/api/v1/identity/users/{user_id}"),
                Some(&admin),
                json!({ "status": "INACTIVE" }),
            )
            .await;
        assert_eq!(deactivated.status, StatusCode::OK, "{}", deactivated.body);
        assert_eq!(
            revoked_reasons(&single, user_id).await,
            [Some("account deactivated".to_owned())]
        );

        let (body, found) = quiet_refusal(&single, user_id, &token, "account deactivated").await;
        bodies.push(("account deactivated", body));
        problems.extend(found);
    }

    {
        let user_id = user(&single, "deleted.user").await;
        let (_, token) = session_for(&single, "deleted.user").await;
        let deleted = single
            .delete(&format!("/api/v1/identity/users/{user_id}"), Some(&admin))
            .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
        assert_eq!(
            revoked_reasons(&single, user_id).await,
            [Some("account deleted".to_owned())]
        );

        let (body, found) = quiet_refusal(&single, user_id, &token, "account deleted").await;
        bodies.push(("account deleted", body));
        problems.extend(found);
    }

    {
        let user_id = user(&single, "admin.set.user").await;
        let (_, token) = session_for(&single, "admin.set.user").await;
        let set = single
            .post(
                &format!("/api/v1/identity/users/{user_id}/password"),
                Some(&admin),
                json!({ "password": NEW_PASSWORD }),
            )
            .await;
        assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);
        assert_eq!(
            revoked_reasons(&single, user_id).await,
            [Some("password changed".to_owned())]
        );

        let (body, found) = quiet_refusal(
            &single,
            user_id,
            &token,
            "password changed by an administrator",
        )
        .await;
        bodies.push(("password changed by an administrator", body));
        problems.extend(found);
    }

    {
        let user_id = user(&single, "self.changed.user").await;
        let (access, token) = session_for(&single, "self.changed.user").await;
        let changed = change_own_password(&single, &access).await;
        assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
        assert_eq!(
            revoked_reasons(&single, user_id).await,
            [Some("password changed".to_owned())]
        );

        let (body, found) =
            quiet_refusal(&single, user_id, &token, "password changed by its holder").await;
        bodies.push(("password changed by its holder", body));
        problems.extend(found);
    }

    // Byte-identical to the reuse refusal: the caller cannot tell which
    // class stopped it, so the refresh is not a way to learn why.
    if single_reference != reference {
        problems.push(format!(
            "the two deployments' reuse refusals differ: {single_reference} and {reference}"
        ));
    }
    for (case, body) in &bodies {
        if *body != reference {
            problems.push(format!(
                "{case}: the refusal reads apart from the reuse refusal: {body} and {reference}"
            ));
        }
    }

    assert_eq!(bodies.len(), 9, "every case ran");
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

/// The reset token out of the one message delivered so far.
async fn reset_token_from_mail(app: &TestApp) -> String {
    let sent = app.mail_delivered(1).await;
    let last = sent.last().expect("a message was delivered");

    let marker = "token=";
    let start = last.body.find(marker).expect("the body carries a link") + marker.len();
    last.body[start..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect()
}

/// `POST /auth/change-password` from `PASSWORD` to `NEW_PASSWORD`.
async fn change_own_password(app: &TestApp, access: &str) -> common::TestResponse {
    app.post(
        "/api/v1/auth/change-password",
        Some(access),
        json!({ "currentPassword": PASSWORD, "newPassword": NEW_PASSWORD }),
    )
    .await
}

/// The refresh token a single-tenant sign-in with `password` returns.
async fn signed_in_with(app: &TestApp, username: &str, password: &str) -> String {
    let response = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": username, "password": password }),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned()
}

/// #710's AC2: the revoke was for a moment, and the session the user signed
/// in to after it is not the one revoked. The stale token's refusal ends
/// only itself.
#[tokio::test]
async fn a_stale_token_revoked_for_a_password_change_does_not_end_a_later_session() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "changed.then.back").await;
    let (access, stale) = session_for(&app, "changed.then.back").await;

    let changed = change_own_password(&app, &access).await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    let fresh = signed_in_with(&app, "changed.then.back", NEW_PASSWORD).await;

    let refused = refreshed(&app, &stale).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(
        live_refresh_tokens(&app, user_id).await,
        1,
        "the stale token's refusal ended the session signed in after the change"
    );

    let renewed = refreshed(&app, &fresh).await;
    assert_eq!(renewed.status, StatusCode::OK, "{}", renewed.body);
}

#[tokio::test]
async fn a_stale_token_revoked_for_a_suspension_does_not_end_a_session_after_reactivation() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let admin = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, user_id, username) = tenant_user(&app, "ACME").await;
    let stale = refresh_token_in(&app, "ACME", &username).await;

    for status in ["SUSPENDED", "ACTIVE"] {
        let set = app
            .put(
                &format!("{TENANTS}/{tenant}"),
                Some(&admin),
                json!({ "status": status }),
            )
            .await;
        assert_eq!(set.status, StatusCode::OK, "{status}: {}", set.body);
    }
    let fresh = refresh_token_in(&app, "ACME", &username).await;

    let refused = refreshed(&app, &stale).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(
        live_refresh_tokens(&app, user_id).await,
        1,
        "the stale token's refusal ended the session signed in after the reactivation"
    );

    let renewed = refreshed(&app, &fresh).await;
    assert_eq!(renewed.status, StatusCode::OK, "{}", renewed.body);
}

/// #710's AC3, a pin: a replayed rotated token is the evidence the reuse
/// branch exists for. Until #710 no test read the row it writes.
#[tokio::test]
async fn a_replayed_rotated_token_is_recorded_as_token_reuse_and_ends_the_family() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "replayed.user").await;
    let (_, first) = session_for(&app, "replayed.user").await;

    let rotated = refreshed(&app, &first).await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let second = rotated.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string")
        .to_owned();

    let replayed = refreshed(&app, &first).await;
    assert_eq!(
        replayed.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        replayed.body
    );

    assert_eq!(reuse_records(&app, user_id).await, [replay_record(user_id)]);
    assert_eq!(live_refresh_tokens(&app, user_id).await, 0);

    let after = refreshed(&app, &second).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED, "{}", after.body);
}

/// #710's AC4: a reason the code does not recognise, or none at all, is
/// replay evidence. A missed alarm is the worse error.
#[tokio::test]
async fn a_revoked_token_whose_reason_is_unknown_or_missing_is_token_reuse() {
    let app = TestApp::spawn().await;

    for (username, reason) in [("no.reason", None), ("odd.reason", Some("x"))] {
        let user_id = user(&app, username).await;
        let (_, token) = session_for(&app, username).await;
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = now(), revoked_reason = $2 \
             WHERE token_hash = $1",
        )
        .bind(hash_refresh_token(&token))
        .bind(reason)
        .execute(&app.pool)
        .await
        .expect("revokes the token for the reason");

        let refused = refreshed(&app, &token).await;
        assert_eq!(
            refused.status,
            StatusCode::UNAUTHORIZED,
            "{reason:?}: {}",
            refused.body
        );
        assert_eq!(
            reuse_records(&app, user_id).await,
            [replay_record(user_id)],
            "{reason:?}"
        );
    }
}

/// #710's AC5, a pin: a signed-out token presented again means a copy
/// outlived the sign-out.
#[tokio::test]
async fn a_signed_out_token_presented_again_is_token_reuse() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "signed.out.again").await;
    let (access, refresh) = session_for(&app, "signed.out.again").await;

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

    let refused = refreshed(&app, &refresh).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(reuse_records(&app, user_id).await, [replay_record(user_id)]);
}

/// #710's AC5, a pin: a sibling the reuse sweep ended is evidence too. The
/// sweep ended it because a copy of the family was loose.
#[tokio::test]
async fn a_sibling_the_reuse_sweep_ended_is_token_reuse_when_presented() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "swept.sibling").await;
    let (_, replayed) = session_for(&app, "swept.sibling").await;
    let (_, sibling) = session_for(&app, "swept.sibling").await;

    replay_refusal(&app, &replayed).await;
    assert_eq!(reuse_rows(&app, user_id).await, 1);

    let refused = refreshed(&app, &sibling).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);
    assert_eq!(
        reuse_records(&app, user_id).await,
        [replay_record(user_id), replay_record(user_id)]
    );
}

// ---------------------------------------------------------------------------
// #751: two concurrent refreshes of one token both renewed.
//
// The refresh read the token on the pool and rotated it later, discarding the
// rotation's rows-affected count, so a refresh that lost the row to another
// rotation, or to an administrator's revoke, still issued a session. The
// rotation's `UPDATE ... WHERE revoked_at IS NULL` is now the decision, and
// the rotation and the new token's insert are one transaction.
//
// **Staged with a row lock**: no trigger, no DDL. A connection of the test's
// own holds `FOR UPDATE` on the token's row; a refresh's plain read takes no
// lock, so it reads the token live and then waits in its rotation `UPDATE`.
// The test sees it waiting in `pg_stat_activity` and only then releases.
// ---------------------------------------------------------------------------

/// How long a raced refresh is given to be seen waiting, or to answer.
const PATIENCE: Duration = Duration::from_secs(30);

/// A transaction on a connection of the test's own, holding `FOR UPDATE` on
/// one refresh token's row.
struct HeldRow {
    connection: PgConnection,
    token_hash: String,
}

impl HeldRow {
    async fn take(app: &TestApp, token: &str) -> Self {
        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the lock's connection");
        let token_hash = hash_refresh_token(token);

        sqlx::query("BEGIN")
            .execute(&mut connection)
            .await
            .expect("begins the lock's transaction");
        sqlx::query("SELECT id FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE")
            .bind(&token_hash)
            .fetch_one(&mut connection)
            .await
            .expect("locks the token's row");

        HeldRow {
            connection,
            token_hash,
        }
    }

    /// Revokes the row inside the held transaction, as an administrator's
    /// revoke does, so it commits ahead of the refresh waiting on it.
    async fn revoke(&mut self, reason: &str) {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at = now(), revoked_reason = $2 \
             WHERE token_hash = $1",
        )
        .bind(&self.token_hash)
        .bind(reason)
        .execute(&mut self.connection)
        .await
        .expect("revokes the held row");
    }

    /// Ends the transaction with `COMMIT` or `ROLLBACK`.
    async fn release(mut self, how: &str) {
        sqlx::query(how)
            .execute(&mut self.connection)
            .await
            .expect("releases the row");
        self.connection
            .close()
            .await
            .expect("closes the lock's connection");
    }
}

/// A refresh of `token` sent in the background.
fn refresh_in_background(app: &Arc<TestApp>, token: &str) -> JoinHandle<common::TestResponse> {
    let app = Arc::clone(app);
    let token = token.to_owned();
    tokio::spawn(async move { refreshed(&app, &token).await })
}

/// Returns once `count` statements wait on a lock in an `UPDATE` of
/// `refresh_tokens`. A refresh that answers first is a failure: it did not
/// wait for the row.
async fn rotations_waiting(
    app: &TestApp,
    count: i64,
    running: &[&JoinHandle<common::TestResponse>],
) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'
               AND query ~ '^\\s*UPDATE refresh_tokens'",
        )
        .fetch_one(&app.pool)
        .await
        .expect("reads pg_stat_activity");
        if waiting >= count {
            return;
        }
        assert!(
            running.iter().all(|handle| !handle.is_finished()),
            "a refresh answered without waiting for the row"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "{waiting} of {count} refreshes were seen waiting"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `running`'s answer, failing rather than hanging when it never comes.
async fn answer(running: JoinHandle<common::TestResponse>) -> common::TestResponse {
    tokio::time::timeout(PATIENCE, running)
        .await
        .expect("a raced refresh never answered")
        .expect("a raced refresh panicked")
}

/// #751's AC1 and AC2: of two refreshes that both read the token live, one
/// renews; the other lost the row to a rotation, so it is a replay, and the
/// sweep it runs ends the winner's new token too.
#[tokio::test]
async fn two_refreshes_of_one_token_at_once_renew_at_most_one() {
    let app = Arc::new(TestApp::spawn().await);
    user(&app, "replay.reference").await;
    let (_, token) = session_for(&app, "replay.reference").await;
    let reference = replay_refusal(&app, &token).await;

    let user_id = user(&app, "racing.user").await;
    let (_, token) = session_for(&app, "racing.user").await;

    let held = HeldRow::take(&app, &token).await;
    let first = refresh_in_background(&app, &token);
    let second = refresh_in_background(&app, &token);
    rotations_waiting(&app, 2, &[&first, &second]).await;
    held.release("ROLLBACK").await;

    let answers = [answer(first).await, answer(second).await];
    let statuses: Vec<StatusCode> = answers.iter().map(|answer| answer.status).collect();
    let records = reuse_records(&app, user_id).await;
    let live = live_refresh_tokens(&app, user_id).await;
    let seen = format!(
        "statuses {statuses:?}, {} reuse row(s), {live} live",
        records.len()
    );

    assert!(
        statuses.contains(&StatusCode::OK) && statuses.contains(&StatusCode::UNAUTHORIZED),
        "one refresh renews and one is refused: {seen}"
    );
    let refused = answers
        .iter()
        .find(|answer| answer.status == StatusCode::UNAUTHORIZED)
        .expect("one refusal");
    assert_eq!(refused.body, reference, "{seen}");
    assert_eq!(records, [replay_record(user_id)], "{seen}");
    assert_eq!(
        live, 0,
        "the loser's sweep missed the winner's token: {seen}"
    );
}

/// #751's AC3: a refresh that read the token live and then lost the row to an
/// administrator's revoke is refused as that revoke's refusal, not as a
/// replay. Its reason is read after the race is lost, not from the first
/// read, where the token was live and the reason empty.
#[tokio::test]
async fn a_refresh_that_loses_to_an_administrative_revoke_is_refused_and_not_token_reuse() {
    let app = Arc::new(TestApp::spawn().await);
    user(&app, "replay.reference").await;
    let (_, token) = session_for(&app, "replay.reference").await;
    let reference = replay_refusal(&app, &token).await;

    let user_id = user(&app, "losing.user").await;
    let (_, token) = session_for(&app, "losing.user").await;

    let mut held = HeldRow::take(&app, &token).await;
    let running = refresh_in_background(&app, &token);
    rotations_waiting(&app, 1, &[&running]).await;
    held.revoke("tenant suspended").await;
    held.release("COMMIT").await;

    let refused = answer(running).await;
    let reused = reuse_rows(&app, user_id).await;
    let live = live_refresh_tokens(&app, user_id).await;
    let seen = format!(
        "answered {} {}, {reused} reuse row(s), {live} live",
        refused.status, refused.body
    );

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{seen}");
    assert!(refused.data()["accessToken"].is_null(), "{seen}");
    assert_eq!(refused.body, reference, "{seen}");
    assert_eq!(reused, 0, "{seen}");
    assert_eq!(live, 0, "{seen}");
}

/// #751's AC4, a pin: a rotation revokes the old token as `rotated` and links
/// the new one to it.
#[tokio::test]
async fn a_rotation_links_the_new_token_to_the_old_and_leaves_one_live() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "rotation.link").await;
    let (_, old) = session_for(&app, "rotation.link").await;

    let rotated = refreshed(&app, &old).await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let new = rotated.data()["refreshToken"]
        .as_str()
        .expect("refreshToken is a string");

    let (old_id, reason): (Uuid, Option<String>) =
        sqlx::query_as("SELECT id, revoked_reason FROM refresh_tokens WHERE token_hash = $1")
            .bind(hash_refresh_token(&old))
            .fetch_one(&app.pool)
            .await
            .expect("reads the old token");
    let rotated_from: Option<Uuid> =
        sqlx::query_scalar("SELECT rotated_from_id FROM refresh_tokens WHERE token_hash = $1")
            .bind(hash_refresh_token(new))
            .fetch_one(&app.pool)
            .await
            .expect("reads the new token");

    assert_eq!(reason.as_deref(), Some("rotated"));
    assert_eq!(rotated_from, Some(old_id));
    assert_eq!(live_refresh_tokens(&app, user_id).await, 1);
}

/// #751's AC5: the rotation and the new token's insert are one transaction,
/// so an insert that fails leaves the old token live rather than the user
/// holding nothing, and the client's retry is not a replay.
#[tokio::test]
async fn a_refresh_whose_new_token_cannot_be_stored_keeps_the_old_one() {
    let app = TestApp::spawn().await;
    let user_id = user(&app, "insert.fails").await;
    let (_, token) = session_for(&app, "insert.fails").await;

    execute(
        &app,
        "CREATE FUNCTION test_refuse_refresh_token() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             RAISE EXCEPTION 'injected refresh token insert failure';
         END
         $$",
    )
    .await;
    execute(
        &app,
        "CREATE TRIGGER test_refuse_refresh_token BEFORE INSERT ON refresh_tokens
         FOR EACH ROW EXECUTE FUNCTION test_refuse_refresh_token()",
    )
    .await;

    let failed = refreshed(&app, &token).await;
    let revoked: Option<String> =
        sqlx::query_scalar("SELECT revoked_at::text FROM refresh_tokens WHERE token_hash = $1")
            .bind(hash_refresh_token(&token))
            .fetch_one(&app.pool)
            .await
            .expect("reads the token");
    assert_eq!(
        failed.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        failed.body
    );
    assert_eq!(
        revoked, None,
        "a refresh that stored no new token revoked the old one"
    );
    assert_eq!(live_refresh_tokens(&app, user_id).await, 1);

    execute(
        &app,
        "DROP TRIGGER test_refuse_refresh_token ON refresh_tokens",
    )
    .await;
    execute(&app, "DROP FUNCTION test_refuse_refresh_token()").await;

    let renewed = refreshed(&app, &token).await;
    assert_eq!(renewed.status, StatusCode::OK, "{}", renewed.body);
    assert_eq!(reuse_rows(&app, user_id).await, 0);
}
