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
//!
//! # Two corpora, because an expression is not a form
//!
//! `cases.json` holds expressions and is what #154 built. `forms.json` holds
//! whole *submissions* — a definition, a payload a client sent, and the payload
//! the server must store — and is what [#164] added, because the property the
//! Tamper-Proof Pattern actually needs is not "the two engines agree about
//! `sum`" but *"the number the server persists is the number the person filling
//! in the form was looking at"*. Two engines can agree on every operator and
//! still disagree about a whole form: `calculateMode`, the order the
//! calculations settle in, and what happens to a hidden field are all decisions
//! above the evaluator.
//!
//! The frontend's half of the form corpus is
//! `src/features/rad/renderer/formParity.spec.ts` rather than
//! `src/lib/jsonlogic.parity.spec.ts` — that file deliberately contains no Vue,
//! and the client's answer to a whole form is a *rendered form*'s answer.
//!
//! [#164]: https://github.com/sujanto-gaws/kelir/issues/164

use std::path::{Path, PathBuf};

use kelir_backend::modules::rad::evaluator::RuleEvaluator;
use kelir_backend::modules::rad::service::evaluation;
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

// ---------------------------------------------------------------------------
// Whole-form submissions (#164 AC5)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FormCase {
    id: String,
    definition: Value,
    payload: Value,
    /// The payload the server must store, or `null` where the submission is
    /// refused.
    secure: Option<Value>,
    /// For a refused case, the S10.3 paths the refusal must name.
    #[serde(default)]
    refused_paths: Vec<String>,
}

/// **The Tamper-Proof Pattern, held to the browser's answer** (#164 AC5).
///
/// Each case is a definition, the payload a client sent, and what the server
/// must store. `formParity.spec.ts` renders the same definition with the same
/// payload in a real `JfssForm` and asserts the settled values agree with
/// `secure` — so between the two, *the number the server persists is the number
/// the person filling in the form was looking at.*
///
/// Where the two disagree the submission is refused rather than silently
/// corrected, which is what `refusedPaths` holds: **D-24**'s division by zero
/// renders blank in the browser and is refused here, naming the field.
#[test]
fn the_backend_stores_what_the_form_corpus_says_it_must() {
    let cases: Vec<FormCase> = read("forms.json");

    assert!(
        !cases.is_empty(),
        "the form corpus is empty; a gate over nothing passes"
    );

    let mut divergences = Vec::new();

    for case in &cases {
        let outcome = evaluation::secure_payload(&case.definition, &case.payload);

        match (&case.secure, outcome) {
            (Some(expected), Ok(actual)) => {
                if !agree(&actual, expected) {
                    divergences.push(format!("  {}: expected {expected}, got {actual}", case.id));
                }
            }
            (Some(expected), Err(details)) => divergences.push(format!(
                "  {}: expected {expected}, and the submission was refused — {details:?}",
                case.id
            )),
            (None, Ok(actual)) => divergences.push(format!(
                "  {}: the submission was expected to be refused and stored {actual}",
                case.id
            )),
            (None, Err(details)) => {
                let named: Vec<&str> = details.iter().map(|detail| detail.path.as_str()).collect();

                for path in &case.refused_paths {
                    if !named.contains(&path.as_str()) {
                        divergences.push(format!(
                            "  {}: the refusal must name `{path}` and named {named:?}",
                            case.id
                        ));
                    }
                }
            }
        }
    }

    assert!(
        divergences.is_empty(),
        "the server's answer differs from the committed form corpus on {} of {} cases.          A change here is a change to what a submission stores, so the frontend half          (`formParity.spec.ts`) moves with it.
{}",
        divergences.len(),
        cases.len(),
        divergences.join("
"),
    );
}
