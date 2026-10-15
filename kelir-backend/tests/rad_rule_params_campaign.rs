//! **The independent campaign on a rule's required parameter**
//! ([#722](https://github.com/sujanto-gaws/kelir/issues/722); plan 20 row 4).
//!
//! The builder's tests are the #722 block of `engine.rs`'s unit tests and the
//! #722 block of `rad_forms.rs`. This file holds what a second reader went
//! looking for:
//!
//! * **The detail lands on the rule it is about** (AC1–AC3, AC8): a bad rule
//!   after others on its component is reported at its own index, two bad rules
//!   on one component are both reported, and rule 10 is not rule 1.
//! * **The lookups the builder's fixtures cannot tell apart**: their every
//!   component has an `id` equal to its `key`, and their every rule name is
//!   spelt right or misspelt beyond recognition. Here a target names a `key`
//!   and not an `id` (AC4), and a rule name in another case is
//!   `RULE_NOT_REGISTERED` and nothing more (AC6).
//! * **What is not empty** (AC1–AC3): a blank target is a target that names
//!   nothing, a blank pattern is a pattern, and a pattern spelling the field's
//!   own key is a pattern — AC5 is about a `target`.
//! * **The scope at its edges** (AC4): a row's rule naming its own grid, a row
//!   reading through a layout inside the row, a grid inside a grid's row, a
//!   hidden target, a rule scoped `client` inside a layout.
//! * **The builder's table is the server's** (AC15's *one table*, across the
//!   two workspaces): `propertyEdits.ts`'s `REQUIRED_PARAMS` is read and every
//!   entry is put to the server — its parameter, its kind and its verdict —
//!   and every registry rule outside it is shown not to be checked.
//! * **What is stored** (AC10, AC12): a published revision whose rule is empty
//!   submits exactly as it did before #722, and is remedied by a corrected
//!   revision; the purchase-requisition fixture still creates and publishes.
//!
//! # Mutations (coding standard §2.9)
//!
//! Run 2026-10-15 against `engine.rs` at `314fd58`, each reverted after and
//! the file checked byte-identical to `HEAD`. *Builder* is the builder's #722
//! tests in `engine.rs` and `rad_forms.rs`; *here* is this file.
//!
//! | Mutation | Builder | Here |
//! |---|---|---|
//! | the path's index counted over the five rules only, not every rule | green | `a_bad_rule_is_reported_at_its_own_index` |
//! | a component's rules stop at the first one reported | green | `a_bad_rule_is_reported_at_its_own_index`, `every_problem_is_reported_beside_a_cycle_and_a_bad_pattern` |
//! | the rule name looked up without case | green | `a_rule_name_in_another_case_is_only_unregistered` |
//! | the self-key arm without its `FieldKey` guard | green | `a_pattern_spelling_the_fields_own_key_is_only_a_pattern` |
//! | the scope's keys taken from `id`, not `key` | green | `a_target_names_a_key_and_not_an_id` |
//! | a blank string counted as empty | green | `a_blank_parameter_is_not_an_empty_one` |
//! | submit hardened: an empty `regex` pattern refuses (`validation.rs`) | green | `a_published_revision_with_empty_rules_submits_as_before_and_is_remedied_by_a_revision` |
//! | submit hardened: an empty `oneOf` list accepts (`validation.rs`) | green | the same |
//!
//! Two survive both, and are equivalent: the self-key arm moved after the
//! not-found arm (a rule's own key is always in its scope, so the not-found
//! arm cannot take it first), and `check_patterns`'s empty-pattern filter
//! removed (an empty pattern compiles and holds no divergent construct, and
//! flags never refuse, so the pattern checks have nothing to say about it).
//!
//! The rest of the run, red in the builder's suite as well, is in the pull
//! request.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::http::{Method, StatusCode};
use common::TestApp;
use kelir_backend::modules::rad::domain::jfss::validate_definition;
use serde_json::{json, Value};
use uuid::Uuid;

// -- Fixtures ---------------------------------------------------------------

/// A text field whose `id` is **not** its `key`, so a lookup that confused
/// the two would show.
fn keyed(key: &str) -> Value {
    json!({
        "id": format!("{key}-field"), "role": "data", "type": "textfield",
        "key": key, "label": key, "validation": {"type": "string"},
    })
}

fn rule(name: &str, params: Value) -> Value {
    json!({"rule": name, "scope": "both", "params": params, "message": "No."})
}

fn scoped(name: &str, scope: &str, params: Value) -> Value {
    json!({"rule": name, "scope": scope, "params": params, "message": "No."})
}

fn with_rules(mut component: Value, rules: Vec<Value>) -> Value {
    component["rules"] = Value::Array(rules);
    component
}

fn datagrid(key: &str, rows: Vec<Value>) -> Value {
    json!({
        "id": format!("{key}-grid"), "role": "data", "type": "datagrid", "key": key,
        "label": key, "validation": {"type": "array"}, "components": rows,
    })
}

fn definition(components: Vec<Value>) -> Value {
    json!({"formId": "rule-params-campaign", "version": "2.0.1", "components": components})
}

/// Every detail the server's save would give, as `(path, code)` — the
/// meta-schema's included, so a fixture this file got wrong shows up as one.
fn found(definition: &Value) -> Vec<(String, String)> {
    validate_definition(definition)
        .into_iter()
        .map(|detail| (detail.path, detail.code))
        .collect()
}

fn at(path: &str, code: &str) -> (String, String) {
    (path.to_owned(), code.to_owned())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repository_root().join(relative);

    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

// -- Where a detail lands ---------------------------------------------------

/// **AC1–AC3 and AC8: the index is the rule's own**, among every rule its
/// component declares — a rule outside the five and a good one before it, and
/// a second bad one at index 10, which the builder's `messageAt` must not read
/// as rule 1's.
#[test]
fn a_bad_rule_is_reported_at_its_own_index() {
    let mut rules = vec![
        rule("passwordStrength", json!({})),
        rule("regex", json!({"pattern": "^A"})),
        rule("oneOf", json!({"values": []})),
    ];
    rules.extend((3..10).map(|_| rule("notOneOf", json!({"values": ["x"]}))));
    rules.push(rule("matchesField", json!({"target": ""})));

    let details = found(&definition(vec![
        with_rules(keyed("amount"), rules),
        keyed("other"),
    ]));

    assert_eq!(
        details,
        [
            at(
                "definition.components.0.rules.2.params.values",
                "RULE_PARAM_MISSING"
            ),
            at(
                "definition.components.0.rules.10.params.target",
                "RULE_PARAM_MISSING"
            ),
        ]
    );
}

/// **AC6: the registry's names are case-sensitive**, and a name it does not
/// define is `RULE_NOT_REGISTERED` and nothing more, however close it is to
/// one of the five.
#[test]
fn a_rule_name_in_another_case_is_only_unregistered() {
    let details = found(&definition(vec![with_rules(
        keyed("amount"),
        vec![
            rule("MatchesField", json!({})),
            rule("ONEOF", json!({"values": []})),
            rule("Regex", json!({"pattern": ""})),
        ],
    )]));

    assert_eq!(
        details,
        [
            at("definition.components.0.rules.0", "RULE_NOT_REGISTERED"),
            at("definition.components.0.rules.1", "RULE_NOT_REGISTERED"),
            at("definition.components.0.rules.2", "RULE_NOT_REGISTERED"),
        ]
    );
}

// -- What is not empty ------------------------------------------------------

/// **AC5 is about a `target`.** A pattern that happens to spell the field's own
/// key is a pattern, and a list holding it is a list.
#[test]
fn a_pattern_spelling_the_fields_own_key_is_only_a_pattern() {
    let details = found(&definition(vec![with_rules(
        keyed("code"),
        vec![
            rule("regex", json!({"pattern": "code"})),
            rule("notOneOf", json!({"values": ["code"]})),
        ],
    )]));

    assert!(details.is_empty(), "{details:?}");
}

/// **AC1–AC3 name `""` and `[]`.** A blank target is a target naming no
/// field, which is the not-found refusal and not the empty one; a blank
/// pattern matches a space; a list holding `""` is not an empty list.
#[test]
fn a_blank_parameter_is_not_an_empty_one() {
    let details = found(&definition(vec![
        with_rules(
            keyed("code"),
            vec![
                rule("regex", json!({"pattern": " "})),
                rule("oneOf", json!({"values": [""]})),
                rule("matchesField", json!({"target": " "})),
            ],
        ),
        keyed("other"),
    ]));

    assert_eq!(
        details,
        [at(
            "definition.components.0.rules.2.params.target",
            "RULE_TARGET_NOT_FOUND"
        )]
    );
}

// -- The scope at its edges (AC4, AC5) --------------------------------------

/// **A target names a `key`**, which is what the record at submit is keyed by.
/// A component's `id` is the builder's handle on it and names nothing in the
/// payload — its own `id` included, which is therefore not AC5's self-target.
#[test]
fn a_target_names_a_key_and_not_an_id() {
    let details = found(&definition(vec![
        keyed("email"),
        with_rules(
            keyed("confirm"),
            vec![
                rule("matchesField", json!({"target": "email-field"})),
                rule("notMatchesField", json!({"target": "confirm-field"})),
                rule("matchesField", json!({"target": "email"})),
                rule("notMatchesField", json!({"target": "confirm"})),
            ],
        ),
    ]));

    assert_eq!(
        details,
        [
            at(
                "definition.components.1.rules.0.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
            at(
                "definition.components.1.rules.1.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
            at(
                "definition.components.1.rules.3.params.target",
                "RULE_PARAM_INVALID"
            ),
        ]
    );
}

/// **A row is not its grid.** At submit a row's rule reads the row, and the
/// row has no `lines` in it.
#[test]
fn a_row_rule_cannot_name_its_own_grid() {
    let details = found(&definition(vec![datagrid(
        "lines",
        vec![
            keyed("qty"),
            with_rules(
                keyed("confirm"),
                vec![rule("matchesField", json!({"target": "lines"}))],
            ),
        ],
    )]));

    assert_eq!(
        details,
        [at(
            "definition.components.0.components.1.rules.0.params.target",
            "RULE_TARGET_NOT_FOUND"
        )]
    );
}

/// **A layout inside a row is as transparent as one at the top**, both for
/// the rule and for its target.
#[test]
fn a_row_rule_reads_through_a_layout_inside_its_row() {
    let details = found(&definition(vec![datagrid(
        "lines",
        vec![
            json!({
                "id": "line-panel", "role": "layout", "type": "fieldset",
                "components": [keyed("qty")],
            }),
            json!({
                "id": "line-columns", "role": "layout", "type": "columns",
                "columns": [{"components": [with_rules(
                    keyed("confirm"),
                    vec![
                        rule("matchesField", json!({"target": "qty"})),
                        rule("matchesField", json!({"target": "elsewhere"})),
                    ],
                )]}],
            }),
        ],
    )]));

    assert_eq!(
        details,
        [at(
            "definition.components.0.components.1.columns.0.components.0.rules.1.params.target",
            "RULE_TARGET_NOT_FOUND"
        )]
    );
}

/// **A grid inside a grid's row has rows of its own.** Its rule reads its own
/// row, not the outer one; the outer row sees the inner grid's key and none of
/// its columns.
#[test]
fn a_rule_in_a_nested_grid_reads_its_own_row() {
    let details = found(&definition(vec![datagrid(
        "orders",
        vec![
            keyed("reference"),
            datagrid(
                "lines",
                vec![
                    keyed("sku"),
                    with_rules(
                        keyed("confirm"),
                        vec![
                            rule("matchesField", json!({"target": "sku"})),
                            rule("matchesField", json!({"target": "reference"})),
                        ],
                    ),
                ],
            ),
            with_rules(
                keyed("note"),
                vec![
                    rule("notMatchesField", json!({"target": "lines"})),
                    rule("notMatchesField", json!({"target": "sku"})),
                ],
            ),
        ],
    )]));

    assert_eq!(
        details,
        [
            at(
                "definition.components.0.components.2.rules.1.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
            at(
                "definition.components.0.components.1.components.1.rules.1.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
        ]
    );
}

/// **A hidden field is still a target** (AC4): whether it is shown is a fact
/// about one submission, and the definition is judged once for all of them.
#[test]
fn a_target_a_conditional_hides_is_still_a_target() {
    let mut reason = keyed("reason");
    reason["conditional"] = json!({
        "action": "show",
        "logic": {"==": [{"var": "kind"}, "other"]},
    });

    let details = found(&definition(vec![
        keyed("kind"),
        reason,
        with_rules(
            keyed("confirm"),
            vec![rule("matchesField", json!({"target": "reason"}))],
        ),
    ]));

    assert!(details.is_empty(), "{details:?}");
}

/// **The `scope` a rule declares does not change the record it reads** (AC4),
/// and a rule inside a layout is reported at its path through the layout.
#[test]
fn a_rule_scoped_to_either_side_inside_a_layout_reads_the_top_level() {
    let details = found(&definition(vec![
        json!({
            "id": "columns", "role": "layout", "type": "columns",
            "columns": [{"components": [with_rules(
                keyed("confirm"),
                vec![
                    scoped("matchesField", "client", json!({"target": "qty"})),
                    scoped("notMatchesField", "server", json!({"target": "qty"})),
                    scoped("matchesField", "client", json!({"target": "lines"})),
                ],
            )]}],
        }),
        datagrid("lines", vec![keyed("qty")]),
    ]));

    assert_eq!(
        details,
        [
            at(
                "definition.components.0.columns.0.components.0.rules.0.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
            at(
                "definition.components.0.columns.0.components.0.rules.1.params.target",
                "RULE_TARGET_NOT_FOUND"
            ),
        ]
    );
}

/// **AC8: every problem at once.** Each of the three codes, two bad rules on
/// one component, a non-empty pattern the existing check refuses (AC2: still
/// reported, and once), and a cycle.
#[test]
fn every_problem_is_reported_beside_a_cycle_and_a_bad_pattern() {
    let number = |key: &str, calculate: Value| {
        json!({
            "id": format!("{key}-field"), "role": "data", "type": "number", "key": key,
            "label": key, "validation": {"type": "number"}, "calculate": calculate,
        })
    };

    let details = found(&definition(vec![
        with_rules(
            keyed("a"),
            vec![
                rule("oneOf", json!({"values": []})),
                rule("regex", json!({"pattern": "(?=x)"})),
                rule("regex", json!({})),
            ],
        ),
        with_rules(keyed("b"), vec![rule("matchesField", json!({"target": 5}))]),
        with_rules(
            keyed("c"),
            vec![rule("notMatchesField", json!({"target": "c"}))],
        ),
        with_rules(
            keyed("d"),
            vec![rule("notMatchesField", json!({"target": "nowhere"}))],
        ),
        number("x", json!({"+": [{"var": "y"}, 1]})),
        number("y", json!({"+": [{"var": "x"}, 1]})),
    ]));

    let mut expected = vec![
        at(
            "definition.components.0.rules.0.params.values",
            "RULE_PARAM_MISSING",
        ),
        at(
            "definition.components.0.rules.1.params.pattern",
            "PATTERN_NOT_COMPILABLE",
        ),
        at(
            "definition.components.0.rules.2.params.pattern",
            "RULE_PARAM_MISSING",
        ),
        at(
            "definition.components.1.rules.0.params.target",
            "RULE_PARAM_INVALID",
        ),
        at(
            "definition.components.2.rules.0.params.target",
            "RULE_PARAM_INVALID",
        ),
        at(
            "definition.components.3.rules.0.params.target",
            "RULE_TARGET_NOT_FOUND",
        ),
    ];
    let mut details_sorted = details.clone();
    details_sorted.retain(|(_, code)| code != "CALCULATION_CYCLE");
    details_sorted.sort();
    expected.sort();

    assert_eq!(details_sorted, expected, "{details:?}");
    assert_eq!(
        details
            .iter()
            .filter(|(_, code)| code == "CALCULATION_CYCLE")
            .count(),
        1,
        "{details:?}"
    );
}

// -- One table, two workspaces (AC15) ---------------------------------------

/// One entry of the builder's `REQUIRED_PARAMS`.
#[derive(Debug)]
struct BuilderEntry {
    param: String,
    kind: String,
    without_it: String,
}

/// `REQUIRED_PARAMS` as `propertyEdits.ts` declares it.
fn builders_table() -> BTreeMap<String, BuilderEntry> {
    let source = read("kelir-frontend/src/features/admin/form-builder/propertyEdits.ts");
    let start = source
        .find("const REQUIRED_PARAMS")
        .expect("propertyEdits.ts declares REQUIRED_PARAMS");
    let block = &source[start..];
    let block = &block[..block.find("\n}").expect("the table closes")];
    let entry = regex::Regex::new(
        r"(\w+):\s*\{\s*param:\s*'(\w+)',\s*kind:\s*'(\w+)',\s*withoutIt:\s*'(\w+)',?\s*\}",
    )
    .expect("the entry pattern compiles");

    entry
        .captures_iter(block)
        .map(|captures| {
            (
                captures[1].to_owned(),
                BuilderEntry {
                    param: captures[2].to_owned(),
                    kind: captures[3].to_owned(),
                    without_it: captures[4].to_owned(),
                },
            )
        })
        .collect()
}

/// Every rule name the server's registry defines, read from `registry_rule`'s
/// arms in `validation.rs` — the catalogue `is_registered` answers from.
fn registry_names() -> Vec<String> {
    let source = read("kelir-backend/src/modules/rad/domain/validation.rs");
    let start = source
        .find("fn registry_rule(")
        .expect("validation.rs declares registry_rule");
    let block = &source[start..];
    let block = &block[..block.find("_ => return None").expect("the match ends")];
    let arm = regex::Regex::new(r#"(?m)^\s*("\w+"(?:\s*\|\s*"\w+")*)\s*=>"#)
        .expect("the arm pattern compiles");
    let name = regex::Regex::new(r#""(\w+)""#).expect("the name pattern compiles");

    arm.captures_iter(block)
        .flat_map(|captures| {
            name.captures_iter(&captures[1])
                .map(|found| found[1].to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// **The builder flags exactly what the server refuses, and says what the
/// server says.** `REQUIRED_PARAMS` exists twice, once per workspace, and each
/// side's own tests pin its copy against literals of their own; a rule, a
/// parameter, a kind or a verdict changed on one side with its tests stays
/// green on both. So the builder's table is read and put to the server:
///
/// - each entry's seeded shape (`''` or `[]`) is `RULE_PARAM_MISSING` at the
///   entry's parameter, and the server's message carries the entry's verdict;
/// - the wrong type for the entry's kind is `RULE_PARAM_INVALID`;
/// - a filled one of the entry's kind is stored;
/// - and **every registry rule outside the table is not checked**, so the
///   server's table holds nothing the builder's does not.
#[test]
fn the_builders_table_is_the_servers_table() {
    let table = builders_table();
    let registry = registry_names();

    assert_eq!(
        table.len(),
        5,
        "the builder's table read as {table:?}; a parser that found nothing would pass the rest"
    );
    assert!(registry.len() >= table.len(), "{registry:?}");

    for (name, entry) in &table {
        assert!(registry.contains(name), "{name} is not a registry rule");

        let (empty, wrong, filled) = match entry.kind.as_str() {
            "text" => (json!(""), json!(5), json!("other")),
            "list" => (json!([]), json!("a"), json!(["a"])),
            kind => panic!("{name}: a kind this test does not know: {kind}"),
        };
        let param = entry.param.as_str();
        let path = format!("definition.components.0.rules.0.params.{param}");
        let with = |value: Value| {
            validate_definition(&definition(vec![
                with_rules(keyed("amount"), vec![rule(name, json!({ param: value }))]),
                keyed("other"),
            ]))
        };

        let missing = with(empty);
        assert_eq!(missing.len(), 1, "{name}: {missing:?}");
        assert_eq!(missing[0].path, path, "{name}");
        assert_eq!(missing[0].code, "RULE_PARAM_MISSING", "{name}");
        assert_eq!(missing[0].rule, *name);
        let verdict = format!("{} every value", entry.without_it);
        assert!(
            missing[0].message.contains(&verdict),
            "{name}: the builder says it {verdict}; the server says: {}",
            missing[0].message
        );

        let mistyped = with(wrong);
        assert_eq!(mistyped.len(), 1, "{name}: {mistyped:?}");
        assert_eq!(mistyped[0].path, path, "{name}");
        assert_eq!(mistyped[0].code, "RULE_PARAM_INVALID", "{name}");

        let stored = with(filled);
        assert!(stored.is_empty(), "{name}: {stored:?}");
    }

    for name in registry.iter().filter(|name| !table.contains_key(*name)) {
        let details = found(&definition(vec![with_rules(
            keyed("amount"),
            vec![rule(name, json!({}))],
        )]));

        assert!(
            details.is_empty(),
            "{name} is outside the builder's table, so the builder neither seeds nor flags \
             it — and the server refuses it: {details:?}"
        );
    }
}

// -- What is stored (AC10, AC12) --------------------------------------------

/// **AC12: the purchase-requisition fixture** — the one definition in the
/// repository carrying a rule — creates and publishes through the API.
#[tokio::test]
async fn the_purchase_requisition_fixture_still_creates_and_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let fixture: Value = serde_json::from_str(&read(
        "kelir-frontend/src/features/rad/__fixtures__/purchase-requisition.json",
    ))
    .expect("the fixture is JSON");
    let key = fixture["formId"]
        .as_str()
        .expect("the fixture has a formId")
        .to_owned();

    let created = app
        .send(
            Method::POST,
            "/api/v1/rad/forms",
            Some(&token),
            Some(json!({"formKey": key, "title": "Purchase requisition", "definition": fixture})),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "body {}", created.body);

    let id = id_of(&created.body["data"]);
    let published = app
        .send(
            Method::POST,
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);
}

fn id_of(form: &Value) -> Uuid {
    form["id"]
        .as_str()
        .expect("the response carries an id")
        .parse()
        .expect("the id is a uuid")
}

/// A form with a `title` and a `budget`, and the rules given on each.
fn live_definition(form_id: &str, title_rules: Value, budget_rules: Value) -> Value {
    json!({
        "formId": form_id,
        "version": "2.0.1",
        "components": [
            {
                "id": "title-field", "role": "data", "type": "textfield", "key": "title",
                "label": "Title", "validation": {"type": "string"}, "rules": title_rules,
            },
            {
                "id": "budget-field", "role": "data", "type": "number", "key": "budget",
                "label": "Budget", "validation": {"type": "number"}, "rules": budget_rules,
            },
        ],
    })
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

/// **AC10: a published revision is not re-checked, and submit is unchanged.**
///
/// The revision went live with a good definition, and its rules are emptied
/// behind the API — the state of a revision published before #722. Submit
/// still does what the rules say, as it did: an empty `regex` accepts every
/// value, and an empty `oneOf` refuses every one with the author's message.
/// **Neither is the definition check**: a submit that ran it would refuse the
/// first and answer the second with `RULE_PARAM_MISSING`.
///
/// The remedy is AC10's: a new revision copies the empty rules and is refused
/// at publish, then corrected, then published.
#[tokio::test]
async fn a_published_revision_with_empty_rules_submits_as_before_and_is_remedied_by_a_revision() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let key = "empty-live";

    let created = send(
        &app,
        &token,
        Method::POST,
        "/api/v1/rad/forms",
        Some(json!({
            "formKey": key,
            "title": "Empty live",
            "definition": live_definition(key, json!([]), json!([])),
        })),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "body {}", created.body);
    let id = id_of(&created.body["data"]);

    let published = send(
        &app,
        &token,
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/publish"),
        None,
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "body {}", published.body);

    let emptied = live_definition(
        key,
        json!([rule("regex", json!({"pattern": ""}))]),
        json!([{"rule": "oneOf", "scope": "both", "params": {"values": []}, "message": "Not a budget we take."}]),
    );
    let affected = sqlx::query(
        "UPDATE rad_forms SET definition_json = $1 WHERE id = $2 AND status = 'PUBLISHED'",
    )
    .bind(&emptied)
    .bind(id)
    .execute(&app.pool)
    .await
    .expect("the published revision is rewritten behind the API")
    .rows_affected();
    assert_eq!(affected, 1);

    let submissions = format!("/api/v1/rad/forms/{id}/submissions");

    let accepted = send(
        &app,
        &token,
        Method::POST,
        &submissions,
        Some(json!({"payload": {"title": "Anything at all", "budget": null}})),
    )
    .await;
    assert_eq!(
        accepted.status,
        StatusCode::CREATED,
        "an empty regex accepted every value before #722: {}",
        accepted.body
    );

    let refused = send(
        &app,
        &token,
        Method::POST,
        &submissions,
        Some(json!({"payload": {"title": "Anything at all", "budget": 5}})),
    )
    .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"],
        json!([{
            "path": "budget", "rule": "oneOf", "code": "VALIDATION_FAILED",
            "message": "Not a budget we take.",
        }]),
        "an empty oneOf refused every value before #722, with the author's message"
    );

    // The remedy: a revision copies the empty rules, is refused at publish,
    // and publishes once corrected.
    let next = send(
        &app,
        &token,
        Method::POST,
        &format!("/api/v1/rad/forms/{id}/revisions"),
        Some(json!({"title": "Corrected"})),
    )
    .await;
    assert_eq!(next.status, StatusCode::CREATED, "body {}", next.body);
    let copy = id_of(&next.body["data"]);

    let refused_publish = send(
        &app,
        &token,
        Method::POST,
        &format!("/api/v1/rad/forms/{copy}/publish"),
        None,
    )
    .await;
    assert_eq!(
        refused_publish.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "body {}",
        refused_publish.body
    );
    let codes: Vec<(&str, &str)> = refused_publish.body["error"]["details"]
        .as_array()
        .expect("details")
        .iter()
        .map(|detail| {
            (
                detail["path"].as_str().unwrap_or_default(),
                detail["code"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            (
                "definition.components.0.rules.0.params.pattern",
                "RULE_PARAM_MISSING"
            ),
            (
                "definition.components.1.rules.0.params.values",
                "RULE_PARAM_MISSING"
            ),
        ]
    );

    let corrected = live_definition(
        key,
        json!([rule("regex", json!({"pattern": "^[A-Z]"}))]),
        json!([rule("oneOf", json!({"values": [5, 10]}))]),
    );
    let saved = send(
        &app,
        &token,
        Method::PUT,
        &format!("/api/v1/rad/forms/{copy}"),
        Some(json!({"definition": corrected})),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "body {}", saved.body);

    let republished = send(
        &app,
        &token,
        Method::POST,
        &format!("/api/v1/rad/forms/{copy}/publish"),
        None,
    )
    .await;
    assert_eq!(
        republished.status,
        StatusCode::OK,
        "body {}",
        republished.body
    );
}
