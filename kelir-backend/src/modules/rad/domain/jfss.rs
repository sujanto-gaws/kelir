//! Validating a stored form definition (FR-RAD-002, #156 AC2).
//!
//! Two checks, and they catch different things.
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
//! **Refused at save, not at render.** A definition is written once and
//! rendered thousands of times, and the render path has no good failure: a form
//! that half-renders is worse than one that was never stored.

use std::sync::OnceLock;

use serde_json::Value;

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

    // Operators are checked even when the shape is wrong. The two checks read
    // different parts of the document, and a caller who has both problems
    // should be told about both.
    details.extend(operator_errors(definition));
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

        // Children live under `components` on a layout component, and under
        // `columns` / `tabs` as slot objects that each carry their own
        // `components` (JFSS v2.0.1 added those shapes). A nested calculate is
        // as much a stored operator as a top-level one.
        walk_components(component, &format!("{here}.components"), details);

        for slot_key in ["columns", "tabs"] {
            if let Some(slots) = component.get(slot_key).and_then(Value::as_array) {
                for (slot_index, slot) in slots.iter().enumerate() {
                    walk_components(
                        slot,
                        &format!("{here}.{slot_key}.{slot_index}.components"),
                        details,
                    );
                }
            }
        }
    }
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
