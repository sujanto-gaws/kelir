//! **Nothing under `/api/v1` may be stored by a cache**
//! ([#881](https://github.com/sujanto-gaws/kelir/issues/881), SDD §11.4).
//!
//! An API response is one caller's data at one moment: a profile, a token, a
//! document, a file. Without a `Cache-Control` the browser, and any proxy
//! between it and the backend, may keep a copy by heuristic freshness, and a
//! copy outlives the session that fetched it. So every response from the
//! `/api/v1` nest says `Cache-Control: no-store`, authenticated or not, and a
//! refusal says it as well as a success.
//!
//! **The operational routes are exempt**, and [`EXEMPT`] names each with its
//! reason. They are outside the nest, so the rule is the router's shape rather
//! than a list the layer checks.
//!
//! **The unmatched path is the case to watch.** axum answers `/api/v1/nope`
//! from a fallback, and a nested router without one of its own borrows the
//! root's, which no layer on the nest wraps. The 404 below is that case; it
//! failed until the nest had its own fallback.
//!
//! **A route's own value stands** (`SetResponseHeader::if_not_present`). No
//! route sets one today, so that clause is pinned by a unit test in
//! `router.rs` over a test-local route rather than here.
//!
//! # Seen to fail
//!
//! Each mutation was made alone, this file run, and the mutation reverted.
//! **Seen red, 2026-11-04**, in `kelir-rust-tools:1.89`.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | The layer removed | every test here but [`EXEMPT`]'s, 5 of 6, and `attachment_download`'s happy path; the walk's 149 `/api/v1` cells |
//! | The layer on the root router | [`EXEMPT`]'s test, on all six routes, `/version` among them; the walk's five operational cells |
//! | The layer on the nest, with no fallback of its own | `an_unmatched_api_path_is_not_stored` alone: `/api/v1/nope answered 404 Not Found with Cache-Control []` |
//! | `overriding` for `if_not_present` | `router::tests::a_route_that_sets_its_own_cache_control_keeps_it` |

mod common;

use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use utoipa::OpenApi;
use uuid::Uuid;

use common::{RawResponse, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME, TEST_PEER};
use kelir_backend::router::ApiDoc;

/// The one value. `Pragma` and `Expires` are not sent: `no-store` is HTTP/1.1,
/// and nothing this API serves speaks an older dialect.
const NO_STORE: &str = "no-store";

/// **Outside the nest, and why.** A row here is a decision that the route's
/// answer may be cached by whoever reads it.
const EXEMPT: [(&str, &str); 6] = [
    ("/health", "a load balancer's probe; no data"),
    ("/health/live", "a liveness probe; no data"),
    ("/health/ready", "a readiness probe; no data"),
    ("/version", "the build, which deploy.sh reads"),
    (
        "/deployment",
        "unauthenticated by D-18; identifies the deployment only",
    ),
    ("/api/docs/openapi.json", "the public contract"),
];

/// A request through the whole router, keeping the headers [`TestApp::send`]
/// drops: they are the assertion here.
async fn answer(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> RawResponse {
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
    .expect("the request builds");

    // The auth routes resolve the caller's address from it; see `TEST_PEER`.
    request.extensions_mut().insert(ConnectInfo(TEST_PEER));

    let response = app
        .router()
        .oneshot(request)
        .await
        .expect("the router answers");

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body reads")
        .to_vec();

    RawResponse {
        status,
        headers,
        bytes,
    }
}

/// Exactly one `Cache-Control`, and it is `no-store`.
#[track_caller]
fn assert_no_store(response: &RawResponse, what: &str) {
    let values: Vec<_> = response
        .headers
        .get_all(header::CACHE_CONTROL)
        .iter()
        .map(|value| value.to_str().unwrap_or("<not text>").to_owned())
        .collect();

    assert_eq!(
        values,
        [NO_STORE],
        "{what} answered {} with Cache-Control {values:?}",
        response.status
    );
}

fn login_body(password: &str) -> Value {
    json!({ "username": ADMIN_USERNAME, "password": password })
}

// ---------------------------------------------------------------------------
// AC1, AC2: a success, and the auth routes with and without a token
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_signed_in_profile_is_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let me = answer(&app, Method::GET, "/api/v1/auth/me", Some(&token), None).await;

    assert_eq!(me.status, StatusCode::OK);
    assert_no_store(&me, "GET /auth/me with a token");
}

#[tokio::test]
async fn the_auth_routes_say_it_before_there_is_a_session() {
    let app = TestApp::spawn().await;

    // The answer carrying the tokens is the one a cache must never keep.
    let signed_in = answer(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(login_body(ADMIN_PASSWORD)),
    )
    .await;
    assert_eq!(signed_in.status, StatusCode::OK);
    assert_no_store(&signed_in, "a successful POST /auth/login");

    let refused = answer(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(login_body("not the password")),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_no_store(&refused, "a failed POST /auth/login");

    let anonymous = answer(&app, Method::GET, "/api/v1/auth/me", None, None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_no_store(&anonymous, "GET /auth/me with no token");
}

// ---------------------------------------------------------------------------
// AC3: the refusals, the unmatched path among them
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_refusal_is_not_stored_either() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let unknown_id = answer(
        &app,
        Method::GET,
        &format!("/api/v1/documents/{}", Uuid::now_v7()),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(unknown_id.status, StatusCode::NOT_FOUND);
    assert_no_store(&unknown_id, "a document that does not exist");

    let invalid = answer(
        &app,
        Method::POST,
        "/api/v1/document-types",
        Some(&token),
        Some(json!({ "typeCode": "PR_CACHE", "name": "PR_CACHE", "nope": true })),
    )
    .await;
    assert_eq!(
        invalid.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&invalid.bytes)
    );
    assert_no_store(&invalid, "a body with an unknown field");

    // A path that is routed, with a method that is not: the method router's
    // own answer, inside the route.
    let wrong_method = answer(&app, Method::DELETE, "/api/v1/auth/me", Some(&token), None).await;
    assert_eq!(wrong_method.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_no_store(&wrong_method, "DELETE /auth/me");
}

#[tokio::test]
async fn an_unmatched_api_path_is_not_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (uri, token) in [
        ("/api/v1/nope", Some(token.as_str())),
        ("/api/v1/nope", None),
        ("/api/v1/documents/not/a/route", Some(token.as_str())),
    ] {
        let unmatched = answer(&app, Method::GET, uri, token, None).await;

        assert_eq!(unmatched.status, StatusCode::NOT_FOUND, "{uri}");
        // The answer it gave before the layer: no envelope, no body (AC8).
        assert!(unmatched.bytes.is_empty(), "{uri}: {:?}", unmatched.bytes);
        assert_no_store(&unmatched, uri);
    }
}

/// **Every documented operation, with no token**, whatever it answers: the
/// rule is the nest's, not a route's, so a route added tomorrow is held to it
/// tomorrow. Outside the nest, nothing.
#[tokio::test]
async fn every_documented_operation_answers_by_where_it_is_mounted() {
    let app = TestApp::spawn().await;
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut failures = Vec::new();
    let mut walked = 0;
    for (template, item) in paths {
        for (key, method) in [
            ("get", Method::GET),
            ("post", Method::POST),
            ("put", Method::PUT),
            ("patch", Method::PATCH),
            ("delete", Method::DELETE),
        ] {
            if item[key].is_null() {
                continue;
            }

            // Every capture a fresh uuid: the answer's status is not the claim.
            let mut path = template.clone();
            while let Some(start) = path.find('{') {
                let end = start + path[start..].find('}').expect("a closed capture");
                path.replace_range(start..=end, &Uuid::now_v7().to_string());
            }

            let body = (method != Method::GET && method != Method::DELETE).then(|| json!({}));
            let response = answer(&app, method.clone(), &path, None, body).await;
            let cache_control = response.header(header::CACHE_CONTROL);
            let expected = template
                .starts_with("/api/v1/")
                .then(|| NO_STORE.to_owned());

            walked += 1;
            if cache_control != expected {
                failures.push(format!(
                    "{method} {template} answered {} with Cache-Control {cache_control:?}, \
                     expected {expected:?}",
                    response.status
                ));
            }
        }
    }

    assert!(walked > 100, "the walk reached only {walked} operations");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------
// AC5: the exempt routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_operational_routes_are_left_to_their_reader() {
    let app = TestApp::spawn().await;

    // Every route is asked before anything is asserted, so a failure names all
    // of them and not the first.
    let mut failures = Vec::new();
    for (uri, reason) in EXEMPT {
        let response = answer(&app, Method::GET, uri, None, None).await;
        let cache_control = response.header(header::CACHE_CONTROL);

        if response.status != StatusCode::OK || cache_control.is_some() {
            failures.push(format!(
                "{uri} answered {} with Cache-Control {cache_control:?}; \
                 it is exempt ({reason}) and answers 200 with none",
                response.status
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
