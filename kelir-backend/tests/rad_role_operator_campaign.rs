//! **The independent campaign on every role's operators against the
//! registries** ([#948](https://github.com/sujanto-gaws/kelir/issues/948);
//! plan 21 row 50, **D-149** A, PR #950, AC-50-12).
//!
//! The builder's tests are the #948 block of `engine.rs`'s unit tests and the
//! #948 block of `rad_forms.rs`. This file goes past them at the endpoint,
//! asserting path, rule, code and message of every detail:
//!
//! * **Every §4.3.1 position, nested** (AC-50-4, AC-50-8): a display, a layout
//!   and an action in a panel, a `columns` slot, a `tabs` slot and a row
//!   template, a fieldset inside a slot and inside a row, a grid inside a tab,
//!   beside data fields refused by the catalogue: each told once, at create,
//!   at a `PUT` of a draft, and at the publish of a draft stored behind the
//!   API, which stays a draft with no audit row (AC-50-5).
//! * **Name for name** (AC-50-7): both registries, written out here so a drift
//!   in `jfss.rs` is red in this file, on every non-data role, with the
//!   floor-only names (`in`, `if`) refused in a display's `calculate` and
//!   `sum`'s D-22 arity on a display and on a layout.
//! * **A live form** (AC-50-6): a layout in a `tabs` slot hiding with `cat` and
//!   a display in a row holding `datetime` keep submitting; the revision is
//!   refused at both nested paths.
//!
//! # Mutations (coding standard §2.9)
//!
//! Run 2026-11-13 in `kelir-rust-tools:1.89`, each made alone on `engine.rs`
//! at `6c81be0` (code as `fb79983`) in a separate worktree. *Builder unit* is
//! `--lib rad::domain::engine`, *builder HTTP* is `rad_forms.rs`, *here* this
//! file. Red first: against `main`'s engine this file is 3 red of 3, each
//! setup accepted (create showed only the 2 data refusals; the other two
//! stored with a `201`). With the fix unmutated: 90, 61 and 3 green. AC-50-12's
//! six first, then four of the campaign's own.
//!
//! | Mutation | Builder unit | Builder HTTP | Here |
//! |---|---|---|---|
//! | the display arm removed (`_ => {}`) | red (4) | red (3) | red (3 of 3) |
//! | the layout arm removed | red (3) | red (3) | red (3 of 3) |
//! | the floor and registry swapped, non-data only | red (2) | red (3) | red (3 of 3) |
//! | `columns` and `tabs` slots not walked | red (1) | **green** | red, `every_role_…`, `a_live_form_…` |
//! | the row template not walked | red (1) | **green** | red, `every_role_…`, `a_live_form_…` |
//! | the walk removed from `definition_errors` | red (4) | red (3) | red (3 of 3) |
//! | `Some("data") => {}` removed (a data field told twice) | red (2) | **green** | red, `every_role_…` |
//! | the walk moved to publish only (`computed_path_errors`) | red (4) | red (2) | red (3 of 3) |
//! | a container's children checked but not walked further | **green** | **green** | red, `every_role_…` |
//! | an action skipped (`Some("action") => {}`) | red (3) | red (1) | red, `every_role_…`, `each_registry_…` |
//!
//! The second-last survives the builder's whole suite: every nesting there is
//! one container deep, and `every_role_…` holds a display in a fieldset in a
//! `columns` slot. Every mutant is killed.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::rad::repository::form as form_repo;
use serde_json::{json, Value};
use uuid::Uuid;

/// `jfss.rs`'s `CALCULATE_OPERATORS`, which is crate-private, space-separated.
const CALCULATE: &str = "var + - * / % min max map filter reduce all some none sum";

/// What `jfss.rs`'s `CONDITIONAL_OPERATORS` holds beside [`CALCULATE`].
const FLOOR_ONLY: &str = "== === != !== > >= < <= and or ! !! if ?: in missing missing_some";

type Detail = (String, String, String, String);

fn number(key: &str) -> Value {
    json!({"id": key, "role": "data", "type": "number", "key": key, "label": key,
           "validation": {"type": "number"}})
}

fn note(id: &str) -> Value {
    json!({"id": id, "role": "display", "type": "paragraph", "content": "Note"})
}

fn display(id: &str, calculate: Value) -> Value {
    let mut component = note(id);
    component["calculate"] = calculate;
    component
}

fn button(id: &str) -> Value {
    json!({"id": id, "role": "action", "type": "button", "label": "Go", "action": "submit"})
}

fn panel(id: &str, children: Vec<Value>) -> Value {
    json!({"id": id, "role": "layout", "type": "fieldset", "components": children})
}

fn grid(key: &str, rows: Vec<Value>) -> Value {
    json!({"id": key, "role": "data", "type": "datagrid", "key": key, "label": key,
           "validation": {"type": "array"}, "components": rows})
}

fn shown(mut component: Value, logic: Value) -> Value {
    component["conditional"] = json!({"action": "show", "logic": logic});
    component
}

fn bad(operator: &str) -> Value {
    json!({ operator: [1, 2] })
}

fn of_role(role: &str, id: &str) -> Value {
    match role {
        "display" => note(id),
        "layout" => panel(id, vec![note(&format!("{id}-n"))]),
        _ => button(id),
    }
}

fn definition(key: &str, components: Value) -> Value {
    json!({"formId": key, "version": "2.0.1", "components": components})
}

async fn send(
    app: &TestApp,
    token: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> common::TestResponse {
    app.send(method, uri, Some(token), body).await
}

async fn create(app: &TestApp, token: &str, key: &str, components: Value) -> common::TestResponse {
    let body = json!({"formKey": key, "title": key, "definition": definition(key, components)});
    send(app, token, Method::POST, "/api/v1/rad/forms", Some(body)).await
}

/// A clean draft, then `components` written into it behind the API.
async fn planted(app: &TestApp, token: &str, key: &str, components: &Value) -> Uuid {
    let created = create(app, token, key, json!([number("a")])).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id: Uuid = created.body["data"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query("UPDATE rad_forms SET definition_json = $1 WHERE id = $2")
        .bind(definition(key, components.clone()))
        .bind(id)
        .execute(&app.pool)
        .await
        .expect("the draft is rewritten behind the API");
    id
}

/// Every detail of a 422 as `(path, rule, code, message)`, sorted.
fn refusal(response: &common::TestResponse) -> Vec<Detail> {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "VALIDATION_ERROR");
    let field = |detail: &Value, name: &str| detail[name].as_str().unwrap_or_default().to_owned();
    let mut details: Vec<Detail> = response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|d| {
            (
                field(d, "path"),
                field(d, "rule"),
                field(d, "code"),
                field(d, "message"),
            )
        })
        .collect();
    details.sort();
    details
}

fn unregistered(path: &str, operator: &str) -> Detail {
    let path = format!("definition.components.{path}");
    let message = format!(
        "`{operator}` is not in the JFSS rule registry for this property, so it is forbidden — an \
         operator the engine happens to support is not an operator the registry approves"
    );
    (
        path,
        "operator".into(),
        "OPERATOR_NOT_REGISTERED".into(),
        message,
    )
}

fn arity(path: &str, count: usize) -> Detail {
    let path = format!("definition.components.{path}");
    let message = format!(
        "`sum` takes exactly one argument and sums the array it evaluates to; this one has \
         {count}. It would evaluate to 0 on both the server and the browser without reporting \
         anything — for a list of operands use `+`"
    );
    (path, "arity".into(), "SUM_TAKES_ONE_ARRAY".into(), message)
}

fn sorted(mut details: Vec<Detail>) -> Vec<Detail> {
    details.sort();
    details
}

/// **AC-50-4, AC-50-5 and AC-50-8 together.** Eleven bad expressions, each its
/// own operator, on every role at every position and nested two deep, two of
/// them on data fields: eleven details, none twice, at create, at a `PUT` and
/// at the publish of a stored draft, which stays a draft with no audit row.
#[tokio::test]
async fn every_role_is_refused_once_at_its_own_path_in_every_position_and_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mut both = display("both", bad("substr"));
    both["conditional"] = json!({"action": "show", "logic": bad("log")});
    let mut logged = number("t");
    logged["calculate"] = bad("log");
    let components = json!([
        number("a"),
        display("top", bad("in")),
        panel("p", vec![shown(note("n1"), bad("datetime")), logged]),
        {"id": "c", "role": "layout", "type": "columns", "columns": [
            {"components": [panel("cp", vec![display("deep", bad("cat"))])]},
            {"components": [shown(button("go"), bad("flagd"))]}
        ]},
        {"id": "tb", "role": "layout", "type": "tabs", "tabs": [
            {"title": "One", "components": [shown(panel("tp", vec![number("b")]), bad("ext-math"))]},
            {"title": "Two", "components": [grid("rows", vec![
                number("q"),
                shown(number("r"), bad("Var")),
                panel("rp", vec![shown(note("n2"), bad("merge")), display("rd", bad("=="))])
            ])]}
        ]},
        both
    ]);
    let expected = sorted(vec![
        unregistered("1.calculate", "in"),
        unregistered("2.components.0.conditional.logic", "datetime"),
        unregistered("2.components.1.calculate", "log"),
        unregistered("3.columns.0.components.0.components.0.calculate", "cat"),
        unregistered("3.columns.1.components.0.conditional.logic", "flagd"),
        unregistered("4.tabs.0.components.0.conditional.logic", "ext-math"),
        unregistered(
            "4.tabs.1.components.0.components.1.conditional.logic",
            "Var",
        ),
        unregistered(
            "4.tabs.1.components.0.components.2.components.0.conditional.logic",
            "merge",
        ),
        unregistered(
            "4.tabs.1.components.0.components.2.components.1.calculate",
            "==",
        ),
        unregistered("5.calculate", "substr"),
        unregistered("5.conditional.logic", "log"),
    ]);

    let created = create(&app, &token, "everywhere", components.clone()).await;
    assert_eq!(refusal(&created), expected, "at create");

    let id = planted(&app, &token, "everywhere-draft", &json!([number("a")])).await;
    let uri = format!("/api/v1/rad/forms/{id}");
    let body = json!({"title": "Still a draft", "definition": definition("everywhere-draft", components.clone())});
    assert_eq!(
        refusal(&send(&app, &token, Method::PUT, &uri, Some(body)).await),
        expected,
        "at PUT"
    );

    let id = planted(&app, &token, "everywhere-planted", &components).await;
    let published = send(
        &app,
        &token,
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/publish"),
        None,
    )
    .await;
    assert_eq!(refusal(&published), expected, "at publish");
    let read = send(
        &app,
        &token,
        Method::GET,
        &format!("/api/v1/rad/forms/{id}"),
        None,
    )
    .await;
    assert_eq!(read.body["data"]["status"], "DRAFT", "{}", read.body);
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_id = $1 AND event_type = 'RadForm.Published'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the audit trail is countable");
    assert_eq!(audited, 0);
}

/// **AC-50-7.** Every registry name is accepted on its roles in one form; the
/// names each refuses, the floor-only ones in a `calculate` among them, and
/// `sum`'s arity, are told at their own paths in another.
#[tokio::test]
async fn each_registry_is_met_name_for_name_on_every_non_data_role() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let argument = |op: &str| {
        if op == "sum" {
            json!([[1, 2]])
        } else {
            json!([{"var": "a"}, 1])
        }
    };
    let mut accepted = vec![number("a")];
    for (index, op) in CALCULATE.split(' ').enumerate() {
        accepted.push(display(&format!("c{index}"), json!({ op: argument(op) })));
    }
    for role in ["display", "layout", "action"] {
        for (index, op) in CALCULATE
            .split(' ')
            .chain(FLOOR_ONLY.split(' '))
            .enumerate()
        {
            let id = format!("{role}{index}");
            accepted.push(shown(of_role(role, &id), json!({ op: argument(op) })));
        }
    }
    let created = create(&app, &token, "names-accepted", Value::Array(accepted)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let mut refused = vec![number("a")];
    let mut expected = Vec::new();
    for op in ["cat", "==", "log", "in", "if"] {
        expected.push(unregistered(&format!("{}.calculate", refused.len()), op));
        refused.push(display(&format!("r{}", refused.len()), bad(op)));
    }
    for role in ["display", "layout", "action"] {
        for op in ["log", "datetime", "ext-math", "flagd"] {
            expected.push(unregistered(
                &format!("{}.conditional.logic", refused.len()),
                op,
            ));
            refused.push(shown(
                of_role(role, &format!("r{}", refused.len())),
                bad(op),
            ));
        }
    }
    expected.push(arity(&format!("{}.calculate", refused.len()), 2));
    refused.push(display("s2", json!({"sum": [{"var": "a"}, {"var": "a"}]})));
    expected.push(arity(&format!("{}.conditional.logic", refused.len()), 0));
    refused.push(shown(of_role("layout", "s0"), json!({"sum": []})));

    let response = create(&app, &token, "names-refused", Value::Array(refused)).await;
    assert_eq!(refusal(&response), sorted(expected));
}

/// **AC-50-6, nested.** Live with a layout in a `tabs` slot hiding with `cat`
/// and a display in a row holding `datetime`, the form submits, `b` hidden;
/// its next revision is refused at both nested paths.
#[tokio::test]
async fn a_live_form_is_not_rechecked_and_its_next_revision_is_refused_where_nested() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let components = json!([
        number("a"),
        {"id": "tb", "role": "layout", "type": "tabs", "tabs": [{"title": "One", "components": [
            shown(panel("hp", vec![number("b")]), json!({"==": [{"cat": ["a", "b"]}, "x"]}))
        ]}]},
        grid("rows", vec![number("q"), display("rd", json!({"datetime": ["2026-08-25"]}))])
    ]);
    let id = planted(&app, &token, "live-nested", &components).await;
    let published = form_repo::publish(&app.pool, fixtures::SYSTEM_TENANT_ID, id, None).await;
    assert_eq!(published.expect("published behind the API"), 1);

    let payload = json!({"payload": {"a": 1, "b": 5, "rows": [{"q": 2}]}});
    let uri = format!("/api/v1/rad/forms/{id}/submissions");
    let submitted = send(&app, &token, Method::POST, &uri, Some(payload)).await;
    assert_eq!(submitted.status, StatusCode::CREATED, "{}", submitted.body);
    assert!(
        submitted.body["data"]["payload"].get("b").is_none(),
        "{}",
        submitted.body
    );

    let body = json!({"definition": definition("live-nested", components)});
    let uri = format!("/api/v1/rad/forms/{id}/revisions");
    assert_eq!(
        refusal(&send(&app, &token, Method::POST, &uri, Some(body)).await),
        sorted(vec![
            unregistered("1.tabs.0.components.0.conditional.logic", "cat"),
            unregistered("2.components.1.calculate", "datetime"),
        ])
    );
}
