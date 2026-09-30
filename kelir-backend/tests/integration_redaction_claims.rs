//! The documents say an integration log redacts the listed spellings of a
//! secret, and not that it never shows one ([#621]).
//!
//! # Why a test
//!
//! [ADR-0043] §R redacts a finite, named list of the spellings a called system
//! may echo a secret in, and says an encoding outside the list is not caught.
//! Record 20's probe P3 stored and returned two such echoes readable: a double
//! percent-encoding and a UTF-16LE body. **Four documents said otherwise**,
//! that a log or a response *never* shows a secret, and `0050` granted
//! `integration:log:read` to every new tenant on the strength of one of them.
//! By decision D-88's rule a shipped *never* is struck, not narrowed, so the
//! old sentences stay in the documents inside `~~ ~~` beside their
//! replacements.
//!
//! This file reads those documents with every struck span removed and refuses
//! the six phrases that made the claim. A phrase inside `~~ ~~` is history; the
//! same phrase outside it is a promise again.
//!
//! # What it does not read
//!
//! **`0050_integration_log_read_permission.sql`.** A migration is not edited,
//! so its comment still says what it said. Its correction is in Database
//! Schema §12.10 and the `CHANGELOG`.
//!
//! # Seen to fail (coding standard 2.9)
//!
//! 2026-10-01: User Manual §11.6's *Nothing in Kelir can show the unmasked
//! value.* put back unstruck after its replacement reddened
//! `no_document_says_a_log_or_a_response_never_shows_a_secret`, naming the
//! User Manual and the phrase. Restored, green.
//!
//! [#621]: https://github.com/sujanto-gaws/kelir/issues/621
//! [ADR-0043]: ../../docs/architectures/adr/0043.%20A%20Test%20Call%20Runs%20in%20the%20Request,%20Resolves%20Only%20env%20Secrets,%20and%20Connects%20Only%20to%20a%20Checked%20Address.md

use std::fs;
use std::path::PathBuf;

/// The documents that made the claim, relative to the repository root. The
/// SDD said it too, in §9.3.6, and is read with them.
const DOCUMENTS: [&str; 5] = [
    "docs/operations/03. User Manual.md",
    "docs/design/02. Database Schema.md",
    "docs/architectures/03. Kelir Modules for Interfacing with External Systems.md",
    "docs/design/01. System Design Document.md",
    "CHANGELOG.md",
];

/// The phrases, compared without case.
const REFUSED: [&str; 6] = [
    "never a secret",
    "shows no secret",
    "masked wherever it appears",
    "carries a resolved secret",
    "No row carries a resolved secret",
    "Nothing in Kelir can show the unmasked value",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .to_path_buf()
}

/// `text` with every `~~…~~` span removed. An unpaired `~~` keeps what follows
/// it, so a stray marker cannot hide the rest of a document.
fn without_struck_spans(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find("~~") {
        let after = &rest[open + 2..];
        match after.find("~~") {
            Some(close) => {
                kept.push_str(&rest[..open]);
                kept.push(' ');
                rest = &after[close + 2..];
            }
            None => break,
        }
    }

    kept.push_str(rest);
    kept
}

/// Every refused phrase `text` states outside a struck span.
fn claims(text: &str) -> Vec<&'static str> {
    let lower = without_struck_spans(text).to_lowercase();
    REFUSED
        .iter()
        .copied()
        .filter(|phrase| lower.contains(&phrase.to_lowercase()))
        .collect()
}

#[test]
fn no_document_says_a_log_or_a_response_never_shows_a_secret() {
    let root = repository_root();
    let mut found = Vec::new();

    for document in DOCUMENTS {
        let path = root.join(document);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));
        assert!(
            text.len() > 10_000,
            "{document} is too short to be the document; the layout moved"
        );

        for phrase in claims(&text) {
            found.push(format!("{document}: `{phrase}`"));
        }
    }

    assert!(
        found.is_empty(),
        "a document says again what ADR-0043 §R does not promise; strike it with ~~ ~~ \
         beside a sentence that says the listed spellings are redacted:\n{}",
        found.join("\n")
    );
}

#[test]
fn a_struck_claim_is_history_and_an_unstruck_one_is_refused() {
    let struck = "~~Nothing in Kelir can show the unmasked value.~~ An unlisted spelling is shown.";
    assert!(claims(struck).is_empty());

    let unstruck = "~~An old sentence.~~ Nothing in Kelir can show the unmasked value.";
    assert_eq!(
        claims(unstruck),
        ["Nothing in Kelir can show the unmasked value"]
    );

    let across = "~~one~~ never a secret ~~two~~";
    assert_eq!(claims(across), ["never a secret"]);

    let unpaired = "~~ a stray marker, and then shows no secret";
    assert_eq!(claims(unpaired), ["shows no secret"]);
}

#[test]
fn every_document_still_carries_its_struck_sentence() {
    // The strike is the record (D-88). A document that dropped the old
    // sentence instead of striking it would pass the refusal above.
    let root = repository_root();
    for (document, struck) in [
        (
            DOCUMENTS[0],
            "~~Any secret Kelir sent is masked wherever it appears",
        ),
        (
            DOCUMENTS[0],
            "Nothing in Kelir can show the unmasked value.~~",
        ),
        (DOCUMENTS[1], "~~**No row carries a resolved secret.**~~"),
        (DOCUMENTS[1], "records what was sent, never a secret.~~"),
        (
            DOCUMENTS[2],
            "`audit_events` row carries a resolved secret.~~",
        ),
    ] {
        let text = fs::read_to_string(root.join(document)).expect("the document");
        assert!(text.contains(struck), "{document} lost `{struck}`");
    }
}
