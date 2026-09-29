//! An administrator's test call to an integration endpoint (FR-INT-002, #547;
//! ADR-0043), through the API, against a mock system on this machine.
//!
//! # The test seam
//!
//! The mock listens on `127.0.0.1`, and loopback is an address the egress
//! guard always refuses. So the tests that need an answer start the app with
//! `AppConfig::integration_allow_loopback = true` through
//! [`TestApp::spawn_with`] — a field `AppConfig::from_env` never sets, which
//! opens loopback and nothing else (`domain::egress`'s unit tests hold that).
//! **The tests that prove the guard use the harness default**, which is the
//! production default: no seam, no allow-list.
//!
//! # A planted secret
//!
//! Each test that resolves a secret sets its own environment variable (a
//! unique name, so parallel tests do not share one) to a value that appears
//! nowhere else, and the mock **echoes back the `Authorization` header it
//! received** — the worst case, a system that repeats the secret in its body.
//! The assertions then read the response, the whole `integration_logs` row and
//! every `audit_events` row as text, and look for the value in every form it
//! was sent in.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation was made, this file run, the named tests observed red, and
//! the mutation reverted. **Seen red, 2026-09-29.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `domain::secret::environment_name_is_resolvable` accepts any non-empty name | `a_name_outside_the_integration_prefix_is_refused_and_its_value_goes_nowhere` |
//! | `EgressPolicy::check` answers `Ok` for every class | `loopback_is_refused_in_every_spelling_and_nothing_is_sent`, `the_metadata_address_and_an_unlisted_private_address_are_refused`, `a_listed_private_range_passes_the_guard` |
//! | `domain::test_call::redact` returns its input | `a_bearer_secret_reaches_the_system_and_nothing_kelir_keeps_or_returns`, `a_basic_pair_is_sent_as_basic_and_its_password_is_nowhere` |
//! | `redirect(Policy::none())` removed from `outbound::send` | `a_redirect_is_the_answer_and_its_location_is_not_requested` |
//! | The outer `tokio::time::timeout` alone widened to 60 s | green — the client's own timeout held; two layers |
//! | Both layers widened to 60 s (`budget` in `service::test_call`) | `a_slow_system_fails_at_its_timeout` |
//! | `tracing::info!(authorization = value.expose(), ..)` added in `outbound::send` | `no_log_line_carries_the_secret_even_at_trace` |

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const BASE: &str = "/api/v1/integration/external-systems";
const PASSWORD: &str = "integration-test-call-password";

// ---------------------------------------------------------------------------
// The mock system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    authorization: Option<String>,
    correlation_id: Option<String>,
}

struct Mock {
    address: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    async fn start() -> Self {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let recorder = seen.clone();

        let router = Router::new().fallback(move |request: Request| {
            let recorder = recorder.clone();
            async move { answer(request, recorder).await }
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the mock system");
        let address = listener.local_addr().expect("its address");

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("the mock serves");
        });

        Self { address, seen }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the mock's record").clone()
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn answer(request: Request, recorder: Arc<Mutex<Vec<Seen>>>) -> Response {
    let authorization = header_text(request.headers(), "authorization");
    let path = request.uri().path().to_owned();

    recorder.lock().expect("the mock's record").push(Seen {
        method: request.method().to_string(),
        path: path.clone(),
        authorization: authorization.clone(),
        correlation_id: header_text(request.headers(), "x-correlation-id"),
    });

    let echoed = authorization.unwrap_or_default();

    match path.as_str() {
        // The worst case: the secret comes back in the body, in JSON, beside a
        // key the preview masks whatever it holds.
        "/echo" => axum::Json(json!({
            "youSent": echoed,
            "access_token": "a-token-the-system-issued",
            "status": "ok",
        }))
        .into_response(),
        "/text-echo" => format!("you sent {echoed}").into_response(),
        "/fail" => (StatusCode::INTERNAL_SERVER_ERROR, "the system broke").into_response(),
        "/slow" => {
            tokio::time::sleep(Duration::from_secs(6)).await;
            "too late".into_response()
        }
        "/redirect" => {
            (StatusCode::FOUND, [(header::LOCATION, "/landed")], "moved").into_response()
        }
        "/big" => "x".repeat(100 * 1024).into_response(),
        _ => "ok".into_response(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// An app whose egress guard lets the mock on loopback through — the seam.
async fn app_reaching_loopback() -> TestApp {
    TestApp::spawn_with(|config| config.integration_allow_loopback = true).await
}

/// A planted secret behind a fresh environment variable; returns
/// `(reference, value)`.
fn plant(value: &str) -> (String, String) {
    let name = format!(
        "KELIR_INTEGRATION_SECRET_TEST_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    std::env::set_var(&name, value);
    (format!("env://{name}"), value.to_owned())
}

fn id_of(response: &TestResponse) -> Uuid {
    response.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", response.body))
}

struct Target {
    system: Uuid,
    endpoint: Uuid,
}

impl Target {
    fn call_uri(&self) -> String {
        format!(
            "{BASE}/{}/endpoints/{}/test-call",
            self.system, self.endpoint
        )
    }
}

/// A system at `base_url` with one endpoint at `path`, registered through the
/// API.
async fn target(app: &TestApp, token: &str, base_url: &str, method: &str, path: &str) -> Target {
    let code = format!("SYS_{}", Uuid::now_v7().simple());
    let system = app
        .post(
            BASE,
            Some(token),
            json!({
                "systemCode": code,
                "systemName": "Mock system",
                "baseUrl": base_url,
                "timeoutSeconds": 2,
            }),
        )
        .await;
    assert_eq!(system.status, StatusCode::CREATED, "{}", system.body);
    let system = id_of(&system);

    let endpoint = app
        .post(
            &format!("{BASE}/{system}/endpoints"),
            Some(token),
            json!({
                "endpointCode": "PROBE",
                "name": "Probe",
                "method": method,
                "path": path,
            }),
        )
        .await;
    assert_eq!(endpoint.status, StatusCode::CREATED, "{}", endpoint.body);

    Target {
        system,
        endpoint: id_of(&endpoint),
    }
}

async fn credential(app: &TestApp, token: &str, system: Uuid, body: Value) -> Uuid {
    let response = app
        .post(&format!("{BASE}/{system}/credentials"), Some(token), body)
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    id_of(&response)
}

async fn bearer(app: &TestApp, token: &str, system: Uuid, reference: &str) -> Uuid {
    credential(
        app,
        token,
        system,
        json!({ "credentialType": "BEARER_TOKEN", "secretReference": reference }),
    )
    .await
}

async fn call(app: &TestApp, token: &str, target: &Target) -> TestResponse {
    app.post(&target.call_uri(), Some(token), json!({})).await
}

/// Every `integration_logs` row for an endpoint, whole, as JSON.
async fn log_rows(app: &TestApp, endpoint: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT row_to_json(l) FROM integration_logs l WHERE entity_id = $1 ORDER BY started_at",
    )
    .bind(endpoint)
    .fetch_all(&app.pool)
    .await
    .expect("read integration_logs")
}

async fn audit_rows_containing(app: &TestApp, needle: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events a WHERE row_to_json(a)::text LIKE '%' || $1 || '%'",
    )
    .bind(needle)
    .fetch_one(&app.pool)
    .await
    .expect("read audit_events")
}

async fn all_log_text(app: &TestApp) -> String {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(l)::text FROM integration_logs l")
            .fetch_all(&app.pool)
            .await
            .expect("read integration_logs");
    rows.join("\n")
}

/// A caller in the system tenant holding exactly `permissions`.
async fn caller_holding(app: &TestApp, label: &str, permissions: &[&str]) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-CALL-{label}"),
        permissions,
    )
    .await;

    let username = format!("user.call.{}", label.to_lowercase());
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("call.{}@kelir.test", label.to_lowercase()),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

fn error_message(response: &TestResponse) -> String {
    response.body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

// ---------------------------------------------------------------------------
// The secret goes to the system and nowhere else (AC3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_bearer_secret_reaches_the_system_and_nothing_kelir_keeps_or_returns() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-bearer-5d1c9e");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let data = response.data();
    assert_eq!(data["status"], "SUCCESS");
    assert_eq!(data["statusCode"], 200);
    assert_eq!(data["method"], "GET");
    assert_eq!(data["url"], format!("{}/echo", mock.base_url()));
    assert!(data["durationMs"].as_i64().is_some());

    // The system got it, as a bearer token.
    let seen = mock.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/echo");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Bearer {secret}").as_str())
    );

    // It echoed it back, and the preview has it redacted, with the masked key.
    let preview: Value = serde_json::from_str(data["bodyPreview"].as_str().expect("a preview"))
        .expect("the preview of a JSON body is JSON");
    assert_eq!(preview["youSent"], "[REDACTED]");
    assert_eq!(preview["access_token"], "[REDACTED]");
    assert_eq!(preview["status"], "ok");

    // Nowhere in the response.
    assert!(
        !response.body.to_string().contains(&secret),
        "the response carries the secret: {}",
        response.body
    );

    // Nowhere in the log row, which is one row.
    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert!(
        !row.to_string().contains(&secret),
        "the log row carries it: {row}"
    );
    assert_eq!(row["id"], data["logId"]);
    assert_eq!(row["status"], "SUCCESS");
    assert_eq!(row["status_code"], 200);
    assert_eq!(row["direction"], "OUTBOUND");
    assert_eq!(row["integration_type"], "REST");
    assert_eq!(row["method"], "GET");
    assert_eq!(row["endpoint"], format!("{}/echo", mock.base_url()));
    assert_eq!(row["entity_type"], "IntegrationEndpoint");
    assert_eq!(
        row["request_payload_json"]["headers"]["Authorization"],
        "[REDACTED]"
    );
    assert!(row["completed_at"].is_string());
    assert!(row["duration_ms"].as_i64().is_some());

    // The correlation id the system saw is the row's.
    assert_eq!(
        seen[0].correlation_id.as_deref(),
        row["correlation_id"].as_str()
    );

    // Nowhere in the audit trail — which does hold the reference, from the
    // credential's creation, so the search is not simply empty.
    assert_eq!(audit_rows_containing(&app, &secret).await, 0);
    assert!(audit_rows_containing(&app, &reference).await > 0);
}

/// Everything the backend traces, at every level, from one thread.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

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

#[tokio::test]
async fn no_log_line_carries_the_secret_even_at_trace() {
    // `#[tokio::test]` is a current-thread runtime, so the handler, the client
    // and the connection it drives all run on this thread, under this
    // thread's subscriber: every `tracing` event the call emits — Kelir's,
    // reqwest's and hyper's, at TRACE — lands in `captured`.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-traced-6e6e");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let response = {
        let _guard = tracing::subscriber::set_default(subscriber);
        call(&app, &token, &target).await
    };

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let text = String::from_utf8_lossy(&captured.0.lock().expect("the capture")).into_owned();
    assert!(
        text.contains("integration test call"),
        "the capture saw nothing, so its silence proves nothing: {text}"
    );
    for form in [secret.clone(), format!("Bearer {secret}")] {
        assert!(
            !text.contains(&form),
            "a log line carries the secret: {text}"
        );
    }
}

#[tokio::test]
async fn a_basic_pair_is_sent_as_basic_and_its_password_is_nowhere() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, pair) = plant("svc-kelir:planted-basic-password-81f0");
    let encoded = STANDARD.encode(&pair);

    let target = target(&app, &token, &mock.base_url(), "POST", "/text-echo").await;
    credential(
        &app,
        &token,
        target.system,
        json!({ "credentialType": "BASIC_AUTH", "secretReference": reference }),
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let seen = mock.seen();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Basic {encoded}").as_str())
    );
    assert_eq!(response.data()["bodyPreview"], "you sent [REDACTED]");

    let everything = format!("{}\n{}", response.body, all_log_text(&app).await);
    for form in [
        pair.as_str(),
        "planted-basic-password-81f0",
        encoded.as_str(),
    ] {
        assert!(!everything.contains(form), "{form} escaped: {everything}");
        assert_eq!(audit_rows_containing(&app, form).await, 0, "{form}");
    }
}

// ---------------------------------------------------------------------------
// Secret resolution failures are named and logged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_vault_reference_fails_named_and_writes_one_log_row() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(
        &app,
        &token,
        target.system,
        "vault://kelir/erp/api-key#token",
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("SECRET_BACKEND_NOT_CONFIGURED"));
    assert!(mock.seen().is_empty(), "nothing was sent");

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["status_code"].is_null());
    assert!(rows[0]["error_message"]
        .as_str()
        .is_some_and(|message| message.starts_with("SECRET_BACKEND_NOT_CONFIGURED")));
    assert!(
        error_message(&response).contains(rows[0]["id"].as_str().expect("an id")),
        "the failure names its log row: {}",
        response.body
    );
}

#[tokio::test]
async fn an_unset_environment_variable_is_named_and_logged() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(
        &app,
        &token,
        target.system,
        "env://KELIR_INTEGRATION_SECRET_NEVER_SET_ANYWHERE",
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_NOT_FOUND"),
        "{}",
        response.body
    );
    assert!(mock.seen().is_empty());
    assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
}

#[tokio::test]
async fn a_name_outside_the_integration_prefix_is_refused_and_its_value_goes_nowhere() {
    // The product owner's decision on #547: only KELIR_INTEGRATION_SECRET_*
    // is read. KELIR_JWT_SECRET is the case the rule exists for — a caller who
    // can write a reference and a baseUrl would otherwise be sent the key that
    // signs every session.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let jwt = std::env::var("KELIR_JWT_SECRET").unwrap_or_else(|_| {
        let planted = "kelir-planted-jwt-signing-key-c0ffee".to_owned();
        std::env::set_var("KELIR_JWT_SECRET", &planted);
        planted
    });
    // A second, unprefixed name holding a value that exists nowhere else.
    let outside = format!(
        "KELIR_TEST_CALL_OUTSIDE_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    let outside_value = "kelir-planted-outside-prefix-e4e4";
    std::env::set_var(&outside, outside_value);

    for (name, value) in [
        ("KELIR_JWT_SECRET", jwt.as_str()),
        (outside.as_str(), outside_value),
    ] {
        let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
        bearer(&app, &token, target.system, &format!("env://{name}")).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{name}: {}",
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{name}"
        );
        assert!(!response.body.to_string().contains(value), "{name}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(
            rows.len(),
            1,
            "{name}: the refused call still writes its row"
        );
        assert!(rows[0]["error_message"]
            .as_str()
            .is_some_and(|message| message.starts_with("SECRET_NAME_NOT_PERMITTED")));
        assert!(!all_log_text(&app).await.contains(value), "{name}");
        assert_eq!(audit_rows_containing(&app, value).await, 0, "{name}");
    }

    assert!(
        mock.seen().is_empty(),
        "nothing was sent: {:?}",
        mock.seen()
    );

    // Control: the same system shape with a prefixed name resolves and calls.
    let (reference, secret) = plant("kelir-planted-prefixed-ok-1a1a");
    assert!(reference.starts_with("env://KELIR_INTEGRATION_SECRET_"));
    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {secret}"))
    );
}

#[tokio::test]
async fn an_unbuilt_credential_type_is_refused_before_its_secret_is_read() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    for kind in [
        "API_KEY",
        "OAUTH2_CLIENT_CREDENTIALS",
        "JWT",
        "HMAC_SECRET",
        "CERTIFICATE",
        "SFTP_PASSWORD",
    ] {
        let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
        // An unset variable: had the secret been read first, the answer would
        // be SECRET_NOT_FOUND.
        credential(
            &app,
            &token,
            target.system,
            json!({
                "credentialType": kind,
                "secretReference": "env://KELIR_INTEGRATION_SECRET_NEVER_SET_ANYWHERE",
            }),
        )
        .await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{kind}: {}",
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("CREDENTIAL_TYPE_NOT_SUPPORTED"),
            "{kind}"
        );
        assert!(error_message(&response).contains(kind), "{kind}");
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{kind}");
    }

    assert!(mock.seen().is_empty());
}

#[tokio::test]
async fn a_basic_secret_that_is_not_a_pair_is_named() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("no-colon-in-this-planted-value");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    credential(
        &app,
        &token,
        target.system,
        json!({ "credentialType": "BASIC_AUTH", "secretReference": reference }),
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_MALFORMED"),
        "{}",
        response.body
    );
    assert!(!response.body.to_string().contains(&secret));
    assert!(!all_log_text(&app).await.contains(&secret));
    assert!(mock.seen().is_empty());
}

// ---------------------------------------------------------------------------
// Which credential
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exactly_one_active_credential_valid_today_is_required() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-choice-3b7e");

    // None at all.
    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("NO_USABLE_CREDENTIAL"),
        "{}",
        response.body
    );

    // One that expired yesterday and one that is switched off: still none.
    credential(
        &app,
        &token,
        target.system,
        json!({
            "credentialType": "BEARER_TOKEN",
            "secretReference": "env://KELIR_INTEGRATION_SECRET_EXPIRED",
            "validFrom": "2020-01-01",
            "validTo": "2020-12-31",
        }),
    )
    .await;
    credential(
        &app,
        &token,
        target.system,
        json!({
            "credentialType": "BEARER_TOKEN",
            "secretReference": "env://KELIR_INTEGRATION_SECRET_OFF",
            "isActive": false,
        }),
    )
    .await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("NO_USABLE_CREDENTIAL"),
        "{}",
        response.body
    );

    // The one that qualifies is the one used.
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {secret}"))
    );

    // A second that qualifies makes it ambiguous rather than a coin toss.
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("AMBIGUOUS_CREDENTIAL"),
        "{}",
        response.body
    );

    assert_eq!(
        log_rows(&app, target.endpoint).await.len(),
        4,
        "four calls, four rows"
    );
    assert_eq!(mock.seen().len(), 1, "one call reached the system");
}

// ---------------------------------------------------------------------------
// Egress (AC4) — the harness default, which is production's
// ---------------------------------------------------------------------------

#[tokio::test]
async fn loopback_is_refused_in_every_spelling_and_nothing_is_sent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let port = mock.address.port();
    let (reference, _) = plant("kelir-planted-loopback-0a2d");

    for base in [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::ffff:127.0.0.1]:{port}"),
        format!("http://[::1]:{port}"),
        format!("http://2130706433:{port}"),
    ] {
        let target = target(&app, &token, &base, "GET", "/echo").await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{base}: {}",
            response.body
        );
        assert_eq!(response.error_code(), Some("EGRESS_REFUSED"), "{base}");
        assert!(error_message(&response).contains("loopback"), "{base}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1, "{base}");
        assert!(rows[0]["error_message"]
            .as_str()
            .is_some_and(|message| message.starts_with("EGRESS_REFUSED")));
        assert!(
            rows[0]["request_payload_json"]["headers"]["Authorization"].is_null(),
            "{base}: no Authorization was sent, so none is recorded"
        );
    }

    assert!(
        mock.seen().is_empty(),
        "the mock was reached past the guard: {:?}",
        mock.seen()
    );
}

#[tokio::test]
async fn the_metadata_address_and_an_unlisted_private_address_are_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-private-6c4f");

    for (base, class) in [
        ("http://169.254.169.254", "link-local"),
        ("http://[fe80::1]", "link-local"),
        ("http://0.0.0.0:8080", "unspecified"),
        ("http://224.0.0.1", "multicast"),
        ("http://10.255.255.1", "private"),
        ("http://192.168.0.1", "private"),
        ("http://[fd00::1]", "private"),
    ] {
        let target = target(&app, &token, base, "GET", "/latest/meta-data").await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base}: {}",
            response.body
        );
        assert!(
            error_message(&response).contains(class),
            "{base}: {}",
            response.body
        );
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{base}");
    }
}

#[tokio::test]
async fn a_listed_private_range_passes_the_guard() {
    // The address is unroutable from a test runner, so the call fails — at
    // the network, not at the guard. That is the observation: listed, it is
    // no longer EGRESS_REFUSED.
    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = vec!["10.255.255.0/24".parse().expect("a CIDR")];
    })
    .await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-listed-9e21");

    let listed = target(&app, &token, "http://10.255.255.1", "GET", "/x").await;
    bearer(&app, &token, listed.system, &reference).await;
    let response = call(&app, &token, &listed).await;

    assert!(
        matches!(
            response.error_code(),
            Some("UPSTREAM_TIMEOUT" | "UPSTREAM_UNREACHABLE")
        ),
        "a listed address is let through to the network: {}",
        response.body
    );

    // The next address over is outside the list.
    let unlisted = target(&app, &token, "http://10.255.254.1", "GET", "/x").await;
    bearer(&app, &token, unlisted.system, &reference).await;
    let response = call(&app, &token, &unlisted).await;

    assert_eq!(
        response.error_code(),
        Some("EGRESS_REFUSED"),
        "{}",
        response.body
    );
}

#[tokio::test]
async fn a_redirect_is_the_answer_and_its_location_is_not_requested() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-redirect-44aa");

    let target = target(&app, &token, &mock.base_url(), "GET", "/redirect").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["statusCode"], 302);
    assert_eq!(response.data()["status"], "FAILED");

    let paths: Vec<String> = mock.seen().into_iter().map(|seen| seen.path).collect();
    assert_eq!(paths, vec!["/redirect"], "the Location was followed");
}

// ---------------------------------------------------------------------------
// Time (AC5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_slow_system_fails_at_its_timeout() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-slow-2f8b");

    let target = target(&app, &token, &mock.base_url(), "GET", "/slow").await;
    sqlx::query("UPDATE external_systems SET timeout_seconds = 1 WHERE id = $1")
        .bind(target.system)
        .execute(&app.pool)
        .await
        .expect("shorten the timeout");
    bearer(&app, &token, target.system, &reference).await;

    let clock = Instant::now();
    let response = call(&app, &token, &target).await;
    let elapsed = clock.elapsed();

    assert_eq!(
        response.status,
        StatusCode::GATEWAY_TIMEOUT,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("UPSTREAM_TIMEOUT"));
    assert!(
        elapsed < Duration::from_secs(4),
        "the call waited {elapsed:?} for a system with a one-second timeout"
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["duration_ms"].as_i64().is_some_and(|ms| ms >= 900));
}

// ---------------------------------------------------------------------------
// Answers, and the one-row rule (AC6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_system_error_is_an_answer_marked_failed() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-fail-7d0e");

    let target = target(&app, &token, &mock.base_url(), "DELETE", "/fail").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["status"], "FAILED");
    assert_eq!(response.data()["statusCode"], 500);
    assert_eq!(response.data()["bodyPreview"], "the system broke");
    assert_eq!(mock.seen()[0].method, "DELETE");

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows[0]["status"], "FAILED");
    assert_eq!(rows[0]["status_code"], 500);
    assert_eq!(
        rows[0]["response_payload_json"]["bodyPreview"],
        "the system broke"
    );
}

#[tokio::test]
async fn a_long_body_is_cut_in_the_preview_and_the_log() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-big-1c3d");

    let target = target(&app, &token, &mock.base_url(), "GET", "/big").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["bodyTruncated"], true);
    assert_eq!(
        response.data()["bodyPreview"]
            .as_str()
            .map(|text| text.chars().count()),
        Some(2048)
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows[0]["response_payload_json"]["bodyTruncated"], true);
}

#[tokio::test]
async fn an_inactive_system_or_endpoint_is_refused_and_logged() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-inactive-5a5a");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let retired = app
        .put(
            &format!("{BASE}/{}/endpoints/{}", target.system, target.endpoint),
            Some(&token),
            json!({ "status": "INACTIVE" }),
        )
        .await;
    assert_eq!(retired.status, StatusCode::OK, "{}", retired.body);
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("ENDPOINT_NOT_ACTIVE"),
        "{}",
        response.body
    );

    app.put(
        &format!("{BASE}/{}/endpoints/{}", target.system, target.endpoint),
        Some(&token),
        json!({ "status": "ACTIVE" }),
    )
    .await;
    let deactivated = app
        .post(
            &format!("{BASE}/{}/deactivate", target.system),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(deactivated.status, StatusCode::OK, "{}", deactivated.body);
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("EXTERNAL_SYSTEM_NOT_ACTIVE"),
        "{}",
        response.body
    );

    assert_eq!(log_rows(&app, target.endpoint).await.len(), 2);
    assert!(mock.seen().is_empty());
}

#[tokio::test]
async fn a_refused_caller_or_a_missing_target_writes_no_row() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-norow-8b8b");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    // Everything about the system except the call.
    let without = caller_holding(
        &app,
        "NOCALL",
        &[
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:credential:read",
            "integration:credential:create",
        ],
    )
    .await;
    let response = call(&app, &without, &target).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.body);

    // The call alone is enough.
    let with = caller_holding(&app, "CALLONLY", &["integration:endpoint:call"]).await;
    let response = call(&app, &with, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    // An endpoint that is not on this system, and a system in another tenant.
    let stray = Target {
        system: target.system,
        endpoint: Uuid::now_v7(),
    };
    assert_eq!(
        call(&app, &token, &stray).await.status,
        StatusCode::NOT_FOUND
    );

    let other = fixtures::create_tenant(&app.pool, "TNT-CALL-X", "Other tenant").await;
    let theirs = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO external_systems (id, tenant_id, system_code, system_name, base_url)
         VALUES ($1, $2, 'THEIRS', 'Theirs', $3)",
    )
    .bind(theirs)
    .bind(other)
    .bind(mock.base_url())
    .execute(&app.pool)
    .await
    .expect("another tenant's system");
    let their_endpoint = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO integration_endpoints
             (id, tenant_id, external_system_id, endpoint_code, name, method, path)
         VALUES ($1, $2, $3, 'PROBE', 'Probe', 'GET', '/echo')",
    )
    .bind(their_endpoint)
    .bind(other)
    .bind(theirs)
    .execute(&app.pool)
    .await
    .expect("its endpoint");
    let crossing = Target {
        system: theirs,
        endpoint: their_endpoint,
    };
    assert_eq!(
        call(&app, &token, &crossing).await.status,
        StatusCode::NOT_FOUND
    );

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM integration_logs")
        .fetch_one(&app.pool)
        .await
        .expect("count integration_logs");
    assert_eq!(
        total, 1,
        "only the permitted call on a real endpoint wrote a row"
    );
    assert_eq!(mock.seen().len(), 1);
}

// ---------------------------------------------------------------------------
// The permission and the document (AC1, AC7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_permission_is_catalogued_at_its_id_and_held_by_the_administrator() {
    let app = TestApp::spawn().await;

    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM permissions WHERE permission_code = 'integration:endpoint:call'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the catalogue row");
    assert_eq!(id, uuid::uuid!("00000000-0000-0000-0001-000000000076"));

    let granted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM role_permissions
                        WHERE role_id = $1 AND permission_id = $2 AND deleted_at IS NULL)",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the grant");
    assert!(granted);
}

#[tokio::test]
async fn the_route_is_in_the_openapi_document_and_its_answer_has_no_headers() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;

    let operation = &document.body["paths"]
        [&format!("{BASE}/{{id}}/endpoints/{{endpointId}}/test-call")]["post"];
    assert!(
        operation.is_object(),
        "the route is missing from the document"
    );
    assert!(
        operation["requestBody"].is_null(),
        "a test call takes no body"
    );

    let schema = &document.body["components"]["schemas"]["TestCallResponse"]["properties"];
    let mut fields: Vec<&str> = schema
        .as_object()
        .expect("TestCallResponse has properties")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "bodyPreview",
            "bodyTruncated",
            "durationMs",
            "logId",
            "method",
            "status",
            "statusCode",
            "url"
        ],
        "a field added to the answer is a decision about what it may carry"
    );
}
