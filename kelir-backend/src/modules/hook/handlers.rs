//! The `core:` handlers, and the catalogue that resolves a reference to one
//! (LHCS §2, [#339]).
//!
//! **A reference MUST resolve at registration time** — §2 says so, and an
//! unknown `core:` handler is an ERROR rather than a warning. That is what
//! [`resolve`] is for and why [`super::service::registration_errors`] calls it
//! at publish: a workflow definition naming `core:reserve_bugdet` is refused to
//! the person who typed it, not discovered by the first document to reach the
//! transition.
//!
//! # What ships, and why it is three
//!
//! A registry with no handlers would make the whole chain unobservable, and one
//! with a dozen would be inventing product. These three are the smallest set
//! that exercises **all three** of LHCS §5.1's results against real work:
//!
//! - [`continue_always`] returns `CONTINUE` — the identity, and the thing a
//!   definition uses to prove a chain runs at all.
//! - [`set_form_field`] returns `MODIFY` — the *set a field* [#339] names as
//!   Phase 7's reason for system tasks.
//! - [`reject_when`] returns `REJECT` — a guard over the JSON Logic evaluator
//!   [#338] built, which is the other half of the same sentence.
//!
//! **Every one of them is pure.** A handler that reached the database would
//! need the caller's transaction threaded into this signature, and the first
//! one that did would set the shape for the rest — so the boundary is drawn
//! before rather than after: a core handler receives the payload and returns a
//! result. `core:require_attachment`, the catalogue's own example in
//! architectures/01 §12.3, is the first that will not fit and is deliberately
//! not here.
//!
//! [#338]: https://github.com/sujanto-gaws/kelir/issues/338
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

use serde_json::Value;

use super::domain::{form_data_of, setting, HookResult, Rejection};
use crate::modules::rad::evaluator::RuleEvaluator;

/// A core handler: the payload in, a result out.
pub type Handler = fn(&Value, &RuleEvaluator) -> HookResult;

/// Every `core:` handler this build performs, by name.
///
/// A `match` rather than a map, for the reason
/// `rad::domain::validation::registry_rule` gives about the same shape: adding
/// a name without writing what it does does not compile.
pub fn resolve(handler: &str) -> Option<Handler> {
    Some(match handler {
        "continue_always" => continue_always,
        "set_form_field" => set_form_field,
        "reject_when" => reject_when,
        _ => return None,
    })
}

/// The names, for a refusal that says what *is* available.
///
/// A refusal naming only the bad handler leaves somebody guessing; the point of
/// a closed registry is that it can be shown.
pub const NAMES: [&str; 3] = ["continue_always", "set_form_field", "reject_when"];

/// The names as a message lists them.
pub fn available() -> String {
    NAMES
        .iter()
        .map(|name| format!("`core:{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `CONTINUE`, always.
///
/// **Not a placeholder.** A chain that must be observed doing nothing is how a
/// definition author checks that their registration resolved and ran at all,
/// and it is what a service task declares when its step is *advance* and
/// nothing more.
fn continue_always(_payload: &Value, _evaluator: &RuleEvaluator) -> HookResult {
    HookResult::Continue
}

/// `MODIFY`: writes `config.value` into `config.field` of the form payload.
///
/// **The set-a-field step [#339] names.** A `SERVICE_TASK` whose `AUTO`
/// transition registers this stamps a value and advances, which is a step the
/// product performs rather than a person — the definition of the thing.
///
/// `config.value` is taken verbatim and is **not** evaluated as an expression.
/// A handler that took JSON Logic here would be a second evaluation surface
/// beside `reject_when`'s, with a different meaning for the same key; a
/// definition that wants a computed value uses a JWSS variable, which is what
/// `source` is for.
///
/// A missing or non-string `config.field` is `CONTINUE` rather than an error:
/// the registration was accepted, the handler ran, and it had nothing to write.
/// The execution log records the `CONTINUE`, which is where a misconfigured
/// entry is visible.
fn set_form_field(payload: &Value, _evaluator: &RuleEvaluator) -> HookResult {
    let config = payload.get("config").cloned().unwrap_or(Value::Null);

    let Some(field) = setting(&config, "field") else {
        return HookResult::Continue;
    };

    let mut form_data = form_data_of(payload);

    form_data.insert(
        field.to_owned(),
        config.get("value").cloned().unwrap_or(Value::Null),
    );

    HookResult::Modify {
        form_data: Some(Value::Object(form_data)),
        metadata: None,
    }
}

/// `REJECT` when `config.condition` holds against the payload.
///
/// The guard shape architectures/01 §12.3 lists for a transition — *budget
/// availability*, *approval limit check* — expressed with the evaluator
/// [#338] pinned, so a condition here means exactly what the same expression
/// means in a JWSS `condition` or a JFSS `calculate`.
///
/// **The expression sees the whole payload**, which is LHCS §4's object: a
/// condition reads `{"var": "formData.amount"}` and `{"var": "config.limit"}`
/// alike. That is wider than a JWSS condition's context and deliberately so —
/// a handler is registered against a hook rather than against a definition, and
/// the payload is the only thing every registration shares.
///
/// **An expression that cannot be evaluated is a `REJECT`**, not a pass. It is
/// **D-26**'s rule one layer out: a guard that fails open is a guard that is not
/// there, reported as a guard that passed. The code says which happened, so the
/// two are told apart in the log and in the response.
fn reject_when(payload: &Value, evaluator: &RuleEvaluator) -> HookResult {
    let config = payload.get("config").cloned().unwrap_or(Value::Null);

    let Some(condition) = config.get("condition") else {
        return HookResult::Continue;
    };

    match evaluator.evaluate(condition, payload) {
        Ok(Value::Bool(true)) => HookResult::Reject(Rejection::new(
            setting(&config, "code").unwrap_or("HOOK_CONDITION_MET"),
            setting(&config, "message")
                .unwrap_or("a guard on this transition refused it")
                .to_owned(),
        )),
        Ok(_) => HookResult::Continue,
        Err(error) => {
            tracing::info!(
                error = %error.message(),
                "a `core:reject_when` condition produced no value"
            );

            HookResult::Reject(Rejection::new(
                "HOOK_CONDITION_UNDECIDABLE",
                "a guard on this transition is decided by an expression, and that expression \
                 produced no value — so it is refused rather than treated as passed",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(handler: &str, payload: Value) -> HookResult {
        resolve(handler).expect("the handler resolves")(&payload, &RuleEvaluator::new())
    }

    #[test]
    fn resolves_every_name_it_publishes() {
        for name in NAMES {
            assert!(resolve(name).is_some(), "`{name}` does not resolve");
        }
    }

    /// §2: an unknown `core:` handler is an ERROR, so this has to be `None`
    /// rather than a no-op that would let a typo run as a passing guard.
    #[test]
    fn a_name_the_registry_does_not_hold_does_not_resolve() {
        assert!(resolve("reserve_bugdet").is_none());
    }

    #[test]
    fn continue_always_continues() {
        assert_eq!(run("continue_always", json!({})), HookResult::Continue);
    }

    // -- set_form_field ---------------------------------------------------

    #[test]
    fn set_form_field_writes_its_value_and_keeps_the_rest() {
        let outcome = run(
            "set_form_field",
            json!({
                "formData": {"amount": 42, "title": "Desks"},
                "config": {"field": "approved_by_system", "value": true},
            }),
        );

        let HookResult::Modify { form_data, .. } = outcome else {
            panic!("expected MODIFY, got {outcome:?}");
        };
        let form_data = form_data.expect("MODIFY carries the payload it changed");

        assert_eq!(form_data["approved_by_system"], json!(true));
        // The rest survives: a handler that returned only its own field would
        // have the engine replace the payload with one field in it.
        assert_eq!(form_data["amount"], json!(42));
        assert_eq!(form_data["title"], json!("Desks"));
    }

    #[test]
    fn set_form_field_overwrites_a_value_that_is_already_there() {
        let outcome = run(
            "set_form_field",
            json!({"formData": {"stage": "draft"}, "config": {"field": "stage", "value": "checked"}}),
        );

        let HookResult::Modify { form_data, .. } = outcome else {
            panic!("expected MODIFY");
        };

        assert_eq!(form_data.expect("payload")["stage"], json!("checked"));
    }

    #[test]
    fn set_form_field_takes_its_value_verbatim_rather_than_evaluating_it() {
        // A definition wanting a computed value uses a JWSS variable `source`.
        // Evaluating here would be a second expression surface with a different
        // meaning for the same key.
        let outcome = run(
            "set_form_field",
            json!({"formData": {}, "config": {"field": "note", "value": {"var": "formData.amount"}}}),
        );

        let HookResult::Modify { form_data, .. } = outcome else {
            panic!("expected MODIFY");
        };

        assert_eq!(
            form_data.expect("payload")["note"],
            json!({"var": "formData.amount"})
        );
    }

    #[test]
    fn set_form_field_with_no_field_configured_continues() {
        assert_eq!(
            run("set_form_field", json!({"formData": {}, "config": {}})),
            HookResult::Continue
        );
    }

    // -- reject_when ------------------------------------------------------

    #[test]
    fn reject_when_refuses_where_its_condition_holds() {
        let outcome = run(
            "reject_when",
            json!({
                "formData": {"amount": 9_000},
                "config": {
                    "condition": {">": [{"var": "formData.amount"}, 5_000]},
                    "code": "BUDGET_EXCEEDED",
                    "message": "Above the approval limit",
                },
            }),
        );

        let HookResult::Reject(rejection) = outcome else {
            panic!("expected REJECT, got {outcome:?}");
        };

        assert_eq!(rejection.code, "BUDGET_EXCEEDED");
        assert_eq!(rejection.message, "Above the approval limit");
    }

    #[test]
    fn reject_when_continues_where_its_condition_does_not_hold() {
        assert_eq!(
            run(
                "reject_when",
                json!({
                    "formData": {"amount": 100},
                    "config": {"condition": {">": [{"var": "formData.amount"}, 5_000]}},
                }),
            ),
            HookResult::Continue
        );
    }

    /// **D-26 one layer out**: a guard that cannot be decided is refused, not
    /// passed. A guard that fails open is a guard that is not there, reported
    /// as a guard that passed.
    #[test]
    fn reject_when_refuses_a_condition_it_cannot_evaluate() {
        let outcome = run(
            "reject_when",
            json!({
                "formData": {"amount": 10},
                // Division by zero produces no value at all (D-24).
                "config": {"condition": {">": [{"/": [{"var": "formData.amount"}, 0]}, 1]}},
            }),
        );

        let HookResult::Reject(rejection) = outcome else {
            panic!("expected REJECT, got {outcome:?}");
        };

        // And the code says *which* refusal it was, so an undecidable guard is
        // told apart from one that decided against you.
        assert_eq!(rejection.code, "HOOK_CONDITION_UNDECIDABLE");
    }

    #[test]
    fn reject_when_with_no_condition_configured_continues() {
        assert_eq!(
            run("reject_when", json!({"config": {}})),
            HookResult::Continue
        );
    }

    #[test]
    fn a_refusal_that_names_no_code_still_carries_one() {
        let outcome = run(
            "reject_when",
            json!({"formData": {}, "config": {"condition": {"==": [1, 1]}}}),
        );

        let HookResult::Reject(rejection) = outcome else {
            panic!("expected REJECT");
        };

        assert_eq!(rejection.code, "HOOK_CONDITION_MET");
        assert!(!rejection.message.is_empty());
    }
}
