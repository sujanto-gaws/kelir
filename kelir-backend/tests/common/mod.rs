//! Integration test harness (coding standard §2.9).
//!
//! Provides a [`TestApp`]: a private, freshly migrated PostgreSQL database plus
//! the real Axum router built over the real [`AppState`], driven in-process
//! through `tower::ServiceExt::oneshot` so no port is bound and nothing is
//! mocked. Repository and service behaviour is therefore exercised against the
//! actual DDL in `kelir-backend/migrations/`, not against a stand-in.
//!
//! # Isolation: one database per test
//!
//! Cargo runs tests in parallel by default, so every test needs its own data.
//! Three options were available:
//!
//! * **Transactional rollback** — impossible here without changing application
//!   code. [`AppState`] holds a `PgPool`, and the services under test open their
//!   own transactions (`state.pool.begin()`); a test cannot hand them an
//!   enclosing transaction to roll back. Forcing it would mean threading a
//!   connection through `AppState`, which would change production code to suit
//!   the tests and would make it impossible to test transactionality — the
//!   thing this suite most needs to be able to test (outbox atomicity, hook
//!   `REJECT` rollback) once those exist.
//! * **Schema per test** — fast, but `search_path` is per connection, so every
//!   pooled connection would need an `after_connect` hook, and any future
//!   migration that names `public` explicitly would silently escape the
//!   sandbox. It also cannot isolate anything database-scoped.
//! * **Database per test** — chosen. Complete isolation including sequences,
//!   advisory locks and `_sqlx_migrations`; the only cost is running four small
//!   migration files per test, which measures in the tens of milliseconds. It
//!   is also the only option under which concurrency tests (parallel document
//!   numbering, optimistic task races) mean anything.
//!
//! Each database is named `kelir_test_<uuid>` and dropped in [`Drop`], which
//! runs on assertion failure as well as on success. A killed process (Ctrl-C,
//! CI cancellation) can still leave one behind; they are harmless and are
//! removed by dropping the server's databases matching that prefix.
//!
//! # Harness failure is not assertion failure
//!
//! Anything the harness itself needs — reaching PostgreSQL, creating the
//! database, migrating, signing in a fixture user — fails through
//! [`harness_failure`], which prints a banner naming the step and the fix. A
//! developer with no database running gets that, not a puzzling assertion.

// Each integration test binary includes this module and uses only the part it
// needs, so "unused here" is normal and is not dead code.
#![allow(
    dead_code,
    reason = "shared harness; each test binary uses a different subset"
)]

pub mod fixtures;

use std::env;
use std::net::SocketAddr;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

use kelir_backend::config::{AppConfig, AppEnv, BootstrapAdmin};
use kelir_backend::mail::{Mail, Mailer};
use kelir_backend::state::AppState;
use kelir_backend::{db, modules, router};

/// Username of the administrator every [`TestApp`] starts with.
pub const ADMIN_USERNAME: &str = "admin.kelir";

/// Its password. Above `MIN_PASSWORD_LENGTH` and not one of the placeholder
/// secrets `AppConfig` refuses outside development.
pub const ADMIN_PASSWORD: &str = "bootstrap-administrator-password";

/// Signing secret for the harness. Fixed so a token minted in one place in a
/// test verifies in another.
pub const JWT_SECRET: &str = "integration-test-signing-secret";

/// Connections one test instance may hold.
///
/// Each test spawns its own application and so its own pool, and they run
/// concurrently against one PostgreSQL. At the production ceiling of ten, a
/// runner wide enough for twenty tests asks for two hundred connections from a
/// server that allows a hundred by default, and the failure surfaces as an
/// acquire timeout in an unrelated test. Five is above what any single test
/// here uses at once and low enough to keep the arithmetic safe.
pub const TEST_POOL_MAX_CONNECTIONS: u32 = 5;

/// The socket peer every request appears to come from.
///
/// `oneshot` binds no socket, so there is no real peer to report; the router
/// still needs one, because that address is the caller's identity to the
/// authentication rate limiter and to the audit trail. TEST-NET-1
/// (RFC 5737) — never routable, so it cannot be mistaken for a real client.
pub const TEST_PEER: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 1)),
    41234,
);

/// A running application over a private database.
pub struct TestApp {
    /// The pool the router uses. Exposed so tests can assert against the
    /// database directly — what the API reports and what was actually stored
    /// are different claims.
    pub pool: PgPool,
    pub state: AppState,
    /// The database this instance owns, for diagnostics.
    pub database_name: String,
    /// Connection string to the *server*, used to create and drop the above.
    maintenance_url: String,
}

impl TestApp {
    /// Provisions a database, applies every migration, runs the first-run
    /// bootstrap and returns the application over it.
    ///
    /// **The bootstrap is used, not bypassed.** `modules::auth::bootstrap`
    /// is the only path by which a real deployment acquires its first
    /// administrator, so exercising it here means the administrator these tests
    /// authorise with is the same account production would have, and a
    /// regression in the bootstrap (wrong role, missing grant, failing insert)
    /// surfaces as a test failure rather than a live incident.
    ///
    /// The credentials reach it through a constructed [`AppConfig`] rather than
    /// through `KELIR_BOOTSTRAP_ADMIN_*` in the process environment: environment
    /// variables are process-global, and tests running in parallel would race
    /// on them. `AppConfig::from_env`'s own parsing of those variables is
    /// covered by its unit tests.
    pub async fn spawn() -> Self {
        Self::spawn_with(|_| {}).await
    }

    /// A [`TestApp::spawn`] whose configuration the caller adjusts first.
    ///
    /// Exists for one setting: `multi_tenant`. Tenancy is a deployment
    /// property, so the behaviour that depends on it — sign-in requiring a
    /// tenant code, and a second tenant's administrator being able to sign in
    /// at all — cannot be reached from a fixture. Everything else about the
    /// instance is unchanged, including the bootstrap, so a test that adjusts
    /// nothing is exactly `spawn`.
    ///
    /// The adjustment runs *before* the bootstrap, which is what makes it
    /// meaningful: `KELIR_DEFAULT_TENANT_CODE` decides which tenant the first
    /// administrator lands in, and that is the tenant every tenant-administration
    /// check compares against.
    pub async fn spawn_with(adjust: impl FnOnce(&mut AppConfig)) -> Self {
        Self::spawn_with_mailer(adjust, Mailer::captured()).await
    }

    /// A [`TestApp::spawn_with`] whose mailer the caller supplies.
    ///
    /// Exists for one mailer: a [`Mailer::captured_taking`] with a delay, which
    /// is the slow transport #202 needed and could not inject. Everything else
    /// wants the default captured one.
    pub async fn spawn_with_mailer(adjust: impl FnOnce(&mut AppConfig), mailer: Mailer) -> Self {
        let maintenance_url = server_url();
        let database_name = format!("kelir_test_{}", Uuid::now_v7().simple());

        create_database(&maintenance_url, &database_name).await;

        let database_url = with_database(&maintenance_url, &database_name);
        let pool = db::create_pool_with_max_connections(&database_url, TEST_POOL_MAX_CONNECTIONS)
            .unwrap_or_else(|error| {
                harness_failure(
                    "build a connection pool for the test database",
                    &error.to_string(),
                    &database_url,
                )
            });

        // The real migration runner over the real migration directory: whatever
        // a developer's database has drifted to is irrelevant, this is the
        // committed schema.
        db::run_migrations(&pool).await.unwrap_or_else(|error| {
            harness_failure(
                "apply kelir-backend/migrations to the test database",
                &error.to_string(),
                &database_url,
            )
        });

        let mut config = test_config(&database_url);
        adjust(&mut config);

        modules::auth::bootstrap::ensure_administrator(&pool, &config)
            .await
            .unwrap_or_else(|error| {
                harness_failure(
                    "create the first-run administrator",
                    &error.to_string(),
                    &database_url,
                )
            });

        // A captured mailer rather than a real one: the reset flow's tests
        // read the link out of the delivered message, which is how a person
        // gets it. Fetching the token from `password_reset_tokens` instead
        // would prove the row was written and nothing about whether anybody
        // could have used it.
        let state = AppState::with_mailer(pool.clone(), config, mailer);

        Self {
            pool,
            state,
            database_name,
            maintenance_url,
        }
    }

    /// The application router. Rebuilt per call because `oneshot` consumes the
    /// service; the state behind it is shared, so this is not a fresh app.
    ///
    /// A caller driving this directly must put a [`ConnectInfo`] in the
    /// request's extensions, as [`TestApp::send`] does — see [`TEST_PEER`].
    pub fn router(&self) -> Router {
        router::create_router(self.state.clone())
    }

    /// Sends a request through the whole stack: routing, CORS layer,
    /// extractors, handler, service, repository, database.
    pub async fn send(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> TestResponse {
        self.send_from(TEST_PEER, method, uri, token, body).await
    }

    /// As [`TestApp::send`], from a chosen peer address.
    ///
    /// The address is what the authentication rate limiter keys on, so a test
    /// that needs two callers distinguishes them here rather than by sending a
    /// header — `X-Forwarded-For` is deliberately ignored unless the deployment
    /// says how many proxies wrote it (`middleware::client_address`).
    pub async fn send_from(
        &self,
        peer: SocketAddr,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let mut request = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap_or_else(|error| harness_failure("build a test request", &error.to_string(), uri));

        // What `axum::serve` supplies through `into_make_service_with_connect_info`
        // and `oneshot` does not. Handlers on `/auth` resolve the caller's
        // address from it and fail closed without it, deliberately: there is no
        // safe fallback, and inventing a shared one is the defect that made the
        // login rate limit steerable in the first place.
        request.extensions_mut().insert(ConnectInfo(peer));

        let response = self
            .router()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| harness_failure("drive the router", &error.to_string(), uri));

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| {
                harness_failure("read a response body", &error.to_string(), uri)
            });

        TestResponse {
            status,
            // A 204 has no body, and an unparseable body is worth seeing rather
            // than panicking over: the status is usually the assertion.
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    /// A `multipart/form-data` POST, with one file part and an optional
    /// `description`.
    ///
    /// **Built by hand rather than with a crate.** The body is three lines of
    /// formatting; a dependency that exists only to write them is a dependency
    /// to keep in step, and the point of this helper is that the bytes on the
    /// wire are the ones the test wrote. The boundary is fixed for the same
    /// reason — nothing here is guessing what the parser will see.
    pub async fn post_multipart(
        &self,
        uri: &str,
        token: Option<&str>,
        file_name: &str,
        content_type: &str,
        content: &[u8],
        description: Option<&str>,
    ) -> TestResponse {
        const BOUNDARY: &str = "kelirtestboundary";

        let mut body: Vec<u8> = Vec::new();

        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");

        if let Some(description) = description {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(b"Content-Disposition: form-data; name=\"description\"\r\n\r\n");
            body.extend_from_slice(description.as_bytes());
            body.extend_from_slice(b"\r\n");
        }

        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

        self.send_raw(
            uri,
            token,
            &format!("multipart/form-data; boundary={BOUNDARY}"),
            body,
        )
        .await
    }

    /// A `multipart/form-data` POST carrying a `categoryId` part beside the
    /// file — the shape the upload form takes once a person has filed what
    /// they are attaching ([#254](https://github.com/sujanto-gaws/kelir/issues/254)
    /// AC1).
    ///
    /// A separate method rather than a seventh parameter on
    /// [`Self::post_multipart`]: three test files call that one and none of
    /// them is about categories.
    pub async fn post_multipart_with_category(
        &self,
        uri: &str,
        token: Option<&str>,
        file_name: &str,
        content_type: &str,
        content: &[u8],
        category_id: &str,
    ) -> TestResponse {
        const BOUNDARY: &str = "kelirtestboundary";

        let mut body: Vec<u8> = Vec::new();

        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");

        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"categoryId\"\r\n\r\n");
        body.extend_from_slice(category_id.as_bytes());
        body.extend_from_slice(b"\r\n");

        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

        self.send_raw(
            uri,
            token,
            &format!("multipart/form-data; boundary={BOUNDARY}"),
            body,
        )
        .await
    }

    /// A `multipart/form-data` POST with **no** file part — the shape a form
    /// takes when its file input was left empty.
    pub async fn post_multipart_without_file(
        &self,
        uri: &str,
        token: Option<&str>,
        description: &str,
    ) -> TestResponse {
        const BOUNDARY: &str = "kelirtestboundary";

        let mut body: Vec<u8> = Vec::new();

        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"description\"\r\n\r\n");
        body.extend_from_slice(description.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

        self.send_raw(
            uri,
            token,
            &format!("multipart/form-data; boundary={BOUNDARY}"),
            body,
        )
        .await
    }

    /// A GET whose response is **not** JSON.
    ///
    /// Every other helper here parses the body into a `Value`, which is right
    /// for an API that answers in one envelope — and wrong for the one route
    /// that answers with a file. This returns the bytes and the headers, because
    /// for a download those *are* the response: the content type and the
    /// disposition are the security-relevant half.
    pub async fn get_raw(&self, uri: &str, token: Option<&str>) -> RawResponse {
        let mut builder = Request::builder().method(Method::GET).uri(uri);

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let mut request = builder.body(Body::empty()).unwrap_or_else(|error| {
            harness_failure("build a test request", &error.to_string(), uri)
        });

        request.extensions_mut().insert(ConnectInfo(TEST_PEER));

        let response = self
            .router()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| harness_failure("drive the router", &error.to_string(), uri));

        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| {
                harness_failure("read a response body", &error.to_string(), uri)
            })
            .to_vec();

        RawResponse {
            status,
            headers,
            bytes,
        }
    }

    /// A POST of a body this harness does not build for you.
    async fn send_raw(
        &self,
        uri: &str,
        token: Option<&str>,
        content_type: &str,
        body: Vec<u8>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(Method::POST).uri(uri);

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let mut request = builder
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap_or_else(|error| {
                harness_failure("build a test request", &error.to_string(), uri)
            });

        request.extensions_mut().insert(ConnectInfo(TEST_PEER));

        let response = self
            .router()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| harness_failure("drive the router", &error.to_string(), uri));

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| {
                harness_failure("read a response body", &error.to_string(), uri)
            });

        TestResponse {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    /// A request carrying headers this harness does not normally send.
    ///
    /// Exists for one class of test: proving that a header a **caller** wrote
    /// does not reach somewhere it should not. `X-Forwarded-For` is the case —
    /// `middleware::client_address` ignores it entirely unless the deployment
    /// says how many proxies wrote it, and the only way to assert that is to
    /// send one.
    pub async fn send_with_headers(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }

        let mut request = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap_or_else(|error| harness_failure("build a test request", &error.to_string(), uri));

        request.extensions_mut().insert(ConnectInfo(TEST_PEER));

        let response = self
            .router()
            .oneshot(request)
            .await
            .unwrap_or_else(|error| harness_failure("drive the router", &error.to_string(), uri));

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_else(|error| {
                harness_failure("read a response body", &error.to_string(), uri)
            });

        TestResponse {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    pub async fn get(&self, uri: &str, token: Option<&str>) -> TestResponse {
        self.send(Method::GET, uri, token, None).await
    }

    pub async fn post(&self, uri: &str, token: Option<&str>, body: Value) -> TestResponse {
        self.send(Method::POST, uri, token, Some(body)).await
    }

    pub async fn put(&self, uri: &str, token: Option<&str>, body: Value) -> TestResponse {
        self.send(Method::PUT, uri, token, Some(body)).await
    }

    pub async fn delete(&self, uri: &str, token: Option<&str>) -> TestResponse {
        self.send(Method::DELETE, uri, token, None).await
    }

    /// The messages delivered once `count` of them have arrived.
    ///
    /// **Why waiting is now part of reading.** The reset flow hands its send to
    /// the runtime rather than awaiting it (#202), so a message is delivered
    /// shortly *after* the response the test just read. Reading
    /// `captured_messages()` straight away would be a race whose green result
    /// depends on the scheduler.
    ///
    /// Panics as a harness failure if the count does not arrive, because a test
    /// that hangs here is a broken send path rather than a slow one.
    pub async fn mail_delivered(&self, count: usize) -> Vec<Mail> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

        loop {
            let delivered = self.state.mailer.captured_messages();

            if delivered.len() >= count {
                return delivered;
            }

            if std::time::Instant::now() >= deadline {
                harness_failure(
                    &format!("wait for {count} delivered message(s)"),
                    &format!("{} arrived", delivered.len()),
                    &self.database_name,
                );
            }

            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    }

    /// Whatever has been delivered once any detached send has had its turn.
    ///
    /// For the assertions that no mail was sent. There is no arrival to wait
    /// for, so this yields to the runtime and gives a queued send a window
    /// instead — small, because it is paid by every test that calls it, and
    /// enough for an in-process captured send whose only cost is a lock.
    pub async fn mail_settled(&self) -> Vec<Mail> {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        self.state.mailer.captured_messages()
    }

    /// Signs in naming a tenant, as a multi-tenant deployment's client does.
    ///
    /// Separate from [`TestApp::sign_in`] rather than an `Option` parameter on
    /// it, because the two are different claims: `sign_in` asserts that the
    /// unchanged single-tenant login contract still works, and this asserts
    /// that the tenant-code half does. Every existing test uses the first, and
    /// should keep proving that.
    pub async fn sign_in_to(&self, tenant_code: &str, username: &str, password: &str) -> String {
        let response = self
            .post(
                "/api/v1/auth/login",
                None,
                serde_json::json!({
                    "username": username,
                    "password": password,
                    "tenantCode": tenant_code,
                }),
            )
            .await;

        if response.status != StatusCode::OK {
            sign_in_failure(username, &response);
        }

        response.body["data"]["accessToken"]
            .as_str()
            .unwrap_or_else(|| {
                harness_failure(
                    "read accessToken from the sign-in response",
                    &response.body.to_string(),
                    &self.database_name,
                )
            })
            .to_owned()
    }

    /// Signs in through `POST /api/v1/auth/login` and returns the access token.
    ///
    /// **A failure here is not a harness failure.** Every test in this suite
    /// gets its token this way, so a broken login path breaks all of them at
    /// this line — and the harness banner's claim that "nothing about the code
    /// under test has been proven or disproven" would then be flatly false. It
    /// panics through [`sign_in_failure`] instead, which says so (#59).
    pub async fn sign_in(&self, username: &str, password: &str) -> String {
        let response = self
            .post(
                "/api/v1/auth/login",
                None,
                serde_json::json!({ "username": username, "password": password }),
            )
            .await;

        if response.status != StatusCode::OK {
            sign_in_failure(username, &response);
        }

        response.body["data"]["accessToken"]
            .as_str()
            .unwrap_or_else(|| {
                harness_failure(
                    "read accessToken from the sign-in response",
                    &response.body.to_string(),
                    &self.database_name,
                )
            })
            .to_owned()
    }

    /// An access token for the bootstrap administrator, which holds every
    /// permission in the catalogue.
    pub async fn administrator_token(&self) -> String {
        self.sign_in(ADMIN_USERNAME, ADMIN_PASSWORD).await
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let maintenance_url = self.maintenance_url.clone();
        let database_name = self.database_name.clone();

        // Drop is synchronous and may run while unwinding from a failed
        // assertion, so teardown gets its own thread with its own runtime, and
        // is joined so the database is gone before the process moves on.
        //
        // The application pool is deliberately *not* closed here. Its sockets
        // are registered with the test's own current-thread runtime, and that
        // runtime's only thread is the one blocked on this join — awaiting
        // anything on those connections from here would deadlock. `WITH (FORCE)`
        // makes that unnecessary: the server terminates the remaining backends,
        // and the pool's connections are discarded when this struct's fields
        // drop a moment later.
        let cleanup = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test cleanup runtime builds");

            runtime.block_on(async move {
                let Ok(mut connection) = PgConnection::connect(&maintenance_url).await else {
                    return Err("could not reach the server to drop the database".to_owned());
                };

                let forced = sqlx::query(&format!(
                    r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
                ))
                .execute(&mut connection)
                .await;

                // WITH (FORCE) needs PostgreSQL 13 or later. Fall back so an
                // older developer install fails on its own merits rather than on
                // a syntax error during teardown.
                let outcome = match forced {
                    Ok(_) => Ok(()),
                    Err(_) => sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{database_name}""#))
                        .execute(&mut connection)
                        .await
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                };

                let _ = connection.close().await;
                outcome
            })
        });

        let outcome = cleanup.join();

        // Never panic from Drop: during unwinding that aborts the process and
        // the real assertion failure is never reported.
        let message = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(_) => Some("the cleanup thread panicked".to_owned()),
        };

        if let Some(message) = message {
            eprintln!(
                "warning: test database {} was not dropped ({message}); \
                 remove it manually",
                self.database_name
            );
        }
    }
}

/// A response, decoded far enough to assert on.
pub struct TestResponse {
    pub status: StatusCode,
    pub body: Value,
}

impl TestResponse {
    /// The stable machine-readable code from the error envelope
    /// (naming convention §5), if this is an error response.
    pub fn error_code(&self) -> Option<&str> {
        self.body["error"]["code"].as_str()
    }

    /// The `data` member of a success envelope.
    pub fn data(&self) -> &Value {
        &self.body["data"]
    }
}

/// A response this harness did not parse — the file routes answer with bytes.
pub struct RawResponse {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub bytes: Vec<u8>,
}

impl RawResponse {
    /// One header as a `String`, or `None` when it is absent or not text.
    pub fn header(&self, name: header::HeaderName) -> Option<String> {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    }
}

// ---------------------------------------------------------------------------
// Provisioning
// ---------------------------------------------------------------------------

/// Configuration for a test instance.
///
/// Built here rather than reusing `AppConfig::test_default()`, which is
/// `#[cfg(test)]` and so exists only inside the library's own unit tests.
fn test_config(database_url: &str) -> AppConfig {
    AppConfig {
        app_name: "Kelir".to_owned(),
        app_env: AppEnv::Test,
        bind_address: "127.0.0.1:0".to_owned(),
        database_url: database_url.to_owned(),
        jwt_secret: JWT_SECRET.to_owned(),
        storage_driver: "local".to_owned(),
        // **Real object storage, not a double.** This harness's own header says
        // nothing is mocked, and it already requires a live PostgreSQL for the
        // same reason: a repository verified against a stand-in is verified
        // against the stand-in. `KELIR_STORAGE_ENDPOINT` points at the MinIO the
        // compose stack runs and CI starts beside `postgres`; the bucket is
        // provisioned by whoever started it, because this process holds
        // credentials that can put and get objects and not credentials that can
        // create buckets.
        storage_endpoint: env::var("KELIR_STORAGE_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:9000".to_owned()),
        storage_bucket: env::var("KELIR_STORAGE_BUCKET")
            .unwrap_or_else(|_| "kelir-test".to_owned()),
        storage_access_key: env::var("KELIR_STORAGE_ACCESS_KEY")
            .unwrap_or_else(|_| "minioadmin".to_owned()),
        storage_secret_key: env::var("KELIR_STORAGE_SECRET_KEY")
            .unwrap_or_else(|_| "minioadmin".to_owned()),
        storage_region: env::var("KELIR_STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_owned()),
        // **A limit the tests can exceed without sending 25 MB.** The production
        // default is 25 MB; a test that had to reach it would spend a second of
        // wall clock proving a number. The refusal is the same one either way —
        // the layer, not the size.
        storage_max_upload_bytes: env::var("KELIR_STORAGE_MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(4096),
        storage_allowed_mime_types: vec!["application/pdf".to_owned(), "image/png".to_owned()],
        // **The harness does not run the scan worker**, so these address a
        // scanner that is not there — which is the point for every test but the
        // one that starts its own listener and passes the address in. An
        // attachment stays `PENDING` unless a test says otherwise, which is
        // exactly what a deployment with no scanner does.
        clamav_host: env::var("KELIR_CLAMAV_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned()),
        clamav_port: env::var("KELIR_CLAMAV_PORT")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(3310),
        clamav_poll_seconds: 1,
        // The harness uses a captured mailer, so this is never dialled — but a
        // host is left set deliberately: an empty one would exercise the
        // no-SMTP path rather than the one a deployment runs.
        smtp_host: "localhost".to_owned(),
        smtp_port: 1025,
        mail_from: "no-reply@kelir.test".to_owned(),
        frontend_url: "http://localhost:5173".to_owned(),
        multi_tenant: false,
        default_tenant_code: "SYSTEM".to_owned(),
        // Nothing sits in front of the router here, so the peer address below is
        // the whole truth and X-Forwarded-For must not be read. This is also the
        // production default (see `middleware::client_address`).
        trusted_proxy_hops: 0,
        bootstrap_admin: Some(BootstrapAdmin {
            username: ADMIN_USERNAME.to_owned(),
            email: "admin@kelir.test".to_owned(),
            password: ADMIN_PASSWORD.to_owned(),
        }),
    }
}

/// Where the PostgreSQL server is.
///
/// `DATABASE_URL` first: it is what CI sets and what `sqlx::query!` verifies
/// against at compile time, so the database the tests run on is the database the
/// queries were checked against. `KELIR_DATABASE_URL` is accepted as a fallback
/// because that is what a developer running the app already has set.
fn server_url() -> String {
    std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("KELIR_DATABASE_URL"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            harness_failure(
                "find a PostgreSQL to run against",
                "neither DATABASE_URL nor KELIR_DATABASE_URL is set",
                "<unset>",
            )
        })
}

async fn create_database(maintenance_url: &str, database_name: &str) {
    // Eager, unlike the application pool: a database that is not reachable must
    // be reported here, as a harness failure, rather than surfacing later as a
    // request that mysteriously returns 500.
    let mut connection = PgConnection::connect(maintenance_url)
        .await
        .unwrap_or_else(|error| {
            harness_failure("connect to PostgreSQL", &error.to_string(), maintenance_url)
        });

    // The name is a generated UUID, so it cannot carry an injection; quoted
    // regardless, because CREATE DATABASE takes no bind parameters and the
    // habit is worth keeping.
    sqlx::query(&format!(r#"CREATE DATABASE "{database_name}""#))
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| {
            harness_failure(
                &format!("create the test database {database_name}"),
                &error.to_string(),
                maintenance_url,
            )
        });

    let _ = connection.close().await;
}

/// Replaces the database in a connection string, preserving query parameters.
///
/// Public so `tests/harness.rs` can verify it directly. A `#[cfg(test)] mod`
/// here would run in every test binary that includes this module, reporting the
/// same tests two or three times.
pub fn with_database(url: &str, database_name: &str) -> String {
    let (before_query, query) = match url.split_once('?') {
        Some((base, query)) => (base, format!("?{query}")),
        None => (url, String::new()),
    };

    // Skip the scheme so the "//" of "postgres://" is not mistaken for the
    // separator before the database name.
    let authority_start = before_query.find("://").map_or(0, |index| index + 3);
    let prefix = match before_query[authority_start..].find('/') {
        Some(index) => &before_query[..authority_start + index],
        None => before_query,
    };

    format!("{prefix}/{database_name}{query}")
}

/// Hides the password between `://` and `@` in a connection string.
pub fn redact(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let authority_start = scheme_end + 3;
    let Some(at) = url[authority_start..].find('@') else {
        return url.to_owned();
    };
    let credentials = &url[authority_start..authority_start + at];

    match credentials.split_once(':') {
        Some((user, _)) => format!(
            "{}{user}:***{}",
            &url[..authority_start],
            &url[authority_start + at..]
        ),
        None => url.to_owned(),
    }
}

/// Aborts a test because the harness could not do its job.
///
/// Deliberately loud and deliberately distinct from an assertion: the failure
/// is in the setup, so nothing about the code under test has been proven either
/// way, and reading it as a product defect would be wrong.
/// A fixture sign-in was refused.
///
/// Deliberately not a [`harness_failure`]: the login endpoint is application
/// code, and it just answered wrongly. Reporting that as a setup problem would
/// send a reader to check their PostgreSQL while the auth path is broken.
fn sign_in_failure(username: &str, response: &TestResponse) -> ! {
    panic!(
        "
         ==================== SIGN-IN REFUSED ====================
         This is a real failure of POST /api/v1/auth/login, not a setup
         problem. Every test takes its token through this call, so expect the
         rest of the suite to fail here too.
         
         username: {username}
         status:   {}
         body:     {}
         
         If the credentials are right, look at the authentication path before
         looking at the harness.
         =========================================================
",
        response.status, response.body
    )
}

fn harness_failure(step: &str, cause: &str, context: &str) -> ! {
    panic!(
        "\n\
         ==================== INTEGRATION TEST HARNESS FAILURE ====================\n\
         This is a setup failure, NOT a failed assertion. Nothing about the code\n\
         under test has been proven or disproven by this result.\n\
         \n\
         could not: {step}\n\
         cause:     {cause}\n\
         context:   {}\n\
         \n\
         The harness needs a reachable PostgreSQL and permission to create\n\
         databases on it. From the repository root:\n\
         \n\
           docker compose -f deploy/docker/docker-compose.yml up -d postgres\n\
           export DATABASE_URL=postgres://postgres:postgres@localhost:5432/kelir\n\
         \n\
         A natively installed PostgreSQL also listens on 5432; if sign-in fails\n\
         rather than the connection, publish the container on a free port and\n\
         point DATABASE_URL at that (README, 'Port conflicts').\n\
         ==========================================================================\n",
        redact(context)
    )
}
