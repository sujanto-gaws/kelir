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

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

use kelir_backend::config::{AppConfig, AppEnv, BootstrapAdmin};
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
        let maintenance_url = server_url();
        let database_name = format!("kelir_test_{}", Uuid::now_v7().simple());

        create_database(&maintenance_url, &database_name).await;

        let database_url = with_database(&maintenance_url, &database_name);
        let pool = db::create_pool(&database_url).unwrap_or_else(|error| {
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

        let config = test_config(&database_url);

        modules::auth::bootstrap::ensure_administrator(&pool, &config)
            .await
            .unwrap_or_else(|error| {
                harness_failure(
                    "create the first-run administrator",
                    &error.to_string(),
                    &database_url,
                )
            });

        let state = AppState::new(pool.clone(), config);

        Self {
            pool,
            state,
            database_name,
            maintenance_url,
        }
    }

    /// The application router. Rebuilt per call because `oneshot` consumes the
    /// service; the state behind it is shared, so this is not a fresh app.
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
        let mut builder = Request::builder().method(method).uri(uri);

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let request = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap_or_else(|error| harness_failure("build a test request", &error.to_string(), uri));

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

    /// Signs in through `POST /api/v1/auth/login` and returns the access token.
    ///
    /// A fixture step, so a failure here is a harness failure: the test that
    /// called it has not reached its assertion yet.
    pub async fn sign_in(&self, username: &str, password: &str) -> String {
        let response = self
            .post(
                "/api/v1/auth/login",
                None,
                serde_json::json!({ "username": username, "password": password }),
            )
            .await;

        if response.status != StatusCode::OK {
            harness_failure(
                &format!("sign in the fixture user '{username}'"),
                &format!(
                    "expected 200, got {} with body {}",
                    response.status, response.body
                ),
                &self.database_name,
            );
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
        smtp_host: "localhost".to_owned(),
        frontend_url: "http://localhost:5173".to_owned(),
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
