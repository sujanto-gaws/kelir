//! **A deleted tenant's access token is refused at once, and a deleted user's
//! with it** ([#650](https://github.com/sujanto-gaws/kelir/issues/650),
//! decision D-105; ADR-0045), and an access token is accepted for its
//! lifetime and no longer.
//!
//! `middleware::auth::Authenticated` reads, on every request, whether the
//! tenant and the user the token names are deleted. "Deleted" is `deleted_at`,
//! never `status`: a suspended or inactive tenant's token, and an inactive or
//! locked user's, keep working until they expire (D-104), and the tests that
//! hold that are `by_decision_d_104_…` in `auth_session.rs` and
//! `organization_tenants.rs`.
//!
//! The refusal is the 401 an expired token gets, with no word of why. Every
//! route is walked in `deleted_caller_refusals.rs`; this file holds the cases
//! a walk cannot: nothing written, the byte-for-byte envelope, the pairing of
//! `sub` with `tenant_id`, a failed lookup, the routes the check does not
//! reach, and the leeway.
//!
//! # Seen red first (coding standard §2.9)
//!
//! Run 2026-10-01 against `main` at `6631cc9`, before the fix: every test
//! here but `signing_out_still_answers_204_for_a_deleted_caller` and the
//! controls inside the others was red. A deleted tenant's token read and
//! wrote (200, 201), `GET /auth/me` answered 200 for a deleted tenant and 404
//! for a deleted user, a token naming a user of another tenant was served,
//! a token aged by its lifetime plus a second was served, and a lookup that
//! could not run did not exist to fail.
//!
//! The mutation table is in `deleted_caller_refusals.rs`'s header.

mod common;

use axum::http::{header, StatusCode};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use kelir_backend::modules::auth::token::ACCESS_TOKEN_TTL_MINUTES;

const TENANTS: &str = "/api/v1/organization/tenants";
const USERS: &str = "/api/v1/identity/users";
const ME: &str = "/api/v1/auth/me";
const PASSWORD: &str = "a-sufficiently-long-password";

/// A real PDF header: the upload's type check reads the bytes.
const FILE: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Session {
    access: String,
    refresh: String,
}

async fn sign_in_attempt(
    app: &TestApp,
    code: Option<&str>,
    username: &str,
    password: &str,
) -> common::TestResponse {
    let mut body = json!({ "username": username, "password": password });
    if let Some(code) = code {
        body["tenantCode"] = json!(code);
    }

    app.post("/api/v1/auth/login", None, body).await
}

/// Both halves of a session, which `TestApp::sign_in` does not return.
async fn session(app: &TestApp, code: Option<&str>, username: &str, password: &str) -> Session {
    let response = sign_in_attempt(app, code, username, password).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    let field = |name: &str| {
        response.data()[name]
            .as_str()
            .unwrap_or_else(|| panic!("no {name} in {}", response.body))
            .to_owned()
    };

    Session {
        access: field("accessToken"),
        refresh: field("refreshToken"),
    }
}

async fn multi_tenant_app() -> TestApp {
    TestApp::spawn_with(|config| config.multi_tenant = true).await
}

/// A tenant created through the API, and the session its administrator opened
/// while it was live. Returns the tenant's id with it.
async fn tenant_session(app: &TestApp, system: &str, code: &str) -> (Uuid, Session) {
    let username = format!("{}.admin", code.to_lowercase());
    let created = app
        .post(
            TENANTS,
            Some(system),
            json!({
                "tenantCode": code,
                "name": format!("{code} Limited"),
                "administrator": {
                    "username": username,
                    "email": format!("{username}@example.test"),
                    "displayName": "Tenant Administrator",
                    "password": PASSWORD,
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = created.data()["id"]
        .as_str()
        .expect("a created tenant has an id")
        .parse()
        .expect("a tenant id is a uuid");

    (id, session(app, Some(code), &username, PASSWORD).await)
}

async fn delete_tenant(app: &TestApp, system: &str, id: Uuid) {
    let deleted = app.delete(&format!("{TENANTS}/{id}"), Some(system)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
}

/// A user of the system tenant who may read and create users: one read route
/// and one write route for their token to be tried on.
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

fn user_body(username: &str) -> Value {
    json!({
        "username": username,
        "email": format!("{username}@example.test"),
        "password": PASSWORD,
        "displayName": "Created With A Refused Token",
    })
}

/// How many users of that name exist in any tenant, deleted or not, read past
/// the API.
async fn users_named(app: &TestApp, username: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("reads users")
}

async fn rows_in(app: &TestApp, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("count {table}: {error}"))
}

fn signed(claims: &Value) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(common::JWT_SECRET.as_bytes()),
    )
    .expect("claims sign")
}

/// The same claims under the same signature, as the token would be `seconds`
/// later: `iat` and `exp` both moved back, nothing else touched. With
/// `seconds == 0` it is a re-signed copy of a live token, the control for
/// every refusal an aged one gets.
fn aged(access: &str, seconds: i64) -> String {
    let mut claims = decode::<Value>(
        access,
        &DecodingKey::from_secret(common::JWT_SECRET.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .expect("the application's own token verifies under the test secret")
    .claims;

    for claim in ["iat", "exp"] {
        let issued = claims[claim]
            .as_i64()
            .unwrap_or_else(|| panic!("no {claim} in {claims}"));
        claims[claim] = json!(issued - seconds);
    }

    signed(&claims)
}

/// A token this test minted: a live expiry, the permission to list users, and
/// whatever `sub` and `tenant_id` it is given.
fn minted(sub: Uuid, tenant_id: Uuid) -> String {
    signed(&json!({
        "sub": sub,
        "tenant_id": tenant_id,
        "username": "minted",
        "roles": [],
        "permissions": ["identity:user:read"],
        "exp": 4_102_444_800_i64,
        "iat": 1_750_000_000_i64,
    }))
}

async fn id_of_user(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user exists")
}

fn assert_refused(response: &common::TestResponse, what: &str) {
    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "{what}: {}",
        response.body
    );
    assert_eq!(response.error_code(), Some("UNAUTHORIZED"), "{what}");
}

// ---------------------------------------------------------------------------
// A. The refusal
// ---------------------------------------------------------------------------

/// AC1: the refusal is the one an expired token gets, byte for byte.
#[tokio::test]
async fn a_deleted_tenants_token_is_refused_on_a_read_exactly_as_an_expired_token_is() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, acme) = tenant_session(&app, &system, "ACME").await;

    // Control: the token reads while its tenant is live.
    let before = app.get(USERS, Some(&acme.access)).await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.body);

    // What an expired token gets: this one, an hour past its lifetime.
    let expired = app
        .get_raw(
            USERS,
            Some(&aged(&acme.access, ACCESS_TOKEN_TTL_MINUTES * 60 + 3600)),
        )
        .await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED);

    delete_tenant(&app, &system, tenant).await;

    let refused = app.get_raw(USERS, Some(&acme.access)).await;
    assert_eq!(
        refused.status,
        StatusCode::UNAUTHORIZED,
        "a deleted tenant's token still read: {}",
        String::from_utf8_lossy(&refused.bytes)
    );
    assert_eq!(
        String::from_utf8_lossy(&refused.bytes),
        String::from_utf8_lossy(&expired.bytes),
        "the refusal says something an expired token's does not"
    );
    assert_eq!(refused.bytes, expired.bytes);
    assert_eq!(
        refused.headers, expired.headers,
        "the refusal's headers differ from an expired token's"
    );
    assert_eq!(
        refused.header(header::CONTENT_TYPE).as_deref(),
        Some("application/json")
    );

    // The envelope, read: the fixed shape and no reason.
    let body: Value = serde_json::from_slice(&refused.bytes).expect("a JSON envelope");
    assert_eq!(body["success"], false);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    let text = body.to_string().to_lowercase();
    for word in ["tenant", "deleted", "user", "acme"] {
        assert!(!text.contains(word), "the refusal says `{word}`: {body}");
    }
}

/// AC2: a write is refused and nothing is written, and an upload is refused
/// before its body is read.
#[tokio::test]
async fn a_deleted_tenants_token_writes_nothing_and_its_upload_is_refused_unread() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, acme) = tenant_session(&app, &system, "ACME").await;

    // A document of the tenant's own to attach to, made while it was live.
    let made = app
        .post(
            "/api/v1/document-types",
            Some(&acme.access),
            json!({ "typeCode": "PR_GONE", "name": "PR_GONE" }),
        )
        .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let draft = app
        .post(
            "/api/v1/documents",
            Some(&acme.access),
            json!({ "documentTypeId": made.data()["id"], "title": "Two standing desks" }),
        )
        .await;
    assert_eq!(draft.status, StatusCode::CREATED, "{}", draft.body);
    let upload = format!(
        "/api/v1/documents/{}/attachments",
        draft.data()["id"].as_str().expect("a document id")
    );

    // Control: a body over the upload limit is refused for its size once it
    // is read. The harness's limit is 4096 bytes.
    let oversized = vec![b'%'; 8192];
    let too_large = app
        .post_multipart(
            &upload,
            Some(&acme.access),
            "large.pdf",
            "application/pdf",
            &oversized,
            None,
        )
        .await;
    assert_eq!(
        too_large.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        too_large.body
    );
    assert_eq!(
        too_large.body["error"]["details"][0]["code"], "FILE_TOO_LARGE",
        "{}",
        too_large.body
    );

    delete_tenant(&app, &system, tenant).await;
    let audit = rows_in(&app, "audit_events").await;
    let activity = rows_in(&app, "activity_events").await;

    let written = app
        .post(USERS, Some(&acme.access), user_body("acme.late"))
        .await;
    assert_refused(&written, "a deleted tenant's token wrote");
    assert_eq!(
        users_named(&app, "acme.late").await,
        0,
        "the refused write happened anyway"
    );

    // **Refused before the body is read**: the same oversized body is now a
    // 401 and not the 422 reading it produces, and so is a body that is not
    // multipart at all, which the body extractor answers 415.
    let unread = app
        .post_multipart(
            &upload,
            Some(&acme.access),
            "large.pdf",
            "application/pdf",
            &oversized,
            None,
        )
        .await;
    assert_refused(&unread, "an oversized upload was read before the caller");
    let not_multipart = app.post(&upload, Some(&acme.access), json!({})).await;
    assert_refused(&not_multipart, "a body was judged before the caller");

    // And a file the route would store is not stored.
    let stored = app
        .post_multipart(
            &upload,
            Some(&acme.access),
            "quotation.pdf",
            "application/pdf",
            FILE,
            Some("from a deleted tenant"),
        )
        .await;
    assert_refused(&stored, "a deleted tenant's token uploaded");

    for table in ["attachments", "attachment_versions"] {
        assert_eq!(rows_in(&app, table).await, 0, "a row in {table}");
    }
    assert_eq!(rows_in(&app, "audit_events").await, audit, "an audit row");
    assert_eq!(
        rows_in(&app, "activity_events").await,
        activity,
        "an activity row"
    );
}

/// AC4, for one way of deleting the user.
async fn a_deleted_users_token_is_refused(delete: Deletion) {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let gone = user_administering_users(&app, "soon.gone").await;
    user_administering_users(&app, "still.here").await;
    let old = session(&app, None, "soon.gone", PASSWORD).await;
    let other = session(&app, None, "still.here", PASSWORD).await;

    // Control: the token reads while its user is live.
    let before = app.get(USERS, Some(&old.access)).await;
    assert_eq!(before.status, StatusCode::OK, "{}", before.body);

    match delete {
        Deletion::ThroughTheRoute => {
            let deleted = app.delete(&format!("{USERS}/{gone}"), Some(&admin)).await;
            assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
        }
        // `deleted_at` and nothing else: the status stays `ACTIVE` and the
        // refresh tokens stay unrevoked, so what refuses the token is the
        // column and not anything the service does beside setting it.
        Deletion::BySql => {
            sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
                .bind(gone)
                .execute(&app.pool)
                .await
                .expect("set deleted_at");
            let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
                .bind(gone)
                .fetch_one(&app.pool)
                .await
                .expect("the row is still there");
            assert_eq!(status, "ACTIVE");
        }
    }

    let read = app.get(USERS, Some(&old.access)).await;
    assert_refused(&read, "a deleted user's token read");

    let written = app
        .post(USERS, Some(&old.access), user_body("made.afterwards"))
        .await;
    assert_refused(&written, "a deleted user's token wrote");
    assert_eq!(users_named(&app, "made.afterwards").await, 0);

    // The tenant is live, and so is everybody else in it.
    let others = app.get(USERS, Some(&other.access)).await;
    assert_eq!(others.status, StatusCode::OK, "{}", others.body);
    let still = app.get(USERS, Some(&admin)).await;
    assert_eq!(still.status, StatusCode::OK, "{}", still.body);
}

enum Deletion {
    ThroughTheRoute,
    BySql,
}

#[tokio::test]
async fn a_user_deleted_through_the_route_has_their_token_refused_and_nobody_elses() {
    a_deleted_users_token_is_refused(Deletion::ThroughTheRoute).await;
}

#[tokio::test]
async fn a_user_whose_deleted_at_is_set_by_sql_has_their_token_refused_and_nobody_elses() {
    a_deleted_users_token_is_refused(Deletion::BySql).await;
}

/// The tenant's half of AC4's second way: `deleted_at` alone, the status left
/// `ACTIVE`.
#[tokio::test]
async fn a_tenant_whose_deleted_at_is_set_by_sql_has_its_token_refused_and_no_other_tenants() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, acme) = tenant_session(&app, &system, "ACME").await;
    let (_, beta) = tenant_session(&app, &system, "BETA").await;

    sqlx::query("UPDATE tenants SET deleted_at = now() WHERE id = $1")
        .bind(tenant)
        .execute(&app.pool)
        .await
        .expect("set deleted_at");
    let status: String = sqlx::query_scalar("SELECT status FROM tenants WHERE id = $1")
        .bind(tenant)
        .fetch_one(&app.pool)
        .await
        .expect("the row is still there");
    assert_eq!(status, "ACTIVE");

    let read = app.get(USERS, Some(&acme.access)).await;
    assert_refused(&read, "a deleted tenant's token read");
    let written = app
        .post(USERS, Some(&acme.access), user_body("acme.late"))
        .await;
    assert_refused(&written, "a deleted tenant's token wrote");
    assert_eq!(users_named(&app, "acme.late").await, 0);

    for (live, whose) in [
        (&beta.access, "another tenant's"),
        (&system, "the system's"),
    ] {
        let served = app.get(USERS, Some(live)).await;
        assert_eq!(served.status, StatusCode::OK, "{whose}: {}", served.body);
    }
}

/// AC5: 200 before this change for a deleted tenant, 404 for a deleted user.
#[tokio::test]
async fn the_current_user_route_refuses_a_deleted_tenants_token_and_a_deleted_users() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, acme) = tenant_session(&app, &system, "ACME").await;
    let gone = user_administering_users(&app, "soon.gone").await;
    let user = session(&app, Some("SYSTEM"), "soon.gone", PASSWORD).await;

    for token in [&acme.access, &user.access] {
        let before = app.get(ME, Some(token)).await;
        assert_eq!(before.status, StatusCode::OK, "{}", before.body);
    }

    delete_tenant(&app, &system, tenant).await;
    let deleted = app.delete(&format!("{USERS}/{gone}"), Some(&system)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let of_tenant = app.get(ME, Some(&acme.access)).await;
    assert_refused(&of_tenant, "a deleted tenant's token read /auth/me");
    let of_user = app.get(ME, Some(&user.access)).await;
    assert_refused(&of_user, "a deleted user's token read /auth/me");

    let live = app.get(ME, Some(&system)).await;
    assert_eq!(live.status, StatusCode::OK, "{}", live.body);
}

// ---------------------------------------------------------------------------
// B. Routes not behind `Authenticated`
// ---------------------------------------------------------------------------

/// AC10. Signing out identifies the caller from the refresh token and is
/// idempotent; the check on the access token does not reach it, whether or
/// not the request carries one. Sign-in and refresh are refused as they were.
#[tokio::test]
async fn signing_out_still_answers_204_for_a_deleted_caller() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, acme) = tenant_session(&app, &system, "ACME").await;
    let gone = user_administering_users(&app, "soon.gone").await;
    let user = session(&app, Some("SYSTEM"), "soon.gone", PASSWORD).await;

    delete_tenant(&app, &system, tenant).await;
    let deleted = app.delete(&format!("{USERS}/{gone}"), Some(&system)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    for (whose, held, code, username) in [
        ("a deleted tenant's", &acme, "ACME", "acme.admin"),
        ("a deleted user's", &user, "SYSTEM", "soon.gone"),
    ] {
        // Refresh and sign-in: refused before this change, and still.
        let rotated = app
            .post(
                "/api/v1/auth/refresh",
                None,
                json!({ "refreshToken": held.refresh }),
            )
            .await;
        assert_eq!(
            rotated.status,
            StatusCode::UNAUTHORIZED,
            "{whose}: {}",
            rotated.body
        );
        let signed_in = sign_in_attempt(&app, Some(code), username, PASSWORD).await;
        assert_eq!(
            signed_in.status,
            StatusCode::UNAUTHORIZED,
            "{whose}: {}",
            signed_in.body
        );

        // Sign-out, as a browser sends it (the access token in the header)
        // and as a client whose access token is gone sends it.
        for bearer in [Some(held.access.as_str()), None] {
            let signed_out = app
                .post(
                    "/api/v1/auth/logout",
                    bearer,
                    json!({ "refreshToken": held.refresh }),
                )
                .await;
            assert_eq!(
                signed_out.status,
                StatusCode::NO_CONTENT,
                "{whose} sign-out, bearer {}: {}",
                bearer.is_some(),
                signed_out.body
            );
        }
    }
}

// ---------------------------------------------------------------------------
// D. The leeway
// ---------------------------------------------------------------------------

/// AC17. `jsonwebtoken` accepts a token for 60 seconds past its `exp` unless
/// told otherwise; `verify_access_token` tells it otherwise.
#[tokio::test]
async fn a_token_one_second_past_its_lifetime_is_refused_on_a_read_and_a_write() {
    let app = TestApp::spawn().await;
    user_administering_users(&app, "on.time").await;
    let held = session(&app, None, "on.time", PASSWORD).await;

    // Controls: re-signed with its expiry untouched it is accepted, and so is
    // one with thirty seconds of its lifetime left. No leeway is not an
    // early refusal.
    for seconds in [0, ACCESS_TOKEN_TTL_MINUTES * 60 - 30] {
        let live = app.get(USERS, Some(&aged(&held.access, seconds))).await;
        assert_eq!(live.status, StatusCode::OK, "{seconds}: {}", live.body);
    }

    let expired = aged(&held.access, ACCESS_TOKEN_TTL_MINUTES * 60 + 1);

    let read = app.get(USERS, Some(&expired)).await;
    assert_refused(&read, "a token a second past its lifetime read");

    let written = app
        .post(USERS, Some(&expired), user_body("made.late"))
        .await;
    assert_refused(&written, "a token a second past its lifetime wrote");
    assert_eq!(users_named(&app, "made.late").await, 0);
}

// ---------------------------------------------------------------------------
// E. Failure and shape
// ---------------------------------------------------------------------------

/// AC19. A caller with no permission is answered 403 by a route without the
/// database being asked anything, so with the pool closed the only statement
/// that can fail is the check's own.
#[tokio::test]
async fn a_lookup_that_fails_is_a_500_and_neither_a_pass_nor_a_401() {
    let app = TestApp::spawn().await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "no.permissions",
        "no.permissions@kelir.test",
        PASSWORD,
        &[],
    )
    .await;
    let held = session(&app, None, "no.permissions", PASSWORD).await;

    // Control: with the database there, the caller is known and not allowed.
    let forbidden = app.get(USERS, Some(&held.access)).await;
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.body
    );

    app.pool.close().await;

    let failed = app.get(USERS, Some(&held.access)).await;
    assert_eq!(
        failed.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a lookup that could not run must fail closed, and must not sign the caller out: {}",
        failed.body
    );
    assert_eq!(failed.error_code(), Some("INTERNAL_ERROR"));

    // A request with no token at all is still the caller's to fix.
    let anonymous = app.get(USERS, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

/// AC20. The check is on the pair the claims carry, not on each half.
#[tokio::test]
async fn a_token_naming_a_user_of_one_tenant_and_another_tenant_is_refused() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, _) = tenant_session(&app, &system, "ACME").await;
    let administrator = id_of_user(&app, ADMIN_USERNAME).await;
    let acme_administrator = id_of_user(&app, "acme.admin").await;

    // Controls: each user with their own tenant is served.
    for (sub, tenant_id) in [
        (administrator, fixtures::SYSTEM_TENANT_ID),
        (acme_administrator, tenant),
    ] {
        let served = app.get(USERS, Some(&minted(sub, tenant_id))).await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.body);
    }

    // A live user of one tenant and another live tenant: both rows exist and
    // neither is deleted, and the pair is nobody.
    for (sub, tenant_id) in [
        (administrator, tenant),
        (acme_administrator, fixtures::SYSTEM_TENANT_ID),
    ] {
        let crossed = app.get(USERS, Some(&minted(sub, tenant_id))).await;
        assert_refused(&crossed, "a user of one tenant read as another tenant's");
    }

    // The criterion's own case: a live user of A, and a deleted tenant B.
    delete_tenant(&app, &system, tenant).await;
    let crossed = app.get(USERS, Some(&minted(administrator, tenant))).await;
    assert_refused(&crossed, "a live user read a deleted tenant");

    // And a deleted tenant's user with a live tenant: the user row is not
    // deleted (deleting a tenant does not delete its users), the pair is
    // still nobody.
    let crossed = app
        .get(
            USERS,
            Some(&minted(acme_administrator, fixtures::SYSTEM_TENANT_ID)),
        )
        .await;
    assert_refused(&crossed, "a deleted tenant's user read a live tenant");
}

/// A token for a user, or a tenant, with no row at all.
#[tokio::test]
async fn a_token_naming_a_user_or_a_tenant_that_has_no_row_is_refused() {
    let app = TestApp::spawn().await;
    let administrator = id_of_user(&app, ADMIN_USERNAME).await;

    let served = app
        .get(
            USERS,
            Some(&minted(administrator, fixtures::SYSTEM_TENANT_ID)),
        )
        .await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.body);

    let nobody = app
        .get(
            USERS,
            Some(&minted(Uuid::now_v7(), fixtures::SYSTEM_TENANT_ID)),
        )
        .await;
    assert_refused(&nobody, "a token for no user read");

    let nowhere = app
        .get(USERS, Some(&minted(administrator, Uuid::now_v7())))
        .await;
    assert_refused(&nowhere, "a token for no tenant read");
}
