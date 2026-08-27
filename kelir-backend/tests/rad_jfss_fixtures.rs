//! The frontend's JFSS fixtures, checked against the validator that decides
//! whether they can be stored at all.
//!
//! **This exists because a fixture the backend refuses costs six minutes to
//! find out about.** The browser flow seeds its definition over the API, so a
//! definition the save path rejects fails on the seeding step of the slowest
//! job in the pipeline — which is exactly how `validation.minItems` was found
//! in `purchase-requisition.json` during Sprint 8.
//!
//! **The frontend already checks what the frontend can check.**
//! `__fixtures__/fixtures.spec.ts` reads the canonical meta-schema and asserts
//! every property name a fixture uses is one the meta-schema declares. What it
//! cannot check is the half of the save path that is not the meta-schema: the
//! approved operator sets in [`domain::jfss`] and the `sum` arity rule
//! ([#201](https://github.com/sujanto-gaws/kelir/issues/201), decision
//! **D-22**), both of which are Rust and neither of which the meta-schema
//! knows about. A fixture that gains a `calculate` — which [#163] is what every
//! interesting one now has — is checked by nothing else until CI runs a
//! browser.
//!
//! It needs no database, like `jsonlogic_parity.rs` beside it. It is an
//! integration test rather than a unit test because it reads files outside the
//! crate, and a unit test that reaches out of `src/` is a unit test lying about
//! what it is.

use std::path::{Path, PathBuf};

use kelir_backend::modules::rad::domain::jfss::validate_definition;
use serde_json::Value;

/// The fixture directory, resolved from the manifest rather than the working
/// directory: `cargo test` runs from the crate root and a developer's shell may
/// not.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .join("kelir-frontend")
        .join("src")
        .join("features")
        .join("rad")
        .join("__fixtures__")
}

/// Every fixture, **found rather than named**.
///
/// The [Sprint 6 retrospective](../../projects/retrospectives/04.%20Sprint%206%20Retrospective.md)'s
/// eighth action: a test asserting a project-wide property discovers its
/// subjects instead of listing them. A fixture added next sprint is checked
/// here without anybody remembering to add it.
fn fixtures() -> Vec<(String, Value)> {
    let dir = fixtures_dir();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", dir.display()));

    let mut found = Vec::new();

    for entry in entries {
        let path = entry.expect("a readable directory entry").path();

        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }

        let name = path
            .file_name()
            .expect("a file has a name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
        let definition = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} parses as JSON: {error}", path.display()));

        found.push((name, definition));
    }

    found
}

#[test]
fn every_frontend_jfss_fixture_is_a_definition_this_backend_would_store() {
    let fixtures = fixtures();

    assert!(
        !fixtures.is_empty(),
        "no fixtures were found in {} — a check over nothing passes",
        fixtures_dir().display()
    );

    let mut refused = Vec::new();

    for (name, definition) in &fixtures {
        for detail in validate_definition(definition) {
            refused.push(format!(
                "  {name}: {} — {} ({})",
                detail.path, detail.message, detail.code
            ));
        }
    }

    assert!(
        refused.is_empty(),
        "{} fixture problem(s) that would refuse the definition at save, and would \
         therefore fail the browser flow on its seeding step:\n{}",
        refused.len(),
        refused.join("\n"),
    );
}

/// The check above is only worth running if this validator refuses anything.
///
/// A `validate_definition` that returned an empty vector for every input would
/// make the test above green on any fixture at all, which is the shape coding
/// standard §2.9 calls a test reporting on nothing.
#[test]
fn the_validator_this_test_relies_on_refuses_a_definition_it_should() {
    let unapproved = serde_json::json!({
        "formId": "not-storable",
        "version": "2.0.1",
        "components": [{
            "id": "t",
            "role": "data",
            "type": "number",
            "key": "t",
            "label": "T",
            // `pow` is an operator the engine supports and the Calculation Rule
            // Registry does not approve, which is the distinction §2.1 draws.
            "calculate": {"pow": [{"var": "a"}, 2]},
            "validation": {"type": "number"}
        }]
    });

    assert!(
        !validate_definition(&unapproved).is_empty(),
        "the validator accepted an unapproved operator, so the fixture check above proves nothing"
    );
}
