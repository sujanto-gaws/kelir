//! The configuration reference lists every variable this binary reads
//! ([#316]).
//!
//! # Why a test and not a review note
//!
//! [Installation and Deployment](../../docs/operations/01.%20Installation%20and%20Deployment.md)
//! §7.1 is the only document that tells a deployer which environment variables
//! exist and what happens if they are left alone. On 2026-09-02 it was missing
//! eleven of them — ten since Sprint 12 and `KELIR_TRUSTED_PROXY_HOPS` since
//! Sprint 2 — and **every one of those defaults is the development stack's**, so
//! a deployment that followed the document to the letter pointed object storage
//! at `localhost:9000` and the scanner at a host named `clamav`.
//!
//! Nothing failed. The gap was found by diffing the two sides by hand while
//! adding a twelfth variable, which is exactly the kind of finding that arrives
//! by luck. This file is that diff, run on every build.
//!
//! # What counts as *read*
//!
//! A `KELIR_*` name appearing **as a string literal** in the crate's own source,
//! outside its tests. That is a deliberately blunt rule and it is the honest one:
//! the loader reads some names through a helper and across line breaks, so
//! matching call shapes would miss them, and a doc comment naming a variable
//! without quotes — `KELIR_BOOTSTRAP_ADMIN_*`, the prefix, is one — is prose
//! rather than a read.
//!
//! **Blunt in the safe direction.** A name in a string that is not a read still
//! has to be documented, which costs a row; a read that is not documented fails
//! the build, which is the point.
//!
//! # What this deliberately does not check
//!
//! **That every documented variable is read.** §7.2 documents variables the
//! compose files and the deployment scripts consume — `KELIR_VERSION`,
//! `KELIR_SITE_ADDRESS`, thirteen of them — which this binary never sees and
//! must not have to. The check runs one way: everything read is documented.
//!
//! # Seen to fail (coding standard 2.9)
//!
//! Three mutations, run 2026-09-02:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | A variable the binary reads loses its row | *every variable the binary reads is in the configuration reference* |
//! | A row added for a variable nothing reads | *the backend table documents nothing the binary stopped reading* |
//! | The source walk narrowed to `config.rs` alone | both, on the count guard — a scan that stops looking is the failure mode a green check would hide |
//!
//! [#316]: https://github.com/sujanto-gaws/kelir/issues/316

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The crate root, resolved at compile time so the test does not depend on
/// where it was run from.
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `KELIR_*` name quoted in non-test source under `src/`, plus
/// `build.rs` — which reads the environment too, and is the one file outside
/// `src/` that does.
fn variables_read() -> BTreeSet<String> {
    let root = crate_root();
    let mut sources = vec![root.join("build.rs")];

    collect_rust_files(&root.join("src"), &mut sources);

    assert!(
        sources.len() > 20,
        "the source walk found {} files, which is too few to be the crate — the layout moved",
        sources.len()
    );

    let mut names = BTreeSet::new();

    for path in sources {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        // **Cut at the tests.** A test that sets `KELIR_MULTI_TENANT=enabled` to
        // watch the parser refuse it is not a variable a deployment sets, and a
        // fixture naming a variable that does not exist would otherwise demand a
        // row in a document a deployer reads.
        let live = match source.find("#[cfg(test)]") {
            Some(cut) => &source[..cut],
            None => &source,
        };

        names.extend(quoted_names(live));
    }

    names
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

/// `"KELIR_SOMETHING"` — the quotes are the whole rule.
fn quoted_names(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = source;

    while let Some(start) = rest.find("\"KELIR_") {
        let after_quote = &rest[start + 1..];
        let name: String = after_quote
            .chars()
            .take_while(|character| character.is_ascii_uppercase() || *character == '_')
            .collect();

        // Only a name the closing quote ends. `"KELIR_BOOTSTRAP_ADMIN_*"` and
        // `"KELIR_LOG is unset"` are strings that mention a variable rather than
        // name one, and neither is a read.
        if after_quote[name.len()..].starts_with('"') {
            names.insert(name.clone());
        }

        rest = &after_quote[name.len()..];
    }

    names
}

/// Every variable with a row in §7.1 or §7.3 — the two tables of variables this
/// binary and its build consume. §7.2 is the deployment's, and is not read here.
fn variables_documented() -> BTreeSet<String> {
    let reference = crate_root()
        .join("../docs/operations/01. Installation and Deployment.md")
        .canonicalize()
        .expect("the configuration reference is where §7 says it is");

    let document = fs::read_to_string(&reference).expect("the configuration reference reads");

    let backend = section(&document, "### 7.1 Backend", "### 7.2 Deployment");
    let build = section(&document, "### 7.3 Build", "\n---");

    let mut names = BTreeSet::new();

    for table in [backend, build] {
        for line in table.lines() {
            let Some(cell) = line.strip_prefix("| `") else {
                continue;
            };

            let Some(name) = cell.split('`').next() else {
                continue;
            };

            if name.starts_with("KELIR_") {
                names.insert(name.to_owned());
            }
        }
    }

    names
}

fn section<'a>(document: &'a str, from: &str, to: &str) -> &'a str {
    let start = document.find(from).unwrap_or_else(|| {
        panic!("the configuration reference has no {from} — was it renumbered?")
    });

    let rest = &document[start..];
    let end = rest
        .find(to)
        .unwrap_or_else(|| panic!("{from} does not end at {to} — was it renumbered?"));

    &rest[..end]
}

/// **Every variable the binary reads has a row a deployer can find.**
///
/// The failure message names the variables rather than the count, because the
/// person who sees this is the person who just added one.
#[test]
fn every_variable_the_binary_reads_is_in_the_configuration_reference() {
    let read = variables_read();
    let documented = variables_documented();

    assert!(
        read.len() >= 25,
        "only {} variables were found in the source, which is fewer than this crate has — \
         the scan is broken rather than the document: {read:?}",
        read.len()
    );

    let undocumented: Vec<&String> = read.difference(&documented).collect();

    assert!(
        undocumented.is_empty(),
        "these variables are read by the binary and have no row in §7.1 or §7.3 of \
         docs/operations/01. Installation and Deployment.md: {undocumented:?}\n\
         Add a row saying what the default is for — not just what its type is. A default \
         that is the development stack's must say so, because that is the one that lets a \
         deployment start, look healthy, and fail later."
    );
}

/// The document is not allowed to grow rows for variables that no longer exist
/// **in §7.1**, which is the half of the reference that mirrors the code.
///
/// This is the converse of the check above and is deliberately narrower: §7.2's
/// variables belong to the compose files and the scripts, and §7.3's to the
/// build, so neither is expected to appear in `src/`. A row here that nothing
/// reads is a variable somebody removed and a deployer will still set.
#[test]
fn the_backend_table_documents_nothing_the_binary_stopped_reading() {
    let reference = crate_root()
        .join("../docs/operations/01. Installation and Deployment.md")
        .canonicalize()
        .expect("the configuration reference is where §7 says it is");

    let document = fs::read_to_string(&reference).expect("the configuration reference reads");
    let backend = section(&document, "### 7.1 Backend", "### 7.2 Deployment");

    let documented: BTreeSet<String> = backend
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|cell| cell.split('`').next())
        .filter(|name| name.starts_with("KELIR_"))
        .map(str::to_owned)
        .collect();

    let read = variables_read();
    let orphaned: Vec<&String> = documented.difference(&read).collect();

    assert!(
        orphaned.is_empty(),
        "§7.1 has rows for variables the binary no longer reads: {orphaned:?}\n\
         Remove the row, or move it to §7.2 if the compose files still use it."
    );
}
