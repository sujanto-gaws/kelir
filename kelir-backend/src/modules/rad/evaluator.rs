//! The JSON Logic evaluator behind JFSS `calculate` and `conditional` rules
//! (FR-RAD-006, decision **D-10**).
//!
//! **Why this exists on the server at all.** JFSS S8.1 makes the backend
//! re-evaluate every `calculate` expression and overwrite the submitted value
//! before persistence, and S10.2 does the same for `conditional`. The client's
//! arithmetic is a convenience for whoever is typing; the stored figure is this
//! module's answer. That is the Tamper-Proof Pattern, and it only works if the
//! two sides compute the same thing.
//!
//! **Which is why the engine is pinned, not merely chosen.** `datalogic-rs` and
//! the frontend's `@goplasmatic/datalogic-wasm` are one engine compiled for two
//! runtimes. The [operator-parity spike](../../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
//! measured them at 51/51 agreement over the corpus, error cases included —
//! parity by construction rather than by a suite chasing it. Both sides carry
//! `=5.2.0`, and [`parity/`](../../../../parity/README.md) fails the build if
//! they ever stop agreeing.
//!
//! **An unknown operator is an error, never a passthrough.** This is the whole
//! of S8.1.1 and it is not a detail: the crate the registry used to name
//! returns an unrecognised operator's expression *unevaluated and wrapped in
//! `Ok`*, which the S7.3 wrapper then launders into a plausible `0`. A
//! mistyped `sum` therefore turned a 42-rupiah invoice line into 0 with nothing
//! logged and nothing returned. [`tests`] reproduces that pattern and asserts
//! this evaluator refuses it.
//!
//! What is **not** here is the engine around the evaluator — the rule
//! catalogue, the dependency graph, cycle detection and error mapping. Those
//! are Sprints 14–16 under decision **D-2**; this is the part Sprint 8's
//! renderer needs and nothing more.

use std::fmt;

use bumpalo::Bump;
use datalogic_rs::{
    operator::EvalContext, CustomOperator, DataValue, DivisionByZeroHandling, Engine,
    EvaluationConfig, NanHandling, NumericCoercionConfig,
};
use serde_json::Value;

/// The engine version both sides carry. Stated so a test can assert the two
/// halves of decision **D-10** were bumped together rather than one of them.
pub const ENGINE_VERSION: &str = "5.2.0";

/// Why an expression did not produce a value.
///
/// One variant, deliberately: every caller of this module answers the same way
/// — refuse the submission and say which field — and a taxonomy of engine
/// failures would invite handling some of them. Mapping engine errors onto
/// JFSS error codes belongs to the rule engine in Sprints 14–16.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationError {
    message: String,
}

impl EvaluationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The engine's own words. Diagnostic only — it names operators and
    /// argument shapes, so it belongs in a log and not in a response body.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for EvaluationError {}

/// `sum` as Calculation Rule Registry §3.2 requires it.
///
/// Not a JSON Logic operator anywhere — the registry says so, and the spike
/// confirmed no engine ships one — so every environment registers its own, and
/// the two implementations have to agree on the same three edge cases: a
/// non-array argument, an empty array, and non-numeric members. Empty sums to
/// `0`; anything that is not a number contributes nothing rather than poisoning
/// the total with `NaN`.
struct Sum;

impl CustomOperator for Sum {
    fn evaluate<'a>(
        &self,
        args: &[&'a DataValue<'a>],
        _context: &mut EvalContext<'_, 'a>,
        arena: &'a Bump,
    ) -> datalogic_rs::Result<&'a DataValue<'a>> {
        let total = args
            .first()
            .and_then(|value| value.as_array())
            .map(|items| items.iter().filter_map(DataValue::as_f64).sum::<f64>())
            .unwrap_or(0.0);

        Ok(arena.alloc(DataValue::from_f64(total)))
    }
}

/// The registry's mandated normalizations, expressed as engine configuration.
///
/// As configuration rather than as a wrapper each environment hand-writes,
/// because a hand-written wrapper is how two environments end up subtly
/// different — which is the failure the §7.3 wrapper itself has: it normalizes
/// `NaN`, which is falsy, and not `Infinity`, which is truthy, so division by
/// zero was never normalized on the client at all (spike §2.3).
///
/// The frontend builds the same configuration from the same values; they are
/// listed in [`parity/README.md`](../../../../parity/README.md) so a change to
/// one is visibly a change to both.
fn jfss_config() -> EvaluationConfig {
    let mut config = EvaluationConfig::default();

    // §7.3: a null or missing operand yields 0, not NaN.
    config.arithmetic_nan_handling = NanHandling::CoerceToZero;
    // §3.1: division by zero is not a value. `ReturnNull` is the closest the
    // engine offers and [`normalize_numeric`] turns that null into the 0 the
    // registry asks for — **for the operands it reaches**. It does not reach
    // all of them: see [`RuleEvaluator::evaluate_numeric`].
    config.division_by_zero = DivisionByZeroHandling::ReturnNull;
    // The reference implementation compares across types silently rather than
    // throwing, and a form that refuses to render because a blank field was
    // compared to a number is worse than one that treats it as absent.
    config.loose_equality_errors = false;

    let mut coercion = NumericCoercionConfig::default();
    coercion.null_to_zero = true;
    coercion.empty_string_to_zero = true;
    coercion.bool_to_number = true;
    coercion.reject_non_numeric = false;
    config.numeric_coercion = coercion;

    config
}

/// Evaluates JFSS rule expressions.
///
/// Build it once and share it: the engine holds the operator table and the
/// configuration, both of which are read-only after construction, and building
/// one per request would rebuild that table per request.
pub struct RuleEvaluator {
    engine: Engine,
}

impl RuleEvaluator {
    /// The evaluator configured as the registries require.
    pub fn new() -> Self {
        Self {
            engine: Engine::builder()
                .with_config(jfss_config())
                .add_operator("sum", Sum)
                .build(),
        }
    }

    /// Evaluates `expression` against `data`, returning the raw JSON result.
    ///
    /// Raw: no normalization is applied here, because `conditional` rules want
    /// the boolean the expression produced and `calculate` rules want it put
    /// through [`normalize_numeric`]. Deciding that here would make one of the
    /// two tiers wrong.
    pub fn evaluate(&self, expression: &Value, data: &Value) -> Result<Value, EvaluationError> {
        let evaluated = self
            .engine
            .eval_str(&expression.to_string(), &data.to_string())
            .map_err(|error| EvaluationError::new(format!("{error:?}")))?;

        serde_json::from_str(&evaluated).map_err(|error| EvaluationError::new(error.to_string()))
    }

    /// Evaluates a `calculate` expression and normalizes the result.
    ///
    /// The pairing a stored numeric field wants, and the reason it is one
    /// function: every caller that forgets the normalization stores a `null`
    /// in a numeric column.
    ///
    /// **§3.1's "the result is 0" is not fully delivered here, and that is a
    /// known gap rather than an oversight.** Measured on the adopted engine:
    /// `10.5 / 0` returns `null` and normalizes to `0` as the registry asks,
    /// while `10 / 0`, `0 / 0` and `10 % 0` **throw**, so no wrapper is
    /// reached at all and the caller sees an error rather than a zero. The
    /// inconsistency is the engine's, it is identical on the frontend build —
    /// which is what makes it a parity-preserving gap rather than a divergence
    /// — and correcting the normalization spec is where the
    /// [spike](../../../../projects/spikes/01.%20JFSS%20Operator%20Parity.md)
    /// §2.3 put it: with the renderer, not with the evaluator. The tests below
    /// pin the measured behaviour so a version bump that changes it is loud.
    pub fn evaluate_numeric(
        &self,
        expression: &Value,
        data: &Value,
    ) -> Result<f64, EvaluationError> {
        self.evaluate(expression, data).map(|value| {
            let normalized = normalize_numeric(&value);
            debug_assert!(normalized.is_finite(), "normalize_numeric returns finite");
            normalized
        })
    }
}

impl Default for RuleEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculation Rule Registry §7.3, as §3.1 means it rather than as §7.3 writes
/// it.
///
/// §7.3 gives the wrapper as `Number(result) || 0`, and the spike showed that
/// does not implement the §3.1 rule it is cited for: `Infinity` is truthy, so
/// `Infinity || 0` is `Infinity` and division by zero survives the wrapper
/// untouched. A finiteness test is what the rule means — anything that is not a
/// finite number is `0`.
///
/// A `null` reaching here is division by zero, which the configuration turns
/// into `null` because the engine has no return-zero variant. It becomes `0`,
/// which is what §3.1 asks for.
pub fn normalize_numeric(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().filter(|n| n.is_finite()).unwrap_or(0.0),
        // A numeric string is what a form field carries before anything coerces
        // it, and the reference implementation coerces rather than refusing.
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .unwrap_or(0.0),
        Value::Bool(true) => 1.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn evaluator() -> RuleEvaluator {
        RuleEvaluator::new()
    }

    #[test]
    fn evaluates_the_registry_line_total() {
        let result = evaluator()
            .evaluate_numeric(
                &json!({"*": [{"var": "unit_price"}, {"var": "quantity"}]}),
                &json!({"unit_price": 12.5, "quantity": 3}),
            )
            .expect("evaluates");

        assert_eq!(result, 37.5);
    }

    /// Sums are compared as numbers rather than as JSON. The engine serializes
    /// a whole `f64` as `0` and `json!(0.0)` builds `0.0`; `assert_eq!` on the
    /// two `Value`s fails on the encoding while the arithmetic is right.
    fn numeric(evaluator: &RuleEvaluator, expression: Value) -> f64 {
        evaluator
            .evaluate_numeric(&expression, &json!({}))
            .expect("evaluates")
    }

    #[test]
    fn sums_an_array_because_no_engine_ships_sum() {
        assert_eq!(numeric(&evaluator(), json!({"sum": [[1, 2, 3.5]]})), 6.5);
    }

    #[test]
    fn sums_an_empty_array_to_zero() {
        assert_eq!(numeric(&evaluator(), json!({"sum": [[]]})), 0.0);
    }

    #[test]
    fn sums_only_the_numeric_members() {
        // A total of the numbers present, not NaN because one row was blank.
        assert_eq!(
            numeric(&evaluator(), json!({"sum": [[1, null, "x", 2]]})),
            3.0
        );
    }

    #[test]
    fn sums_a_non_array_argument_to_zero() {
        assert_eq!(numeric(&evaluator(), json!({"sum": [42]})), 0.0);
    }

    /// Registry §6.1: the invoice total, which is the pattern the whole
    /// Tamper-Proof argument is built on.
    #[test]
    fn evaluates_the_registry_invoice_total() {
        let expression = json!({
            "sum": [{"map": [
                {"var": "items"},
                {"*": [{"var": "unit_price"}, {"var": "quantity"}]},
            ]}]
        });
        let data = json!({"items": [
            {"unit_price": 10, "quantity": 2},
            {"unit_price": 11, "quantity": 2},
        ]});

        let total = evaluator()
            .evaluate_numeric(&expression, &data)
            .expect("evaluates");

        assert_eq!(total, 42.0);
    }

    /// The defect that disqualified `jsonlogic-rs`, asserted against the engine
    /// that replaced it.
    ///
    /// The registry §6.1 invoice is worth 42. Mistype `sum` as `summ` and an
    /// engine that returns unknown operators unevaluated hands back the
    /// expression object; the §7.3 wrapper then turns that object into `0`, and
    /// a 42-rupiah line is persisted as free with nothing logged. **The
    /// assertion is not that some error occurs — it is that 42 does not become
    /// 0 silently**, which is the only part a caller could get wrong.
    #[test]
    fn refuses_an_unknown_operator_instead_of_quietly_returning_zero() {
        let mistyped = json!({
            "summ": [{"map": [
                {"var": "items"},
                {"*": [{"var": "unit_price"}, {"var": "quantity"}]},
            ]}]
        });
        let data = json!({"items": [
            {"unit_price": 10, "quantity": 2},
            {"unit_price": 11, "quantity": 2},
        ]});

        let outcome = evaluator().evaluate(&mistyped, &data);

        assert!(
            outcome.is_err(),
            "an unknown operator must be an error, not a value: got {outcome:?}"
        );

        // And the shape the defect actually took: a passthrough that the
        // numeric wrapper launders into 0.
        assert_eq!(
            normalize_numeric(&json!({"summ": [1, 2]})),
            0.0,
            "an expression object normalizes to 0 — which is why it must never reach the wrapper"
        );
    }

    #[test]
    fn a_misspelled_variable_is_absent_rather_than_an_error() {
        // Distinct from an unknown operator on purpose: a form half filled in
        // is normal, and refusing to evaluate it would make every partially
        // typed document an error.
        let result = evaluator()
            .evaluate_numeric(
                &json!({"*": [{"var": "unit_price"}, {"var": "quantitee"}]}),
                &json!({"unit_price": 12.5, "quantity": 3}),
            )
            .expect("evaluates");

        assert_eq!(result, 0.0);
    }

    #[test]
    fn a_fractional_division_by_zero_normalizes_to_zero() {
        // §3.1 says the result is 0, and this is the path that delivers it:
        // the engine returns `null` and the wrapper turns it into 0. The §7.3
        // wrapper as written would not have — `Infinity || 0` is `Infinity` —
        // which is why `normalize_numeric` tests finiteness instead.
        assert_eq!(numeric(&evaluator(), json!({"/": [10.5, 0]})), 0.0);
    }

    #[test]
    fn arithmetic_never_yields_a_non_finite_number() {
        // An overflow returns null rather than Infinity, so nothing non-finite
        // can reach a numeric column even before the wrapper.
        assert_eq!(numeric(&evaluator(), json!({"*": [1e308, 10]})), 0.0);
    }

    /// The measured §3.1 gap, pinned so a version bump that changes it is loud.
    ///
    /// These three throw rather than returning the 0 the registry asks for.
    /// The frontend build of the same engine throws on the same three, so
    /// parity holds and the non-conformance is the registry-versus-engine gap
    /// the spike §2.3 assigned to the renderer's normalization spec. This test
    /// asserts what is, not what should be; the day it fails is the day the
    /// gap closed or moved.
    #[test]
    fn integer_division_by_zero_throws_rather_than_normalizing() {
        let evaluator = evaluator();

        for expression in [
            json!({"/": [10, 0]}),
            json!({"/": [0, 0]}),
            json!({"%": [10, 0]}),
        ] {
            assert!(
                evaluator.evaluate(&expression, &json!({})).is_err(),
                "{expression} was expected to throw on the adopted engine"
            );
        }
    }

    /// §6.2's discount cap, and the reason the spike called it a wrong business
    /// answer rather than a rounding difference.
    ///
    /// `min` with an absent operand throws here. The reference implementation
    /// returns 0, which satisfies "the result is 0" and silently turns a
    /// discount cap into no discount at all. Refusing is the better failure of
    /// the two, and it is what both adopted sides do.
    #[test]
    fn a_cap_with_an_absent_operand_is_refused_rather_than_zeroed() {
        let outcome = evaluator().evaluate(
            &json!({"min": [{"var": "computed"}, 100]}),
            &json!({"other": 1}),
        );

        assert!(outcome.is_err(), "got {outcome:?}");
    }

    #[test]
    fn normalizes_every_non_finite_shape_to_zero() {
        assert_eq!(normalize_numeric(&json!(42.5)), 42.5);
        assert_eq!(normalize_numeric(&json!("42.5")), 42.5);
        assert_eq!(normalize_numeric(&json!("  7 ")), 7.0);
        assert_eq!(normalize_numeric(&json!(null)), 0.0);
        assert_eq!(normalize_numeric(&json!("not a number")), 0.0);
        assert_eq!(normalize_numeric(&json!(true)), 1.0);
        assert_eq!(normalize_numeric(&json!(false)), 0.0);
        assert_eq!(normalize_numeric(&json!([1, 2])), 0.0);
        assert_eq!(normalize_numeric(&json!({"a": 1})), 0.0);
    }

    #[test]
    fn evaluates_a_conditional_expression_as_a_boolean() {
        let result = evaluator()
            .evaluate(
                &json!({">": [{"var": "total"}, 1000]}),
                &json!({"total": 1500}),
            )
            .expect("evaluates");

        assert_eq!(result, json!(true));
    }
}
