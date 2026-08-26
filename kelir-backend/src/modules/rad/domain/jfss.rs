//! Validating a stored form definition (FR-RAD-002, #156 AC2).
//!
//! Three checks, and they catch different things.
//!
//! **Shape**, against the JFSS v2.0.1 meta-schema. That file is normative —
//! "where this document and the Meta-Schema disagree, the Meta-Schema is
//! normative" ([JFSS](../../../../../docs/schema/JSON%20Form%20Schema.md) §1.3)
//! — so a definition that does not validate is not a JFSS document and is
//! refused rather than stored.
//!
//! **Operators**, against the [Calculation Rule
//! Registry](../../../../../docs/schema/JFSS%20Calculation%20Rule%20Registry.md).
//! The meta-schema deliberately does not do this: its `jsonLogic` definition
//! says "deep operator/arity validation is deferred to the runtime libraries
//! and the Calculation Rule Registry" and accepts any single-key object. So a
//! definition that passes the meta-schema can still carry an operator no
//! registry approves, and **that is the hole this check closes.**
//!
//! It matters more since decision **D-10** than it did before. The adopted
//! engine ships a far wider operator surface than the registry approves —
//! `datetime`, `ext-string`, `ext-array`, `ext-math`, `flagd` — and every one
//! of them would evaluate happily on both sides. Parity is not the same as
//! governance: two runtimes agreeing on an operator nobody approved is two
//! runtimes agreeing on something the registry says is FORBIDDEN. The registry
//! is the allow-list; this is where it is enforced.
//!
//! **Lookup bindings**, against the source allow-list of
//! [`super::lookup::LookupSource`] (FR-RAD-007, [#161]). A lookup field is the
//! one component whose options come from the database, so it names a
//! master-data source — and the meta-schema cannot check that either, for a
//! reason it states about component types generally: `type` is an open
//! vocabulary defined by each implementation's registry, so which types exist
//! and what they require is a **registry-level** constraint, exactly as JFSS
//! §4.4 says of a text-bearing display component. This is Kelir's registry.
//!
//! **Refused at save, not at render.** A definition is written once and
//! rendered thousands of times, and the render path has no good failure: a form
//! that half-renders is worse than one that was never stored. A lookup naming a
//! source nobody serves is the sharpest case of it — the field renders, the
//! chooser opens, and it is empty, which is indistinguishable from master data
//! that happens to hold nothing.
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161

use std::sync::OnceLock;

use serde_json::Value;

use super::lookup::LookupSource;
use crate::error::ValidationDetail;

/// The meta-schema, vendored into the crate.
///
/// A copy of `docs/schema/jfss-meta-v2.0.1.json`, which stays canonical. It is
/// duplicated because the release image builds from the `kelir-backend`
/// directory alone — `include_str!` cannot reach `docs/` there, and a validator
/// that reads the schema from disk at startup would make a deployment's
/// correctness depend on a file nobody copied. `tests/rad_jfss_meta_schema.rs`
/// asserts the two are byte-identical, so the duplicate cannot drift quietly.
const META_SCHEMA: &str = include_str!("../jfss-meta-v2.0.1.json");

/// Operators approved for `calculate` (Calculation Rule Registry §2.1, §2.2).
///
/// §2.2's `sum` is here because Kelir registers it as a custom operator in both
/// environments, which is what the registry requires of it.
const CALCULATE_OPERATORS: &[&str] = &[
    "var", "+", "-", "*", "/", "%", "min", "max", "map", "filter", "reduce", "all", "some", "none",
    "sum",
];

/// Operators approved inside `conditional.logic`.
///
/// **This set is a floor, not a registry, and D-15 owns making it normative.**
/// The Calculation Rule Registry governs `calculate` only; the
/// [operator-parity spike](../../../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
/// §2.6 found that `conditional.logic` needs exactly the operators §2.3 forbids
/// in `calculate` and is bounded by no registry at all. Carrying that gap to
/// Sprints 14–16 is decision **D-15**.
///
/// Leaving the tier unbounded until then was the alternative, and it is worse:
/// it lets the engine's proprietary surface into stored schemas through the
/// door `calculate` closes. So the set below is the base operators plus the
/// comparison and logical ones — derived from §2.3's own stated reason for
/// forbidding them in `calculate`, which is that they return booleans rather
/// than numbers. That reason makes them exactly what a conditional wants.
const CONDITIONAL_OPERATORS: &[&str] = &[
    "var",
    "+",
    "-",
    "*",
    "/",
    "%",
    "min",
    "max",
    "map",
    "filter",
    "reduce",
    "all",
    "some",
    "none",
    "sum",
    "==",
    "===",
    "!=",
    "!==",
    ">",
    ">=",
    "<",
    "<=",
    "and",
    "or",
    "!",
    "!!",
    "if",
    "?:",
    "in",
    "missing",
    "missing_some",
];

/// The compiled meta-schema. Compiled once — it is ~12 KB of JSON Schema, and
/// compiling it per request would put that on the save path of every form.
fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();

    VALIDATOR.get_or_init(|| {
        let schema: Value =
            serde_json::from_str(META_SCHEMA).expect("the vendored JFSS meta-schema is valid JSON");

        jsonschema::validator_for(&schema).expect("the vendored JFSS meta-schema compiles")
    })
}

/// Validates a form definition, returning every problem rather than the first.
///
/// Every problem, because a form definition is edited by a person and a
/// validator that reports one error per round trip turns a ten-mistake document
/// into ten round trips.
pub fn validate_definition(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = shape_errors(definition);

    // Operators and lookups are checked even when the shape is wrong. The three
    // checks read different parts of the document, and a caller who has more
    // than one problem should be told about all of them.
    details.extend(operator_errors(definition));
    details.extend(lookup_errors(definition));
    details
}

/// Meta-schema violations, as validation details naming the JSON path.
fn shape_errors(definition: &Value) -> Vec<ValidationDetail> {
    validator()
        .iter_errors(definition)
        .map(|error| {
            // `instance_path` is a JSON Pointer (`/components/0/key`); the
            // envelope's `path` is dotted (`components.0.key`), which is what
            // every other validation detail in this API uses and what a JFSS
            // error path looks like (JFSS §12.4).
            let path = error.instance_path().to_string();
            let dotted = path.trim_start_matches('/').replace('/', ".");
            let dotted = if dotted.is_empty() {
                "definition".to_owned()
            } else {
                format!("definition.{dotted}")
            };

            ValidationDetail::new(dotted, "jfss", "INVALID_DEFINITION", error.to_string())
        })
        .collect()
}

/// Operators used outside the approved sets.
fn operator_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();

    walk_components(definition, "definition.components", &mut details);
    details
}

/// Walks the component tree, checking every `calculate` and `conditional`.
///
/// Written as a walk of the *document* rather than a search for the two keys,
/// because a search would also find a `calculate` inside `settings` or inside
/// some future property that is not a rule at all, and refuse a document for
/// carrying a word.
fn walk_components(node: &Value, path: &str, details: &mut Vec<ValidationDetail>) {
    let Some(components) = node.get("components").and_then(Value::as_array) else {
        return;
    };

    for (index, component) in components.iter().enumerate() {
        let here = format!("{path}.{index}");

        if let Some(calculate) = component.get("calculate") {
            check_operators(
                calculate,
                CALCULATE_OPERATORS,
                &format!("{here}.calculate"),
                details,
            );
        }

        if let Some(logic) = component.get("conditional").and_then(|c| c.get("logic")) {
            check_operators(
                logic,
                CONDITIONAL_OPERATORS,
                &format!("{here}.conditional.logic"),
                details,
            );
        }

        // A nested calculate is as much a stored operator as a top-level one.
        for (child, child_path) in child_containers(component, &here) {
            walk_components(child, &child_path, details);
        }
    }
}

/// Every node under `component` whose `components` array holds children, with
/// the path each of them is at.
///
/// **The closed set of three shapes, in one place** (JFSS §4.3.1): children live
/// under `components` on a single-slot container, and under `columns` / `tabs`
/// as slot objects that each carry their own `components`. §4.3.1 requires a
/// conformant implementation to traverse all three *wherever* it walks the tree,
/// and this file now walks it twice — once for operators and once for lookups.
/// Two traversals that have to agree about the shapes are two places to forget
/// one, which is the failure §4.3.1 describes: traversing only `components`
/// silently ignores every child nested inside a `columns` or `tabs` container.
///
/// A `data` component's `components` is a row template rather than a set of
/// siblings, and it is descended into all the same: a lookup declared inside a
/// datagrid row is as much a stored lookup as one at the top level.
fn child_containers<'a>(component: &'a Value, here: &str) -> Vec<(&'a Value, String)> {
    let mut containers = vec![(component, format!("{here}.components"))];

    for slot_key in ["columns", "tabs"] {
        if let Some(slots) = component.get(slot_key).and_then(Value::as_array) {
            for (slot_index, slot) in slots.iter().enumerate() {
                containers.push((slot, format!("{here}.{slot_key}.{slot_index}.components")));
            }
        }
    }

    containers
}

/// Collects operator keys from a JSON Logic expression and refuses unapproved
/// ones.
///
/// An expression is an object with exactly one key — the operator — whose value
/// holds the arguments, and arguments are themselves expressions. So the walk
/// is: every object key at an operator position, recursively.
///
/// `var`'s argument is a data path rather than an expression, and a path is a
/// string or an array of two, neither of which is an object — so it needs no
/// special case. A path that *is* an object would be malformed, and reporting
/// its key as an unapproved operator says so accurately enough.
fn check_operators(
    expression: &Value,
    approved: &[&str],
    path: &str,
    details: &mut Vec<ValidationDetail>,
) {
    match expression {
        Value::Object(map) => {
            for (operator, argument) in map {
                if !approved.contains(&operator.as_str()) {
                    details.push(ValidationDetail::new(
                        path,
                        "operator",
                        "OPERATOR_NOT_REGISTERED",
                        format!(
                            "`{operator}` is not in the JFSS rule registry for this property, \
                             so it is forbidden — an operator the engine happens to support is \
                             not an operator the registry approves"
                        ),
                    ));
                }

                if operator == "sum" {
                    check_sum_arity(argument, path, details);
                }

                check_operators(argument, approved, path, details);
            }
        }
        Value::Array(items) => {
            for item in items {
                check_operators(item, approved, path, details);
            }
        }
        _ => {}
    }
}

/// Refuses a `sum` that would evaluate to `0` without meaning to
/// ([#201](https://github.com/sujanto-gaws/kelir/issues/201), decision **D-22**).
///
/// **The defect is silence, not arithmetic.** `sum` reads one argument and
/// expects it to be an array. Given anything else it contributes nothing and
/// answers `0` — on both engines, identically, which is D-10 working exactly as
/// designed and is precisely what hides this: JFSS S8.1's server-side
/// re-evaluation catches a client that *disagrees* with the server, and here
/// the two agree on the wrong number. The Tamper-Proof Pattern confirms the `0`
/// instead of refusing it.
///
/// `{"sum": [a, b]}` is the mistake worth guarding, and it is a natural one:
/// `+` sits beside `sum` in the registry, in the same allow-list, with the same
/// bracket syntax, and it *does* take a list of operands. A form author who
/// writes `sum` where they meant `+` gets a grand total of zero and no error
/// anywhere on the path — which is the shape the Calculation Rule Registry §4.1
/// describes as turning a 42-rupiah invoice line into free.
///
/// **Refused at save rather than at evaluation.** A definition is written once
/// and rendered thousands of times; refusing here costs one round trip to the
/// person who can fix it, and refusing at render costs a form that half-works
/// for everyone else. It also leaves the two engines untouched, so nothing
/// about parity moves.
///
/// **A non-array argument is left alone**, and that is measured rather than
/// assumed. JSON Logic's shorthand — `{"sum": {"var": "line_totals"}}`, the
/// argument given directly instead of wrapped in a list — is one argument, and
/// both engines evaluate it correctly (`3` for `[1, 2]`). Refusing it would
/// refuse definitions that work. What cannot work is an argument *list* of any
/// length but one: zero arguments has nothing to sum, and two or more means the
/// author expected `+`.
fn check_sum_arity(argument: &Value, path: &str, details: &mut Vec<ValidationDetail>) {
    let Some(arguments) = argument.as_array() else {
        // The shorthand. One argument, evaluated correctly on both sides.
        return;
    };

    if arguments.len() == 1 {
        return;
    }

    details.push(ValidationDetail::new(
        path,
        "arity",
        "SUM_TAKES_ONE_ARRAY",
        format!(
            "`sum` takes exactly one argument and sums the array it evaluates to; \
             this one has {}. It would evaluate to 0 on both the server and the \
             browser without reporting anything — for a list of operands use `+`",
            arguments.len()
        ),
    ));
}

/// Where a form declares which of its fields are lookups, and onto what.
///
/// **`settings` is the extension point JFSS gives an implementation, and it is
/// the only one.** The specification is a `Final Standard` — frozen at v2.0.1,
/// errata only, and a successor takes a new major version ([naming
/// convention](../../../../../docs/standards/02.%20Naming%20Convention.md)
/// §10.1) — so a new component property is not a thing this issue may add: the
/// meta-schema closes a component with `unevaluatedProperties: false`, and
/// `settings` is the one object in the document declared
/// `additionalProperties: true` and described as "global form configurations".
/// A definition carrying a lookup binding is therefore a conformant JFSS v2.0.1
/// document, which a definition carrying a `dataSource` property would not be.
///
/// The cost is stated rather than hidden: the binding does not sit on the
/// component it binds, which is a departure from JFSS's own component-driven
/// philosophy. Making it local would mean JFSS v3.0.0, and that is a phase-scale
/// decision rather than a sprint item.
const LOOKUP_BINDINGS: &str = "settings.lookups";

/// A `data` component this document declares, as the lookup check sees it.
struct DataComponent<'a> {
    path: String,
    id: Option<&'a str>,
    component_type: Option<&'a str>,
    has_options: bool,
}

/// Lookup bindings that name nothing, and lookup fields nothing binds.
///
/// **Bound by component `id`, not by `key`.** JFSS §4.1 makes `id` the globally
/// unique identifier of a component *instance*, which is exactly the join a
/// binding table needs. `key` is not that: a `data` component inside a datagrid
/// row template holds a key in the row's scope, so two components in one
/// document may legitimately carry the same `key` and a binding on it would be
/// ambiguous about which field it meant.
///
/// The five refusals below are the five ways a binding can be wrong, and each is
/// a form that renders wrongly rather than one that fails to render — which is
/// the whole reason they are caught here.
fn lookup_errors(definition: &Value) -> Vec<ValidationDetail> {
    let mut details = Vec::new();
    let mut components = Vec::new();

    collect_data_components(definition, "definition.components", &mut components);

    let bindings = definition
        .get("settings")
        .and_then(|settings| settings.get("lookups"));

    let mut bound: Vec<&str> = Vec::new();

    match bindings {
        None => {}
        Some(Value::Object(map)) => {
            for (id, source) in map {
                bound.push(id.as_str());
                check_binding(id, source, &components, &mut details);
            }
        }
        Some(_) => details.push(ValidationDetail::new(
            format!("definition.{LOOKUP_BINDINGS}"),
            "type",
            "LOOKUP_BINDINGS_INVALID",
            format!(
                "`{LOOKUP_BINDINGS}` must be an object mapping a component id to a \
                 lookup source, one of {}",
                LookupSource::keys()
            ),
        )),
    }

    // The other direction. A field the renderer will treat as a lookup, with
    // nothing saying where its options come from, is a chooser that opens empty
    // — and an empty chooser reads as master data holding nothing.
    for component in &components {
        if component.component_type != Some(LOOKUP_TYPE) {
            continue;
        }

        if component.id.is_some_and(|id| bound.contains(&id)) {
            continue;
        }

        details.push(ValidationDetail::new(
            component.path.clone(),
            "lookup",
            "LOOKUP_SOURCE_MISSING",
            format!(
                "a `{LOOKUP_TYPE}` component takes its options from master data, so \
                 `{LOOKUP_BINDINGS}` must bind its `id` to one of {}",
                LookupSource::keys()
            ),
        ));
    }

    details
}

/// The component type whose options come from master data rather than the
/// definition.
///
/// `type` is an open vocabulary (JFSS §4.2), so this is Kelir's registry naming
/// its own — the same way the renderer's component registry names `textfield`.
const LOOKUP_TYPE: &str = "lookup";

/// One binding: the component it names, and the source it points at.
fn check_binding(
    id: &str,
    source: &Value,
    components: &[DataComponent<'_>],
    details: &mut Vec<ValidationDetail>,
) {
    let path = format!("definition.{LOOKUP_BINDINGS}.{id}");

    let Some(source) = source.as_str() else {
        details.push(ValidationDetail::new(
            path,
            "type",
            "LOOKUP_SOURCE_INVALID",
            format!(
                "a lookup binding is the name of a master-data source, one of {}",
                LookupSource::keys()
            ),
        ));

        return;
    };

    if LookupSource::from_key(source).is_none() {
        details.push(ValidationDetail::new(
            path.clone(),
            "enum",
            "LOOKUP_SOURCE_UNKNOWN",
            format!(
                "`{source}` is not a master-data source this deployment serves; \
                 it must be one of {}",
                LookupSource::keys()
            ),
        ));
    }

    let matched: Vec<&DataComponent<'_>> = components
        .iter()
        .filter(|component| component.id == Some(id))
        .collect();

    match matched.as_slice() {
        [] => details.push(ValidationDetail::new(
            path,
            "reference",
            "LOOKUP_COMPONENT_NOT_FOUND",
            format!(
                "`{id}` is not the id of a data component in this definition, so the \
                 binding would resolve options for a field that does not exist"
            ),
        )),
        [component] => {
            if component.has_options {
                details.push(ValidationDetail::new(
                    format!("{}.options", component.path),
                    "conflict",
                    "LOOKUP_HAS_STATIC_OPTIONS",
                    "a bound lookup takes its options from master data, so it must not \
                     also carry an `options` array — a component holding both leaves the \
                     renderer to choose which one a person sees"
                        .to_owned(),
                ));
            }
        }
        matched => details.push(ValidationDetail::new(
            path,
            "reference",
            "LOOKUP_COMPONENT_AMBIGUOUS",
            format!(
                "`{id}` is the id of {} data components; JFSS §4.1 makes `id` unique per \
                 component instance, and a binding cannot say which one it meant",
                matched.len()
            ),
        )),
    }
}

/// Every `data` component in the document, with the path it is at.
///
/// Descends through the same three child-container shapes the operator walk
/// does, by way of the same [`child_containers`] — the two must agree about
/// where children live, and this is how they are made to.
fn collect_data_components<'a>(node: &'a Value, path: &str, found: &mut Vec<DataComponent<'a>>) {
    let Some(components) = node.get("components").and_then(Value::as_array) else {
        return;
    };

    for (index, component) in components.iter().enumerate() {
        let here = format!("{path}.{index}");

        if component.get("role").and_then(Value::as_str) == Some("data") {
            found.push(DataComponent {
                path: here.clone(),
                id: component.get("id").and_then(Value::as_str),
                component_type: component.get("type").and_then(Value::as_str),
                has_options: component.get("options").is_some(),
            });
        }

        for (child, child_path) in child_containers(component, &here) {
            collect_data_components(child, &child_path, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The smallest document the meta-schema accepts, as a base for mutation.
    fn minimal() -> Value {
        json!({
            "formId": "purchase-requisition",
            "version": "2.0.1",
            "title": "Purchase requisition",
            "components": [
                {
                    "id": "quantity",
                    "role": "data",
                    "type": "number",
                    "key": "quantity",
                    "label": "Quantity",
                    "validation": { "type": "number" }
                }
            ]
        })
    }

    fn paths(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.path.as_str()).collect()
    }

    fn with_calculate(calculate: Value) -> Value {
        let mut definition = minimal();
        definition["components"][0]["calculate"] = calculate;
        definition
    }

    #[test]
    fn refuses_a_sum_whose_argument_list_is_not_one_long() {
        // `+` is beside `sum` in the registry, in the same allow-list, with the
        // same bracket syntax, and it does take a list of operands. Writing
        // `sum` where `+` was meant evaluates to 0 on both engines and reports
        // nothing (#201).
        for arguments in [
            json!({ "sum": [{ "var": "a" }, { "var": "b" }] }),
            json!({ "sum": [] }),
            json!({ "sum": [{ "var": "a" }, { "var": "b" }, { "var": "c" }] }),
        ] {
            let details = validate_definition(&with_calculate(arguments.clone()));

            assert!(
                details
                    .iter()
                    .any(|detail| detail.code == "SUM_TAKES_ONE_ARRAY"),
                "expected {arguments} to be refused, got {details:?}"
            );
        }
    }

    #[test]
    fn accepts_the_two_sum_shapes_that_evaluate_correctly() {
        // Measured on both engines rather than assumed: the wrapped form and
        // JSON Logic's shorthand both answer 3 for `[1, 2]`. Refusing the
        // shorthand would refuse definitions that work.
        for arguments in [
            json!({ "sum": [{ "var": "line_totals" }] }),
            json!({ "sum": { "var": "line_totals" } }),
        ] {
            assert_eq!(
                validate_definition(&with_calculate(arguments.clone())),
                Vec::new(),
                "expected {arguments} to be accepted"
            );
        }
    }

    #[test]
    fn refuses_a_nested_sum_too() {
        // The walk recurses, so a `sum` inside a `map` inside a `sum` is as
        // much a stored operator as a top-level one.
        let details = validate_definition(&with_calculate(json!({
            "sum": [{ "map": [{ "var": "items" }, { "sum": [{ "var": "a" }, { "var": "b" }] }] }]
        })));

        assert!(details
            .iter()
            .any(|detail| detail.code == "SUM_TAKES_ONE_ARRAY"));
    }

    #[test]
    fn accepts_a_conforming_definition() {
        assert_eq!(validate_definition(&minimal()), Vec::new());
    }

    #[test]
    fn refuses_a_definition_missing_a_required_property() {
        let mut definition = minimal();
        definition.as_object_mut().expect("object").remove("formId");

        let details = validate_definition(&definition);

        assert!(!details.is_empty(), "a document with no formId is not JFSS");
        assert!(details
            .iter()
            .all(|detail| detail.code == "INVALID_DEFINITION"));
    }

    #[test]
    fn refuses_a_version_outside_the_two_line() {
        let mut definition = minimal();
        definition["version"] = json!("1.9.0");

        assert!(!validate_definition(&definition).is_empty());
    }

    #[test]
    fn accepts_the_registry_invoice_calculation() {
        let mut definition = minimal();
        definition["components"][0]["calculate"] = json!({
            "sum": [{"map": [
                {"var": "items"},
                {"*": [{"var": "unit_price"}, {"var": "quantity"}]},
            ]}]
        });

        assert_eq!(validate_definition(&definition), Vec::new());
    }

    /// The check the meta-schema cannot make, and the reason this module exists.
    #[test]
    fn refuses_an_operator_the_engine_supports_and_the_registry_does_not() {
        let mut definition = minimal();
        // `datetime` is real in the adopted engine and appears in no registry.
        definition["components"][0]["calculate"] = json!({
            "+": [{"var": "days"}, {"datetime": ["2026-08-25"]}]
        });

        let details = validate_definition(&definition);

        assert!(
            details
                .iter()
                .any(|detail| detail.code == "OPERATOR_NOT_REGISTERED"),
            "an unregistered operator must be refused; got {details:?}"
        );
        assert_eq!(
            paths(&details),
            vec!["definition.components.0.calculate"],
            "the refusal names the property it is in"
        );
    }

    #[test]
    fn refuses_a_forbidden_operator_in_calculate() {
        let mut definition = minimal();
        // §2.3: comparison operators return booleans, not numeric values.
        definition["components"][0]["calculate"] = json!({">": [{"var": "a"}, 10]});

        let details = validate_definition(&definition);

        assert!(details
            .iter()
            .any(|detail| detail.code == "OPERATOR_NOT_REGISTERED"));
    }

    #[test]
    fn allows_a_comparison_inside_a_conditional() {
        // The same operator, in the property it belongs to. A check that
        // refused it everywhere would make the conditional tier unusable.
        let mut definition = minimal();
        definition["components"][0]["conditional"] = json!({
            "action": "show",
            "logic": {">": [{"var": "total"}, 1000]}
        });

        assert_eq!(validate_definition(&definition), Vec::new());
    }

    #[test]
    fn refuses_an_unregistered_operator_inside_a_conditional_too() {
        let mut definition = minimal();
        definition["components"][0]["conditional"] = json!({
            "action": "show",
            "logic": {"flagd": ["some-flag"]}
        });

        let details = validate_definition(&definition);

        assert!(details
            .iter()
            .any(|detail| detail.code == "OPERATOR_NOT_REGISTERED"));
    }

    #[test]
    fn finds_an_operator_nested_in_a_layout_component() {
        // The walk has to descend, or a builder puts every rule one panel down
        // and the check sees nothing.
        let definition = json!({
            "formId": "nested",
            "version": "2.0.1",
            "components": [{
                "id": "panel",
                "role": "layout",
                "type": "panel",
                "components": [{
                    "id": "total",
                    "role": "data",
                    "type": "number",
                    "key": "total",
                    "label": "Total",
                    "validation": { "type": "number" },
                    "calculate": {"ext-math": [1, 2]}
                }]
            }]
        });

        let details = validate_definition(&definition);

        assert!(
            details
                .iter()
                .any(|detail| detail.code == "OPERATOR_NOT_REGISTERED"),
            "got {details:?}"
        );
        assert_eq!(
            paths(&details),
            vec!["definition.components.0.components.0.calculate"]
        );
    }

    // -----------------------------------------------------------------------
    // Lookup bindings (FR-RAD-007, #161 AC1)
    // -----------------------------------------------------------------------

    /// A definition with one lookup field, bound to `source`.
    fn with_lookup(source: Value) -> Value {
        json!({
            "formId": "purchase-requisition",
            "version": "2.0.1",
            "settings": { "lookups": { "supplier_field": source } },
            "components": [{
                "id": "supplier_field",
                "role": "data",
                "type": "lookup",
                "key": "supplier_id",
                "label": "Supplier",
                "validation": { "type": "string" }
            }]
        })
    }

    fn codes(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.code.as_str()).collect()
    }

    #[test]
    fn accepts_a_lookup_bound_to_a_source_this_deployment_serves() {
        // The whole shape, end to end: a conformant JFSS v2.0.1 document — no
        // property outside the frozen meta-schema — that declares a field whose
        // options come from master data.
        for source in crate::modules::rad::domain::lookup::SOURCES {
            assert_eq!(
                validate_definition(&with_lookup(json!(source.key()))),
                Vec::new(),
                "{} must be bindable",
                source.key()
            );
        }
    }

    #[test]
    fn refuses_a_lookup_bound_to_a_source_nobody_serves() {
        // The failure this is here for: the field renders, the chooser opens,
        // and it is empty — which reads as master data holding nothing rather
        // than as a definition nobody can serve.
        let details = validate_definition(&with_lookup(json!("parties")));

        assert_eq!(
            codes(&details),
            vec!["LOOKUP_SOURCE_UNKNOWN"],
            "{details:?}"
        );
        assert_eq!(
            paths(&details),
            vec!["definition.settings.lookups.supplier_field"]
        );
        assert!(
            details[0].message.contains("supplier"),
            "the refusal must name what the author may write: {}",
            details[0].message
        );
    }

    #[test]
    fn refuses_a_binding_that_is_not_the_name_of_a_source() {
        let details = validate_definition(&with_lookup(json!({ "entity": "supplier" })));

        assert!(details
            .iter()
            .any(|detail| detail.code == "LOOKUP_SOURCE_INVALID"));
    }

    #[test]
    fn refuses_a_lookups_object_that_is_not_an_object() {
        let mut definition = with_lookup(json!("supplier"));
        definition["settings"]["lookups"] = json!(["supplier"]);

        let details = validate_definition(&definition);

        assert!(details
            .iter()
            .any(|detail| detail.code == "LOOKUP_BINDINGS_INVALID"));
    }

    #[test]
    fn refuses_a_lookup_field_nothing_binds() {
        // A `lookup` component with no source is a chooser with nothing behind
        // it, and the renderer has no way to tell that from an empty list.
        let mut definition = with_lookup(json!("supplier"));
        definition["settings"] = json!({});

        let details = validate_definition(&definition);

        assert_eq!(
            codes(&details),
            vec!["LOOKUP_SOURCE_MISSING"],
            "{details:?}"
        );
        assert_eq!(paths(&details), vec!["definition.components.0"]);
    }

    #[test]
    fn refuses_a_binding_that_names_no_component() {
        let mut definition = with_lookup(json!("supplier"));
        definition["settings"]["lookups"] = json!({ "customer_field": "customer" });

        let details = validate_definition(&definition);

        // Both directions fire, and both are true: the binding names nothing,
        // and the lookup field is unbound.
        assert!(details
            .iter()
            .any(|detail| detail.code == "LOOKUP_COMPONENT_NOT_FOUND"));
        assert!(details
            .iter()
            .any(|detail| detail.code == "LOOKUP_SOURCE_MISSING"));
    }

    #[test]
    fn refuses_a_binding_onto_an_id_two_components_share() {
        // JFSS §4.1 makes `id` unique per component instance and JSON Schema
        // cannot express it, so a binding is where the ambiguity surfaces.
        let mut definition = with_lookup(json!("supplier"));
        definition["components"]
            .as_array_mut()
            .expect("array")
            .push(json!({
                "id": "supplier_field",
                "role": "data",
                "type": "textfield",
                "key": "supplier_name",
                "label": "Supplier name",
                "validation": { "type": "string" }
            }));

        let details = validate_definition(&definition);

        assert!(
            details
                .iter()
                .any(|detail| detail.code == "LOOKUP_COMPONENT_AMBIGUOUS"),
            "{details:?}"
        );
    }

    #[test]
    fn refuses_a_bound_lookup_that_also_carries_static_options() {
        // Two answers to the same question, and the renderer would have to pick
        // one. Refusing is the only reading that is not a guess.
        let mut definition = with_lookup(json!("supplier"));
        definition["components"][0]["options"] = json!([{ "label": "Acme", "value": "acme" }]);

        let details = validate_definition(&definition);

        assert_eq!(
            codes(&details),
            vec!["LOOKUP_HAS_STATIC_OPTIONS"],
            "{details:?}"
        );
        assert_eq!(paths(&details), vec!["definition.components.0.options"]);
    }

    #[test]
    fn finds_a_lookup_nested_in_a_tab() {
        // The traversal §4.3.1 requires. A builder that puts every field one tab
        // down would otherwise store lookups this check never sees.
        let definition = json!({
            "formId": "nested-lookup",
            "version": "2.0.1",
            "components": [{
                "id": "tabs",
                "role": "layout",
                "type": "tabs",
                "tabs": [{
                    "title": "Supplier",
                    "components": [{
                        "id": "supplier_field",
                        "role": "data",
                        "type": "lookup",
                        "key": "supplier_id",
                        "label": "Supplier",
                        "validation": { "type": "string" }
                    }]
                }]
            }]
        });

        let details = validate_definition(&definition);

        assert_eq!(
            codes(&details),
            vec!["LOOKUP_SOURCE_MISSING"],
            "{details:?}"
        );
        assert_eq!(
            paths(&details),
            vec!["definition.components.0.tabs.0.components.0"]
        );
    }

    #[test]
    fn binds_a_lookup_inside_a_datagrid_row_by_its_own_id() {
        // Why the binding is on `id` and not on `key`: a row template's keys
        // live in the row's scope, so two components in one document may share
        // one. The id is unique per instance, so the binding is not ambiguous.
        let definition = json!({
            "formId": "line-items",
            "version": "2.0.1",
            "settings": { "lookups": { "row_supplier": "supplier" } },
            "components": [{
                "id": "line_items",
                "role": "data",
                "type": "datagrid",
                "key": "line_items",
                "label": "Line items",
                "validation": { "type": "array" },
                "components": [{
                    "id": "row_supplier",
                    "role": "data",
                    "type": "lookup",
                    "key": "supplier_id",
                    "label": "Supplier",
                    "validation": { "type": "string" }
                }]
            }]
        });

        assert_eq!(validate_definition(&definition), Vec::new());
    }

    #[test]
    fn leaves_a_definition_with_no_lookups_alone() {
        // The check must cost nothing to a form that has no lookup in it, or
        // every existing definition becomes unsaveable.
        assert_eq!(validate_definition(&minimal()), Vec::new());
    }

    #[test]
    fn reports_every_problem_rather_than_the_first() {
        let mut definition = minimal();
        definition["components"][0]["calculate"] = json!({"flagd": ["a"]});
        definition["components"]
            .as_array_mut()
            .expect("array")
            .push(json!({
                "id": "second",
                "role": "data",
                "type": "number",
                "key": "second",
                "label": "Second",
                "validation": { "type": "number" },
                "calculate": {"datetime": ["b"]}
            }));

        let details = validate_definition(&definition);

        assert_eq!(
            details
                .iter()
                .filter(|detail| detail.code == "OPERATOR_NOT_REGISTERED")
                .count(),
            2,
            "a person editing a definition should see both, not one per round trip"
        );
    }
}
