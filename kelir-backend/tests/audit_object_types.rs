//! Every object type this crate writes to the audit trail can be placed
//! ([#252], **D-61**).
//!
//! # Why a source walk and not a list
//!
//! `audit::domain::readable_by` maps an object type to the permission its
//! recorded values need (**D-49**). A type with no arm withholds its values
//! from everybody — the safe direction, and deliberate — but it withholds them
//! from a caller holding every permission the object has, which for a `Must`
//! requirement is a hole rather than a policy.
//!
//! **The test this replaces listed its subjects.** It named nineteen types and
//! said in its own doc comment that the list was
//! `grep -rn 'object_type:' src/` reduced to its constants and literals. The
//! grep was run on 2026-09-01. On 2026-09-02 the attachment tail added
//! `EXTERNAL_REFERENCE`, the list did not grow, and the test stayed green while
//! every external-reference row withheld its values from everybody. The
//! [Sprint 13 independent pass](../../projects/verifications/13.%20Sprint%2013%20Independent%20Pass.md)
//! found it, finding 2.
//!
//! That is [sprint plan](../../projects/planning/01.%20Sprint%20Plan.md)
//! verification rule 6, which has been written down since the Sprint 6
//! retrospective: *a test asserting a project-wide property discovers its
//! subjects rather than listing them; an enumerating test fails the way the
//! list it checks fails.* **This file runs the grep rather than quoting it.**
//!
//! # What counts as an object type
//!
//! Three shapes, because three are what the crate uses:
//!
//! 1. `const …OBJECT_TYPE: &str = "VALUE";` — every audit object-type constant
//!    is named this way, in nine modules.
//! 2. `object_type: "VALUE"` — a literal written straight into an `AuditEntry`.
//! 3. A string literal in the body of a `fn object_type(` — the shape
//!    `master_data::domain::record_status` uses to answer for two entities at
//!    once.
//!
//! # What this deliberately does not close
//!
//! **A fourth shape.** A type reaching `AuditEntry.object_type` some other way
//! — built by `format!`, read from a column, returned by a differently named
//! method — is invisible here, and unlike
//! [`configuration_reference`](configuration_reference.rs) that bluntness runs
//! in the *unsafe* direction: the scan misses it and the values are withheld in
//! silence, which is the defect this file exists for.
//!
//! **The close is the type system, and it is not free.** `AuditEntry.object_type`
//! is `&'a str`; an enum with an exhaustive `match` in `readable_by` and no
//! wildcard would make an unplaceable type *not compile*, which is what this
//! project did to `activity::record`'s executor and to `scanner::scan`'s
//! outcome. It is 92 construction sites across nine modules and belongs to a
//! sprint rather than to a finding's fix. Recorded in **D-61** as the option
//! not taken.
//!
//! Until then the count guard below is the honest half: a walk that stops
//! finding things fails, so the scan cannot quietly cover less than it did.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Three mutations, run 2026-09-03:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | The `EXTERNAL_REFERENCE` arm removed from `readable_by` — the defect itself | *every object type this crate writes can be placed* |
//! | An arm added for `SOMETHING_NOBODY_WRITES` | *every arm answers for a type something writes* |
//! | The source walk narrowed to `modules/audit` alone | both, on the count guard — a scan that stops looking is the failure a green check would hide |
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kelir_backend::modules::audit::domain::readable_by;

/// The crate root, resolved at compile time so the test does not depend on
/// where it was run from.
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every object type the crate's own source writes, by the three shapes the
/// module doc names.
fn object_types_written() -> BTreeSet<String> {
    let mut sources = Vec::new();
    collect_rust_files(&crate_root().join("src"), &mut sources);

    // The same guard `configuration_reference` uses, for the same reason: a
    // walk that finds nothing passes every assertion below it.
    assert!(
        sources.len() > 20,
        "the source walk found {} files, which is too few to be the crate — the layout moved",
        sources.len()
    );

    let mut types = BTreeSet::new();

    for path in sources {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        // Cut at the tests: a fixture writing `"SOMETHING_A_PLUGIN_WROTE"` to
        // watch the withholding work is not a type this crate writes.
        let live = match source.find("#[cfg(test)]") {
            Some(cut) => &source[..cut],
            None => &source,
        };

        types.extend(constant_values(live));
        types.extend(field_literals(live));
        types.extend(object_type_fn_literals(live));
    }

    types
}

fn collect_rust_files(directory: &Path, into: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} could not be listed: {error}", directory.display()));

    for entry in entries {
        let path = entry.expect("a directory entry").path();

        if path.is_dir() {
            collect_rust_files(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            into.push(path);
        }
    }
}

/// Shape 1 — `const ATTACHMENT_OBJECT_TYPE: &str = "ATTACHMENT";`
///
/// Keyed on the constant's *name* ending in `OBJECT_TYPE`, which is the naming
/// this crate already follows in all nine modules that declare one.
fn constant_values(source: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();

    for line in source.lines() {
        let line = line.trim();

        let Some(rest) = line.strip_prefix("const ").or_else(|| {
            line.strip_prefix("pub const ")
                .or_else(|| line.strip_prefix("pub(crate) const "))
        }) else {
            continue;
        };

        let Some((name, tail)) = rest.split_once(':') else {
            continue;
        };

        if !name.trim().ends_with("OBJECT_TYPE") {
            continue;
        }

        if let Some(value) = first_string_literal(tail) {
            values.insert(value);
        }
    }

    values
}

/// Shape 2 — `object_type: "USER",`
fn field_literals(source: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut rest = source;

    while let Some(start) = rest.find("object_type:") {
        let tail = &rest[start + "object_type:".len()..];
        let line_end = tail.find('\n').unwrap_or(tail.len());

        if let Some(value) = first_string_literal(&tail[..line_end]) {
            values.insert(value);
        }

        rest = &tail[line_end.min(tail.len())..];
    }

    values
}

/// Shape 3 — every string literal in the body of a `fn object_type(`.
///
/// The body is taken as everything up to the next `\n    }`, which is where a
/// method in an `impl` block ends. Blunt, and over-inclusive rather than under:
/// a literal that is not an object type costs an arm, and a missed one costs a
/// record's contents.
fn object_type_fn_literals(source: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut rest = source;

    while let Some(start) = rest.find("fn object_type(") {
        let body = &rest[start..];
        let end = body.find("\n    }").map(|at| at + 6).unwrap_or(body.len());
        let body = &body[..end];

        let mut scan = body;
        while let Some(value) = first_string_literal(scan) {
            values.insert(value.clone());

            let Some(after) = scan.find(&format!("\"{value}\"")) else {
                break;
            };
            scan = &scan[after + value.len() + 2..];
        }

        rest = &body[end.min(body.len())..];
    }

    values
}

/// The first `"SCREAMING_SNAKE"` literal in a fragment, which is the only shape
/// an object type takes.
fn first_string_literal(fragment: &str) -> Option<String> {
    let start = fragment.find('"')?;
    let after = &fragment[start + 1..];
    let value: String = after
        .chars()
        .take_while(|character| character.is_ascii_uppercase() || *character == '_')
        .collect();

    if value.is_empty() || !after[value.len()..].starts_with('"') {
        return None;
    }

    Some(value)
}

/// Every `"TYPE" =>` arm in `readable_by`, read from its own source.
fn object_types_placed() -> BTreeSet<String> {
    let source = fs::read_to_string(crate_root().join("src/modules/audit/domain.rs"))
        .expect("the audit domain source");

    let start = source
        .find("pub fn readable_by(")
        .expect("readable_by moved; this test reads its arms from source");
    let body = &source[start..];
    let end = body.find("\n}").map(|at| at + 2).unwrap_or(body.len());

    let mut arms = BTreeSet::new();

    for line in body[..end].lines() {
        let line = line.trim();

        // An arm, not a comment mentioning one. `"PARTY_ROLE"` is named in this
        // function's comments precisely because it is *not* an arm.
        if line.starts_with("//") || !line.contains("=>") {
            continue;
        }

        let Some((left, _)) = line.split_once("=>") else {
            continue;
        };

        for part in left.split('|') {
            if let Some(value) = first_string_literal(part) {
                arms.insert(value);
            }
        }
    }

    arms
}

/// **Direction 1 — the one that failed.** A type the crate writes with no arm
/// withholds its values from every caller, including one holding every
/// permission the object has.
#[test]
fn every_object_type_this_crate_writes_can_be_placed() {
    let written = object_types_written();

    assert!(
        written.len() >= 18,
        "the walk found only {} object types, which is fewer than this crate has — \
         the scan stopped looking: {written:?}",
        written.len()
    );

    let unplaceable: Vec<_> = written
        .iter()
        .filter(|object_type| readable_by(object_type).is_none())
        .collect();

    assert!(
        unplaceable.is_empty(),
        "{unplaceable:?} are written by this crate and cannot be placed, so their values are \
         withheld from everybody — give each one the read permission of the object it names \
         (D-49), in `audit::domain::readable_by`"
    );
}

/// **Direction 2 — an arm for a type nothing writes.** Harmless to a caller and
/// corrosive to the map: an unreachable arm is the same ageing checklist as the
/// list this file replaced, and it reads as though somebody checked.
#[test]
fn every_arm_answers_for_a_type_something_writes() {
    let written = object_types_written();
    let placed = object_types_placed();

    assert!(
        placed.len() >= 15,
        "only {} arms were read out of `readable_by` — the reader broke, not the map",
        placed.len()
    );

    let unreachable: Vec<_> = placed.difference(&written).collect();

    assert!(
        unreachable.is_empty(),
        "{unreachable:?} have an arm in `readable_by` and are written by nothing — either the \
         write was removed and the arm outlived it, or the arm is a guess. `PARTY_ROLE` was the \
         first of these and its reasoning is kept as a comment where the arm was"
    );
}
