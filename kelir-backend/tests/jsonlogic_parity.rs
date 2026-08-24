//! The backend half of the JFSS parity gate (issue #154, decision **D-10**).
//!
//! `parity/cases.json` is the corpus the operator-parity spike built;
//! `parity/expectations.json` is what the adopted engine answers, run through
//! the configuration and the custom `sum` this repository supplies. The
//! frontend generates that file and `jsonlogic.parity.spec.ts` asserts the
//! frontend reproduces it; this asserts the backend does. **If the two engines
//! ever stop agreeing, one of the two fails and names the case** — which is the
//! gate D-10 was bought with, and the reason both sides pin `5.2.0` exactly.
//!
//! It needs no database, unlike every other file in this directory. It is an
//! integration test rather than a unit test because it reads a file outside the
//! crate, and a unit test that reaches out of `src/` is a unit test lying about
//! what it is.
//!
//! **Error messages are not compared, and could not be**: one side words them
//! in Rust's `Debug` and the other in a JS `Error`. What is compared is whether
//! the expression produced a value at all, and which value — the same
//! comparison the spike made when it reported 51/51 "error cases included".

use std::path::{Path, PathBuf};

use kelir_backend::modules::rad::evaluator::RuleEvaluator;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    expr: Value,
    data: Value,
}

#[derive(Debug, Deserialize)]
struct Expectation {
    id: String,
    ok: bool,
    #[serde(default)]
    value: Value,
}

/// `parity/`, resolved from the manifest rather than the working directory:
/// `cargo test` runs from the crate root and a developer's shell may not.
fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .join("parity")
}

fn read<T: for<'de> Deserialize<'de>>(name: &str) -> Vec<T> {
    let path = parity_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));

    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{} parses: {error}", path.display()))
}

/// Compares two results the way JSON means them rather than the way the two
/// runtimes encode them.
///
/// `serde_json` distinguishes the integer `0` from the float `0.0`; JavaScript
/// has one number type, so the expectation file always writes `0`. Comparing
/// `Value`s directly would therefore report a divergence on every whole-numbered
/// result, which is an encoding difference and not a disagreement about
/// arithmetic. Every other shape compares structurally.
fn agree(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        },
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| agree(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| agree(a, b)))
        }
        _ => left == right,
    }
}

#[test]
fn the_backend_reproduces_every_committed_parity_expectation() {
    let cases: Vec<Case> = read("cases.json");
    let expectations: Vec<Expectation> = read("expectations.json");

    assert!(
        cases.len() > 50,
        "the corpus has {} cases; a gate over nothing passes",
        cases.len()
    );
    assert_eq!(
        cases.len(),
        expectations.len(),
        "every case needs an expectation and no expectation may be an orphan"
    );

    let evaluator = RuleEvaluator::new();
    // Collected rather than asserted case by case, so one run names every case
    // that moved instead of stopping at the first.
    let mut divergences = Vec::new();

    for (case, expectation) in cases.iter().zip(&expectations) {
        assert_eq!(
            case.id, expectation.id,
            "the corpus and the expectations are in different orders"
        );

        match (evaluator.evaluate(&case.expr, &case.data), expectation.ok) {
            (Ok(actual), true) => {
                if !agree(&actual, &expectation.value) {
                    divergences.push(format!(
                        "  {}: expected {}, got {actual}",
                        case.id, expectation.value
                    ));
                }
            }
            (Err(error), false) => {
                let _ = error;
            }
            (Ok(actual), false) => divergences.push(format!(
                "  {}: the frontend engine refused this and the backend returned {actual}",
                case.id
            )),
            (Err(error), true) => divergences.push(format!(
                "  {}: the frontend engine returned {} and the backend refused it — {error}",
                case.id, expectation.value
            )),
        }
    }

    assert!(
        divergences.is_empty(),
        "the two sides of decision D-10 disagree on {} of {} cases. \
         Both must run the same pinned engine version; if one was bumped \
         deliberately, bump the other and regenerate the expectations with \
         `npm run parity:update` in kelir-frontend.\n{}",
        divergences.len(),
        cases.len(),
        divergences.join("\n"),
    );
}
