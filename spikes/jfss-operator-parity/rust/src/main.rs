//! Evaluates the shared JFSS parity corpus with the two candidate Rust crates.
//!
//!   * `jsonlogic-rs` 0.5.0 — the crate behind github.com/bestowinc/json-logic-rs,
//!     which the Calculation Rule Registry names as "json-logic-rs".
//!   * `datalogic-rs` 5.2.0 — under two configurations: stock, and tuned to the
//!     normalizations the registry mandates (S3.1, S7.3).

use bumpalo::Bump;
use datalogic_rs::{
    operator::EvalContext, CustomOperator, DataValue, Engine, EvaluationConfig, NanHandling,
    NumericCoercionConfig,
};
use serde_json::{json, Value};

#[derive(serde::Deserialize)]
struct Case {
    id: String,
    expr: Value,
    data: Value,
}

/// `sum` as Calculation Rule Registry S3.2 requires it: sums the numbers in the
/// single array argument, empty array yields 0.
struct Sum;

impl CustomOperator for Sum {
    fn evaluate<'a>(
        &self,
        args: &[&'a DataValue<'a>],
        _ctx: &mut EvalContext<'_, 'a>,
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

/// The registry's mandated normalizations expressed as engine configuration
/// rather than as a hand-written wrapper.
fn jfss_config() -> EvaluationConfig {
    let mut config = EvaluationConfig::default();
    // S7.3: a null or missing operand yields 0, not NaN.
    config.arithmetic_nan_handling = NanHandling::CoerceToZero;
    // S3.1: x / 0 is 0. No ReturnZero variant exists, so the wrapper still has
    // to turn the null into 0.
    config.division_by_zero = datalogic_rs::DivisionByZeroHandling::ReturnNull;
    // json-logic-js compares across types silently rather than erroring.
    config.loose_equality_errors = false;

    let mut coercion = NumericCoercionConfig::default();
    coercion.null_to_zero = true;
    coercion.empty_string_to_zero = true;
    coercion.bool_to_number = true;
    coercion.reject_non_numeric = false;
    config.numeric_coercion = coercion;

    config
}

fn outcome(result: Result<Value, String>) -> Value {
    match result {
        Ok(raw) => json!({ "ok": true, "raw": raw }),
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

fn datalogic(engine: &Engine, case: &Case) -> Result<Value, String> {
    engine
        .eval_str(&case.expr.to_string(), &case.data.to_string())
        .map_err(|error| format!("{error:?}"))
        .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
}

/// The spike directory, resolved from the manifest rather than the shell's
/// working directory so the harness runs from anywhere.
fn spike_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rust crate sits inside the spike directory")
        .to_path_buf()
}

fn main() {
    let cases: Vec<Case> =
        // The corpus lives in parity/ since Sprint 7 promoted it into a CI
        // gate (#154); this harness reads it there rather than keeping a copy.
        serde_json::from_str(&std::fs::read_to_string(spike_root().join("../../parity/cases.json")).expect("corpus is readable"))
            .expect("corpus parses");

    let stock = Engine::builder().add_operator("sum", Sum).build();
    let tuned = Engine::builder()
        .with_config(jfss_config())
        .add_operator("sum", Sum)
        .build();

    let results: Vec<Value> = cases
        .iter()
        .map(|case| {
            json!({
                "id": case.id,
                "jsonlogic_rs": outcome(
                    jsonlogic_rs::apply(&case.expr, &case.data)
                        .map_err(|error| error.to_string()),
                ),
                "datalogic_stock": outcome(datalogic(&stock, case)),
                "datalogic_tuned": outcome(datalogic(&tuned, case)),
            })
        })
        .collect();

    std::fs::write(
        spike_root().join("results-rust.json"),
        serde_json::to_string_pretty(&results).expect("serializes"),
    )
    .expect("writes");

    println!("{} cases evaluated", results.len());
}
