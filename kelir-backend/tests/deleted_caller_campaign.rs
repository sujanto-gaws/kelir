//! **The independent campaign on the per-request deletion check**
//! ([#650](https://github.com/sujanto-gaws/kelir/issues/650), decision D-105;
//! ADR-0045; plan 18 row 17, PR #656).
//!
//! `auth_deleted_caller.rs` and `deleted_caller_refusals.rs` are the builder's.
//! This file holds what they said no test held, and what a second reader went
//! looking for:
//!
//! * **The operator's log line** (criterion 9): which of the tenant and the
//!   user was deleted or missing, with both ids, and none of it in the answer.
//! * **Refused before a byte of the body exists** (criterion 2): a request
//!   over a real socket whose body is never sent is answered 401, where a live
//!   caller's is left waiting for it.
//! * **The order of the extractors** (criterion 3): a deleted caller is told
//!   nothing by a path that does not parse, a body that is not JSON, a query
//!   that is refused, a body over the limit or a method the route does not
//!   serve. Walked over every operation that declares `security: bearer`.
//! * **The pool** (criterion 22): the check holds no connection across the
//!   handler, a burst wider than the pool is served, and a pool with nothing
//!   to give is a 500 and not a 401.
//! * **`deleted_at` and nothing else** (criterion 21, decision D-104): every
//!   combination of tenant status, user status, lock and deletion.
//! * **The expiry second**, sampled so that a leeway of one cannot pass.
//! * **Who issues a token**, and that a deployment whose default tenant is not
//!   `SYSTEM` is served.
//!
//! The race that criterion 8 leaves in `integration::service::test_call` is
//! held in `integration_test_call.rs`, beside the fixtures it needs.
//!
//! # Mutations
//!
//! Each test says which mutations were **seen red** against it (made alone,
//! this file and the builder's two run, reverted; 2026-10-02) and which were
//! **planned and not run**: the campaign's cargo runs were stopped by the
//! machine for memory before the second batch finished. A mutation listed as
//! not run is a claim nobody has checked.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use utoipa::OpenApi;
use uuid::Uuid;

use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use kelir_backend::mail::Mailer;
use kelir_backend::modules::auth::token::{verify_access_token, ACCESS_TOKEN_TTL_MINUTES};
use kelir_backend::router::ApiDoc;
use kelir_backend::state::AppState;

const TENANTS: &str = "/api/v1/organization/tenants";
const USERS: &str = "/api/v1/identity/users";
const ME: &str = "/api/v1/auth/me";
const PASSWORD: &str = "a-sufficiently-long-password";

/// What the extractor says when it refuses a token for its tenant or user.
const REFUSAL_LINE: &str = "access token refused";

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// A response, whole.
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Answer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    fn code(&self) -> Option<String> {
        serde_json::from_slice::<Value>(&self.bytes)
            .ok()
            .and_then(|body| body["error"]["code"].as_str().map(ToOwned::to_owned))
    }

    fn is_the_refusal(&self) -> bool {
        self.status == StatusCode::UNAUTHORIZED && self.code().as_deref() == Some("UNAUTHORIZED")
    }
}

/// A peer address nobody else in this binary has used.
///
/// `POST /auth/change-password` is behind the authentication rate limiter,
/// which counts refusals by peer and runs before any extractor. A walk that
/// sent every refused request from one address would be answered 429 there,
/// and that would be the limiter's answer and not the extractor's.
fn fresh_peer() -> SocketAddr {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let [_, b, c, d] = n.to_be_bytes();

    // TEST-NET-2 and onward into space no test peer is drawn from.
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(198, 51_u8.wrapping_add(b), c, d)),
        40_000,
    )
}

/// One request through a router, the body and its content type as given.
async fn send(
    router: Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    content: Option<(&str, Bytes)>,
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let mut request = match content {
        Some((content_type, body)) => builder
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body)),
        None => builder.body(Body::empty()),
    }
    .unwrap_or_else(|error| panic!("build a request for {uri}: {error}"));
    request.extensions_mut().insert(ConnectInfo(fresh_peer()));

    let response = router
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("drive the router for {uri}: {error}"));

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a response body reads")
        .to_vec();

    Answer {
        status,
        headers,
        bytes,
    }
}

async fn get(app: &TestApp, uri: &str, token: &str) -> Answer {
    send(app.router(), Method::GET, uri, Some(token), None).await
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

async fn multi_tenant_app() -> TestApp {
    TestApp::spawn_with(|config| config.multi_tenant = true).await
}

/// A tenant made through the route, and its administrator's access token,
/// issued while it was live. Returns the tenant's id and the administrator's.
async fn tenant_with_administrator(
    app: &TestApp,
    system: &str,
    code: &str,
) -> (Uuid, Uuid, String) {
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
    let tenant: Uuid = created.data()["id"]
        .as_str()
        .expect("a created tenant has an id")
        .parse()
        .expect("a tenant id is a uuid");
    let administrator = id_of_user(app, &username).await;

    (
        tenant,
        administrator,
        app.sign_in_to(code, &username, PASSWORD).await,
    )
}

/// A user of the system tenant who may read and create users.
async fn user_reading_users(app: &TestApp, username: &str) -> Uuid {
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

async fn id_of_user(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user exists")
}

async fn execute(app: &TestApp, statement: &str, id: Uuid) {
    sqlx::query(statement)
        .bind(id)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

fn signed(claims: &Value) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(common::JWT_SECRET.as_bytes()),
    )
    .expect("claims sign")
}

/// A token this test minted: a far expiry, the permission to list users, and
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

/// The application's own token with `exp` at a chosen instant and `iat` a
/// lifetime before it; nothing else touched.
fn expiring_at(access: &str, exp: i64) -> String {
    let mut claims = decode::<Value>(
        access,
        &DecodingKey::from_secret(common::JWT_SECRET.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .expect("the application's own token verifies under the test secret")
    .claims;

    claims["exp"] = json!(exp);
    claims["iat"] = json!(exp - ACCESS_TOKEN_TTL_MINUTES * 60);

    signed(&claims)
}

async fn row_counts(app: &TestApp) -> BTreeMap<String, i64> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY 1",
    )
    .fetch_all(&app.pool)
    .await
    .expect("list the tables");

    let mut counts = BTreeMap::new();
    for table in tables {
        let count: i64 = sqlx::query_scalar(&format!(r#"SELECT count(*) FROM "{table}""#))
            .fetch_one(&app.pool)
            .await
            .unwrap_or_else(|error| panic!("count {table}: {error}"));
        counts.insert(table, count);
    }
    counts
}

// ---------------------------------------------------------------------------
// 1. The operator's log line (criterion 9)
// ---------------------------------------------------------------------------

/// Everything the backend traces from this thread.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the capture")).into_owned()
    }

    fn refusals(&self) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| line.contains(REFUSAL_LINE))
            .map(ToOwned::to_owned)
            .collect()
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the capture").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// One request under a subscriber of this test's own, at `level`, and what
/// the backend traced while answering it.
///
/// `#[tokio::test]` is a current-thread runtime, so the extractor, the handler
/// and the statement they run are all on this thread, under this thread's
/// subscriber (`integration_test_call.rs` reads a trace the same way).
async fn traced(
    app: &TestApp,
    level: tracing::Level,
    uri: &str,
    token: Option<&str>,
) -> (Answer, Captured) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();

    let answer = {
        let _guard = tracing::subscriber::set_default(subscriber);
        send(app.router(), Method::GET, uri, token, None).await
    };

    (answer, captured)
}

/// **Criterion 9.** A refusal is logged for the operator with the user id, the
/// tenant id and which of the two was deleted or missing; the answer says
/// none of it, whichever it was; and a request that is not refused for this
/// reason does not write the line.
///
/// Seen red, each alone, and by no test of the builder's: the line written
/// at `debug`; `tenant` and `user` swapped in the line; a missing row
/// reported as `deleted`; `user_id` carrying the tenant's id; a tenant with
/// no row read as live (the answer is still 401, because the user is then
/// missing too: only the line differs).
#[tokio::test]
async fn a_refusal_tells_the_operator_which_of_the_two_it_was_and_the_caller_nothing() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let system_administrator = id_of_user(&app, ADMIN_USERNAME).await;

    // A tenant deleted through the route: its administrator's row is live.
    let (gone_tenant, gone_tenants_user, of_deleted_tenant) =
        tenant_with_administrator(&app, &system, "GONE").await;
    // A user deleted through the route, in a live tenant.
    let gone_user = user_reading_users(&app, "soon.gone").await;
    let of_deleted_user = app.sign_in_to("SYSTEM", "soon.gone", PASSWORD).await;
    // A tenant and its user both deleted.
    let (both_tenant, both_user, of_both) = tenant_with_administrator(&app, &system, "BOTH").await;
    // A live tenant and its live user, for the crossed pair.
    let (live_tenant, live_tenants_user, of_live_tenant) =
        tenant_with_administrator(&app, &system, "LIVE").await;

    // Control, before anything is deleted: a served request writes no line,
    // and the capture is not blind (it sees the request's own statements).
    for token in [
        &of_deleted_tenant,
        &of_deleted_user,
        &of_both,
        &of_live_tenant,
    ] {
        let (served, captured) = traced(&app, tracing::Level::TRACE, USERS, Some(token)).await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.text());
        assert!(
            captured.refusals().is_empty(),
            "a served request was logged as refused: {:?}",
            captured.refusals()
        );
        assert!(
            captured.text().contains("deleted_at"),
            "the capture saw no statement, so its silence proves nothing: {}",
            captured.text()
        );
    }

    for id in [gone_tenant, both_tenant] {
        let deleted = app.delete(&format!("{TENANTS}/{id}"), Some(&system)).await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    }
    let deleted = app
        .delete(&format!("{USERS}/{gone_user}"), Some(&system))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    execute(
        &app,
        "UPDATE users SET deleted_at = now() WHERE id = $1",
        both_user,
    )
    .await;

    // What an expired token gets, and that it is not this line's business.
    let expired_token = expiring_at(&system, chrono::Utc::now().timestamp() - 3600);
    let (expired, captured) =
        traced(&app, tracing::Level::TRACE, USERS, Some(&expired_token)).await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED);
    assert!(
        captured.refusals().is_empty(),
        "an expired token was logged as a deleted caller's: {:?}",
        captured.refusals()
    );
    let (anonymous, captured) = traced(&app, tracing::Level::TRACE, USERS, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert!(captured.refusals().is_empty());

    let nobody = Uuid::now_v7();
    let nowhere = Uuid::now_v7();
    let cases: [(&str, String, Uuid, Uuid, &str, &str); 7] = [
        (
            "a deleted tenant, its user live",
            of_deleted_tenant.clone(),
            gone_tenants_user,
            gone_tenant,
            "deleted",
            "live",
        ),
        (
            "a live tenant, its user deleted",
            of_deleted_user.clone(),
            gone_user,
            fixtures::SYSTEM_TENANT_ID,
            "live",
            "deleted",
        ),
        (
            "both deleted",
            of_both.clone(),
            both_user,
            both_tenant,
            "deleted",
            "deleted",
        ),
        (
            "a live tenant and no such user",
            minted(nobody, fixtures::SYSTEM_TENANT_ID),
            nobody,
            fixtures::SYSTEM_TENANT_ID,
            "live",
            "missing",
        ),
        (
            "no such tenant, and a user who is somebody else's",
            minted(system_administrator, nowhere),
            system_administrator,
            nowhere,
            "missing",
            "missing",
        ),
        (
            "a live user of one live tenant, named with another",
            minted(live_tenants_user, fixtures::SYSTEM_TENANT_ID),
            live_tenants_user,
            fixtures::SYSTEM_TENANT_ID,
            "live",
            "missing",
        ),
        (
            "a live user named with a deleted tenant that is not theirs",
            minted(system_administrator, gone_tenant),
            system_administrator,
            gone_tenant,
            "deleted",
            "missing",
        ),
    ];

    for (what, token, user_id, tenant_id, tenant, user) in cases {
        // At WARN: the level a deployment that logs warnings and errors keeps.
        let (refused, captured) = traced(&app, tracing::Level::WARN, USERS, Some(&token)).await;
        assert!(refused.is_the_refusal(), "{what}: {}", refused.text());

        let lines = captured.refusals();
        assert_eq!(
            lines.len(),
            1,
            "{what}: one refusal, one line. Captured: {}",
            captured.text()
        );
        let line = &lines[0];
        assert!(line.contains("WARN"), "{what}: not a warning: {line}");
        for field in [
            format!("user_id={user_id}"),
            format!("tenant_id={tenant_id}"),
            format!("tenant=\"{tenant}\""),
            format!("user=\"{user}\""),
        ] {
            assert!(line.contains(&field), "{what}: no `{field}` in: {line}");
        }
        // The credential is not the operator's to read.
        assert!(
            !captured.text().contains(&token),
            "{what}: the log carries the token"
        );

        // The caller is told what an expired token's holder is told.
        assert_eq!(refused.bytes, expired.bytes, "{what}: {}", refused.text());
        assert_eq!(refused.headers, expired.headers, "{what}");
        let body = refused.text().to_lowercase();
        for word in [
            user_id.to_string(),
            tenant_id.to_string(),
            "tenant".to_owned(),
            "deleted".to_owned(),
            "missing".to_owned(),
            "user".to_owned(),
        ] {
            assert!(
                !body.contains(&word),
                "{what}: the answer says `{word}`: {body}"
            );
        }
    }

    // Still served, and still silent.
    let (served, captured) = traced(&app, tracing::Level::WARN, USERS, Some(&of_live_tenant)).await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.text());
    assert!(captured.refusals().is_empty());
    let _ = live_tenant;
}

/// A row that was there when the token was issued and is not there at all
/// now (a restore, a hand-run `DELETE`): no row is no caller.
#[tokio::test]
async fn a_user_whose_row_is_removed_outright_is_refused_as_missing() {
    let app = TestApp::spawn().await;
    let user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "never.signed.in",
        "never.signed.in@kelir.test",
        PASSWORD,
        &[],
    )
    .await;
    let token = minted(user, fixtures::SYSTEM_TENANT_ID);

    let served = get(&app, USERS, &token).await;
    assert_eq!(served.status, StatusCode::OK, "{}", served.text());

    execute(&app, "DELETE FROM users WHERE id = $1", user).await;

    let (refused, captured) = traced(&app, tracing::Level::WARN, USERS, Some(&token)).await;
    assert!(refused.is_the_refusal(), "{}", refused.text());
    let lines = captured.refusals();
    assert_eq!(lines.len(), 1, "{}", captured.text());
    assert!(lines[0].contains("tenant=\"live\""), "{}", lines[0]);
    assert!(lines[0].contains("user=\"missing\""), "{}", lines[0]);
}

// ---------------------------------------------------------------------------
// 2. `deleted_at` and nothing else (criterion 21; decisions D-104, D-105)
// ---------------------------------------------------------------------------

/// Every combination of the columns that could be mistaken for "deleted".
///
/// Three tenant statuses, four user statuses, a lock that has and has not
/// run out, and `deleted_at` on each side: 96 states of one tenant and one
/// user, each asked with the one token issued while both were live and
/// active. **The token is served exactly when neither `deleted_at` is set.**
///
/// Planned and not run: the user's predicate widened by `status = 'LOCKED'`,
/// by `locked_until > now()` or by `status = 'PENDING_ACTIVATION'`; the
/// tenant's widened by `status = 'SUSPENDED'` or `'INACTIVE'`.
#[tokio::test]
async fn a_token_is_served_exactly_when_neither_deleted_at_is_set_whatever_the_statuses() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, user, token) = tenant_with_administrator(&app, &system, "ACME").await;

    let mut wrong = Vec::new();
    let mut served = 0;
    let mut refused = 0;

    for tenant_status in ["ACTIVE", "SUSPENDED", "INACTIVE"] {
        for tenant_deleted in [false, true] {
            for user_status in ["ACTIVE", "INACTIVE", "LOCKED", "PENDING_ACTIVATION"] {
                for locked in [false, true] {
                    for user_deleted in [false, true] {
                        sqlx::query(
                            "UPDATE tenants SET status = $2, \
                             deleted_at = CASE WHEN $3 THEN now() END WHERE id = $1",
                        )
                        .bind(tenant)
                        .bind(tenant_status)
                        .bind(tenant_deleted)
                        .execute(&app.pool)
                        .await
                        .expect("set the tenant");
                        sqlx::query(
                            "UPDATE users SET status = $2, \
                             locked_until = CASE WHEN $3 THEN now() + interval '1 hour' END, \
                             deleted_at = CASE WHEN $4 THEN now() END WHERE id = $1",
                        )
                        .bind(user)
                        .bind(user_status)
                        .bind(locked)
                        .bind(user_deleted)
                        .execute(&app.pool)
                        .await
                        .expect("set the user");

                        let state = format!(
                            "tenant {tenant_status}{}, user {user_status}{}{}",
                            if tenant_deleted { " deleted" } else { "" },
                            if locked { " locked for an hour" } else { "" },
                            if user_deleted { " deleted" } else { "" },
                        );
                        let answer = get(&app, USERS, &token).await;

                        if tenant_deleted || user_deleted {
                            refused += 1;
                            if !answer.is_the_refusal() {
                                wrong.push(format!(
                                    "{state}: served {} {}",
                                    answer.status,
                                    answer.text()
                                ));
                            }
                        } else {
                            served += 1;
                            if answer.status != StatusCode::OK {
                                wrong.push(format!(
                                    "{state}: refused {} {}",
                                    answer.status,
                                    answer.text()
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        (served, refused),
        (24, 72),
        "the table is not the one written"
    );
    assert!(
        wrong.is_empty(),
        "{} of 96 states answered wrongly:\n{}",
        wrong.len(),
        wrong.join("\n")
    );

    // Another tenant's token was served throughout, and is now.
    let still = get(&app, USERS, &system).await;
    assert_eq!(still.status, StatusCode::OK, "{}", still.text());
}

/// The orders things happen in: a tenant suspended and then deleted is
/// refused; a tenant deleted and then undeleted by hand is live under the
/// rule, though the soft delete left its status `INACTIVE`, and its token is
/// served again; the same for a user. No route restores either, so the
/// restoring is SQL.
#[tokio::test]
async fn a_restored_tenant_and_a_restored_user_are_live_again_with_the_status_deletion_left() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, _, of_tenant) = tenant_with_administrator(&app, &system, "ACME").await;
    let user = user_reading_users(&app, "comes.back").await;
    let of_user = app.sign_in_to("SYSTEM", "comes.back", PASSWORD).await;

    // Suspended through the route: D-104, the token is served.
    let suspended = app
        .put(
            &format!("{TENANTS}/{tenant}"),
            Some(&system),
            json!({ "status": "SUSPENDED" }),
        )
        .await;
    assert_eq!(suspended.status, StatusCode::OK, "{}", suspended.body);
    let answer = get(&app, USERS, &of_tenant).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());

    // Then deleted through the route: D-105, refused.
    let deleted = app
        .delete(&format!("{TENANTS}/{tenant}"), Some(&system))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let answer = get(&app, USERS, &of_tenant).await;
    assert!(answer.is_the_refusal(), "{}", answer.text());

    // Then undeleted by hand, the status left as the deletion wrote it.
    execute(
        &app,
        "UPDATE tenants SET deleted_at = NULL WHERE id = $1",
        tenant,
    )
    .await;
    let status: String = sqlx::query_scalar("SELECT status FROM tenants WHERE id = $1")
        .bind(tenant)
        .fetch_one(&app.pool)
        .await
        .expect("the tenant's row");
    assert_eq!(status, "INACTIVE", "what the soft delete wrote");
    let answer = get(&app, USERS, &of_tenant).await;
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "a tenant that is not deleted is live, whatever its status: {}",
        answer.text()
    );

    // The user, the same way.
    let deleted = app.delete(&format!("{USERS}/{user}"), Some(&system)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let answer = get(&app, USERS, &of_user).await;
    assert!(answer.is_the_refusal(), "{}", answer.text());

    execute(
        &app,
        "UPDATE users SET deleted_at = NULL WHERE id = $1",
        user,
    )
    .await;
    let status: String = sqlx::query_scalar("SELECT status FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .expect("the user's row");
    assert_eq!(status, "INACTIVE", "what the soft delete wrote");
    let answer = get(&app, USERS, &of_user).await;
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "a user who is not deleted is live, whatever their status: {}",
        answer.text()
    );
}

// ---------------------------------------------------------------------------
// 3. Refused before a byte of the body exists (criterion 2)
// ---------------------------------------------------------------------------

/// The application on a real socket, so that a request can be sent a piece at
/// a time.
async fn served(app: &TestApp) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a port");
    let address = listener.local_addr().expect("a local address");
    let service = app
        .router()
        .into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });

    address
}

/// Sends a request's head, declaring a body of `body.len()` bytes, **and not
/// the body**. Returns the connection and whatever was answered within
/// `wait`, which is nothing if the server is waiting for the body.
async fn head_only(
    address: SocketAddr,
    uri: &str,
    token: &str,
    content_type: &str,
    body: &[u8],
    wait: Duration,
) -> (tokio::net::TcpStream, String) {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect");
    let head = format!(
        "POST {uri} HTTP/1.1\r\nHost: kelir.test\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .expect("write a head");
    stream.flush().await.expect("flush");

    let answered = read_for(&mut stream, wait).await;
    (stream, answered)
}

/// Whatever arrives on a connection within `wait`.
async fn read_for(stream: &mut tokio::net::TcpStream, wait: Duration) -> String {
    let mut received = Vec::new();
    let deadline = Instant::now() + wait;
    let mut buffer = [0_u8; 4096];

    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, stream.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(read)) => {
                received.extend_from_slice(&buffer[..read]);
                // A whole answer: the head and a JSON body that has closed.
                let text = String::from_utf8_lossy(&received);
                if text.contains("\r\n\r\n") && text.trim_end().ends_with('}') {
                    break;
                }
            }
        }
    }

    String::from_utf8_lossy(&received).into_owned()
}

/// **Criterion 2, at the socket.** The builder's test shows it by answer: a
/// body that would be refused when read is answered 401 instead. This one
/// never sends the body. A deleted caller's request is answered 401 on its
/// head alone, on a JSON route and on the multipart upload; a live caller's
/// same head is left unanswered, because the handler is waiting for bytes
/// that have not come, and is answered once they do.
///
/// Planned and not run: the upload's caller judged after `read_file_part`
/// (`caller: Result<Authenticated, AppError>`).
#[tokio::test]
async fn a_deleted_callers_request_is_answered_before_any_of_its_body_is_sent() {
    const BOUNDARY: &str = "kelircampaignboundary";

    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, _, of_tenant) = tenant_with_administrator(&app, &system, "ACME").await;
    let gone_user = user_reading_users(&app, "soon.gone").await;
    let of_user = app.sign_in_to("SYSTEM", "soon.gone", PASSWORD).await;

    // A document in each tenant to upload to, made while both were live.
    let mut uploads = Vec::new();
    for (token, code) in [(&of_tenant, "PR_GONE"), (&system, "PR_LIVE")] {
        let made = app
            .post(
                "/api/v1/document-types",
                Some(token),
                json!({ "typeCode": code, "name": code }),
            )
            .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        let draft = app
            .post(
                "/api/v1/documents",
                Some(token),
                json!({ "documentTypeId": made.data()["id"], "title": "Two standing desks" }),
            )
            .await;
        assert_eq!(draft.status, StatusCode::CREATED, "{}", draft.body);
        uploads.push(format!(
            "/api/v1/documents/{}/attachments",
            draft.data()["id"].as_str().expect("a document id")
        ));
    }

    let multipart = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
         filename=\"quotation.pdf\"\r\nContent-Type: application/pdf\r\n\r\n\
         %PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes();
    let multipart_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let user = serde_json::to_vec(&json!({
        "username": "made.by.socket",
        "email": "made.by.socket@example.test",
        "password": PASSWORD,
        "displayName": "Made Over A Socket",
    }))
    .expect("a body");

    let address = served(&app).await;

    // Control: a live caller's head is not answered. The handler wants the
    // body, and the body has not been sent.
    let quiet = Duration::from_millis(750);
    let (mut stream, early) =
        head_only(address, USERS, &system, "application/json", &user, quiet).await;
    assert_eq!(early, "", "a live caller was answered before its body");
    stream.write_all(&user).await.expect("send the body");
    let late = read_for(&mut stream, Duration::from_secs(10)).await;
    assert!(late.starts_with("HTTP/1.1 201"), "{late}");

    let (mut stream, early) = head_only(
        address,
        &uploads[1],
        &system,
        &multipart_type,
        &multipart,
        quiet,
    )
    .await;
    assert_eq!(
        early, "",
        "a live caller's upload was answered before its body"
    );
    stream.write_all(&multipart).await.expect("send the body");
    let late = read_for(&mut stream, Duration::from_secs(10)).await;
    assert!(late.starts_with("HTTP/1.1 200"), "{late}");

    let deleted = app
        .delete(&format!("{TENANTS}/{tenant}"), Some(&system))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let deleted = app
        .delete(&format!("{USERS}/{gone_user}"), Some(&system))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let before = row_counts(&app).await;

    // A deleted caller's head is answered, with no body ever sent.
    for (whose, token, uri, content_type, body) in [
        (
            "a deleted tenant's write",
            &of_tenant,
            USERS,
            "application/json",
            &user,
        ),
        (
            "a deleted tenant's upload",
            &of_tenant,
            uploads[0].as_str(),
            multipart_type.as_str(),
            &multipart,
        ),
        (
            "a deleted user's write",
            &of_user,
            USERS,
            "application/json",
            &user,
        ),
        (
            "a deleted user's upload",
            &of_user,
            uploads[1].as_str(),
            multipart_type.as_str(),
            &multipart,
        ),
    ] {
        let (_stream, answered) = head_only(
            address,
            uri,
            token,
            content_type,
            body,
            Duration::from_secs(10),
        )
        .await;
        assert!(
            answered.starts_with("HTTP/1.1 401"),
            "{whose} was not refused on its head alone: {answered:?}"
        );
        assert!(answered.contains("\"UNAUTHORIZED\""), "{whose}: {answered}");
    }

    assert_eq!(
        row_counts(&app).await,
        before,
        "a refused head wrote something"
    );
}

// ---------------------------------------------------------------------------
// 4. The order of the extractors, on every operation (criterion 3)
// ---------------------------------------------------------------------------

struct Operation {
    method: Method,
    template: String,
    /// The names of the path captures, in order.
    captures: Vec<String>,
}

impl Operation {
    /// The path with every capture replaced by what `value` gives it.
    fn path(&self, value: impl Fn(&str) -> String) -> String {
        let mut path = self.template.clone();
        for capture in &self.captures {
            path = path.replace(&format!("{{{capture}}}"), &value(capture));
        }
        path
    }

    fn valid_path(&self) -> String {
        self.path(|_| Uuid::now_v7().to_string())
    }
}

const METHODS: [(&str, Method); 5] = [
    ("get", Method::GET),
    ("post", Method::POST),
    ("put", Method::PUT),
    ("patch", Method::PATCH),
    ("delete", Method::DELETE),
];

fn declares_bearer(operation: &Value) -> bool {
    operation["security"]
        .as_array()
        .is_some_and(|schemes| schemes.iter().any(|scheme| scheme.get("bearer").is_some()))
}

fn captures_of(template: &str) -> Vec<String> {
    template
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split('}').next())
        .map(ToOwned::to_owned)
        .collect()
}

/// Every operation in the document, and whether it declares `security:
/// bearer`.
fn documented() -> Vec<(Operation, bool)> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        for (key, method) in METHODS {
            let operation = &item[key];
            if operation.is_object() {
                operations.push((
                    Operation {
                        method,
                        template: template.clone(),
                        captures: captures_of(template),
                    },
                    declares_bearer(operation),
                ));
            }
        }
    }

    operations
}

fn authenticated_operations() -> Vec<Operation> {
    documented()
        .into_iter()
        .filter_map(|(operation, bearer)| bearer.then_some(operation))
        .collect()
}

/// Three callers as the builder's walk makes them: one live, one whose tenant
/// is then deleted, one whose user is.
async fn three_callers(app: &TestApp) -> (String, String, String) {
    let live = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, _, of_deleted_tenant) = tenant_with_administrator(app, &live, "GONE").await;
    let user = user_reading_users(app, "soon.gone").await;
    let of_deleted_user = app.sign_in_to("SYSTEM", "soon.gone", PASSWORD).await;

    for token in [&live, &of_deleted_tenant, &of_deleted_user] {
        let answer = get(app, USERS, token).await;
        assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());
    }

    let deleted = app
        .delete(&format!("{TENANTS}/{tenant}"), Some(&live))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let deleted = app.delete(&format!("{USERS}/{user}"), Some(&live)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    (live, of_deleted_tenant, of_deleted_user)
}

/// A request body and the content type it is sent as, if it has one.
type Content = Option<(&'static str, Bytes)>;

/// One way of sending a request that an extractor other than the caller's
/// would refuse.
struct Malformation {
    name: &'static str,
    /// Only operations with a path capture are sent it.
    needs_capture: bool,
    build: fn(&Operation) -> (String, Content),
}

fn over_the_body_limit() -> Bytes {
    static BODY: std::sync::OnceLock<Bytes> = std::sync::OnceLock::new();
    // Axum reads at most 2 MiB of a body unless a route says otherwise; a
    // JSON string of 3 MiB is over it, and is one allocation shared by every
    // request that carries it.
    BODY.get_or_init(|| {
        let mut body = Vec::with_capacity(3 * 1024 * 1024 + 16);
        body.extend_from_slice(b"{\"x\":\"");
        body.resize(3 * 1024 * 1024, b'a');
        body.extend_from_slice(b"\"}");
        Bytes::from(body)
    })
    .clone()
}

const MALFORMATIONS: [Malformation; 8] = [
    Malformation {
        name: "a path id that is not a uuid",
        needs_capture: true,
        build: |operation| (operation.path(|_| "not-a-uuid".to_owned()), None),
    },
    Malformation {
        name: "a path id of 4,000 characters",
        needs_capture: true,
        build: |operation| (operation.path(|_| "a".repeat(4000)), None),
    },
    Malformation {
        name: "a path id holding an encoded NUL and a traversal",
        needs_capture: true,
        build: |operation| (operation.path(|_| "%00..%2F..%2Fetc".to_owned()), None),
    },
    Malformation {
        name: "a body that is not JSON, sent as JSON",
        needs_capture: false,
        build: |operation| {
            (
                operation.valid_path(),
                Some(("application/json", Bytes::from_static(b"{\"unterminated"))),
            )
        },
    },
    Malformation {
        name: "a body of another content type",
        needs_capture: false,
        build: |operation| {
            (
                operation.valid_path(),
                Some(("text/plain", Bytes::from_static(b"not for this route"))),
            )
        },
    },
    Malformation {
        name: "a query no route accepts",
        needs_capture: false,
        build: |operation| {
            (
                format!(
                    "{}?page=abc&pageSize=-1&search=%00&statusId=NOPE&from=never",
                    operation.valid_path()
                ),
                None,
            )
        },
    },
    Malformation {
        name: "a JSON body over the body limit",
        needs_capture: false,
        build: |operation| {
            (
                operation.valid_path(),
                Some(("application/json", over_the_body_limit())),
            )
        },
    },
    Malformation {
        name: "a JSON body of the wrong shape",
        needs_capture: false,
        build: |operation| {
            (
                operation.valid_path(),
                Some(("application/json", Bytes::from_static(b"[1, 2, 3]"))),
            )
        },
    },
];

/// **A deleted caller is told nothing by a request that is wrong in some
/// other way.** `Authenticated` is the first extractor that can refuse in
/// every handler, so a path that does not parse, a body that is not JSON, a
/// refused query or a body over the limit is still answered 401 and nothing
/// else. The builder's walk sends a well-formed path and an empty object;
/// this one sends each operation eight requests that another extractor
/// refuses, with a live caller first as the control: the live caller must not
/// be answered 401, and each malformation must be refused for itself on at
/// least one operation, or it is not a malformation.
///
/// Seen red: `PathParam` placed before `caller` in
/// `identity::handlers::get_user` (6 of 3,036 cells, and the source guard
/// below; the builder's walk and its count guard stayed green). Planned and
/// not run: `QueryParams` placed before `caller` in `list_users`; the check
/// applied to `GET` only.
#[tokio::test]
async fn a_deleted_caller_is_refused_before_a_malformed_path_body_or_query_is_judged() {
    let operations = authenticated_operations();
    assert!(
        operations.len() >= 143,
        "{} operations declare `security: bearer`; there were 143",
        operations.len()
    );

    let app = multi_tenant_app().await;
    let (live, of_deleted_tenant, of_deleted_user) = three_callers(&app).await;

    let mut failures = Vec::new();
    let mut cells = 0;
    let mut report = Vec::new();

    // --- The control: what a live caller is told -----------------------------
    for malformation in &MALFORMATIONS {
        let mut answers: BTreeMap<u16, usize> = BTreeMap::new();
        for operation in &operations {
            if malformation.needs_capture && operation.captures.is_empty() {
                continue;
            }
            cells += 1;
            let (uri, content) = (malformation.build)(operation);
            let answer = send(
                app.router(),
                operation.method.clone(),
                &uri,
                Some(&live),
                content,
            )
            .await;
            *answers.entry(answer.status.as_u16()).or_default() += 1;
            if answer.status == StatusCode::UNAUTHORIZED {
                failures.push(format!(
                    "{} {} with {}: a live token was refused — {}",
                    operation.method,
                    operation.template,
                    malformation.name,
                    answer.text()
                ));
            }
        }

        // A malformation nothing refuses proves nothing about order.
        let refused_for_itself: usize = answers
            .iter()
            .filter(|(status, _)| matches!(status, 400 | 413 | 415 | 422))
            .map(|(_, count)| count)
            .sum();
        assert!(
            refused_for_itself > 0,
            "no route refuses {} from a live caller: {answers:?}",
            malformation.name
        );
        report.push(format!("live, {}: {answers:?}", malformation.name));
    }

    // --- The refusals --------------------------------------------------------
    let before = row_counts(&app).await;

    for (whose, token) in [
        ("a deleted tenant's token", &of_deleted_tenant),
        ("a deleted user's token", &of_deleted_user),
    ] {
        for malformation in &MALFORMATIONS {
            for operation in &operations {
                if malformation.needs_capture && operation.captures.is_empty() {
                    continue;
                }
                cells += 1;
                let (uri, content) = (malformation.build)(operation);
                let answer = send(
                    app.router(),
                    operation.method.clone(),
                    &uri,
                    Some(token),
                    content,
                )
                .await;
                if !answer.is_the_refusal() {
                    let text = answer.text();
                    failures.push(format!(
                        "{} {} with {}: {whose} was answered {} {}",
                        operation.method,
                        operation.template,
                        malformation.name,
                        answer.status,
                        &text[..text.len().min(300)]
                    ));
                }
            }
        }
    }

    let after = row_counts(&app).await;
    let moved: Vec<String> = after
        .iter()
        .filter(|(table, count)| before.get(*table) != Some(*count))
        .map(|(table, count)| format!("{table}: {:?} -> {count}", before.get(table)))
        .collect();

    println!(
        "{cells} cells over {} operations\n{}",
        operations.len(),
        report.join("\n")
    );
    assert!(
        failures.is_empty(),
        "{} of {cells} cells over {} operations failed:\n{}",
        failures.len(),
        operations.len(),
        failures.join("\n")
    );
    assert!(
        moved.is_empty(),
        "a refused request wrote something: {moved:?}"
    );
}

/// **A method the document does not list is not a way in.** For every path
/// that carries an authenticated operation, every method the document does
/// not list on it is sent with a deleted caller's token and a live one's.
/// `HEAD` is served by the `GET` handler, so it is refused like `GET`; any
/// other is 405 for both, which is the router's answer and no handler's.
/// A handler registered on a method its annotation does not name would be
/// invisible to the walk, and would answer something else here.
#[tokio::test]
async fn a_method_the_document_does_not_list_reaches_no_handler() {
    let mut listed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (operation, _) in documented() {
        listed
            .entry(operation.template.clone())
            .or_default()
            .insert(operation.method.to_string());
    }
    let templates: BTreeSet<String> = authenticated_operations()
        .into_iter()
        .map(|operation| operation.template)
        .collect();

    let app = multi_tenant_app().await;
    let (live, of_deleted_tenant, of_deleted_user) = three_callers(&app).await;

    let mut failures = Vec::new();
    let mut cells = 0;
    let before = row_counts(&app).await;

    for template in &templates {
        let methods = &listed[template];
        let operation = Operation {
            method: Method::GET,
            template: template.clone(),
            captures: captures_of(template),
        };
        let uri = operation.valid_path();

        for method in [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::HEAD,
        ] {
            if methods.contains(method.as_str()) {
                continue;
            }
            let head_of_get = method == Method::HEAD && methods.contains("GET");

            for (whose, token, dead) in [
                ("a live token", &live, false),
                ("a deleted tenant's token", &of_deleted_tenant, true),
                ("a deleted user's token", &of_deleted_user, true),
            ] {
                cells += 1;
                let content =
                    (method != Method::GET && method != Method::HEAD && method != Method::DELETE)
                        .then(|| ("application/json", Bytes::from_static(b"{}")));
                let answer = send(app.router(), method.clone(), &uri, Some(token), content).await;

                let expected = if head_of_get {
                    // Served by the `GET` handler: behind the same check.
                    if dead {
                        answer.status == StatusCode::UNAUTHORIZED
                    } else {
                        answer.status != StatusCode::UNAUTHORIZED
                    }
                } else {
                    answer.status == StatusCode::METHOD_NOT_ALLOWED
                };
                if !expected {
                    failures.push(format!(
                        "{method} {template}: {whose} was answered {} {}",
                        answer.status,
                        answer.text()
                    ));
                }
            }
        }
    }

    assert!(cells > 300, "only {cells} cells were sent");
    assert!(
        failures.is_empty(),
        "{} of {cells} cells failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(
        row_counts(&app).await,
        before,
        "an unlisted method wrote something"
    );
}

/// **The operations that take no token are these and no others.** The walk
/// reads `security: bearer`; an operation published without it is outside the
/// walk and outside the check, so the set is written down. A new route that
/// serves without a token has to be added here by somebody who meant it.
#[test]
fn the_operations_that_take_no_access_token_are_the_ten_that_should_not() {
    let open: BTreeSet<String> = documented()
        .into_iter()
        .filter(|(_, bearer)| !bearer)
        .map(|(operation, _)| format!("{} {}", operation.method, operation.template))
        .collect();

    let expected: BTreeSet<String> = [
        "GET /health",
        "GET /health/live",
        "GET /health/ready",
        "GET /version",
        "GET /deployment",
        "POST /api/v1/auth/login",
        "POST /api/v1/auth/refresh",
        "POST /api/v1/auth/logout",
        "POST /api/v1/auth/forgot-password",
        "POST /api/v1/auth/reset-password",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect();

    assert_eq!(open, expected);
}

/// Every `.rs` file under `src`, with its path.
fn sources() -> Vec<(String, String)> {
    fn visit(directory: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push((
                    path.to_string_lossy().replace('\\', "/"),
                    std::fs::read_to_string(&path).expect("the source reads"),
                ));
            }
        }
    }

    let mut out = Vec::new();
    visit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// **The order, from the source.** In every handler that takes the caller,
/// the only argument before it is the state, which cannot refuse. Axum runs a
/// handler's extractors in the order they are written, so this is what makes
/// the walk above true for a route added tomorrow before anyone walks it.
///
/// And the three ways a route could take a token without this check: a
/// caller that is optional (`Option<Authenticated>`), one whose failure the
/// handler holds (`Result<Authenticated, …>`), and a second reader of the
/// token (`verify_access_token` called anywhere but the extractor) or of the
/// header.
#[test]
fn the_caller_is_the_first_argument_that_can_refuse_in_every_handler() {
    let sources = sources();
    assert!(sources.len() > 50, "the scan is not reading the crate");

    let mut handlers = 0;
    let mut out_of_order = Vec::new();
    let mut other_readers = Vec::new();

    for (path, text) in &sources {
        // The production half of the file: a `#[cfg(test)]` module is tests.
        let production = text.split("#[cfg(test)]").next().unwrap_or(text);

        for needle in ["Option<Authenticated>", "Result<Authenticated"] {
            if production.contains(needle) {
                other_readers.push(format!("{path}: takes `{needle}`"));
            }
        }
        if !path.ends_with("src/middleware/auth.rs") && !path.ends_with("auth/token.rs") {
            for needle in [
                "verify_access_token(",
                "AUTHORIZATION)",
                "\"authorization\")",
            ] {
                if production.contains(needle) {
                    other_readers.push(format!("{path}: `{needle}`"));
                }
            }
        }

        if !path.ends_with("/handlers.rs") {
            assert!(
                !production.contains(": Authenticated,") || path.ends_with("middleware/auth.rs"),
                "{path} takes the caller by value outside a handlers.rs, where the guard does \
                 not count it"
            );
            continue;
        }

        let mut offset = 0;
        while let Some(found) = production[offset..].find(": Authenticated,") {
            let at = offset + found;
            offset = at + 1;
            handlers += 1;

            let start = production[..at].rfind("async fn ").expect("a handler");
            let signature = &production[start..at];
            let name = signature["async fn ".len()..]
                .split('(')
                .next()
                .unwrap_or_default();
            let arguments: String = signature
                .split_once('(')
                .map(|(_, rest)| rest)
                .unwrap_or_default()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();

            // `State(state):State<AppState>,caller` and nothing else.
            let before_the_caller = arguments
                .rsplit_once(',')
                .map(|(before, _)| before)
                .unwrap_or_default();
            let only_the_state = before_the_caller.starts_with("State(")
                && before_the_caller.ends_with("):State<AppState>")
                && !before_the_caller.contains(',');
            if !only_the_state {
                out_of_order.push(format!("{path}: `{name}` takes `{arguments}`"));
            }
        }
    }

    assert_eq!(
        handlers,
        authenticated_operations().len(),
        "the handlers that take the caller, and the operations that declare `security: bearer`"
    );
    assert!(
        out_of_order.is_empty(),
        "an extractor that can refuse runs before the caller is judged:\n{}",
        out_of_order.join("\n")
    );
    assert!(
        other_readers.is_empty(),
        "a token is read somewhere the check does not reach:\n{}",
        other_readers.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 5. The pool (criterion 22)
// ---------------------------------------------------------------------------

/// The same application over the same database, through a pool of its own
/// size.
fn router_over_a_pool_of(app: &TestApp, connections: u32) -> (Router, sqlx::PgPool) {
    let pool = kelir_backend::db::create_pool_with_max_connections(
        &app.state.config.database_url,
        connections,
    )
    .expect("a pool builds");
    let state = AppState::with_mailer(
        pool.clone(),
        (*app.state.config).clone(),
        Mailer::captured(),
    );

    (kelir_backend::router::create_router(state), pool)
}

async fn document_type(app: &TestApp, token: &str, code: &str) -> Value {
    let made = app
        .post(
            "/api/v1/document-types",
            Some(token),
            json!({ "typeCode": code, "name": code }),
        )
        .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.data()["id"].clone()
}

/// **The check holds no connection across the handler.** Through a pool of
/// one connection, a request whose handler opens a transaction is served: the
/// check's connection is back before the handler asks for its own. If the
/// extractor kept it, the handler would wait for the only connection there
/// is, held by its own request, until the acquire timeout.
///
/// Seen red: the extractor acquiring a connection and keeping it for two
/// seconds past its return (ten requests took 20.6 s; the burst below was
/// red with it, on 500s from the acquire timeout). No test of the builder's
/// reddened.
#[tokio::test]
async fn through_a_pool_of_one_connection_a_transactional_route_is_served() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ONE").await;
    let (router, pool) = router_over_a_pool_of(&app, 1);

    let started = Instant::now();
    for index in 0..5 {
        let read = send(router.clone(), Method::GET, ME, Some(&token), None).await;
        assert_eq!(read.status, StatusCode::OK, "{}", read.text());

        let body = serde_json::to_vec(
            &json!({ "documentTypeId": type_id, "title": format!("Desk {index}") }),
        )
        .expect("a body");
        let written = send(
            router.clone(),
            Method::POST,
            "/api/v1/documents",
            Some(&token),
            Some(("application/json", Bytes::from(body))),
        )
        .await;
        assert_eq!(written.status, StatusCode::CREATED, "{}", written.text());
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "ten requests took {:?}: something waited for a connection",
        started.elapsed()
    );
    assert!(pool.size() <= 1);
    pool.close().await;
}

/// How a burst went.
struct Burst {
    statuses: BTreeMap<u16, usize>,
    elapsed: Duration,
    bodies_of_failures: Vec<String>,
}

/// `count` requests at once, alternating a transactional write and a read
/// with the live token, and one in four with the dead one.
async fn burst(router: &Router, count: usize, live: &str, dead: &str, type_id: &Value) -> Burst {
    let mut tasks = tokio::task::JoinSet::new();
    let started = Instant::now();

    for index in 0..count {
        let router = router.clone();
        let dead_caller = index % 4 == 3;
        let token = if dead_caller { dead } else { live }.to_owned();
        let body = serde_json::to_vec(
            &json!({ "documentTypeId": type_id, "title": format!("Burst {index}") }),
        )
        .expect("a body");

        tasks.spawn(async move {
            let answer = if index % 2 == 0 {
                send(
                    router,
                    Method::POST,
                    "/api/v1/documents",
                    Some(&token),
                    Some(("application/json", Bytes::from(body))),
                )
                .await
            } else {
                send(router, Method::GET, "/api/v1/documents", Some(&token), None).await
            };
            (index, dead_caller, answer)
        });
    }

    let mut statuses = BTreeMap::new();
    let mut bodies_of_failures = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        let (index, dead_caller, answer) = joined.expect("a request task");
        *statuses.entry(answer.status.as_u16()).or_default() += 1;

        let expected = if dead_caller {
            StatusCode::UNAUTHORIZED
        } else if index % 2 == 0 {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        if answer.status != expected {
            bodies_of_failures.push(format!(
                "request {index}: {} {}, expected {expected}",
                answer.status,
                answer.text()
            ));
        }
    }

    Burst {
        statuses,
        elapsed: started.elapsed(),
        bodies_of_failures,
    }
}

/// **A burst wider than the pool is served.** Every authenticated request
/// now takes a connection for the check and gives it back, then its handler
/// takes one. Sixty requests at once against five connections (the harness's
/// pool), ten (the production default) and one: every live caller's write
/// and read is answered, every deleted caller's is 401, none is a 500 from an
/// acquire timeout, and the deleted caller wrote nothing.
///
/// On a multi-threaded runtime, so the requests do run at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_wider_than_the_pool_is_served_and_the_deleted_caller_in_it_refused() {
    const REQUESTS: usize = 60;

    let app = multi_tenant_app().await;
    let live = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (tenant, _, dead) = tenant_with_administrator(&app, &live, "GONE").await;
    let type_id = document_type(&app, &live, "PR_BURST").await;
    let deleted = app
        .delete(&format!("{TENANTS}/{tenant}"), Some(&live))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let mut created = 0_i64;
    for connections in [common::TEST_POOL_MAX_CONNECTIONS, 10, 1] {
        let (router, pool) = router_over_a_pool_of(&app, connections);
        let outcome = burst(&router, REQUESTS, &live, &dead, &type_id).await;
        println!(
            "pool of {connections}: {REQUESTS} requests in {:?}: {:?}",
            outcome.elapsed, outcome.statuses
        );

        assert!(
            outcome.bodies_of_failures.is_empty(),
            "pool of {connections}: {:?}\n{}",
            outcome.statuses,
            outcome.bodies_of_failures.join("\n")
        );
        assert_eq!(outcome.statuses.get(&500), None, "pool of {connections}");
        assert!(
            outcome.elapsed < Duration::from_secs(30),
            "pool of {connections}: {REQUESTS} requests took {:?}",
            outcome.elapsed
        );
        assert!(pool.size() <= connections);
        pool.close().await;

        // 30 writes a burst, a quarter of all requests the dead caller's, and
        // of its 15, the even-numbered ones are none: 3 mod 4 is odd.
        created += 30;
        let documents: i64 = sqlx::query_scalar("SELECT count(*) FROM documents")
            .fetch_one(&app.pool)
            .await
            .expect("count documents");
        assert_eq!(documents, created, "pool of {connections}");
    }
}

/// **A pool with nothing to give is a 500, not a 401 and not a hang.** With
/// every connection held elsewhere the check cannot be asked; the request
/// waits for the acquire timeout and fails closed with the answer a browser
/// does not sign out on. A request with no token, a token that does not
/// verify and one that has expired are refused without waiting, because the
/// signature and the expiry are judged before the database is asked. When a
/// connection comes back the caller is served.
///
/// Seen red, and by no test of the builder's: a connection taken before
/// `verify_access_token`. Planned and not run: a failed lookup answered as a
/// pass; a failed lookup answered 401.
#[tokio::test]
async fn a_pool_with_no_connection_to_give_fails_closed_and_refuses_bad_tokens_without_waiting() {
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
    // A caller the route answers 403 with no statement of its own, so the
    // only statement a request of theirs runs is the check's.
    let token = app.sign_in("no.permissions", PASSWORD).await;
    let forbidden = get(&app, USERS, &token).await;
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.text()
    );

    let mut held = Vec::new();
    for _ in 0..common::TEST_POOL_MAX_CONNECTIONS {
        held.push(app.pool.acquire().await.expect("a connection"));
    }

    // Refused on the token alone, at once.
    let expired = expiring_at(&token, chrono::Utc::now().timestamp() - 60);
    for (what, bearer) in [
        ("no token", None),
        ("a token that does not verify", Some("not.a.token")),
        ("an expired token", Some(expired.as_str())),
    ] {
        let started = Instant::now();
        let answer = send(app.router(), Method::GET, USERS, bearer, None).await;
        assert!(answer.is_the_refusal(), "{what}: {}", answer.text());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{what} waited {:?} for a connection it has no use for",
            started.elapsed()
        );
    }

    // A good token waits for the check, and fails closed when it cannot run.
    let started = Instant::now();
    let answer = get(&app, USERS, &token).await;
    let waited = started.elapsed();
    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "after {waited:?}: {}",
        answer.text()
    );
    assert_eq!(answer.code().as_deref(), Some("INTERNAL_ERROR"));
    assert!(
        waited >= Duration::from_secs(4) && waited < Duration::from_secs(20),
        "the acquire timeout is five seconds; this took {waited:?}"
    );

    // One connection back: the caller is known again.
    held.pop();
    let answer = get(&app, USERS, &token).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{}", answer.text());
}

// ---------------------------------------------------------------------------
// 6. The expiry second
// ---------------------------------------------------------------------------

/// **A leeway of one second cannot pass this.** A token whose `exp` is one
/// second before now is refused under no leeway always, and served under a
/// leeway of one unless a second ticks between minting and verifying. So the
/// sample is taken again whenever the clock's second changed while it was
/// being taken: within one second, `exp = now - 1` is refused only if the
/// leeway is zero.
///
/// This is a sample discarded for its precondition, not a retry of a failed
/// assertion: nothing is asserted on a sample whose second moved.
///
/// Planned and not run: `validation.leeway = 1`, and the leeway left at the
/// library's sixty, each over twenty runs.
#[tokio::test]
async fn within_one_second_a_token_that_expired_the_second_before_is_refused() {
    let app = TestApp::spawn().await;
    user_reading_users(&app, "on.time").await;
    let token = app.sign_in("on.time", PASSWORD).await;

    for _ in 0..50 {
        let now = chrono::Utc::now().timestamp();
        let just_expired = expiring_at(&token, now - 1);
        let not_yet = expiring_at(&token, now + 30);

        let refused = get(&app, USERS, &just_expired).await;
        let unit = verify_access_token(common::JWT_SECRET, &just_expired);
        let control = get(&app, USERS, &not_yet).await;

        if chrono::Utc::now().timestamp() != now {
            // A second ticked: the sample does not show what it was for.
            continue;
        }

        assert_eq!(control.status, StatusCode::OK, "{}", control.text());
        assert!(
            refused.is_the_refusal(),
            "a token whose `exp` was the second before this one was served: {}",
            refused.text()
        );
        assert!(unit.is_err(), "and verified");
        return;
    }

    panic!("fifty samples each straddled a second; the clock is not one this test can read");
}

// ---------------------------------------------------------------------------
// 7. Who issues a token
// ---------------------------------------------------------------------------

/// **Every token the application issues names a pair the check accepts.**
/// One function issues an access token (`auth::service::issue_session`),
/// reached by sign-in and by refresh. The token a refresh issues is served; a
/// token issued by a refresh the moment before its user is deleted is
/// refused.
#[tokio::test]
async fn a_token_issued_by_a_refresh_is_served_and_refused_once_its_user_is_deleted() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let user = user_reading_users(&app, "comes.and.goes").await;

    let signed_in = app
        .post(
            "/api/v1/auth/login",
            None,
            json!({ "username": "comes.and.goes", "password": PASSWORD }),
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);
    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": signed_in.data()["refreshToken"] }),
        )
        .await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    let refreshed = rotated.data()["accessToken"]
        .as_str()
        .expect("an access token")
        .to_owned();

    for uri in [ME, USERS] {
        let answer = get(&app, uri, &refreshed).await;
        assert_eq!(answer.status, StatusCode::OK, "{uri}: {}", answer.text());
    }

    let deleted = app.delete(&format!("{USERS}/{user}"), Some(&admin)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    for uri in [ME, USERS] {
        let answer = get(&app, uri, &refreshed).await;
        assert!(answer.is_the_refusal(), "{uri}: {}", answer.text());
    }
}

/// **A deployment whose one tenant is not `SYSTEM`.** With
/// `KELIR_DEFAULT_TENANT_CODE` naming another tenant and `multi_tenant` off,
/// sign-in resolves to that tenant with no code sent. The check reads the
/// tenant the token names, not the seeded one: that tenant's administrator is
/// served on every route tried, and is refused when that tenant is deleted,
/// while the seeded `SYSTEM` tenant stays live throughout.
///
/// The tenant is made through the route first, because the bootstrap places
/// an administrator only in a database with no user and a tenant made by
/// hand has no `ROLE-ADMIN` to give one.
#[tokio::test]
async fn a_single_tenant_deployment_whose_tenant_is_not_system_is_served_and_then_refused() {
    let app = multi_tenant_app().await;
    let system = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let (lone, administrator, _) = tenant_with_administrator(&app, &system, "LONE-CO").await;

    let mut config = (*app.state.config).clone();
    config.default_tenant_code = "LONE-CO".to_owned();
    config.multi_tenant = false;
    let router = kelir_backend::router::create_router(AppState::with_mailer(
        app.pool.clone(),
        config,
        Mailer::captured(),
    ));

    // No tenant code: the deployment has one tenant, and it is this one.
    let body = serde_json::to_vec(&json!({ "username": "lone-co.admin", "password": PASSWORD }))
        .expect("a body");
    let signed_in = send(
        router.clone(),
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(("application/json", Bytes::from(body))),
    )
    .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.text());
    let session: Value = serde_json::from_slice(&signed_in.bytes).expect("a session");
    let token = session["data"]["accessToken"]
        .as_str()
        .expect("an access token")
        .to_owned();

    let claims = verify_access_token(common::JWT_SECRET, &token).expect("the token verifies");
    assert_eq!(claims.tenant_id, lone);
    assert_eq!(claims.sub, administrator);

    let routes = [
        ME,
        USERS,
        "/api/v1/identity/roles",
        "/api/v1/documents",
        "/api/v1/tasks",
    ];
    for uri in routes {
        let answer = send(router.clone(), Method::GET, uri, Some(&token), None).await;
        assert_eq!(answer.status, StatusCode::OK, "{uri}: {}", answer.text());
    }

    execute(
        &app,
        "UPDATE tenants SET deleted_at = now() WHERE id = $1",
        lone,
    )
    .await;

    for uri in routes {
        let answer = send(router.clone(), Method::GET, uri, Some(&token), None).await;
        assert!(answer.is_the_refusal(), "{uri}: {}", answer.text());
    }

    // The seeded tenant is somebody else, and is served.
    let answer = get(&app, USERS, &system).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text());
}
