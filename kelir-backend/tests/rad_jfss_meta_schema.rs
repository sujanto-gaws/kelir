//! The vendored JFSS meta-schema is the canonical one (#156 AC2).
//!
//! `src/modules/rad/jfss-meta-v2.0.1.json` is a copy of
//! `docs/schema/jfss-meta-v2.0.1.json`. The copy exists because the release
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

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .to_path_buf()
}

#[test]
fn the_vendored_meta_schema_matches_the_canonical_one() {
    let canonical_path = repository_root().join("docs/schema/jfss-meta-v2.0.1.json");
    let vendored_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/rad/jfss-meta-v2.0.1.json");

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
        "the vendored meta-schema has drifted from docs/schema/jfss-meta-v2.0.1.json. \
         The canonical file wins: copy it over src/modules/rad/jfss-meta-v2.0.1.json."
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
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules/rad/jfss-meta-v2.0.1.json"),
    )
    .expect("the vendored meta-schema is readable");

    let schema: serde_json::Value =
        serde_json::from_str(&vendored).expect("the vendored meta-schema parses");

    jsonschema::validator_for(&schema).expect("the vendored meta-schema compiles");
}
