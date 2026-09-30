//! **A value PostgreSQL cannot store is a 422 on every list route, not a 500**
//! ([#601](https://github.com/sujanto-gaws/kelir/issues/601), with
//! [#594](https://github.com/sujanto-gaws/kelir/issues/594) folded in).
//!
//! PostgreSQL refuses two things a query string can carry and a Rust type
//! happily holds: a NUL (0x00), which `text` cannot contain, and an instant
//! outside `timestamptz`'s range (before 4714 BC), which chrono parses. Bound
//! into a statement, either fails it, and the caller is told `INTERNAL_ERROR`.
//! [#597](https://github.com/sujanto-gaws/kelir/pull/597) closed the NUL for
//! every `search` and [#595](https://github.com/sujanto-gaws/kelir/pull/595)
//! the date for one route; each was a fix per parameter, and `/audit` had
//! both defects because nobody had looked at it.
//!
//! **This walk looks at every route instead of a list of them.** It reads the
//! operations from `ApiDoc::openapi()` — the document
//! `router::tests::every_annotated_route_reaches_the_document` already holds
//! equal to what is served — takes every `get` with a query parameter, and
//! classifies each parameter by its schema, not its name. It names no route
//! and no parameter. A list route added tomorrow is walked tomorrow.
//!
//! **What stops the walk from walking nothing.** A handler whose annotation
//! omits `params(...)` has no query parameter in the document, so the walk
//! would silently skip it. The guard counts `QueryParams<` in every
//! `modules/**/handlers.rs` and requires the walked operations to number the
//! same, and requires at least one parameter of every class — a schema change
//! that turned every date into a plain string would otherwise leave the date
//! cells with nothing to send.
//!
//! **Where the refusal lives** is not this file's business, and it asserts
//! answers rather than mechanisms. For the record: a NUL is refused by
//! `crate::extract::QueryParams` in any decoded value, and a date by
//! `utils::storable`, called by both services that take a date range.
//!
//! # Seen red first (coding standard §2.9)
//!
//! Run 2026-09-30 against `origin/main` at `63f9224`, before the fix:
//!
//! - **The guard was red first**: 31 operations against 33 `QueryParams<`.
//!   `RowQuery` and `ActionQuery` lacked `#[into_params(parameter_in = Query)]`
//!   and were documented as *required path* parameters. Fixed in the
//!   annotations, and the walk run again.
//! - **Then 57 of 291 cells failed.** Seven were 500s, all on `/audit`:
//!   `objectType` and `eventType` with `%00` and `a%00b`, and `from`, `to` and
//!   both together at `-5000-01-01T00:00:00Z`. `/audit` also *accepted*
//!   `-4713-11-23T23:59:59.999999999Z`, a nanosecond before PostgreSQL's range,
//!   on both bounds. `/rad/lists/{id}/rows` never refused a NUL in `sort`,
//!   `dir` or a filter (its unresolved id answered 404 first). The other 43
//!   were refusals with another code: a NUL in a uuid, an enum or a
//!   closed-vocabulary string answered `INVALID_TYPE`, `UNKNOWN_VALUE` or
//!   `INVALID_VALUE`, which is a 422 but not the NUL's.
//!
//! After the fix, every cell passes. **Seen red, 2026-09-30**, each mutation
//! reverted before the next:
//!
//! - `refuse_nul(query)?` removed from `QueryParams` — 52 cells, four of them
//!   `/audit` 500s.
//! - `refuse_out_of_range` removed from `audit::service::search_audit` — five
//!   `/audit` date cells, three of them 500s.
//! - the same call removed from `integration::domain::log::validate_query` —
//!   the same five cells on `/integration/logs`.
//! - `>=` made `>` on the lower bound in `utils::storable` — the four cells
//!   sending PostgreSQL's first instant.
//! - `.take(1)` on `refuse_out_of_range`'s details — the two both-bounds cells.
//! - `params(AuditSearch)` removed from `search_audit` — the guard, 32 of 33.
//! - `#[into_params(parameter_in = Query)]` removed from `RowQuery` — the
//!   guard, 32 of 33.
//!
//! **The `test-engineer` gate (PR #612) added three things**, each seen red
//! under a mutation that left the walk above green:
//!
//! - `AuditSearch::to` documented as `value_type = String` *and* its bound
//!   dropped from `search_audit`: the walk classed `to` as a string and sent
//!   it no date, and `/audit?to=-5000-01-01T00:00:00Z` answered 500 unseen.
//!   Every string now also receives that date and must not answer 500 — one
//!   cell red.
//! - `QueryParams` refusing every empty query string: the positive control
//!   read any refusal but `INVALID_CHARACTER`/`OUT_OF_RANGE` as accepted. It
//!   now requires a 2xx, a 404, or a 422 naming only a `required` parameter —
//!   33 cells red.
//! - `RowQuery::filters` dropped from the document (`#[param(ignore)]`): the
//!   walk stopped sending the flattened key, and the guard asked for only five
//!   classes. It now asks for `map` too.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use axum::http::StatusCode;
use serde_json::Value;
use utoipa::OpenApi;
use uuid::Uuid;

use common::{TestApp, TestResponse};
use kelir_backend::router::ApiDoc;

/// What the walk sends a parameter depends on what its schema says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    DateTime,
    Uuid,
    /// `format: int32` is how utoipa writes a `u32`, which is what every paging
    /// parameter is.
    Integer {
        int32: bool,
    },
    Enum,
    String,
    /// An exploded form object — `RowQuery`'s filters — whose keys are the
    /// caller's. The walk sends one key of its own.
    Map,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Self::DateTime => "date-time",
            Self::Uuid => "uuid",
            Self::Integer { .. } => "integer",
            Self::Enum => "enum",
            Self::String => "string",
            Self::Map => "map",
        }
    }
}

#[derive(Debug)]
struct Parameter {
    name: String,
    class: Class,
    /// Whether the document says the operation cannot be called without it —
    /// the one reason the positive control accepts a refusal.
    required: bool,
}

#[derive(Debug)]
struct Operation {
    /// The path with every `{capture}` resolved.
    path: String,
    /// The path as documented, for the failure listing.
    template: String,
    query: Vec<Parameter>,
}

/// Follows a `$ref` to its component, and through a `oneOf`/`anyOf`/`allOf` to
/// the one member that is not `null` — which is how utoipa writes an
/// `Option<Enum>`.
fn resolve<'a>(schema: &'a Value, components: &'a Value) -> &'a Value {
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .unwrap_or_else(|| panic!("a reference outside the components: {reference}"));
        return resolve(&components[name], components);
    }

    for combinator in ["oneOf", "anyOf", "allOf"] {
        if let Some(members) = schema[combinator].as_array() {
            let concrete: Vec<&Value> = members
                .iter()
                .filter(|member| member["type"] != "null")
                .collect();
            if let [only] = concrete.as_slice() {
                return resolve(only, components);
            }
        }
    }

    schema
}

/// The schema's `type`, which OpenAPI 3.1 may write as `["string", "null"]`.
fn type_of(schema: &Value) -> Option<&str> {
    match &schema["type"] {
        Value::String(single) => Some(single),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null"),
        _ => None,
    }
}

/// Classifies a parameter from its schema. **A schema this cannot place fails
/// the walk** rather than being skipped: a parameter the walk sends nothing to
/// is a parameter it claims to cover and does not.
fn classify(name: &str, schema: &Value, components: &Value) -> Class {
    let schema = resolve(schema, components);

    if schema["enum"].is_array() {
        return Class::Enum;
    }

    match (type_of(schema), schema["format"].as_str()) {
        (Some("string"), Some("date-time")) => Class::DateTime,
        (Some("string"), Some("uuid")) => Class::Uuid,
        (Some("string"), _) => Class::String,
        (Some("integer"), format) => Class::Integer {
            int32: format == Some("int32"),
        },
        (Some("object"), _) if schema["additionalProperties"]["type"] == "string" => Class::Map,
        _ => panic!(
            "`{name}` has a schema the walk cannot classify, so it would send it nothing: \
             {schema}"
        ),
    }
}

/// A value for a path capture that reaches the handler.
///
/// A uuid capture gets a fresh id, which names nothing: the route may answer
/// 404, which the walk allows, and never 500. **Any other capture needs an
/// `example`** in its annotation, because a string the walk made up would be
/// refused before the query parameters were read — `/rad/lookups/{source}`
/// answers 404 to an unknown source, and its search would never be reached.
fn capture_value(parameter: &Value, components: &Value) -> String {
    let name = parameter["name"].as_str().expect("a named parameter");
    if classify(name, &parameter["schema"], components) == Class::Uuid {
        return Uuid::now_v7().to_string();
    }

    parameter["example"]
        .as_str()
        .or_else(|| parameter["schema"]["example"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "path parameter `{name}` is not a uuid and has no `example`, so the walk \
                 cannot reach the handler behind it; give it one in its `params(...)`"
            )
        })
        .to_owned()
}

/// Every `get` in the document that takes a query parameter.
fn discover() -> Vec<Operation> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let components = &document["components"]["schemas"];
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        let Some(parameters) = item["get"]["parameters"].as_array() else {
            continue;
        };

        let query: Vec<Parameter> = parameters
            .iter()
            .filter(|parameter| parameter["in"] == "query")
            .map(|parameter| {
                let name = parameter["name"].as_str().expect("a named parameter");
                Parameter {
                    name: name.to_owned(),
                    class: classify(name, &parameter["schema"], components),
                    required: parameter["required"] == true,
                }
            })
            .collect();
        if query.is_empty() {
            continue;
        }

        let mut path = template.clone();
        for parameter in parameters.iter().filter(|p| p["in"] == "path") {
            let name = parameter["name"].as_str().expect("a named parameter");
            path = path.replace(
                &format!("{{{name}}}"),
                &capture_value(parameter, components),
            );
        }
        // **A capture the annotation does not declare gets a uuid.** utoipa
        // lists a path parameter only when `params(...)` names it, and most
        // `{id}` routes do not. A uuid is the only capture such a route could
        // take: a string one would need its `example` declared, above. If the
        // guess is wrong, the path extractor answers 400 and every cell on the
        // route fails naming it — the walk does not pass by guessing.
        while let Some(start) = path.find('{') {
            let end = start + path[start..].find('}').expect("a closed capture");
            path.replace_range(start..=end, &Uuid::now_v7().to_string());
        }

        operations.push(Operation {
            path,
            template: template.clone(),
            query,
        });
    }

    operations
}

/// Every `QueryParams<` written in a module's handlers — the number of list
/// routes that read a query string, counted from the source rather than from
/// the document the walk is checking.
fn query_extractor_sites() -> usize {
    fn visit(directory: &Path, count: &mut usize) {
        for entry in std::fs::read_dir(directory).expect("the directory reads") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                visit(&path, count);
            } else if path.file_name().is_some_and(|name| name == "handlers.rs") {
                let text = std::fs::read_to_string(&path).expect("the source reads");
                *count += text.matches("QueryParams<").count();
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

/// One request and one expectation, and what went wrong if it was not met.
struct Walk<'a> {
    app: &'a TestApp,
    token: &'a str,
    failures: Vec<String>,
    cells: usize,
}

/// What a cell must answer.
enum Expect<'a> {
    /// A 422 naming every one of these parameters with this `code` — and, when
    /// given, this `rule`.
    Refused {
        paths: &'a [&'a str],
        code: &'a str,
        rule: Option<&'a str>,
    },
    /// Anything but a 500, and no refusal of the kinds this walk is about: the
    /// value is one PostgreSQL can store.
    Accepted,
    /// Anything but a 500. For a value whose answer depends on what the
    /// parameter means, which the walk knows only from the document.
    Survives,
    /// **An ordinary request is served** (the positive control): a 2xx, a 404
    /// for the unresolved capture, or a 422 naming only parameters the
    /// document marks `required`. [`Expect::Accepted`] would pass a route that
    /// refused *everything* with any other code, and the walk's negative cells
    /// cannot tell a refusal of the hostile value from a refusal of every
    /// value (#612's gate).
    Ordinary { required: &'a [&'a str] },
}

const NUL_CODE: &str = "INVALID_CHARACTER";
const RANGE_CODE: &str = "OUT_OF_RANGE";
/// The key the walk sends into a [`Class::Map`] parameter.
const MAP_KEY: &str = "walkedFilter";

impl Walk<'_> {
    async fn cell(&mut self, operation: &Operation, query: &str, expect: Expect<'_>) {
        self.cells += 1;
        let uri = format!("{}?{query}", operation.path);
        let response = self.app.get(&uri, Some(self.token)).await;
        let at = format!("GET {}?{query}", operation.template);

        if response.status == StatusCode::INTERNAL_SERVER_ERROR {
            self.failures.push(format!("{at}: 500 {}", response.body));
            return;
        }

        match expect {
            Expect::Survives => {}
            Expect::Ordinary { required } => {
                if let Err(problem) = ordinary(&response, required) {
                    self.failures.push(format!(
                        "{at}: {problem} — got {} {}",
                        response.status, response.body
                    ));
                }
            }
            Expect::Accepted => {
                if let Some(detail) = details(&response)
                    .iter()
                    .find(|detail| detail["code"] == NUL_CODE || detail["code"] == RANGE_CODE)
                {
                    self.failures
                        .push(format!("{at}: a storable value was refused: {detail}"));
                }
            }
            Expect::Refused { paths, code, rule } => {
                if let Err(problem) = refused(&response, paths, code, rule) {
                    self.failures.push(format!(
                        "{at}: {problem} — got {} {}",
                        response.status, response.body
                    ));
                }
            }
        }
    }
}

fn details(response: &TestResponse) -> Vec<Value> {
    response.body["error"]["details"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// An answer to a request with nothing hostile in it: served, not found, or
/// refused only for a parameter the document says it cannot do without.
fn ordinary(response: &TestResponse, required: &[&str]) -> Result<(), String> {
    if response.status.is_success() || response.status == StatusCode::NOT_FOUND {
        return Ok(());
    }
    if response.status != StatusCode::UNPROCESSABLE_ENTITY || required.is_empty() {
        return Err("an ordinary request was not served".into());
    }

    // A missing field has no path of its own in serde's error, so the
    // extractor names it `query` and the field in the message (#122): that
    // form counts when the message names a required parameter.
    let names_a_required = |detail: &Value| {
        let path = detail["path"].as_str().unwrap_or_default();
        let message = detail["message"].as_str().unwrap_or_default();
        required.contains(&path)
            || (path == "query"
                && required
                    .iter()
                    .any(|name| message.contains(&format!("`{name}`"))))
    };

    let details = details(response);
    if !details.is_empty() && details.iter().all(names_a_required) {
        Ok(())
    } else {
        Err(format!(
            "an ordinary request was refused for something other than its required \
             parameters {required:?}"
        ))
    }
}

/// The refusal ADR 0017's envelope describes, read as JSON: `success: false`,
/// `VALIDATION_ERROR`, a message, and a detail with all four fields for every
/// parameter named.
fn refused(
    response: &TestResponse,
    paths: &[&str],
    code: &str,
    rule: Option<&str>,
) -> Result<(), String> {
    if response.status != StatusCode::UNPROCESSABLE_ENTITY {
        return Err("expected 422".into());
    }
    let body = &response.body;
    if body["success"] != false
        || body["error"]["code"] != "VALIDATION_ERROR"
        || !body["error"]["message"].is_string()
    {
        return Err("not the VALIDATION_ERROR envelope".into());
    }

    let details = details(response);
    if details.iter().any(|detail| {
        ["path", "rule", "code", "message"]
            .iter()
            .any(|field| !detail[*field].is_string())
    }) {
        return Err("a detail without path, rule, code and message".into());
    }

    for path in paths {
        let named = details.iter().any(|detail| {
            detail["path"] == *path
                && detail["code"] == code
                && rule.is_none_or(|rule| detail["rule"] == rule)
        });
        if !named {
            return Err(format!("no `{code}` detail on `{path}`"));
        }
    }

    Ok(())
}

/// **The walk** (#601 criteria 1–6). Every cell is sent as the bootstrap
/// administrator, who holds every permission, so a refusal is never a 403
/// standing in front of the query.
#[tokio::test]
async fn a_value_postgresql_cannot_store_is_refused_on_every_list_route() {
    let operations = discover();

    // --- The guard (criterion 2) ---------------------------------------------
    let sites = query_extractor_sites();
    let templates: Vec<&str> = operations.iter().map(|op| op.template.as_str()).collect();
    assert_eq!(
        operations.len(),
        sites,
        "the walk found {} operations with a query parameter, and the handlers hold {sites} \
         `QueryParams<` — a handler that reads a query string and documents none is one the \
         walk cannot see. Walked: {templates:#?}",
        operations.len()
    );

    let classes: BTreeSet<&str> = operations
        .iter()
        .flat_map(|op| op.query.iter().map(|p| p.class.name()))
        .collect();
    // `map` too: without it, `RowQuery`'s filters could fall out of the
    // document and the walk would stop sending the flattened key, silently
    // (#612's gate).
    for class in ["date-time", "uuid", "integer", "enum", "string", "map"] {
        assert!(
            classes.contains(class),
            "no `{class}` parameter anywhere, so that class's cells send nothing: {classes:?}"
        );
    }

    // --- The cells (criteria 3–5) ---------------------------------------------
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mut walk = Walk {
        app: &app,
        token: &token,
        failures: Vec::new(),
        cells: 0,
    };

    for operation in &operations {
        for parameter in &operation.query {
            let name = parameter.name.as_str();
            let nul = Expect::Refused {
                paths: &[name],
                code: NUL_CODE,
                rule: None,
            };

            match parameter.class {
                Class::String => {
                    walk.cell(operation, &format!("{name}=%00"), nul).await;
                    walk.cell(
                        operation,
                        &format!("{name}=a%00b"),
                        Expect::Refused {
                            paths: &[name],
                            code: NUL_CODE,
                            rule: None,
                        },
                    )
                    .await;
                    // The control: the refusal keys on the NUL, not on the
                    // parameter or on an unusual character.
                    walk.cell(operation, &format!("{name}=a%01b"), Expect::Accepted)
                        .await;
                    // A date PostgreSQL cannot store, sent to every *string*.
                    // For a real string it is text; for a date the document
                    // misdescribes as a string, it is the only out-of-range
                    // cell the walk sends, so a missing bound is still a 500
                    // here rather than a walk that is silent on dates
                    // (#612's gate).
                    walk.cell(
                        operation,
                        &format!("{name}=-5000-01-01T00:00:00Z"),
                        Expect::Survives,
                    )
                    .await;
                }
                Class::Map => {
                    walk.cell(
                        operation,
                        &format!("{MAP_KEY}=a%00b"),
                        Expect::Refused {
                            paths: &[MAP_KEY],
                            code: NUL_CODE,
                            rule: None,
                        },
                    )
                    .await;
                }
                Class::Enum => {
                    walk.cell(operation, &format!("{name}=%00"), nul).await;
                }
                Class::Uuid => {
                    walk.cell(operation, &format!("{name}=%00"), nul).await;
                    walk.cell(
                        operation,
                        &format!("{name}=not-a-uuid"),
                        Expect::Refused {
                            paths: &[name],
                            code: "INVALID_TYPE",
                            rule: None,
                        },
                    )
                    .await;
                }
                Class::DateTime => {
                    for value in ["-5000-01-01T00:00:00Z", "-4713-11-23T23:59:59.999999999Z"] {
                        walk.cell(
                            operation,
                            &format!("{name}={value}"),
                            Expect::Refused {
                                paths: &[name],
                                code: RANGE_CODE,
                                rule: Some("range"),
                            },
                        )
                        .await;
                    }
                    // PostgreSQL's first instant is stored, so it is accepted.
                    walk.cell(
                        operation,
                        &format!("{name}=-4713-11-24T00:00:00Z"),
                        Expect::Accepted,
                    )
                    .await;
                }
                Class::Integer { int32 } => {
                    walk.cell(operation, &format!("{name}=4294967295"), Expect::Accepted)
                        .await;
                    if int32 {
                        walk.cell(
                            operation,
                            &format!("{name}=4294967296"),
                            Expect::Refused {
                                paths: &[name],
                                code: "INVALID_TYPE",
                                rule: None,
                            },
                        )
                        .await;
                    }
                }
            }
        }

        // Both bounds bad at once: both are named, not only the first.
        let dates: Vec<&str> = operation
            .query
            .iter()
            .filter(|p| p.class == Class::DateTime)
            .map(|p| p.name.as_str())
            .collect();
        if dates.len() > 1 {
            let query = dates
                .iter()
                .map(|name| format!("{name}=-5000-01-01T00:00:00Z"))
                .collect::<Vec<_>>()
                .join("&");
            walk.cell(
                operation,
                &query,
                Expect::Refused {
                    paths: &dates,
                    code: RANGE_CODE,
                    rule: Some("range"),
                },
            )
            .await;
        }
    }

    // --- The positive control, last ------------------------------------------
    // Every operation with nothing hostile in it: none of the refusals above
    // may be standing in front of an ordinary request.
    for operation in &operations {
        let required: Vec<&str> = operation
            .query
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        walk.cell(
            operation,
            "",
            Expect::Ordinary {
                required: &required,
            },
        )
        .await;
    }

    assert!(
        walk.failures.is_empty(),
        "{} of {} cells over {} operations failed:\n{}",
        walk.failures.len(),
        walk.cells,
        operations.len(),
        walk.failures.join("\n")
    );
}
