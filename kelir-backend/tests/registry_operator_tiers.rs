//! The operator tiers in code are the operator tiers in the registry
//! ([#393](https://github.com/sujanto-gaws/kelir/issues/393), decision **D-15**).
//!
//! [Calculation Rule Registry](../../docs/schema/JFSS%20Calculation%20Rule%20Registry.md)
//! §2.1–§2.2 govern `calculate` and §2.5 governs `conditional.logic`.
//! `rad::domain::jfss` carries both as `&[&str]` constants, because the check
//! runs on the save path of every form and reading a document from disk there
//! would make a deployment's correctness depend on a file somebody copied —
//! the reasoning `rad_jfss_meta_schema.rs` gives for the vendored meta-schema,
//! one document over.
//!
//! **A duplicate that nothing checks is a duplicate that drifts**, and this one
//! would drift silently in the direction that matters least visibly: the
//! registry would describe a tier the engine did not enforce, and every author
//! reading the document would be reading a promise nothing kept.
//!
//! # Why the constants are read out of the source rather than imported
//!
//! They are `pub(crate)`. Widening them to `pub` so a test in `tests/` could
//! see them would change the crate's surface to suit its tests, which is a
//! worse trade than parsing two files — and parsing is what the two other
//! document-checking tests here already do.
//!
//! # What this deliberately does not check
//!
//! **That the operators work.** `jsonlogic_parity.rs` is where an operator's
//! behaviour is asserted across the two runtimes. This asserts that the list
//! the engine refuses against and the list the registry publishes are the same
//! list.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Three mutations, run 2026-09-09, all three red — recorded in the pull
//! request that adds this file.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .to_path_buf()
}

/// Every operator named in a `&[&str]` constant in `rad::domain::jfss`.
fn constant(name: &str) -> BTreeSet<String> {
    let source =
        fs::read_to_string(repository_root().join("kelir-backend/src/modules/rad/domain/jfss.rs"))
            .expect("the jfss module is readable");

    let opening = format!("const {name}: &[&str] = &[");
    let body = source
        .split_once(&opening)
        .unwrap_or_else(|| panic!("{name} is declared in rad::domain::jfss"))
        .1
        .split_once("];")
        .expect("the constant is closed")
        .0;

    quoted(body)
}

/// The `"…"` tokens in a fragment of Rust source.
fn quoted(fragment: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = fragment;

    while let Some((_, after)) = rest.split_once('"') {
        let Some((token, remainder)) = after.split_once('"') else {
            break;
        };

        found.insert(token.to_owned());
        rest = remainder;
    }

    found
}

/// The operators §2.5's table lists, read from its `Operators` column.
///
/// **The column rather than the section**, because the prose around the table
/// names `log` and `generateInvoiceId` in backticks precisely to say they are
/// *not* in the tier — reading the whole section would collect the exclusions
/// as members and the test would assert the opposite of the document.
fn tier_2_5() -> BTreeSet<String> {
    let registry =
        fs::read_to_string(repository_root().join("docs/schema/JFSS Calculation Rule Registry.md"))
            .expect("the calculation registry is readable");

    let section = registry
        .split_once("### 2.5 Conditional Operators")
        .expect("§2.5 exists")
        .1;

    let mut found = BTreeSet::new();

    for line in section.lines() {
        let line = line.trim();

        // The table ends at the first line that is not a row.
        if !line.starts_with('|') {
            if !found.is_empty() {
                break;
            }
            continue;
        }

        let cells: Vec<&str> = line.trim_matches('|').split('|').collect();

        let [_, operators, _] = cells.as_slice() else {
            continue;
        };

        // The header and its separator carry no backticked operator.
        found.extend(backticked(operators));
    }

    found
}

/// The `` `…` `` tokens in one table cell.
fn backticked(cell: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = cell;

    while let Some((_, after)) = rest.split_once('`') {
        let Some((token, remainder)) = after.split_once('`') else {
            break;
        };

        found.insert(token.to_owned());
        rest = remainder;
    }

    found
}

/// The rule, and the whole point: **§2.5 and the engine are one list.**
#[test]
fn the_conditional_tier_in_code_is_the_tier_in_the_registry() {
    let code = constant("CONDITIONAL_OPERATORS");
    let document = tier_2_5();

    let missing_from_code: Vec<_> = document.difference(&code).cloned().collect();
    let missing_from_document: Vec<_> = code.difference(&document).cloned().collect();

    assert!(
        missing_from_code.is_empty() && missing_from_document.is_empty(),
        "the Calculation Rule Registry §2.5 and `rad::domain::jfss::CONDITIONAL_OPERATORS` \
         disagree.\n  approved by the registry and refused by the engine: {missing_from_code:?}\
         \n  accepted by the engine and unlisted by the registry: {missing_from_document:?}\n\n\
         §2.5 is the authority (registry 1.7.0, D-15). Change the document, then the constant."
    );
}

/// **A conditional may read and compute**, so nothing approved for `calculate`
/// is withheld from it — which §2.5 states and this holds.
///
/// The other direction is the point of the tier and is not asserted here: §2.3
/// forbids in `calculate` exactly what a conditional needs.
#[test]
fn everything_calculate_approves_is_approved_in_a_conditional() {
    let calculate = constant("CALCULATE_OPERATORS");
    let conditional = constant("CONDITIONAL_OPERATORS");

    let withheld: Vec<_> = calculate.difference(&conditional).cloned().collect();

    assert!(
        withheld.is_empty(),
        "these are approved for `calculate` and refused in a `conditional`: {withheld:?} — \
         §2.5 says the tier is §2.1 and §2.2 in full"
    );
}

/// **A walk that finds nothing passes every assertion above it.**
#[test]
fn the_walk_finds_both_tiers_and_they_are_not_empty() {
    let calculate = constant("CALCULATE_OPERATORS");
    let conditional = constant("CONDITIONAL_OPERATORS");
    let document = tier_2_5();

    assert!(
        calculate.len() >= 10,
        "the walk read {} calculate operators out of the source, which is too few — \
         the constant moved or its shape changed",
        calculate.len()
    );
    assert!(
        conditional.len() > calculate.len(),
        "the conditional tier ({}) should be strictly larger than the calculate tier ({}) — \
         it is that one plus what §2.3 forbids there",
        conditional.len(),
        calculate.len()
    );
    assert!(
        document.len() >= 30,
        "the walk read {} operators out of §2.5's table, which is too few — the table moved, \
         or its `Operators` column stopped being the second one",
        document.len()
    );
}

/// **`log` is forbidden in both properties**, and for a reason no property
/// makes acceptable: it is a side effect rather than a wrong return type.
///
/// It is the one row of §2.3 that §2.5 does not adopt, so it is the cheapest
/// thing that would go wrong if the tier were built by copying §2.3 wholesale.
#[test]
fn log_is_in_neither_tier() {
    assert!(!constant("CALCULATE_OPERATORS").contains("log"));
    assert!(!constant("CONDITIONAL_OPERATORS").contains("log"));
    assert!(!tier_2_5().contains("log"));
}
