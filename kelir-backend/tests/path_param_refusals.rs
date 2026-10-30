//! **A value PostgreSQL cannot store is a 422 wherever a request carries it,
//! not a 500** ([#657](https://github.com/sujanto-gaws/kelir/issues/657), with
//! [#668](https://github.com/sujanto-gaws/kelir/issues/668); ADR-0050, which
//! extends ADR-0044).
//!
//! ADR-0044 refused a NUL in every query value and an out-of-range date in
//! every query bound, and left two places out: a path capture and a JSON body.
//! Both reached a bind. `GET /rad/lists/by-key/%00` answered 500, and so did
//! a NUL in a role code or a credential's `validFrom` of `-5000-01-01`.
//!
//! **This file walks the routes, as `list_route_refusals.rs` does for
//! queries.** It reads every operation from `ApiDoc::openapi()` and sends
//! every `{capture}` a NUL, and every JSON body a string holding one. It does
//! not classify a capture from the document: two of the four string captures
//! are on routes whose annotations declare no `params(...)`, so a walk that
//! trusted the document's types would miss them. Every capture is sent the
//! same thing.
//!
//! **What stops the walks from walking nothing.** Each guard counts a spelling
//! in every `modules/**/handlers.rs` — `PathParam<` and `JsonBody<` — and
//! requires the walked operations to number the same. A handler that reads a
//! capture or a body some other way is one the walk cannot see, and the count
//! reddens.
//!
//! **Dates in a body are not discoverable from the document** the way a
//! `date-time` query parameter is: a body is one schema, and its dates are
//! nested in it. So the date cells are written per route below, and a source
//! guard requires every date field of a `Deserialize` request struct to read
//! through a `utils::storable` helper, which is where the bound lives.
//!
//! # Seen red first (coding standard §2.9)
//!
//! Run 2026-10-30 against `main` at `d767b48`, this file written first and
//! nothing in `src/` changed. **The two walk guards were green**: 100
//! operations with a capture (113 captures) against 100 `PathParam<`, and 52
//! with a JSON body against 52 `JsonBody<`. **The source guard was red**: 22
//! date fields without a helper. All twelve tests failed.
//!
//! **The 500s:**
//!
//! - `GET /rad/lists/by-key/%00` and `/by-key/a%00b`, the path walk's only
//!   two.
//! - `PUT` and `DELETE /master-data/parties/{real}/roles/%00` and
//!   `…/ORGANIZATION_UNIT%00` with a valid body, four; and the `PUT` for a
//!   party that does not exist (P8).
//! - `POST /identity/roles` with `"roleCode":"R21-P7\u0000"` (P7).
//! - **`POST /auth/login` and `POST /auth/forgot-password` with a NUL in
//!   `username`, which take no token.**
//! - `validFrom` at `-5000-01-01` on `POST …/credentials` (P6); the day before
//!   PostgreSQL's first date in `validFrom` on both credential routes and in
//!   `workflows[0].validFrom` on both document-type routes, four; and `validTo`
//!   on the update-credential route (*date out of range*).
//!
//! **The wrong-code cells:** 218 uuid-capture cells answered 400; the two
//! `/rad/lookups/{source}` cells 404; the walk's body-less party-role cells
//! 415 (`PUT`) and 404 (`DELETE`, a party id that names nothing); 104 body
//! cells 422 `UNKNOWN_FIELD`; a NUL in login's `password` or `tenantCode` or
//! in refresh's token 401, in logout's 204, and in reset-password's token 422
//! `exists`; a body key 422 `UNKNOWN_FIELD`; and 26 date cells refused by a
//! later check, a window or a reference, without reading the date's range.
//! `POST /identity/delegations` **accepted** a `startsAt` a nanosecond before
//! PostgreSQL's first instant, 201. Seven `thruDate` cells passed on `main`,
//! because a window check answers `OUT_OF_RANGE` for an end before its start;
//! they now also require the bound's own message. The eight first-value cells
//! passed on `main`: PostgreSQL stores `-4713-11-24` and its midnight.
//!
//! **Seen red after the fix, 2026-10-30**, each mutation alone and reverted:
//!
//! - the `RawPathParams` check disabled — four tests here (the walk, both
//!   party-role tests, AC5/AC6) and three `extract` unit tests;
//! - `nul_refusal("id")` for every capture — the same four, two unit tests;
//! - `refuse_nul_in_body` removed — the body walk, the no-token test, P7 and
//!   the JSON controls, two unit tests;
//! - keys not walked — the JSON controls; the key unit test since also sends
//!   a key whose value holds no NUL;
//! - the NUL test made `is_control()` — two unit tests; the walk's control
//!   since carries `\\u0000` so that it is walked, not passed by the fast path;
//! - `>=` made `>` in the `NaiveDate` bound — both date tests here, three unit
//!   tests;
//! - one helper attribute removed (`customerSince`) — the source guard and
//!   the per-route date cells;
//! - the sentinel's mapping removed — P6, both date tests, one unit test;
//! - the fast path made always true — one unit test only, as intended: the
//!   walk's answers do not change;
//! - `date` for `option_date` on one field — does not compile.

mod common;

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use chrono::{TimeDelta, Utc};
use serde_json::{json, Value};
use tower::ServiceExt;
use utoipa::OpenApi;
use uuid::Uuid;

use common::{fixtures, TestApp};
use kelir_backend::router::ApiDoc;

const NUL_CODE: &str = "INVALID_CHARACTER";
const RANGE_CODE: &str = "OUT_OF_RANGE";

/// PostgreSQL's first `date` and first `timestamptz`, 4714-11-24 BC, as chrono
/// writes them, and the day and the nanosecond before.
const FIRST_DATE: &str = "-4713-11-24";
const DAY_BEFORE: &str = "-4713-11-23";
const FIRST_INSTANT: &str = "-4713-11-24T00:00:00Z";
const NANOSECOND_BEFORE: &str = "-4713-11-23T23:59:59.999999999Z";

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// A response, decoded far enough to assert on.
struct Answer {
    status: StatusCode,
    body: Value,
    text: String,
}

/// A peer address nobody else in this binary has used.
///
/// The `/auth` routes are behind the authentication rate limiter, which counts
/// by peer and runs before any extractor. A walk that sent every body from one
/// address would be answered 429 there, and that would be the limiter's answer
/// and not the extractor's.
fn fresh_peer() -> SocketAddr {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let [_, b, c, d] = n.to_be_bytes();

    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, b, c, d)), 40_000)
}

/// One request through the whole stack, the body as bytes and sent as JSON.
async fn send(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Bytes>,
) -> Answer {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let mut request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body)),
        None => builder.body(Body::empty()),
    }
    .unwrap_or_else(|error| panic!("build a request for {uri}: {error}"));
    request.extensions_mut().insert(ConnectInfo(fresh_peer()));

    let response = app
        .router()
        .oneshot(request)
        .await
        .unwrap_or_else(|error| panic!("drive the router for {uri}: {error}"));

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a response body reads");

    Answer {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn send_json(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: &Value,
) -> Answer {
    send(app, method, uri, token, Some(Bytes::from(body.to_string()))).await
}

fn details(answer: &Answer) -> Vec<Value> {
    answer.body["error"]["details"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The refusal ADR-0017's envelope describes: a 422 `VALIDATION_ERROR` with a
/// detail on `path` carrying `code` and `rule`.
fn refused_at(answer: &Answer, path: &str, code: &str, rule: &str) -> Result<(), String> {
    if answer.status != StatusCode::UNPROCESSABLE_ENTITY {
        return Err("expected 422".into());
    }
    if answer.body["success"] != false || answer.body["error"]["code"] != "VALIDATION_ERROR" {
        return Err("not the VALIDATION_ERROR envelope".into());
    }
    let named = details(answer).iter().any(|detail| {
        detail["path"] == path
            && detail["code"] == code
            && detail["rule"] == rule
            && detail["message"].is_string()
    });
    if named {
        Ok(())
    } else {
        Err(format!("no `{code}` detail with rule `{rule}` on `{path}`"))
    }
}

fn assert_refused_at(answer: &Answer, path: &str, code: &str, rule: &str, label: &str) {
    if let Err(problem) = refused_at(answer, path, code, rule) {
        panic!("{label}: {problem} — got {} {}", answer.status, answer.text);
    }
}

/// Whether the answer refuses the value as one PostgreSQL cannot store.
fn refuses_as_unstorable(answer: &Answer) -> bool {
    details(answer)
        .iter()
        .any(|detail| detail["code"] == NUL_CODE || detail["code"] == RANGE_CODE)
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

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
    /// The names of the path captures, in order, read from the template and
    /// not from `parameters`.
    captures: Vec<String>,
    bearer: bool,
    json_body: bool,
}

impl Operation {
    /// The path with `capture` given `value` and every other capture a uuid.
    fn path_with(&self, capture: &str, value: &str) -> String {
        let mut path = self.template.clone();
        for name in &self.captures {
            let given = if name == capture {
                value.to_owned()
            } else {
                Uuid::now_v7().to_string()
            };
            path = path.replace(&format!("{{{name}}}"), &given);
        }
        path
    }

    fn any_path(&self) -> String {
        self.path_with("", "")
    }

    fn label(&self) -> String {
        format!("{} {}", self.method, self.template)
    }
}

fn captures_of(template: &str) -> Vec<String> {
    template
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split('}').next())
        .map(ToOwned::to_owned)
        .collect()
}

/// Every operation in the document.
fn operations() -> Vec<Operation> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        for (key, method) in METHODS {
            let operation = &item[key];
            if !operation.is_object() {
                continue;
            }
            operations.push(Operation {
                method,
                template: template.clone(),
                captures: captures_of(template),
                bearer: operation["security"].as_array().is_some_and(|schemes| {
                    schemes.iter().any(|scheme| scheme.get("bearer").is_some())
                }),
                json_body: operation["requestBody"]["content"]["application/json"].is_object(),
            });
        }
    }

    operations
}

/// How many times `spelling` is written in a module's `handlers.rs`, counted
/// from the source rather than from the document the walk is checking.
fn handler_sites(spelling: &str) -> usize {
    fn visit(directory: &Path, spelling: &str, count: &mut usize) {
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                visit(&path, spelling, count);
            } else if path.file_name().is_some_and(|name| name == "handlers.rs") {
                let text = std::fs::read_to_string(&path).expect("the source reads");
                *count += text.matches(spelling).count();
            }
        }
    }

    let mut count = 0;
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules"),
        spelling,
        &mut count,
    );
    count
}

/// The failures of a walk, the 500s apart: a 500 is the defect these routes
/// had, and a wrong code is a refusal in the wrong words.
#[derive(Default)]
struct Failures {
    internal: Vec<String>,
    other: Vec<String>,
    cells: usize,
}

impl Failures {
    fn record(&mut self, at: String, answer: &Answer, problem: Option<String>) {
        self.cells += 1;
        let text = &answer.text[..answer.text.len().min(300)];
        if answer.status == StatusCode::INTERNAL_SERVER_ERROR {
            self.internal.push(format!("{at}: 500 {text}"));
        } else if let Some(problem) = problem {
            self.other
                .push(format!("{at}: {problem} — got {} {text}", answer.status));
        }
    }

    fn assert_none(&self, operations: usize) {
        assert!(
            self.internal.is_empty() && self.other.is_empty(),
            "{} of {} cells over {operations} operations failed.\n\
             {} answered 500:\n{}\n\
             {} answered something else:\n{}",
            self.internal.len() + self.other.len(),
            self.cells,
            self.internal.len(),
            self.internal.join("\n"),
            self.other.len(),
            self.other.join("\n"),
        );
    }
}

// ---------------------------------------------------------------------------
// AC1–AC3: a NUL in any path capture
// ---------------------------------------------------------------------------

/// **The path walk** (AC1, AC2). Every capture of every operation, any method,
/// is sent `%00` and `a%00b` while the others hold a uuid, as the bootstrap
/// administrator, who holds every permission. Each must be a 422
/// `INVALID_CHARACTER` naming that capture and no 500. `a%01b` is the control:
/// it is never refused as a NUL.
///
/// No body is sent. `PathParam` runs before `JsonBody`, so a capture is judged
/// before a body is; the party-role routes are walked again with a real party
/// and a valid body below, where a body-less `PUT` would stop at the body.
#[tokio::test]
async fn a_nul_in_any_path_capture_is_refused_on_every_route() {
    let operations: Vec<Operation> = operations()
        .into_iter()
        .filter(|operation| !operation.captures.is_empty())
        .collect();

    // --- The guard (AC2) -----------------------------------------------------
    let sites = handler_sites("PathParam<");
    let walked: Vec<String> = operations.iter().map(Operation::label).collect();
    assert_eq!(
        operations.len(),
        sites,
        "the walk found {} operations with a path capture, and the handlers hold {sites} \
         `PathParam<` — a handler that reads a capture another way is one the walk cannot \
         see. Walked: {walked:#?}",
        operations.len()
    );

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mut failures = Failures::default();
    let mut beyond_a_uuid = Vec::new();

    for operation in &operations {
        for capture in &operation.captures {
            for value in ["%00", "a%00b"] {
                let uri = operation.path_with(capture, value);
                let answer = send(&app, operation.method.clone(), &uri, Some(&token), None).await;
                let problem = refused_at(&answer, capture, NUL_CODE, "pattern").err();
                failures.record(
                    format!(
                        "{} {}",
                        operation.method,
                        operation
                            .template
                            .replace(&format!("{{{capture}}}"), value,)
                    ),
                    &answer,
                    problem,
                );
            }

            // The control: the refusal keys on the NUL, not on the capture or
            // on an unusual character.
            let uri = operation.path_with(capture, "a%01b");
            let answer = send(&app, operation.method.clone(), &uri, Some(&token), None).await;
            let problem = refuses_as_unstorable(&answer)
                .then(|| "a%01b was refused as unstorable".to_owned());
            failures.record(
                format!(
                    "{} {} with a%01b in {capture}",
                    operation.method, operation.template
                ),
                &answer,
                problem,
            );
            if answer.status != StatusCode::BAD_REQUEST {
                beyond_a_uuid.push(format!("{} {capture}", operation.label()));
            }
        }
    }

    // A walk whose every capture is a uuid would hold nothing a string capture
    // can carry to a bind: `a%01b` is a 400 on a uuid and something else on a
    // string.
    assert!(
        !beyond_a_uuid.is_empty(),
        "every capture answered a%01b with 400, so the walk reached no string capture"
    );

    failures.assert_none(operations.len());
}

// ---------------------------------------------------------------------------
// AC4: the party-role routes, with a real party and a valid body
// ---------------------------------------------------------------------------

const PARTIES: &str = "/api/v1/master-data/parties";

async fn party(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = send_json(
        app,
        Method::POST,
        PARTIES,
        Some(token),
        &json!({
            "partyId": code,
            "partyTypeId": "PERSON",
            "person": { "firstName": "Ani", "lastName": "Wijaya" },
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);

    created.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("a created party has an id")
}

/// **A body that would be accepted**, so the only thing wrong with the
/// request is the capture. An empty body is refused by `JsonBody` before the
/// service binds anything, which would pass on `main` for the wrong reason.
fn assignment() -> Value {
    json!({ "fromDate": "2026-01-01T00:00:00Z" })
}

#[tokio::test]
async fn a_nul_role_type_is_refused_for_a_real_party_on_both_party_role_routes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = party(&app, &token, "R13-PARTY").await;
    let mut failures = Failures::default();

    for role_type in ["%00", "ORGANIZATION_UNIT%00"] {
        let uri = format!("{PARTIES}/{id}/roles/{role_type}");

        let put = send_json(&app, Method::PUT, &uri, Some(&token), &assignment()).await;
        let problem = refused_at(&put, "roleTypeId", NUL_CODE, "pattern").err();
        failures.record(format!("PUT …/roles/{role_type}"), &put, problem);

        let delete = send(&app, Method::DELETE, &uri, Some(&token), None).await;
        let problem = refused_at(&delete, "roleTypeId", NUL_CODE, "pattern").err();
        failures.record(format!("DELETE …/roles/{role_type}"), &delete, problem);
    }
    failures.assert_none(2);

    // Nothing was assigned on the way.
    let roles = send(
        &app,
        Method::GET,
        &format!("{PARTIES}/{id}/roles"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(roles.status, StatusCode::OK, "{}", roles.text);
    assert!(
        !roles.text.contains("ORGANIZATION_UNIT"),
        "a refused assignment was stored: {}",
        roles.text
    );
}

/// **P8**: the service binds the role type before it looks the party up, so a
/// party that does not exist is no protection.
#[tokio::test]
async fn p8_a_nul_role_type_is_refused_for_a_party_that_does_not_exist() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let uri = format!("{PARTIES}/{}/roles/%00", Uuid::now_v7());

    let put = send_json(&app, Method::PUT, &uri, Some(&token), &assignment()).await;

    assert_refused_at(&put, "roleTypeId", NUL_CODE, "pattern", "P8");
}

// ---------------------------------------------------------------------------
// AC5, AC6: a capture without a NUL is answered as before
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_capture_without_a_nul_is_answered_as_it_was() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let id = party(&app, &token, "R13-SECOND").await;
    let get = |uri: String| {
        let app = &app;
        let token = &token;
        async move { send(app, Method::GET, &uri, Some(token), None).await }
    };

    // Another control character is legal `text`: the list is not found.
    let by_key = get("/api/v1/rad/lists/by-key/a%01b".into()).await;
    assert_eq!(by_key.status, StatusCode::NOT_FOUND, "{}", by_key.text);

    // A role type that does not exist is the role-type miss, not the NUL's.
    let miss = send_json(
        &app,
        Method::PUT,
        &format!("{PARTIES}/{id}/roles/a%01b"),
        Some(&token),
        &assignment(),
    )
    .await;
    assert_refused_at(
        &miss,
        "roleTypeId",
        "NOT_FOUND",
        "exists",
        "a%01b role type",
    );

    // A capture that is not UTF-8 once decoded stays the 400 it was.
    for uri in [
        "/api/v1/rad/lists/by-key/%FF".to_owned(),
        format!("{PARTIES}/{id}/roles/%FF"),
        format!("{PARTIES}/%FF"),
    ] {
        let answer = get(uri.clone()).await;
        // `GET` on the role route is not served; the other two are.
        if answer.status == StatusCode::METHOD_NOT_ALLOWED {
            continue;
        }
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "{uri}: {}",
            answer.text
        );
    }
    let not_utf8 = send_json(
        &app,
        Method::PUT,
        &format!("{PARTIES}/{id}/roles/%FF"),
        Some(&token),
        &assignment(),
    )
    .await;
    assert_eq!(
        not_utf8.status,
        StatusCode::BAD_REQUEST,
        "{}",
        not_utf8.text
    );

    // AC6: an unknown source and an unknown list key are still not found, and
    // a NUL in the source is the NUL's refusal, ahead of the lookup.
    let source = get("/api/v1/rad/lookups/nonesuch/options".into()).await;
    assert_eq!(source.status, StatusCode::NOT_FOUND, "{}", source.text);
    let key = get("/api/v1/rad/lists/by-key/nonesuch".into()).await;
    assert_eq!(key.status, StatusCode::NOT_FOUND, "{}", key.text);
    let nul_source = get("/api/v1/rad/lookups/%00/options".into()).await;
    assert_refused_at(&nul_source, "source", NUL_CODE, "pattern", "lookup source");

    // And the real thing still succeeds.
    let source = get("/api/v1/rad/lookups/supplier/options".into()).await;
    assert_eq!(source.status, StatusCode::OK, "{}", source.text);

    let created = send_json(
        &app,
        Method::POST,
        "/api/v1/rad/lists",
        Some(&token),
        &json!({
            "listKey": "r13-requisitions",
            "title": "Requisitions",
            "status": "ACTIVE",
            "columns": [{ "columnKey": "title", "label": "Subject" }],
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    // A list renders once a document type names it.
    let bound = send_json(
        &app,
        Method::POST,
        "/api/v1/document-types",
        Some(&token),
        &json!({ "typeCode": "R13_LISTED", "name": "Listed", "listId": id_of(&created) }),
    )
    .await;
    assert_eq!(bound.status, StatusCode::CREATED, "{}", bound.text);
    let rendered = get("/api/v1/rad/lists/by-key/r13-requisitions".into()).await;
    assert_eq!(rendered.status, StatusCode::OK, "{}", rendered.text);

    let assigned = send_json(
        &app,
        Method::PUT,
        &format!("{PARTIES}/{id}/roles/ORGANIZATION_UNIT"),
        Some(&token),
        &assignment(),
    )
    .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.text);
    let removed = send(
        &app,
        Method::DELETE,
        &format!("{PARTIES}/{id}/roles/ORGANIZATION_UNIT"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.text);
}

// ---------------------------------------------------------------------------
// AC8–AC10: a NUL in a JSON body
// ---------------------------------------------------------------------------

/// **The body walk** (AC8). Every operation with a JSON request body is sent
/// a body holding a NUL at the top and one nested in an array, with a uuid in
/// every capture, and must answer a 422 `INVALID_CHARACTER` at the path as
/// sent. The refusal runs before the body is fitted to its type, so the body
/// need not be one the route would accept, and an unknown field is not what
/// it is refused for. `\u0001` is the control.
///
/// The operations that take no token, `/auth`'s among them, are sent no
/// token: a NUL there is reachable by anyone.
#[tokio::test]
async fn a_nul_in_any_json_body_string_is_refused_on_every_route() {
    let all = operations();
    let operations: Vec<&Operation> = all.iter().filter(|op| op.json_body).collect();

    // --- The guard -----------------------------------------------------------
    let sites = handler_sites("JsonBody<");
    let walked: Vec<String> = operations.iter().map(|op| op.label()).collect();
    assert_eq!(
        operations.len(),
        sites,
        "the walk found {} operations with a JSON request body, and the handlers hold \
         {sites} `JsonBody<` — a handler that reads a body and documents none is one the \
         walk cannot see. Walked: {walked:#?}",
        operations.len()
    );
    assert!(
        operations.iter().any(|op| !op.bearer),
        "no operation without a token takes a body, so the walk reaches none of /auth"
    );

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mut failures = Failures::default();

    // The control carries the escape's text too, `\\u0000`, so the body is
    // walked rather than passed by the fast path: a walk that refused any
    // control character would refuse it.
    let cells: [(Value, Option<&str>); 3] = [
        (json!({ "walkedField": "a\u{0}b" }), Some("walkedField")),
        (json!({ "a": [{ "b": "\u{0}" }] }), Some("a[0].b")),
        (
            json!({ "walkedField": "a\u{1}b", "escaped": "\\u0000" }),
            None,
        ),
    ];

    for operation in &operations {
        let caller = operation.bearer.then_some(token.as_str());
        for (body, path) in &cells {
            let answer = send_json(
                &app,
                operation.method.clone(),
                &operation.any_path(),
                caller,
                body,
            )
            .await;
            let problem = match path {
                Some(path) => refused_at(&answer, path, NUL_CODE, "pattern").err(),
                None => refuses_as_unstorable(&answer)
                    .then(|| "\\u0001 was refused as unstorable".to_owned()),
            };
            failures.record(
                format!("{} with {body}", operation.label()),
                &answer,
                problem,
            );
        }
    }

    failures.assert_none(operations.len());
}

/// **No token is needed to reach these**, so a NUL in a real field of each is
/// the probe that matters most: a 500 here is one anybody can cause.
#[tokio::test]
async fn a_nul_in_a_field_of_a_route_that_takes_no_token_is_refused() {
    let app = TestApp::spawn().await;
    let mut failures = Failures::default();

    let cells = [
        (
            "/api/v1/auth/login",
            json!({ "username": "admin.kelir\u{0}", "password": "x" }),
            "username",
        ),
        (
            "/api/v1/auth/login",
            json!({ "username": "admin.kelir", "password": "x\u{0}" }),
            "password",
        ),
        (
            "/api/v1/auth/login",
            json!({ "username": "admin.kelir", "password": "x", "tenantCode": "SYS\u{0}" }),
            "tenantCode",
        ),
        (
            "/api/v1/auth/refresh",
            json!({ "refreshToken": "a\u{0}b" }),
            "refreshToken",
        ),
        (
            "/api/v1/auth/logout",
            json!({ "refreshToken": "a\u{0}b" }),
            "refreshToken",
        ),
        (
            "/api/v1/auth/forgot-password",
            json!({ "username": "admin.kelir\u{0}" }),
            "username",
        ),
        (
            "/api/v1/auth/reset-password",
            json!({ "token": "a\u{0}b", "newPassword": "a-sufficiently-long-password" }),
            "token",
        ),
    ];

    for (uri, body, path) in &cells {
        let answer = send_json(&app, Method::POST, uri, None, body).await;
        let problem = refused_at(&answer, path, NUL_CODE, "pattern").err();
        failures.record(format!("POST {uri} with {body}"), &answer, problem);
    }

    failures.assert_none(cells.len());
}

/// **P7**: a role code is bound as it was sent.
#[tokio::test]
async fn p7_a_nul_in_a_role_code_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let answer = send_json(
        &app,
        Method::POST,
        "/api/v1/identity/roles",
        Some(&token),
        &json!({ "roleCode": "R21-P7\u{0}", "name": "P7" }),
    )
    .await;

    assert_refused_at(&answer, "roleCode", NUL_CODE, "pattern", "P7");

    // The same body without the NUL is a role.
    let created = send_json(
        &app,
        Method::POST,
        "/api/v1/identity/roles",
        Some(&token),
        &json!({ "roleCode": "R21-P7", "name": "P7" }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
}

/// AC10: what is not JSON stays a 400, a key is refused as a value is, and a
/// free-form value is walked like a typed one.
#[tokio::test]
async fn json_controls_a_key_and_a_free_form_value() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let roles = "/api/v1/identity/roles";
    let raw = |bytes: &'static [u8]| Some(Bytes::from_static(bytes));

    // A lone surrogate and a raw NUL byte are not JSON: there is no field to
    // name, so they stay the 400 they were. The second also holds the
    // escape, so the walk is entered and must not answer for the parse.
    for (label, body) in [
        (
            "a lone surrogate",
            raw(br#"{"roleCode":"\ud800","name":"x"}"#),
        ),
        (
            "a lone surrogate beside an escaped NUL",
            raw(br#"{"roleCode":"\ud800","name":"\u0000"}"#),
        ),
        (
            "a raw NUL byte",
            raw(b"{\"roleCode\":\"a\x00b\",\"name\":\"x\"}"),
        ),
    ] {
        let answer = send(&app, Method::POST, roles, Some(&token), body).await;
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "{label}: {}",
            answer.text
        );
    }

    // A key holding a NUL is refused at its path as sent. A query key is not
    // (ADR-0044 §4): a body key can be bound, as a free-form object's is.
    let key = send(
        &app,
        Method::POST,
        roles,
        Some(&token),
        raw(br#"{"roleCode":"R13","name":"x","a\u0000b":1}"#),
    )
    .await;
    assert_refused_at(&key, "a\u{0}b", NUL_CODE, "pattern", "a key");

    // A backslash before `u0000` is text, not a NUL: the escape the fast path
    // looks for is there, and the walk finds nothing.
    let escaped = send(
        &app,
        Method::POST,
        roles,
        Some(&token),
        raw(br#"{"roleCode":"R13-ESCAPED","name":"\\u0000"}"#),
    )
    .await;
    assert_eq!(escaped.status, StatusCode::CREATED, "{}", escaped.text);
    assert_eq!(escaped.body["data"]["name"], "\\u0000");

    // A free-form `Value`: a form definition is stored as `jsonb`, which
    // cannot hold `\u0000` either.
    let form = send_json(
        &app,
        Method::POST,
        "/api/v1/rad/forms",
        Some(&token),
        &json!({
            "formKey": "r13-form",
            "title": "Form",
            "definition": { "formId": "r13-form", "note": "a\u{0}b" },
        }),
    )
    .await;
    assert_refused_at(
        &form,
        "definition.note",
        NUL_CODE,
        "pattern",
        "a free-form value",
    );

    // Every string holding one is named, each once, and a key whose value is
    // also a NUL is named once.
    let both = send(
        &app,
        Method::POST,
        roles,
        Some(&token),
        raw(br#"{"roleCode":"\u0000","name":"\u0000","x\u0000":"\u0000"}"#),
    )
    .await;
    let mut paths: Vec<String> = details(&both)
        .iter()
        .map(|detail| detail["path"].as_str().unwrap_or_default().to_owned())
        .collect();
    paths.sort();
    assert_eq!(paths, ["name", "roleCode", "x\u{0}"], "{}", both.text);
}

// ---------------------------------------------------------------------------
// AC11, AC12: a date in a body PostgreSQL cannot store
// ---------------------------------------------------------------------------

const SYSTEMS: &str = "/api/v1/integration/external-systems";

fn id_of(answer: &Answer) -> Uuid {
    answer.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", answer.text))
}

async fn system(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = send_json(
        app,
        Method::POST,
        SYSTEMS,
        Some(token),
        &json!({ "systemCode": code, "systemName": format!("{code} system") }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    id_of(&created)
}

/// **P6**: a credential's `validFrom` was bound as parsed.
#[tokio::test]
async fn p6_a_credential_valid_from_before_4714_bc_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "R13-P6").await;
    let path = format!("{SYSTEMS}/{system}/credentials");

    let answer = send_json(
        &app,
        Method::POST,
        &path,
        Some(&token),
        &json!({
            "credentialType": "CERTIFICATE",
            "secretReference": "vault://kelir/bank/cert",
            "validFrom": "-5000-01-01",
        }),
    )
    .await;

    assert_refused_at(&answer, "validFrom", RANGE_CODE, "range", "P6");
}

/// A published form, which a document type may bind.
async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = send_json(
        app,
        Method::POST,
        "/api/v1/rad/forms",
        Some(token),
        &json!({
            "formKey": key,
            "title": "Purchase requisition",
            "definition": {
                "formId": key,
                "version": "2.0.1",
                "components": [{
                    "id": "quantity", "role": "data", "type": "number", "key": "quantity",
                    "label": "Quantity", "validation": { "type": "number" }
                }]
            },
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let id = id_of(&created);

    let published = send(
        app,
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/publish"),
        Some(token),
        None,
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.text);
    id
}

/// A published workflow definition, which a document type may bind.
async fn published_workflow(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = send_json(
        app,
        Method::POST,
        "/api/v1/workflow/definitions",
        Some(token),
        &json!({
            "workflowKey": key,
            "name": "Standard approval",
            "definition": {
                "workflowKey": key,
                "version": "1.0.0",
                "name": "Standard approval",
                "initialState": "APPROVAL",
                "states": [
                    { "code": "APPROVAL", "name": "Approval",
                      "mapsToDocumentStatus": "PENDING_APPROVAL",
                      "task": { "taskDefinitionKey": "approval", "taskName": "Approve",
                                "assignment": { "assigneeType": "OWNER" } } },
                    { "code": "COMPLETED", "name": "Completed",
                      "mapsToDocumentStatus": "COMPLETED", "isFinal": true }
                ],
                "transitions": [
                    { "from": "APPROVAL", "to": "COMPLETED", "action": "APPROVE",
                      "allowedBy": "OWNER" }
                ]
            },
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let id = id_of(&created);

    let publication = send_json(
        app,
        Method::POST,
        &format!("/api/v1/workflow/definitions/{id}/publication"),
        Some(token),
        &json!({}),
    )
    .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.text);
    id
}

/// A date field a body carries: where it sits in the body, the path a refusal
/// names it by, and whether it is a `date` or a `timestamptz`.
struct DateField {
    pointer: &'static str,
    path: &'static str,
    instant: bool,
}

const fn day(pointer: &'static str, path: &'static str) -> DateField {
    DateField {
        pointer,
        path,
        instant: false,
    }
}

const fn instant(pointer: &'static str, path: &'static str) -> DateField {
    DateField {
        pointer,
        path,
        instant: true,
    }
}

/// One route's date cells.
struct DateRoute {
    method: Method,
    path: String,
    /// A body every date field below sits in, each in range. Only its shape
    /// matters: the refusal comes while it is being read.
    full: Value,
    fields: Vec<DateField>,
    /// A body the route accepts, holding the first field at PostgreSQL's
    /// first value.
    accepted: Value,
}

const PARTY_INPUTS: [DateField; 11] = [
    day("/person/birthDate", "person.birthDate"),
    day(
        "/identifications/0/issueDate",
        "identifications[0].issueDate",
    ),
    day(
        "/identifications/0/expireDate",
        "identifications[0].expireDate",
    ),
    instant(
        "/relationshipsFrom/0/fromDate",
        "relationshipsFrom[0].fromDate",
    ),
    instant(
        "/relationshipsFrom/0/thruDate",
        "relationshipsFrom[0].thruDate",
    ),
    instant("/relationshipsTo/0/fromDate", "relationshipsTo[0].fromDate"),
    instant("/relationshipsTo/0/thruDate", "relationshipsTo[0].thruDate"),
    instant("/classifications/0/fromDate", "classifications[0].fromDate"),
    instant("/classifications/0/thruDate", "classifications[0].thruDate"),
    instant(
        "/contactMechanisms/0/fromDate",
        "contactMechanisms[0].fromDate",
    ),
    instant(
        "/contactMechanisms/0/thruDate",
        "contactMechanisms[0].thruDate",
    ),
];

/// Every party input that carries a date, each holding one in range.
fn party_inputs(full: &mut Value) {
    let relationship = json!({
        "partyIdFrom": "A", "partyIdTo": "B", "partyRelationshipTypeId": "EMPLOYMENT",
        "fromDate": "2026-01-01T00:00:00Z", "thruDate": "2026-12-31T00:00:00Z",
    });
    full["person"] = json!({ "firstName": "Ani", "birthDate": "1990-01-01" });
    full["identifications"] = json!([{
        "partyIdentificationTypeId": "NPWP", "idValue": "1",
        "issueDate": "2020-01-01", "expireDate": "2030-01-01",
    }]);
    full["relationshipsFrom"] = json!([relationship.clone()]);
    full["relationshipsTo"] = json!([relationship]);
    full["classifications"] = json!([{
        "partyClassTypeId": "SIZE",
        "fromDate": "2026-01-01T00:00:00Z", "thruDate": "2026-12-31T00:00:00Z",
    }]);
    full["contactMechanisms"] = json!([{
        "contactMechTypeId": "EMAIL_ADDRESS",
        "fromDate": "2026-01-01T00:00:00Z", "thruDate": "2026-12-31T00:00:00Z",
    }]);
}

/// The eight routes whose bodies carry a date PostgreSQL narrows, with what
/// each needs to accept one.
async fn date_routes(app: &TestApp, token: &str) -> Vec<DateRoute> {
    let system = system(app, token, "R13-DATES").await;
    let credentials = format!("{SYSTEMS}/{system}/credentials");
    let credential = send_json(
        app,
        Method::POST,
        &credentials,
        Some(token),
        &json!({ "credentialType": "CERTIFICATE", "secretReference": "vault://kelir/r13/cert" }),
    )
    .await;
    assert_eq!(
        credential.status,
        StatusCode::CREATED,
        "{}",
        credential.text
    );
    let credential = id_of(&credential);

    let form = published_form(app, token, "r13-dates-form").await;
    let workflow = published_workflow(app, token, "wf_r13_dates").await;
    let binding = |valid_from: &str| {
        json!([{
            "workflowDefinitionId": workflow, "priority": 1,
            "validFrom": valid_from, "validTo": "2030-01-01",
        }])
    };
    let document_type = send_json(
        app,
        Method::POST,
        "/api/v1/document-types",
        Some(token),
        &json!({ "typeCode": "R13_EXISTING", "name": "Existing", "formId": form }),
    )
    .await;
    assert_eq!(
        document_type.status,
        StatusCode::CREATED,
        "{}",
        document_type.text
    );
    let document_type = id_of(&document_type);

    let updated_party = party(app, token, "R13-UPDATED").await;
    let role_party = party(app, token, "R13-ROLE").await;

    let delegate = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "r13.delegate",
        "r13.delegate@example.test",
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;
    let ends_at = (Utc::now() + TimeDelta::days(7)).to_rfc3339();

    let mut new_party = json!({ "partyId": "R13-FULL", "partyTypeId": "PERSON" });
    party_inputs(&mut new_party);
    let mut changed_party = json!({});
    party_inputs(&mut changed_party);

    vec![
        DateRoute {
            method: Method::POST,
            path: credentials.clone(),
            full: json!({
                "credentialType": "CERTIFICATE", "secretReference": "vault://kelir/r13/a",
                "validFrom": "2026-01-01", "validTo": "2027-01-01",
            }),
            fields: vec![day("/validFrom", "validFrom"), day("/validTo", "validTo")],
            accepted: json!({
                "credentialType": "CERTIFICATE", "secretReference": "vault://kelir/r13/b",
                "validFrom": FIRST_DATE,
            }),
        },
        DateRoute {
            method: Method::PUT,
            path: format!("{credentials}/{credential}"),
            full: json!({ "validFrom": "2026-01-01", "validTo": "2027-01-01" }),
            fields: vec![day("/validFrom", "validFrom"), day("/validTo", "validTo")],
            accepted: json!({ "validFrom": FIRST_DATE }),
        },
        DateRoute {
            method: Method::POST,
            path: "/api/v1/document-types".into(),
            full: json!({
                "typeCode": "R13_FULL", "name": "Full", "workflows": binding("2026-01-01"),
            }),
            fields: vec![
                day("/workflows/0/validFrom", "workflows[0].validFrom"),
                day("/workflows/0/validTo", "workflows[0].validTo"),
            ],
            accepted: json!({
                "typeCode": "R13_FIRST", "name": "First", "formId": form,
                "workflows": binding(FIRST_DATE),
            }),
        },
        DateRoute {
            method: Method::PUT,
            path: format!("/api/v1/document-types/{document_type}"),
            full: json!({ "workflows": binding("2026-01-01") }),
            fields: vec![
                day("/workflows/0/validFrom", "workflows[0].validFrom"),
                day("/workflows/0/validTo", "workflows[0].validTo"),
            ],
            accepted: json!({ "workflows": binding(FIRST_DATE) }),
        },
        DateRoute {
            method: Method::POST,
            path: PARTIES.into(),
            full: new_party,
            fields: PARTY_INPUTS.into(),
            accepted: json!({
                "partyId": "R13-FIRST", "partyTypeId": "PERSON",
                "person": { "firstName": "Ani", "lastName": "Wijaya", "birthDate": FIRST_DATE },
            }),
        },
        DateRoute {
            method: Method::PUT,
            path: format!("{PARTIES}/{updated_party}"),
            full: changed_party,
            fields: PARTY_INPUTS.into(),
            accepted: json!({
                "person": { "firstName": "Ani", "lastName": "Wijaya", "birthDate": FIRST_DATE },
            }),
        },
        DateRoute {
            method: Method::PUT,
            path: format!("{PARTIES}/{role_party}/roles/CUSTOMER"),
            full: json!({
                "fromDate": "2026-01-01T00:00:00Z", "thruDate": "2026-12-31T00:00:00Z",
                "profile": {
                    "customer": { "customerSince": "2024-06-01" },
                    "employee": { "joinDate": "2024-06-01", "resignDate": "2025-06-01" },
                },
            }),
            fields: vec![
                instant("/fromDate", "fromDate"),
                instant("/thruDate", "thruDate"),
                day(
                    "/profile/customer/customerSince",
                    "profile.customer.customerSince",
                ),
                day("/profile/employee/joinDate", "profile.employee.joinDate"),
                day(
                    "/profile/employee/resignDate",
                    "profile.employee.resignDate",
                ),
            ],
            accepted: json!({
                "fromDate": FIRST_INSTANT,
                "profile": { "customer": { "customerNumber": "R13-C1", "customerSince": FIRST_DATE } },
            }),
        },
        DateRoute {
            method: Method::POST,
            path: "/api/v1/identity/delegations".into(),
            full: json!({
                "delegateUserId": delegate,
                "startsAt": "2026-01-01T00:00:00Z", "endsAt": ends_at,
            }),
            fields: vec![
                instant("/startsAt", "startsAt"),
                instant("/endsAt", "endsAt"),
            ],
            accepted: json!({
                "delegateUserId": delegate, "startsAt": FIRST_INSTANT, "endsAt": ends_at,
            }),
        },
    ]
}

/// **Per route, per date field** (AC12): the day before PostgreSQL's first
/// `date`, or the nanosecond before its first `timestamptz`, is a 422
/// `OUT_OF_RANGE` at the field's path, nested where it is nested. The first
/// value itself is accepted and stored.
#[tokio::test]
async fn every_body_date_is_refused_below_postgresqls_range_and_accepted_at_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let routes = date_routes(&app, &token).await;
    let mut failures = Failures::default();
    let mut fields = 0;

    for route in &routes {
        for field in &route.fields {
            fields += 1;
            let mut body = route.full.clone();
            *body
                .pointer_mut(field.pointer)
                .unwrap_or_else(|| panic!("{} is not in {}", field.pointer, route.full)) =
                json!(if field.instant {
                    NANOSECOND_BEFORE
                } else {
                    DAY_BEFORE
                });

            let answer =
                send_json(&app, route.method.clone(), &route.path, Some(&token), &body).await;
            // The bound's own refusal, which names the bound. A window check
            // that answers `OUT_OF_RANGE` for an end before its start would
            // otherwise pass a `thruDate` cell on `main` without reading the
            // date's range at all.
            let problem = refused_at(&answer, field.path, RANGE_CODE, "range")
                .and_then(|()| {
                    details(&answer)
                        .iter()
                        .any(|detail| {
                            detail["path"] == field.path
                                && detail["message"]
                                    .as_str()
                                    .is_some_and(|message| message.contains("-4713-11-24"))
                        })
                        .then_some(())
                        .ok_or_else(|| "the refusal does not name PostgreSQL's bound".to_owned())
                })
                .err();
            failures.record(
                format!("{} {} at {}", route.method, route.path, field.path),
                &answer,
                problem,
            );
        }

        let answer = send_json(
            &app,
            route.method.clone(),
            &route.path,
            Some(&token),
            &route.accepted,
        )
        .await;
        let stored = route
            .fields
            .first()
            .map(|field| {
                if field.instant {
                    FIRST_INSTANT
                } else {
                    FIRST_DATE
                }
            })
            .expect("a route with a date");
        let problem = if !answer.status.is_success() {
            Some(format!("PostgreSQL's first {stored} was not accepted"))
        } else if !answer.text.contains(stored) {
            Some(format!("{stored} did not come back as sent"))
        } else {
            None
        };
        failures.record(
            format!("{} {} with {stored}", route.method, route.path),
            &answer,
            problem,
        );
    }

    failures.assert_none(routes.len());
    assert_eq!(routes.len(), 8, "the eight routes #668 counted");
    // 22 fields: the nine party-input fields are sent on two routes, and the
    // relationship's two in both of a party's relationship collections.
    assert_eq!(
        fields, 37,
        "the cells the 22 fields make on the eight routes"
    );
}

/// `Option<Option<NaiveDate>>` on the update-credential route: below range is
/// refused, the first date is stored, and `null` still clears.
#[tokio::test]
async fn an_updated_credential_bound_is_refused_below_range_and_cleared_by_null() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let system = system(&app, &token, "R13-UPDATE").await;
    let credentials = format!("{SYSTEMS}/{system}/credentials");
    let created = send_json(
        &app,
        Method::POST,
        &credentials,
        Some(&token),
        &json!({
            "credentialType": "CERTIFICATE", "secretReference": "vault://kelir/r13/u",
            "validTo": "2030-01-01",
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let path = format!("{credentials}/{}", id_of(&created));

    let below = send_json(
        &app,
        Method::PUT,
        &path,
        Some(&token),
        &json!({ "validTo": DAY_BEFORE }),
    )
    .await;
    assert_refused_at(
        &below,
        "validTo",
        RANGE_CODE,
        "range",
        "validTo below range",
    );
    let unchanged = send(&app, Method::GET, &path, Some(&token), None).await;
    assert_eq!(
        unchanged.body["data"]["validTo"], "2030-01-01",
        "a refusal wrote"
    );

    let first = send_json(
        &app,
        Method::PUT,
        &path,
        Some(&token),
        &json!({ "validFrom": FIRST_DATE }),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);
    assert_eq!(first.body["data"]["validFrom"], FIRST_DATE);
    assert_eq!(first.body["data"]["validTo"], "2030-01-01", "left alone");

    let cleared = send_json(
        &app,
        Method::PUT,
        &path,
        Some(&token),
        &json!({ "validTo": null }),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.text);
    assert_eq!(cleared.body["data"]["validTo"], Value::Null);
    assert_eq!(cleared.body["data"]["validFrom"], FIRST_DATE, "left alone");
}

// ---------------------------------------------------------------------------
// AC13: every request date reads through a storable helper
// ---------------------------------------------------------------------------

/// The helper a date field of this type must name, or `None` for a shape no
/// helper reads.
fn helper_for(field_type: &str) -> Option<&'static str> {
    match field_type {
        "NaiveDate" => Some("date"),
        "Option<NaiveDate>" => Some("option_date"),
        "Option<Option<NaiveDate>>" => Some("present_or_absent_date"),
        "DateTime<Utc>" => Some("instant"),
        "Option<DateTime<Utc>>" => Some("option_instant"),
        _ => None,
    }
}

/// One struct field: its attributes, name and type, as written.
struct Field {
    attributes: String,
    name: String,
    field_type: String,
}

/// The `Deserialize` structs in `text` that are not query parameters, and
/// their fields. A hand parse, line by line, of the shapes the crate writes:
/// attributes above the struct, `{` on its line, one field per line, and a
/// `#[cfg(test)] mod tests` at the bottom of a file, which is not scanned.
fn request_structs(text: &str) -> Vec<(String, Vec<Field>)> {
    /// An attribute can span lines; it ends where its brackets close.
    fn open_brackets(line: &str) -> i32 {
        line.chars().fold(0, |depth, character| match character {
            '[' => depth + 1,
            ']' => depth - 1,
            _ => depth,
        })
    }

    /// Gathers attributes line by line. Returns `true` when the line was one,
    /// or part of one, or a comment between them.
    fn gather(line: &str, attributes: &mut String, depth: &mut i32) -> bool {
        if *depth > 0 || line.starts_with("#[") {
            attributes.push_str(line);
            *depth += open_brackets(line);
            return true;
        }
        line.starts_with("//") || line.is_empty()
    }

    let text = text.split("#[cfg(test)]\nmod tests").next().unwrap_or(text);
    let mut lines = text.lines().map(str::trim);
    let mut structs = Vec::new();
    let mut attributes = String::new();
    let mut depth = 0;

    while let Some(line) = lines.next() {
        if gather(line, &mut attributes, &mut depth) {
            continue;
        }
        let derives = std::mem::take(&mut attributes);
        let Some(rest) = line
            .strip_prefix("pub struct ")
            .or_else(|| line.strip_prefix("pub(crate) struct "))
            .or_else(|| line.strip_prefix("struct "))
            .filter(|rest| rest.ends_with('{'))
        else {
            continue;
        };
        let name = rest.split(['<', ' ', '{']).next().unwrap_or_default();

        let mut fields = Vec::new();
        let mut field_attributes = String::new();
        let mut field_depth = 0;
        for line in lines.by_ref() {
            if gather(line, &mut field_attributes, &mut field_depth) {
                continue;
            }
            if line == "}" {
                break;
            }
            let declaration = line
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ")
                .trim_end_matches(',');
            if let Some((field, field_type)) = declaration.split_once(':') {
                fields.push(Field {
                    attributes: std::mem::take(&mut field_attributes),
                    name: field.trim().to_owned(),
                    field_type: field_type.replace("chrono::", "").replace(' ', ""),
                });
            }
        }

        if derives.contains("Deserialize") && !derives.contains("IntoParams") {
            structs.push((name.to_owned(), fields));
        }
    }

    structs
}

/// **The source guard** (AC13). Every date field of a `Deserialize` struct in
/// `src/` that is not a set of query parameters reads through the
/// `utils::storable` helper for its type, so the bound cannot be skipped by a
/// field written tomorrow. Query parameters are excluded: their bounds are
/// named all at once by the service (ADR-0044), and a deserializer stops at
/// the first.
#[test]
fn every_request_date_field_reads_through_a_storable_helper() {
    fn visit(directory: &Path, files: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).expect("the source reads");
                files.push((path.display().to_string(), text.replace("\r\n", "\n")));
            }
        }
    }

    let mut files = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );

    let mut checked = BTreeMap::new();
    let mut failures = Vec::new();
    for (file, text) in &files {
        for (name, fields) in request_structs(text) {
            for field in fields {
                if !field.field_type.contains("NaiveDate")
                    && !field.field_type.contains("DateTime<Utc>")
                {
                    continue;
                }
                let at = format!("{name}.{} in {file}", field.name);
                let Some(helper) = helper_for(&field.field_type) else {
                    failures.push(format!(
                        "{at}: `{}` is a date shape no `storable` helper reads",
                        field.field_type
                    ));
                    continue;
                };
                let wanted = format!("storable::{helper}\"");
                if field.attributes.contains("deserialize_with")
                    && field.attributes.contains(&wanted)
                {
                    checked.insert(at, helper);
                } else {
                    failures.push(format!(
                        "{at}: `{}` must read through `deserialize_with = \
                         \"crate::utils::storable::{helper}\"`",
                        field.field_type
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} request date fields do not read through a storable helper:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(
        checked.len() >= 22,
        "the scan found {} date fields with a helper; there were 22, so it is not reading \
         the request structs: {checked:#?}",
        checked.len()
    );
}
