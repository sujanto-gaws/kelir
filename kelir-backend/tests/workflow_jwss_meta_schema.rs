//! The vendored JWSS meta-schema is the canonical one (#174 AC1).
//!
//! `src/modules/workflow/jwss-meta-v1.0.0.json` is a copy of
//! `docs/schema/jwss-meta-v1.0.0.json`. The copy exists because the release
//! image builds from the `kelir-backend` directory alone, so `include_str!`
//! cannot reach `docs/` — and a validator that read the schema off disk at
//! startup would make a deployment's correctness depend on a file somebody
//! remembered to copy.
//!
//! A duplicate that nothing checks is a duplicate that drifts, and this one
//! would drift *silently*: the validator would keep enforcing last year's
//! schema while the documentation described this year's, and every stored
//! definition would conform to the wrong one. So the two are compared here.
//!
//! It needs no database, unlike most of this directory. It is an integration
//! test because it reads a file outside the crate, and a unit test that reaches
//! out of `src/` is a unit test lying about what it is.
//!
//! **The canonical file was extracted in this sprint**, which is what the JWSS's
//! own closing paragraph said would happen "when the publish validator is
//! implemented". Until then the specification's fenced block was the normative
//! artifact; the two are now the same document, and the block says so.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .to_path_buf()
}

#[test]
fn the_vendored_meta_schema_matches_the_canonical_one() {
    let canonical_path = repository_root().join("docs/schema/jwss-meta-v1.0.0.json");
    let vendored_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/workflow/jwss-meta-v1.0.0.json");

    let canonical = std::fs::read_to_string(&canonical_path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", canonical_path.display()));
    let vendored = std::fs::read_to_string(&vendored_path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", vendored_path.display()));

    // Compared as parsed JSON rather than byte for byte. Line endings are
    // governed by `.gitattributes` and a working tree can still hold either,
    // so a byte comparison would fail on a checkout rather than on a change —
    // which is a test that cries wolf until somebody deletes it.
    let canonical: serde_json::Value =
        serde_json::from_str(&canonical).expect("the canonical meta-schema parses");
    let vendored: serde_json::Value =
        serde_json::from_str(&vendored).expect("the vendored meta-schema parses");

    assert_eq!(
        vendored, canonical,
        "the vendored meta-schema has drifted from docs/schema/jwss-meta-v1.0.0.json. \
         The canonical file wins: copy it over src/modules/workflow/jwss-meta-v1.0.0.json."
    );
}

/// The vendored copy is a schema the validator can actually compile.
///
/// Separate from the comparison above because it fails differently: a copy that
/// matches the canonical file and does not compile means the *canonical* file
/// is broken, and the panic inside `OnceLock` would otherwise surface as every
/// form save returning 500 with no explanation.
#[test]
fn the_vendored_meta_schema_compiles() {
    let vendored = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/workflow/jwss-meta-v1.0.0.json"),
    )
    .expect("the vendored meta-schema is readable");

    let schema: serde_json::Value =
        serde_json::from_str(&vendored).expect("the vendored meta-schema parses");

    jsonschema::validator_for(&schema).expect("the vendored meta-schema compiles");
}

/// The specification's fenced block and the extracted file are one document.
///
/// The JWSS says so — *"That file and this block are the same document, and
/// `tests/workflow_jwss_meta_schema.rs` compares them"* — which makes this a
/// **contract test**: it asserts that behaviour matches a documented promise,
/// so the thing to mutate when verifying it is the documented text as well as
/// the code (coding standard §2.9).
///
/// It exists because the specification is what a person reads and the file is
/// what the validator compiles. Nothing else keeps them equal, and a
/// specification that describes a schema the product does not enforce is worse
/// than no specification: a workflow author would write to it and be refused.
///
/// **Seen red** against the fenced block with `"minItems": 2` changed to `1` on
/// `states`: the two documents disagree about the smallest legal workflow.
#[test]
fn the_specification_carries_the_same_meta_schema_as_the_extracted_file() {
    let specification =
        std::fs::read_to_string(repository_root().join("docs/schema/JSON Workflow Schema.md"))
            .expect("the JWSS specification is readable");

    // The last fenced JSON block of the document is the meta-schema; it is the
    // only one under the "JWSS v1.0.0 Meta-Schema" heading, and taking the last
    // rather than searching for a marker means an example added above it does
    // not move the answer.
    let block = specification
        .rsplit("```json")
        .next()
        .and_then(|rest| rest.split("```").next())
        .expect("the specification carries a fenced JSON block");

    let published: serde_json::Value =
        serde_json::from_str(block).expect("the specification's meta-schema parses");

    let extracted =
        std::fs::read_to_string(repository_root().join("docs/schema/jwss-meta-v1.0.0.json"))
            .expect("the extracted meta-schema is readable");
    let extracted: serde_json::Value =
        serde_json::from_str(&extracted).expect("the extracted meta-schema parses");

    assert_eq!(
        extracted, published,
        "docs/schema/jwss-meta-v1.0.0.json has drifted from the fenced block in \
         docs/schema/JSON Workflow Schema.md. They are one document; change both."
    );
}
