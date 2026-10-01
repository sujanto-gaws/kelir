//! **A deleted tenant's token, and a deleted user's, is a 401 on every
//! authenticated route** ([#650](https://github.com/sujanto-gaws/kelir/issues/650)
//! criterion 3, decision D-105; ADR-0045).
//!
//! **This walk looks at every route instead of a list of them**, as
//! `list_route_refusals.rs` does for another rule. It reads the operations
//! from `ApiDoc::openapi()` — the document
//! `router::tests::every_annotated_route_reaches_the_document` holds equal to
//! what is served — takes every operation that declares `security: bearer`,
//! and sends each one three times: with a live token, with a token whose
//! tenant has since been deleted, and with a token whose user has. It names no
//! route. A route added tomorrow is walked tomorrow.
//!
//! **What stops the walk from reaching 401 for the wrong reason.** The live
//! token is sent to every operation first and must *not* be answered 401: a
//! route that refused every caller would otherwise pass both refusal cells.
//! The three tokens are checked on one route before the deletions, so each is
//! a token the application issued and served.
//!
//! **What stops the walk from walking nothing.** A handler whose annotation
//! omits `security(("bearer" = []))` is not in the walk. The guard counts
//! `: Authenticated,` in every `modules/**/handlers.rs` — a handler's argument
//! of that type is what puts a route behind the check — and requires the
//! walked operations to number the same.
//!
//! **Nothing is written by a refused request.** The row count of every table
//! is read before the two refused passes and after them, and none may move:
//! audit, activity and `integration_logs` among them.
//!
//! The cases a walk cannot hold — the envelope byte for byte, the pairing of
//! `sub` with `tenant_id`, a failed lookup, sign-out, the leeway — are in
//! `auth_deleted_caller.rs`.
//!
//! # Seen red first (coding standard §2.9)
//!
//! Run 2026-10-01 against `main` at `6631cc9`, before the fix: the guard was
//! green (143 operations, 143 handler arguments) and **286 of 429 cells
//! failed**, every refusal cell of every operation. No live cell failed.
//!
//! # Seen to fail under mutation
//!
//! The table is written with the fix, once there is something to mutate.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use utoipa::OpenApi;
use uuid::Uuid;

use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use kelir_backend::router::ApiDoc;

const PASSWORD: &str = "a-sufficiently-long-password";
const USERS: &str = "/api/v1/identity/users";

#[derive(Debug)]
struct Operation {
    method: Method,
    /// The path with every `{capture}` resolved.
    path: String,
    /// The path as documented, for the failure listing.
    template: String,
}

/// Whether an operation's `security` names the `bearer` scheme.
fn declares_bearer(operation: &Value) -> bool {
    operation["security"]
        .as_array()
        .is_some_and(|schemes| schemes.iter().any(|scheme| scheme.get("bearer").is_some()))
}

/// A value for a path capture.
///
/// A capture the annotation declares with an `example` gets it; every other
/// capture gets a fresh uuid, which is what almost all of them are and names
/// nothing. The walk does not need to reach the handler's own work: the live
/// cell may answer 404 or 422, and must only not answer 401.
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

/// Every operation in the document that declares `security: bearer`.
fn discover() -> Vec<Operation> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        for (key, method) in [
            ("get", Method::GET),
            ("post", Method::POST),
            ("put", Method::PUT),
            ("patch", Method::PATCH),
            ("delete", Method::DELETE),
        ] {
            let operation = &item[key];
            if !declares_bearer(operation) {
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
                path,
                template: template.clone(),
            });
        }
    }

    operations
}

/// Every `: Authenticated,` written in a module's handlers: the number of
/// routes behind the check, counted from the source rather than from the
/// document the walk is reading.
fn authenticated_handler_sites() -> usize {
    fn visit(directory: &Path, count: &mut usize) {
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                visit(&path, count);
            } else if path.file_name().is_some_and(|name| name == "handlers.rs") {
                let text = std::fs::read_to_string(&path).expect("the source reads");
                *count += text.matches(": Authenticated,").count();
            }
        }
    }

    let mut count = 0;
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules"),
        &mut count,
    );
    count
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

async fn send(app: &TestApp, operation: &Operation, token: &str) -> common::TestResponse {
    // A body for every method that takes one. An empty object is refused by
    // most routes for what it lacks, which is an answer and not a 401.
    let body =
        (operation.method != Method::GET && operation.method != Method::DELETE).then(|| json!({}));

    app.send(operation.method.clone(), &operation.path, Some(token), body)
        .await
}

async fn access_token(app: &TestApp, code: &str, username: &str, password: &str) -> String {
    app.sign_in_to(code, username, password).await
}

/// **The walk** (#650 criterion 3).
#[tokio::test]
async fn a_deleted_tenants_token_and_a_deleted_users_are_refused_on_every_authenticated_route() {
    let operations = discover();

    // --- The guard -----------------------------------------------------------
    let sites = authenticated_handler_sites();
    assert!(sites > 0, "no handler takes `Authenticated`");
    assert_eq!(
        operations.len(),
        sites,
        "the document declares `security: bearer` on {} operations, and the handlers hold \
         {sites} `: Authenticated,` arguments — a route behind the check that does not \
         declare it is one the walk cannot see. Walked: {:#?}",
        operations.len(),
        operations
            .iter()
            .map(|operation| format!("{} {}", operation.method, operation.template))
            .collect::<Vec<_>>()
    );

    // --- Three callers -------------------------------------------------------
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let live = access_token(&app, "SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD).await;

    let created = app
        .post(
            "/api/v1/organization/tenants",
            Some(&live),
            json!({
                "tenantCode": "GONE",
                "name": "Gone Limited",
                "administrator": {
                    "username": "gone.admin",
                    "email": "gone.admin@example.test",
                    "displayName": "Tenant Administrator",
                    "password": PASSWORD,
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let tenant = created.data()["id"]
        .as_str()
        .expect("a tenant id")
        .to_owned();
    let of_deleted_tenant = access_token(&app, "GONE", "gone.admin", PASSWORD).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-SOON-GONE",
        &["identity:user:read"],
    )
    .await;
    let user = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "soon.gone",
        "soon.gone@kelir.test",
        PASSWORD,
        &[role],
    )
    .await;
    let of_deleted_user = access_token(&app, "SYSTEM", "soon.gone", PASSWORD).await;

    // Each is a token the application issued and serves.
    for (whose, token) in [
        ("the live caller's", &live),
        ("the tenant's", &of_deleted_tenant),
        ("the user's", &of_deleted_user),
    ] {
        let served = app.get(USERS, Some(token)).await;
        assert_eq!(served.status, StatusCode::OK, "{whose}: {}", served.body);
    }

    let deleted = app
        .delete(
            &format!("/api/v1/organization/tenants/{tenant}"),
            Some(&live),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let deleted = app.delete(&format!("{USERS}/{user}"), Some(&live)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let mut failures = Vec::new();
    let mut cells = 0;

    // --- The positive control, first -----------------------------------------
    // A live token on every operation is answered something other than 401.
    // First, so that whatever an ordinary request writes is written before
    // the counts below are read.
    for operation in &operations {
        cells += 1;
        let response = send(&app, operation, &live).await;
        if response.status == StatusCode::UNAUTHORIZED {
            failures.push(format!(
                "{} {}: a live token was refused — {}",
                operation.method, operation.template, response.body
            ));
        }
    }

    // --- The refusals ---------------------------------------------------------
    let before = row_counts(&app).await;
    for table in ["audit_events", "activity_events", "integration_logs"] {
        assert!(before.contains_key(table), "no table {table}: {before:?}");
    }

    for (whose, token) in [
        ("a deleted tenant's token", &of_deleted_tenant),
        ("a deleted user's token", &of_deleted_user),
    ] {
        for operation in &operations {
            cells += 1;
            let response = send(&app, operation, token).await;
            if response.status != StatusCode::UNAUTHORIZED
                || response.error_code() != Some("UNAUTHORIZED")
            {
                failures.push(format!(
                    "{} {}: {whose} was answered {} {}",
                    operation.method, operation.template, response.status, response.body
                ));
            }
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
