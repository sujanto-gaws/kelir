//! What is left of the object-type scan once the type holds the property
//! ([#252], [#323], **D-49**, **D-61**).
//!
//! # This file used to be three hundred lines and it is now one assertion
//!
//! `audit::domain::ObjectType::readable_by` maps an object type to the
//! permission its recorded values need (**D-49**). A type with no permission
//! withholds its values from everybody — the safe direction, and deliberate for
//! a row this build did not write — but withholding them from a caller holding
//! every permission the object has is a hole rather than a policy.
//!
//! **The first version of this listed its subjects.** It named nineteen types
//! and said in its own doc comment that the list was
//! `grep -rn 'object_type:' src/` reduced to its constants and literals. The
//! grep was run on 2026-09-01. On 2026-09-02 the attachment tail added
//! `EXTERNAL_REFERENCE`, the list did not grow, and the test stayed green while
//! every external-reference row withheld its values from everybody — the
//! [Sprint 13 independent pass](../../projects/verifications/13.%20Sprint%2013%20Independent%20Pass.md),
//! finding 2.
//!
//! **The second version ran the grep rather than quoting it**, walking the
//! crate for the three shapes an object type was written in. It named its own
//! limit in its module documentation: *a type reaching `AuditEntry.object_type`
//! some other way — built by `format!`, read from a column, returned by a
//! differently named method — is invisible here, and that bluntness runs in the
//! unsafe direction.* **D-61** recorded the close it could not make, and
//! [#323] is that close.
//!
//! **`AuditEntry.object_type` is an `ObjectType` now**, so:
//!
//! * there is no fourth shape, because there is no shape — a value that is not
//!   a variant does not compile;
//! * `readable_by` is exhaustive with no wildcard, so a variant without a
//!   permission does not compile either;
//! * *every object type this crate writes can be placed* is therefore not a
//!   test any more. It is the type.
//!
//! `audit::domain`'s own unit tests carry what remains — the guarantee named,
//! the round trip through the column, and `ALL` covering every variant.
//!
//! # So what is this file still for
//!
//! **One thing the type cannot say: that nothing still writes an object type as
//! a string.** The eleven modules were rewritten by hand and by script; a site
//! that kept a literal would have failed to compile, but a *new* module could
//! reintroduce the shape by declaring its own `&str` constant and its own
//! parallel vocabulary, and the compiler would have no opinion about a constant
//! nothing passes to `AuditEntry`.
//!
//! That is a weaker property than the one this file used to assert and it is
//! the honest remainder: the walk is cheap, it discovers its subjects, and it
//! fails if the shape #323 removed comes back.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `const ATTACHMENT_OBJECT_TYPE: &str = "ATTACHMENT";` put back in `modules::attachment` | *no module keeps its own object-type vocabulary* |
//! | The source walk narrowed to `modules/audit` alone | the same test, on the count guard — a scan that stops looking is the failure a green check would hide |
//!
//! Run 2026-09-06. The mutations that mattered to the old file — an arm removed
//! from `readable_by`, an arm added for a type nothing writes — have no
//! equivalent here: the first does not compile and the second is a variant
//! whose absence of a writer the type does not police. Both are stated in
//! `audit::domain` rather than left implied.
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252
//! [#323]: https://github.com/sujanto-gaws/kelir/issues/323

use std::fs;
use std::path::{Path, PathBuf};

/// The crate root, resolved at compile time so the test does not depend on
/// where it was run from.
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

/// **No module keeps a second vocabulary for what the audit trail calls
/// things** ([#323]).
///
/// `audit::domain::ObjectType` is the vocabulary. A `const …OBJECT_TYPE: &str`
/// beside it is the shape this issue removed from eleven modules: a per-module
/// string that a scan has to find, that nothing maps to a permission, and that
/// the compiler has no opinion about.
///
/// **A named constant rather than any string literal**, because the value is a
/// naming convention this crate follows and a bare `"ATTACHMENT"` somewhere is
/// not evidence of anything. Of the seventy sites #323 rewrote, forty-nine
/// reached an `AuditEntry` through such a constant and twenty-one were literals
/// written straight into one; the second shape cannot come back, because it
/// does not compile.
///
/// [#323]: https://github.com/sujanto-gaws/kelir/issues/323
#[test]
fn no_module_keeps_its_own_object_type_vocabulary() {
    let mut sources = Vec::new();
    collect_rust_files(&crate_root().join("src"), &mut sources);

    // The same guard `configuration_reference` uses, for the same reason: a walk
    // that finds nothing passes every assertion below it.
    assert!(
        sources.len() > 20,
        "the source walk found {} files, which is too few to be the crate — the layout moved",
        sources.len()
    );

    let mut offenders = Vec::new();

    for path in sources {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        for (number, line) in source.lines().enumerate() {
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

            if !name.trim().ends_with("OBJECT_TYPE") || !tail.contains("&str") {
                continue;
            }

            offenders.push(format!("{}:{}", path.display(), number + 1));
        }
    }

    assert!(
        offenders.is_empty(),
        "{offenders:?} declare an object type as a string. The vocabulary is \
         `audit::domain::ObjectType`, whose `readable_by` is exhaustive — a constant beside it is \
         a second name for the same thing, mapped to no permission (#323)"
    );
}
