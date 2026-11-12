//! **The independent campaign on a `calculate` reading the whole record**
//! ([#936](https://github.com/sujanto-gaws/kelir/issues/936); plan 21 row 48,
//! **D-137** B, merged as #938).
//!
//! The builder's tests are the #936 block of `engine.rs`'s unit tests and the
//! #936 block of `rad_forms.rs`. This file holds what a second reader went
//! looking for, at the endpoint:
//!
//! * **The scan's operands are the engine's** (AC-48-1, AC-48-4): every `var`
//!   operand the engine answers with the whole record is kept from going live,
//!   at save or at publish, and every one it does not is not refused for it.
//!   The engine is asked here, so a bump of `datalogic-rs` that widens the set
//!   is red in this file rather than silent.
//! * **The scope rule through operators a `calculate` may hold** (AC-48-2):
//!   the builder's table nests under `if`, `merge` and `!!`, which the
//!   registry refuses in a `calculate` at save, so at the endpoint only the
//!   arithmetic, `min`/`max`, `sum` and the iterators reach the scan. Sources
//!   of sources, defaults of defaults, a `reduce` initial value inside a body
//!   and a body inside a source.
//! * **Paths below `components.N`** (AC-48-2, AC-48-7): `columns` and `tabs`
//!   slots, a fieldset in a slot, a row template inside a tab, a layout inside
//!   a row and a grid inside a row, each refused at its own path in document
//!   order, with the message asserted over the wire, which `rad_forms.rs`
//!   does not.
//! * **One component, two properties** (AC-48-3, AC-48-6): a whole-record
//!   `calculate` beside a computed `conditional` is two details, its own
//!   message each; both shapes in one `calculate` and a whole-record
//!   `conditional` beside it is one.
//! * **A live form** (AC-48-9, AC-48-10): publishing it again is a conflict,
//!   not a validation error, because the status is read before the check; its
//!   refused clone, corrected, publishes and computes.
//!
//! # Mutations (coding standard §2.9)
//!
//! Run 2026-11-10 against `engine.rs` and `form.rs` at `44ffc22`, each reverted
//! after and the tree checked clean against `HEAD`. *Builder unit* is the
//! `rad::domain::engine` unit tests; *builder HTTP* is `rad_forms.rs`; *here*
//! is this file. AC-48-11's six first, then four of the campaign's own.
//!
//! | Mutation | Builder unit | Builder HTTP | Here |
//! |---|---|---|---|
//! | the check deleted | red | red | red (4 of 4) |
//! | the check applied to `conditional.logic` | red | red | `a_whole_record_read_is_refused_at_its_own_path_however_deep` |
//! | the scan descending into iterator bodies | red | **green** | `the_scope_rule_holds_at_depth_through_a_calculates_own_operators` |
//! | `reduce`'s initial value dropped from the scan | red | **green** | the same |
//! | a `var` default dropped from the scan | red | **green** | the same, and `the_publish_keeps_back_exactly_…` |
//! | the check run before `validate_definition` | green | red | green (not this file's question) |
//! | `reads_computed_path` widened to `null` | red | red | red (3 of 4) |
//! | `[null]` dropped from the whole-record operands | red | red | red |
//! | every array opening with `null` counted (`[null, X]` refused) | red | red | red |
//! | the whole-record message chosen before the computed one | red | **green** | `a_whole_record_read_is_refused_at_its_own_path_however_deep` |
//! | the scan descending into every `var` operand (`[null, {"var": null}]` refused) | **green** | **green** | `the_publish_keeps_back_exactly_…`, `the_scope_rule_holds_…` |
//!
//! The last survives the builder's whole suite: it refuses a default the engine
//! never takes, which is the over-refusal AC-48-4's probe rules out.
//!
//! # Row 49's mutations (#941, D-147 A)
//!
//! Run 2026-11-12 in `kelir-rust-tools:1.89`, each made alone on `engine.rs`
//! at `ca50a6b` and the file restored, then this file's binary run. Only
//! *here* was run, not the builder's suites. Red first: against `main`'s
//! engine this file is 7 red of 8. The one green is
//! `a_data_fields_empty_path_is_a_cycle_at_save_wherever_it_is_read_in_scope`,
//! a guard on what row 49 leaves unchanged. With the engine restored it is
//! 8 of 8 green.
//!
//! | Mutation | Here |
//! |---|---|
//! | the empty path not whole (`var_path == Some("")` dropped) | red (3 of the row's 3) |
//! | a `var` default dropped from the scan | red, `a_display_keeps_back_exactly_…` and 3 more |
//! | the scan descending into iterator bodies | red, `an_empty_path_on_a_display_…`, the data-field guard and 1 more |
//! | an iterator's source and initial value dropped from the scan | red, `an_empty_path_on_a_display_…` and 1 more |
//! | the check applied to `conditional.logic` | red, `an_empty_path_on_a_display_…` and 1 more |
//! | a grid's rows not walked | red, `an_empty_path_on_a_display_…` and 1 more |
//! | a container's slots not walked | red, `an_empty_path_on_a_display_…` and 1 more |

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::modules::rad::evaluator::RuleEvaluator;
use kelir_backend::modules::rad::repository::form as form_repo;
use serde_json::{json, Value};
use uuid::Uuid;

// -- Fixtures ---------------------------------------------------------------

const COMPUTED: &str = "EXPRESSION_PATH_COMPUTED";

/// `engine.rs`'s `WHOLE_RECORD_MESSAGE`, which is private: AC-48-5's text, as
/// AC-49-8 (#941) reworded it to claim no loop.
const WHOLE_RECORD_MESSAGE: &str = "A `var` in this `calculate` names no field, so it reads the \
     whole record, and a publish cannot see which fields it depends on. Name each field it needs \
     as a path of its own (JFSS S12.2).";

/// `engine.rs`'s `COMPUTED_PATH_MESSAGE` (#822), also private.
const COMPUTED_PATH_MESSAGE: &str = "A `var`, `missing` or `missing_some` in this expression \
     works out the path it reads as it runs, or climbs out of its scope to read it, so a publish \
     cannot see what it reads: the order this form's fields are evaluated in takes no account of \
     it, and a loop through it goes unseen. Write each path as a string, with any default beside \
     it; an iterator's body reads only its element (JFSS S12.2).";

fn number_field(key: &str) -> Value {
    json!({
        "id": key, "role": "data", "type": "number", "key": key, "label": key,
        "validation": {"type": "number"}
    })
}

fn calculated(key: &str, calculate: Value) -> Value {
    let mut field = number_field(key);
    field["calculate"] = calculate;
    field
}

fn datagrid(key: &str, rows: Vec<Value>) -> Value {
    json!({
        "id": key, "role": "data", "type": "datagrid", "key": key, "label": key,
        "validation": {"type": "array"}, "components": rows
    })
}

fn fieldset(id: &str, children: Vec<Value>) -> Value {
    json!({"id": id, "role": "layout", "type": "fieldset", "components": children})
}

fn definition_of(form_id: &str, components: Value) -> Value {
    json!({"formId": form_id, "version": "2.0.1", "components": components})
}

async fn create(app: &TestApp, token: &str, key: &str, definition: Value) -> common::TestResponse {
    app.send(
        Method::POST,
        "/api/v1/rad/forms",
        Some(token),
        Some(json!({"formKey": key, "title": key, "definition": definition})),
    )
    .await
}

async fn create_draft(app: &TestApp, token: &str, key: &str, definition: Value) -> Uuid {
    let created = create(app, token, key, definition).await;

    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "{key}: {}",
        created.body
    );
    id_of(&created.body["data"])
}

fn id_of(form: &Value) -> Uuid {
    form["id"]
        .as_str()
        .expect("the response carries an id")
        .parse()
        .expect("the id is a uuid")
}

async fn publish(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.send(
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/publish"),
        Some(token),
        None,
    )
    .await
}

/// Every detail of a refusal as `(path, message)`, each held to the rule and
/// code a publish refusal of this kind carries.
fn details(response: &common::TestResponse) -> Vec<(String, String)> {
    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.body["error"]["code"], "VALIDATION_ERROR");

    response.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| {
            assert_eq!(detail["rule"], "expressionPath", "{detail}");
            assert_eq!(detail["code"], COMPUTED, "{detail}");
            (
                detail["path"].as_str().unwrap_or_default().to_owned(),
                detail["message"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn whole_at(path: &str) -> (String, String) {
    (path.to_owned(), WHOLE_RECORD_MESSAGE.to_owned())
}

fn computed_at(path: &str) -> (String, String) {
    (path.to_owned(), COMPUTED_PATH_MESSAGE.to_owned())
}

async fn status_of(app: &TestApp, token: &str, id: Uuid) -> Value {
    app.send(
        Method::GET,
        &format!("/api/v1/rad/forms/{id}"),
        Some(token),
        None,
    )
    .await
    .body["data"]["status"]
        .clone()
}

// -- The scan's operands are the engine's -----------------------------------

/// What one `{"var": operand}` in a `calculate` comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Saves, and the publish refuses it with the whole-record message.
    Whole,
    /// Saves, and the publish refuses it with #822's computed message.
    Computed,
    /// Refused at save, as the `calculate` reading the field it fills.
    Cycle,
    /// Saves and publishes.
    Published,
}

/// **AC-48-1 and AC-48-4, against the engine rather than a list.** Each
/// operand's outcome is stated, and the engine is asked what it answers: an
/// operand it answers with the whole record must never reach `Published`, and
/// one it does not must never be refused as `Whole`.
///
/// Past the builder's probe: `[[]]` reads the whole record too, and is caught
/// as the computed shape; `""`, `[""]` and `["", 3]` read it and are caught as
/// a cycle at save; `[null, null]`, booleans, `"."`, fractions and negatives
/// answer `null` and publish; and `[null, {"var": null}]` publishes, because
/// the engine takes no default after `null` and the scan must not read one.
#[tokio::test]
async fn the_publish_keeps_back_exactly_the_operands_the_engine_reads_as_the_whole_record() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let evaluator = RuleEvaluator::new();
    // No key any operand names, so a `var` with a default takes it: the scan
    // reads a default because it is evaluated whenever the path misses.
    let record = json!({"c": 1, "d": "two"});

    let table = [
        (json!(null), Outcome::Whole),
        (json!([]), Outcome::Whole),
        (json!([null]), Outcome::Whole),
        (json!(["a", {"var": null}]), Outcome::Whole),
        (json!([0, {"var": []}]), Outcome::Whole),
        (
            json!(["a", {"var": ["b", {"var": [null]}]}]),
            Outcome::Whole,
        ),
        (json!([[]]), Outcome::Computed),
        (json!([[null]]), Outcome::Computed),
        (json!([[], "a"]), Outcome::Computed),
        (json!({}), Outcome::Computed),
        (json!([{}]), Outcome::Computed),
        (json!(""), Outcome::Cycle),
        (json!([""]), Outcome::Cycle),
        (json!(["", 3]), Outcome::Cycle),
        (json!([null, "a"]), Outcome::Published),
        (json!([null, null]), Outcome::Published),
        (json!([null, {"var": null}]), Outcome::Published),
        (json!(false), Outcome::Published),
        (json!(true), Outcome::Published),
        (json!([false]), Outcome::Published),
        (json!([true, "a"]), Outcome::Published),
        (json!("."), Outcome::Published),
        (json!(["."]), Outcome::Published),
        (json!(1.5), Outcome::Published),
        (json!(-1), Outcome::Published),
        (json!([0]), Outcome::Published),
    ];

    for (index, (operand, outcome)) in table.into_iter().enumerate() {
        let read = json!({"var": operand});
        let answer = evaluator.evaluate(&read, &record);
        let reads_whole = matches!(&answer, Ok(value) if *value == record);

        assert!(
            !(reads_whole && outcome == Outcome::Published),
            "{operand} reads the whole record ({answer:?}) and would go live"
        );
        assert!(
            reads_whole || outcome != Outcome::Whole,
            "{operand} does not read the whole record ({answer:?}) and is refused for it"
        );

        let key = format!("operand-{index}");
        let components = json!([number_field("a"), calculated("t", json!({"+": [read, 1]}))]);
        let created = create(&app, &token, &key, definition_of(&key, components)).await;

        if outcome == Outcome::Cycle {
            assert_eq!(
                created.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{operand}: {}",
                created.body
            );
            assert_eq!(
                created.body["error"]["details"][0]["code"], "CALCULATION_CYCLE",
                "{operand}: {}",
                created.body
            );
            continue;
        }

        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "{operand}: {}",
            created.body
        );

        let id = id_of(&created.body["data"]);
        let published = publish(&app, &token, id).await;
        let path = "definition.components.1.calculate";

        match outcome {
            Outcome::Whole => assert_eq!(details(&published), [whole_at(path)], "{operand}"),
            Outcome::Computed => {
                assert_eq!(details(&published), [computed_at(path)], "{operand}")
            }
            _ => assert_eq!(
                published.status,
                StatusCode::OK,
                "{operand}: {}",
                published.body
            ),
        }
    }
}

// -- The scope rule through what a `calculate` may hold ----------------------

/// **AC-48-2 at the endpoint, through the operators the registry lets a
/// `calculate` hold.** Each expression is a form of its own; a refused one
/// carries the whole-record message, an accepted one publishes.
#[tokio::test]
async fn the_scope_rule_holds_at_depth_through_a_calculates_own_operators() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let sum_of_rows = json!({"+": [{"var": "current"}, {"var": "accumulator"}]});

    let refused = [
        // A source inside `sum`, inside arithmetic.
        json!({"*": [2, {"sum": [{"map": [{"var": null}, {"var": "x"}]}]}]}),
        // The source of a source.
        json!({"sum": [{"map": [{"filter": [{"var": []}, {"var": "x"}]}, {"var": "x"}]}]}),
        // `min` inside `max`, under `%`.
        json!({"%": [{"max": [1, {"min": [2, {"var": [null]}]}]}, 7]}),
        // A default of a default.
        json!({"var": ["a", {"var": ["b", {"var": null}]}]}),
        // A default inside `reduce`'s initial value, the body harmless.
        json!({"reduce": [{"var": "rows"}, sum_of_rows, {"var": ["a", {"var": []}]}]}),
        // An iterator in a source position of `all`, `some` and `none`.
        json!({"+": [
            {"all": [{"var": "rows"}, {"var": "x"}]},
            {"some": [{"map": [{"var": null}, 1]}, {"var": "x"}]}
        ]}),
        json!({"none": [{"var": null}, {"var": "x"}]}),
        // A body's whole read is harmless; the next source is not.
        json!({"+": [{"sum": [{"map": [{"var": "rows"}, {"var": null}]}]}, {"var": []}]}),
    ];
    let accepted = [
        // An iterator nested in a body: its source reads the element.
        json!({"sum": [{"map": [{"var": "rows"}, {"sum": [{"map": [{"var": null}, 1]}]}]}]}),
        // A default inside a body reads the element.
        json!({"reduce": [
            {"var": "rows"},
            {"+": [{"var": "accumulator"}, {"var": ["current", {"var": null}]}]},
            0
        ]}),
        // `reduce`'s initial value inside an outer body reads the element.
        json!({"sum": [{"map": [{"var": "rows"}, {"reduce": [[1], sum_of_rows, {"var": []}]}]}]}),
        // `[null, X]`'s default is never taken, and is not read.
        json!({"+": [{"var": [null, {"var": null}]}, 1]}),
    ];

    for (index, expression) in refused.iter().enumerate() {
        let key = format!("deep-refused-{index}");
        let components = json!([
            number_field("a"),
            datagrid("rows", vec![number_field("x")]),
            calculated("t", expression.clone())
        ]);
        let id = create_draft(&app, &token, &key, definition_of(&key, components)).await;

        assert_eq!(
            details(&publish(&app, &token, id).await),
            [whole_at("definition.components.2.calculate")],
            "{expression}"
        );
    }

    for (index, expression) in accepted.iter().enumerate() {
        let key = format!("deep-accepted-{index}");
        let components = json!([
            number_field("a"),
            datagrid("rows", vec![number_field("x")]),
            calculated("t", expression.clone())
        ]);
        let id = create_draft(&app, &token, &key, definition_of(&key, components)).await;
        let published = publish(&app, &token, id).await;

        assert_eq!(
            published.status,
            StatusCode::OK,
            "{expression}: {}",
            published.body
        );
    }
}

// -- Paths below `components.N` -----------------------------------------------

/// **AC-48-2 and AC-48-7, past the builder's grid and fieldset.** Every slot
/// shape and every nesting a row allows, each refused at its own path, in
/// document order, with the message over the wire; a component holding both
/// faults on two properties is told both; a `calculate` holding both shapes is
/// told once, and the whole-record `conditional` beside it not at all. The
/// draft saves, created and updated (AC-48-8).
#[tokio::test]
async fn a_whole_record_read_is_refused_at_its_own_path_however_deep() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let whole = json!({"+": [{"var": null}, 1]});
    let computed = json!({"var": {"var": "which"}});

    let mut two_faults = calculated("two", json!({"var": []}));
    two_faults["conditional"] = json!({"action": "show", "logic": computed});

    let mut one_detail = calculated(
        "one",
        json!({"+": [{"var": [null]}, {"var": {"var": "which"}}]}),
    );
    one_detail["conditional"] = json!({"action": "show", "logic": {"!!": [{"var": null}]}});

    let components = json!([
        number_field("a"),
        {"id": "cols", "role": "layout", "type": "columns", "columns": [
            {"components": [calculated("c0", whole.clone())]},
            {"components": [fieldset("in-col", vec![calculated("c1", whole.clone())])]}
        ]},
        {"id": "tabs", "role": "layout", "type": "tabs", "tabs": [
            {"title": "First", "components": [number_field("plain")]},
            {"title": "Rows", "components": [datagrid("rows", vec![
                number_field("qty"),
                calculated("r1", whole.clone()),
                fieldset("in-row", vec![calculated("r2", whole.clone())]),
                datagrid("inner", vec![calculated("r3", whole.clone())])
            ])]}
        ]},
        two_faults,
        one_detail,
        {"id": "note", "role": "display", "type": "paragraph", "content": "Total",
         "calculate": {"var": []}}
    ]);
    let key = "whole-record-deep";
    let id = create_draft(&app, &token, key, definition_of(key, components.clone())).await;

    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{id}"),
            Some(&token),
            Some(json!({"title": "Still a draft", "definition": definition_of(key, components)})),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let refused = publish(&app, &token, id).await;

    assert_eq!(
        details(&refused),
        [
            whole_at("definition.components.1.columns.0.components.0.calculate"),
            whole_at("definition.components.1.columns.1.components.0.components.0.calculate"),
            whole_at("definition.components.2.tabs.1.components.0.components.1.calculate"),
            whole_at(
                "definition.components.2.tabs.1.components.0.components.2.components.0.calculate"
            ),
            whole_at(
                "definition.components.2.tabs.1.components.0.components.3.components.0.calculate"
            ),
            whole_at("definition.components.3.calculate"),
            computed_at("definition.components.3.conditional.logic"),
            computed_at("definition.components.4.calculate"),
            whole_at("definition.components.5.calculate"),
        ]
    );
    assert_eq!(status_of(&app, &token, id).await, "DRAFT");
}

// -- A live form ---------------------------------------------------------------

/// **AC-48-9 and AC-48-10.** A form live with a whole-record `calculate` is a
/// conflict when published again, not a validation error: the status is read
/// before the check. Its clone is refused, and once corrected publishes and
/// computes from the field it names, while the live revision still reads.
#[tokio::test]
async fn a_live_whole_record_form_is_a_conflict_and_its_corrected_clone_goes_live() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let key = "whole-record-remedy";
    let live = json!([
        number_field("a"),
        calculated("t", json!({"+": [{"var": []}, 7]}))
    ]);
    let id = create_draft(&app, &token, key, definition_of(key, live)).await;

    assert_eq!(
        form_repo::publish(&app.pool, fixtures::SYSTEM_TENANT_ID, id, None)
            .await
            .expect("the draft is published behind the API"),
        1
    );

    let again = publish(&app, &token, id).await;

    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.body);
    assert!(!again.body.to_string().contains(COMPUTED), "{}", again.body);

    let clone = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/revisions"),
            Some(&token),
            Some(json!({})),
        )
        .await;

    assert_eq!(clone.status, StatusCode::CREATED, "{}", clone.body);

    let clone_id = id_of(&clone.body["data"]);

    assert_eq!(
        details(&publish(&app, &token, clone_id).await),
        [whole_at("definition.components.1.calculate")]
    );

    let corrected = json!([
        number_field("a"),
        calculated("t", json!({"+": [{"var": "a"}, 7]}))
    ]);
    let updated = app
        .send(
            Method::PUT,
            &format!("/api/v1/rad/forms/{clone_id}"),
            Some(&token),
            Some(json!({"title": key, "definition": definition_of(key, corrected)})),
        )
        .await;

    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let published = publish(&app, &token, clone_id).await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{clone_id}/submissions"),
            Some(&token),
            Some(json!({"payload": {"a": 2}})),
        )
        .await;

    assert_eq!(submitted.status, StatusCode::CREATED, "{}", submitted.body);
    assert_eq!(submitted.body["data"]["payload"]["t"], json!(9.0));
    assert_eq!(status_of(&app, &token, id).await, "PUBLISHED");
}

// -- A display reading the whole record as an empty path (#941) -----------------

/// **#941 AC-49-2, D-147 A.** The same read on a display is refused at publish
/// as `""`, `[""]` and `null` alike: a display is no node of the graph, so `""`
/// there is no cycle, and the publish's own check refuses it instead.
#[tokio::test]
async fn a_display_reading_the_whole_record_as_an_empty_path_is_refused_as_null_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (index, operand) in [json!(""), json!([""]), json!(null)]
        .into_iter()
        .enumerate()
    {
        let key = format!("display-whole-{index}");
        let components = json!([
            number_field("a"),
            {"id": "note", "role": "display", "type": "paragraph", "content": "Total",
             "calculate": {"var": operand}}
        ]);
        let id = create_draft(&app, &token, &key, definition_of(&key, components)).await;
        let published = publish(&app, &token, id).await;

        assert_eq!(
            published.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{operand} publishes on a display: {}",
            published.body
        );
        assert_eq!(
            details(&published),
            [whole_at("definition.components.1.calculate")],
            "{operand}"
        );
        assert_eq!(status_of(&app, &token, id).await, "DRAFT");
    }
}

// -- Row 49's campaign (#941, D-147 A) ------------------------------------------

/// A display whose `calculate` is `calculate`.
fn display(id: &str, calculate: Value) -> Value {
    json!({"id": id, "role": "display", "type": "paragraph", "content": "Total",
           "calculate": calculate})
}

/// **AC-49-1 and AC-49-7 against the engine, on a display.**
/// The first test's table asks only of a data field, where `""` is a cycle at
/// save; here the same question is asked where nothing but the publish can
/// answer it. Every operand the engine answers with the whole record is
/// refused at publish with the whole-record message, and every operand it does
/// not publishes.
///
/// A layout is not asked here: the JFSS meta-schema refuses a `calculate` on a
/// layout at save, so no layout reaches a publish through the API. The engine's
/// own test reads one.
///
/// Past the builder's three: `["", X]` with an expression and with a third
/// element, an empty path reached through the default of a path that misses
/// (`" "`, `"."`), and the near-misses a reader might take for an empty path,
/// whitespace and dots, which the engine answers `null` and so must publish.
#[tokio::test]
async fn a_display_keeps_back_exactly_the_operands_the_engine_reads_whole() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let evaluator = RuleEvaluator::new();
    let record = json!({"a": 1, "b": "two"});

    let table = [
        (json!(""), true),
        (json!([""]), true),
        (json!(["", 3]), true),
        (json!(["", "a"]), true),
        (json!(["", {"var": "a"}]), true),
        (json!(["", 3, 4]), true),
        (json!(["", null]), true),
        (json!([" ", {"var": ""}]), true),
        (json!([".", {"var": [""]}]), true),
        (json!(["missing", {"var": ["", 0]}]), true),
        (json!(null), true),
        (json!([]), true),
        (json!([null]), true),
        (json!(" "), false),
        (json!("  "), false),
        (json!("\t"), false),
        (json!([" "]), false),
        (json!([" ", 1]), false),
        (json!("."), false),
        (json!(".."), false),
        (json!(" ."), false),
        (json!("a"), false),
        (json!([null, ""]), false),
        (json!([null, {"var": ""}]), false),
    ];

    for (index, (operand, whole)) in table.into_iter().enumerate() {
        let read = json!({"var": operand});
        let answer = evaluator.evaluate(&read, &record);
        let reads_whole = matches!(&answer, Ok(value) if *value == record);

        assert_eq!(
            reads_whole, whole,
            "the table is wrong about {operand}: the engine answers {answer:?}"
        );

        let key = format!("display-operand-{index}");
        let components = json!([number_field("a"), display("note", read.clone())]);
        let id = create_draft(&app, &token, &key, definition_of(&key, components)).await;
        let published = publish(&app, &token, id).await;

        if whole {
            assert_eq!(
                details(&published),
                [whole_at("definition.components.1.calculate")],
                "{operand}"
            );
            assert_eq!(status_of(&app, &token, id).await, "DRAFT", "{operand}");
        } else {
            assert_eq!(
                published.status,
                StatusCode::OK,
                "{operand}: {}",
                published.body
            );
        }
    }
}

/// **AC-49-3 and AC-49-4 at the endpoint, at every depth a display can sit.**
/// Each refused expression is a form of its own, with a display in a grid's
/// row, a `columns` slot, a fieldset in a `tabs` slot and at the top, each
/// refused at its own path in document order. Each accepted one, an empty path
/// in an iterator's body, publishes from the same places beside a display
/// whose `conditional` reads `[""]`.
#[tokio::test]
async fn an_empty_path_on_a_display_is_refused_by_position_not_by_depth() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let sum_of_rows = json!({"+": [{"var": "current"}, {"var": "accumulator"}]});

    let refused = [
        json!({"+": [1, {"*": [2, {"var": ""}]}]}),
        json!({"var": ["a", {"var": [""]}]}),
        json!({"var": ["", {"var": "a"}]}),
        json!({"reduce": [{"var": "rows"}, sum_of_rows, {"var": ""}]}),
        json!({"sum": [{"map": [{"var": [""]}, 1]}]}),
        json!({"+": [{"all": [{"var": ""}, true]}, 0]}),
        json!({"sum": [{"map": [{"filter": [{"var": ["", 0]}, true]}, 1]}]}),
        // A body's empty path is the element; the next source is not.
        json!({"+": [{"sum": [{"map": [{"var": "rows"}, {"var": ""}]}]}, {"var": [""]}]}),
    ];
    let accepted = [
        json!({"sum": [{"map": [{"var": "rows"}, {"var": ""}]}]}),
        json!({"sum": [{"map": [{"var": "rows"}, {"var": [""]}]}]}),
        json!({"sum": [{"map": [{"var": "rows"}, {"var": ["", 0]}]}]}),
        json!({"reduce": [{"var": "rows"}, {"+": [{"var": "accumulator"}, {"var": [""]}]}, 0]}),
        json!({"+": [{"some": [{"var": "rows"}, {"var": ""}]}, 0]}),
        // A `reduce` inside a body: its initial value reads the element.
        json!({"sum": [{"map": [{"var": "rows"}, {"reduce": [[1], sum_of_rows, {"var": ""}]}]}]}),
    ];

    let placed = |expression: &Value| {
        json!([
            number_field("a"),
            datagrid("rows", vec![number_field("x"), display("in-row", expression.clone())]),
            {"id": "cols", "role": "layout", "type": "columns", "columns": [
                {"components": [display("in-col", expression.clone())]}
            ]},
            {"id": "tabs", "role": "layout", "type": "tabs", "tabs": [
                {"title": "One", "components": [
                    fieldset("in-tab", vec![display("deep", expression.clone())])
                ]}
            ]},
            display("top", expression.clone())
        ])
    };

    for (index, expression) in refused.iter().enumerate() {
        let key = format!("display-refused-{index}");
        let id = create_draft(&app, &token, &key, definition_of(&key, placed(expression))).await;

        assert_eq!(
            details(&publish(&app, &token, id).await),
            [
                whole_at("definition.components.1.components.1.calculate"),
                whole_at("definition.components.2.columns.0.components.0.calculate"),
                whole_at("definition.components.3.tabs.0.components.0.components.0.calculate"),
                whole_at("definition.components.4.calculate"),
            ],
            "{expression}"
        );
    }

    for (index, expression) in accepted.iter().enumerate() {
        let key = format!("display-accepted-{index}");
        let mut components = placed(expression);
        components.as_array_mut().expect("a list").push(json!({
            "id": "shown", "role": "display", "type": "paragraph", "content": "Shown",
            "calculate": {"var": "a"},
            "conditional": {"action": "show", "logic": {"!!": [{"var": [""]}]}}
        }));
        let id = create_draft(&app, &token, &key, definition_of(&key, components)).await;
        let published = publish(&app, &token, id).await;

        assert_eq!(
            published.status,
            StatusCode::OK,
            "{expression}: {}",
            published.body
        );
    }
}

/// **AC-49-6, past the builder's one shape.** On a data field every way of
/// reaching the empty path in its own scope, at the top and in a grid's row,
/// is still the cycle a save refuses; and the same read in a body is not.
#[tokio::test]
async fn a_data_fields_empty_path_is_a_cycle_at_save_wherever_it_is_read_in_scope() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let sum_of_rows = json!({"+": [{"var": "current"}, {"var": "accumulator"}]});

    let cycles = [
        json!({"var": ["a", {"var": ""}]}),
        json!({"var": ["", 1]}),
        json!({"reduce": [{"var": "rows"}, sum_of_rows, {"var": [""]}]}),
        json!({"sum": [{"map": [{"var": ""}, 1]}]}),
    ];

    for (index, expression) in cycles.iter().enumerate() {
        let places = [
            (
                "top",
                json!([
                    number_field("a"),
                    datagrid("rows", vec![number_field("x")]),
                    calculated("t", expression.clone())
                ]),
            ),
            (
                "row",
                json!([datagrid(
                    "rows",
                    vec![number_field("x"), calculated("t", expression.clone())]
                )]),
            ),
        ];

        for (place, components) in places {
            let key = format!("data-cycle-{place}-{index}");
            let created = create(&app, &token, &key, definition_of(&key, components)).await;

            assert_eq!(
                created.status,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{place} {expression}: {}",
                created.body
            );
            assert_eq!(
                created.body["error"]["details"][0]["code"], "CALCULATION_CYCLE",
                "{place} {expression}: {}",
                created.body
            );
        }
    }

    let key = "data-body-empty-path";
    let components = json!([
        datagrid("rows", vec![number_field("x")]),
        calculated(
            "t",
            json!({"sum": [{"map": [{"var": "rows"}, {"var": ["", 0]}]}]})
        )
    ]);
    let id = create_draft(&app, &token, key, definition_of(key, components)).await;
    let published = publish(&app, &token, id).await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
}
