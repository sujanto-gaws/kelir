//! **No operation answers a caller with no token, but the ones named here**
//! (NFR-SEC-003, [#779](https://github.com/sujanto-gaws/kelir/issues/779) AC3).
//!
//! `deleted_caller_refusals.rs` walks every operation that **declares** `bearer`.
//! An operation that does not declare it is outside that walk, and so is a
//! route that nothing documents. This one starts from the other end: **every
//! operation in `ApiDoc::openapi()` is sent a request with no `Authorization`
//! header**, and each must answer the 401 and `UNAUTHORIZED` of an expired token
//! unless [`PUBLIC`] names it, with its reason. It names no protected route. A
//! route added tomorrow is walked tomorrow.
//!
//! **What stops a new operation from going round the walk.** Three doors.
//! - An operation in the document that takes no token and is not in [`PUBLIC`]
//!   fails its cell.
//! - An operation in the document is either in [`PUBLIC`] or declares `bearer`,
//!   and never both, so the published contract cannot disagree with this list.
//! - A method served and not in the document is not walked, so
//!   `every_served_method_is_in_the_document` counts the method handlers in
//!   every `.route(…)` in `src/` and holds them equal to the document.
//!   `router::tests::every_annotated_route_reaches_the_document` compares
//!   **paths**, so `get(a).post(b)` with `b` unannotated passes it.
//!
//! **What stops [`PUBLIC`] from going stale.** Each entry must name an operation
//! in the document, and each is sent the same request and must **not** answer
//! 401: a row that stops being public is a row to delete.
//!
//! **Nothing is written by a refused request.** The row count of every table is
//! read before the protected cells and after them, and none may move.
//!
//! The walk says nothing about a **wrong** token (`auth_deleted_caller.rs`,
//! `token.rs`), nor about permissions: 403 is each module's own test, and no walk
//! holds it (the record of #779 says so).
//!
//! # Seen to fail under mutation
//!
//! Each mutation was made alone, this file run, and the mutation reverted.
//! **Seen red, 2026-10-31**, in `kelir-rust-tools:1.89`. The walk had 154
//! operations: 144 protected cells and 10 public.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `GET /identity/permissions` takes `Result<Authenticated, AppError>`, ignores it and answers | the walk, 1 of 154 cells: `no token, and answered 200 OK` |
//! | The extractor answers `Forbidden` when there is no header | the walk, 144 of 154 cells: every protected cell, and no public one |
//! | `security(("bearer" = []))` dropped from `GET /auth/me` | the walk, at its door: `declares no bearer and is not in PUBLIC` |
//! | `GET /health/ready` declares `bearer` | the walk, at its door: `is in PUBLIC and declares bearer` |
//! | `get(list_permissions).post(list_permissions)`, the `post` unannotated | `every_served_method_is_in_the_document`: 155 handlers, 154 operations |
//! | `POST /auth/logout` takes `Authenticated` | the walk, 1 of 154 cells: `is in PUBLIC and refused` |
//! | A row in `PUBLIC` for an operation that does not exist | the walk, at its door: `which is not in the document` |
//! | `GET /version` dropped from `PUBLIC` | the walk, at its door: `declares no bearer and is not in PUBLIC` |

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use axum::http::{Method, StatusCode};
use regex::Regex;
use serde_json::{json, Value};
use utoipa::OpenApi;
use uuid::Uuid;

use common::TestApp;
use kelir_backend::router::ApiDoc;

/// **Every operation a caller reaches with no token, and why.** A row here is a
/// decision that the operation is public; review it as one.
const PUBLIC: [(&str, &str, &str); 10] = [
    ("GET", "/health", "a load balancer's probe; no data"),
    ("GET", "/health/live", "a liveness probe; no data"),
    ("GET", "/health/ready", "a readiness probe; no data"),
    ("GET", "/version", "the build, which the footer shows"),
    (
        "GET",
        "/deployment",
        "the sign-in form reads it before there is a session (D-18)",
    ),
    ("POST", "/api/v1/auth/login", "how a token is obtained"),
    (
        "POST",
        "/api/v1/auth/refresh",
        "takes a refresh token; the access token may have expired",
    ),
    (
        "POST",
        "/api/v1/auth/logout",
        "takes a refresh token; must work once the access token has expired",
    ),
    (
        "POST",
        "/api/v1/auth/forgot-password",
        "a person who cannot sign in; answers 202 whoever asks",
    ),
    (
        "POST",
        "/api/v1/auth/reset-password",
        "takes a reset token from the mail",
    ),
];

/// Served, public and outside the document it serves: it has no annotation of
/// its own, which `router::tests` excuses by name.
const THE_DOCUMENT_ITSELF: &str = "/api/docs/openapi.json";

const METHODS: [(&str, Method); 5] = [
    ("get", Method::GET),
    ("post", Method::POST),
    ("put", Method::PUT),
    ("patch", Method::PATCH),
    ("delete", Method::DELETE),
];

struct Operation {
    method: Method,
    template: String,
    /// The path with every `{capture}` resolved.
    path: String,
    declares_bearer: bool,
}

impl Operation {
    fn is_public(&self) -> bool {
        PUBLIC
            .iter()
            .any(|(method, template, _)| *method == self.method && *template == self.template)
    }

    fn name(&self) -> String {
        format!("{} {}", self.method, self.template)
    }
}

/// A value for a path capture: the annotation's `example` if it gives one, and
/// otherwise a fresh uuid, which is what almost all of them are.
fn capture_value(parameters: Option<&Vec<Value>>, name: &str) -> String {
    parameters
        .into_iter()
        .flatten()
        .filter(|parameter| parameter["in"] == "path" && parameter["name"] == name)
        .find_map(|parameter| {
            parameter["example"]
                .as_str()
                .or_else(|| parameter["schema"]["example"].as_str())
        })
        .map_or_else(|| Uuid::now_v7().to_string(), ToOwned::to_owned)
}

/// Every operation in the document, whatever it declares.
fn every_operation() -> Vec<Operation> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        for (key, method) in METHODS {
            let operation = &item[key];
            if operation.is_null() {
                continue;
            }

            let mut path = template.clone();
            while let Some(start) = path.find('{') {
                let end = start + path[start..].find('}').expect("a closed capture");
                let value =
                    capture_value(operation["parameters"].as_array(), &path[start + 1..end]);
                path.replace_range(start..=end, &value);
            }

            operations.push(Operation {
                method,
                template: template.clone(),
                path,
                declares_bearer: operation["security"].as_array().is_some_and(|schemes| {
                    schemes.iter().any(|scheme| scheme.get("bearer").is_some())
                }),
            });
        }
    }

    operations
}

/// What keeps the document and [`PUBLIC`] from disagreeing: each failure names
/// its row.
fn door_failures(operations: &[Operation]) -> Vec<String> {
    let mut failures = Vec::new();

    for operation in operations {
        match (operation.is_public(), operation.declares_bearer) {
            (false, false) => failures.push(format!(
                "{} declares no bearer and is not in PUBLIC: protect it, or list it with its reason",
                operation.name()
            )),
            (true, true) => failures.push(format!(
                "{} is in PUBLIC and declares bearer: one of the two is wrong",
                operation.name()
            )),
            _ => {}
        }
    }

    let documented: BTreeSet<String> = operations.iter().map(Operation::name).collect();
    for (method, template, _) in PUBLIC {
        let listed = format!("{method} {template}");
        if !documented.contains(&listed) {
            failures.push(format!(
                "PUBLIC names {listed}, which is not in the document"
            ));
        }
    }

    failures
}

/// The row count of every table in the schema.
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

async fn send(app: &TestApp, operation: &Operation) -> common::TestResponse {
    // A body for every method that takes one: an empty object is refused for
    // what it lacks, so a route that read its body before its caller answers
    // 4xx here rather than 401, and the cell says so.
    let body =
        (operation.method != Method::GET && operation.method != Method::DELETE).then(|| json!({}));

    app.send(operation.method.clone(), &operation.path, None, body)
        .await
}

/// **The walk.**
#[tokio::test]
async fn no_operation_answers_a_caller_with_no_token_unless_it_is_named_public() {
    let operations = every_operation();

    let failures = door_failures(&operations);
    assert!(
        failures.is_empty(),
        "the document and PUBLIC disagree:\n{}",
        failures.join("\n")
    );

    let app = TestApp::spawn().await;
    let mut failures = Vec::new();

    // The public operations first: a row that stopped being public is a stale
    // row, and whatever an ordinary request writes is written before the counts.
    for operation in operations.iter().filter(|operation| operation.is_public()) {
        let response = send(&app, operation).await;
        if matches!(
            response.status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            failures.push(format!(
                "{} is in PUBLIC and refused: {} {}",
                operation.name(),
                response.status,
                response.body
            ));
        }
    }

    let document = app.get(THE_DOCUMENT_ITSELF, None).await;
    assert_eq!(document.status, StatusCode::OK, "{}", document.body);

    let before = row_counts(&app).await;
    for table in ["audit_events", "activity_events", "integration_logs"] {
        assert!(before.contains_key(table), "no table {table}: {before:?}");
    }

    let protected: Vec<&Operation> = operations
        .iter()
        .filter(|operation| !operation.is_public())
        .collect();
    assert!(
        !protected.is_empty(),
        "nothing to walk: the document has no protected operation"
    );

    for operation in &protected {
        let response = send(&app, operation).await;
        if response.status != StatusCode::UNAUTHORIZED
            || response.error_code() != Some("UNAUTHORIZED")
        {
            failures.push(format!(
                "{}: no token, and answered {} {}",
                operation.name(),
                response.status,
                response.body
            ));
        }
    }

    let after = row_counts(&app).await;
    let moved: Vec<String> = after
        .iter()
        .filter(|(table, count)| before.get(*table) != Some(*count))
        .map(|(table, count)| format!("{table}: {:?} -> {count}", before.get(table)))
        .collect();

    assert!(
        failures.is_empty(),
        "{} of {} cells failed:\n{}",
        failures.len(),
        operations.len(),
        failures.join("\n")
    );
    assert!(
        moved.is_empty(),
        "a refused request wrote something: {moved:?}"
    );
}

/// Each `.rs` file under `directory`, cut at its `#[cfg(test)]`: a test module
/// builds routers of its own, and they are served by nothing.
fn production_sources(directory: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).expect("the directory reads") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            production_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = std::fs::read_to_string(&path).expect("the source reads");
            let production = text.split("#[cfg(test)]").next().unwrap_or_default();
            out.push(production.to_owned());
        }
    }
}

/// The arguments of each `.route(…)` in `text`, to its balancing parenthesis.
fn route_calls(text: &str) -> Vec<&str> {
    const OPEN: &str = ".route(";

    text.match_indices(OPEN)
        .filter_map(|(start, _)| {
            let after = start + OPEN.len();
            let mut depth = 1;
            for (offset, character) in text[after..].char_indices() {
                match character {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    return Some(&text[after..after + offset]);
                }
            }
            None
        })
        .collect()
}

/// **The third door.** A method handler is what serves an operation, so the
/// count of them in `src/` is the count of operations, read from something
/// other than the document the walk reads. `any` and `on` count too, so that a
/// route written with either cannot stand outside the count.
#[test]
fn every_served_method_is_in_the_document() {
    let mut sources = Vec::new();
    production_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    );

    let handler = Regex::new(r"\b(get|post|put|patch|delete|any|on)\(").expect("a valid pattern");
    let calls: Vec<&str> = sources.iter().flat_map(|text| route_calls(text)).collect();
    assert!(
        calls.len() > 100,
        "the scan found {} .route calls, so it is not reading the routers",
        calls.len()
    );

    let served: usize = calls
        .iter()
        // `THE_DOCUMENT_ITSELF` is served and, by name, not in the document.
        .filter(|call| !call.contains(THE_DOCUMENT_ITSELF))
        .map(|call| handler.find_iter(call).count())
        .sum();

    let documented = every_operation().len();
    assert_eq!(
        served, documented,
        "src/ serves {served} method handlers and the document has {documented} operations: \
         a method without an annotation is one the walk cannot see"
    );
}
