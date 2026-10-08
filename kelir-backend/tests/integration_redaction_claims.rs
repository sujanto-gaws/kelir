//! The texts say what the code does about a secret, an address and an owner,
//! and no more ([#621], [#664], [#665], [#666]).
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
//! Record 21 found the same claim, and two more, in texts this file did not
//! read:
//!
//! - **#664.** The User Manual, the `EGRESS_REFUSED` explanation and ADR-0043
//!   §4 said loopback, link-local and metadata addresses are *never* reached.
//!   An IPv6 address in one of five translated forms is judged public whatever
//!   IPv4 address it carries (D-103).
//! - **#665.** The test call's published OpenAPI description and schema, and
//!   three integration screens, said every form of the secret is masked. The
//!   description also said the connection is pinned to *the* checked address,
//!   one, where every checked address is.
//! - **#666.** JWSS §5.3 said an `OWNER` task goes to whoever submits the
//!   document, where the code routes it to the creator, and that a task scoped
//!   twice needs somebody in both departments, where a grant with no department
//!   satisfies every scope (ADR-0021). Two comments said the creator and the
//!   submitter coincide on a first submit.
//!
//! This file reads every one of those texts with each struck span removed and
//! whitespace run together, since the screens wrap at 100 columns. It refuses
//! every phrase that made a claim, in every text, compared without case and on
//! word boundaries, and it requires the sentences that replaced them. A phrase
//! inside `~~ ~~` is history; the same phrase outside it is a promise again.
//!
//! The OpenAPI texts are read from `ApiDoc::openapi()`, which is what
//! `/api/docs/openapi.json` serialises, not from the doc comments that feed it.
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
//! `no_document_says_a_log_or_a_response_never_shows_a_secret`, as this
//! file's refusal was then named, naming the User Manual and the phrase.
//! Restored, green.
//!
//! 2026-10-08, two runs, each restored to green:
//!
//! - **An old sentence put back, unstruck, in every text read**: in each of
//!   the 13 files, in `handlers.rs`'s doc comment and in `TestCallResponse`'s.
//!   `no_text_says_more_than_the_code_does` alone reddened, naming all 15
//!   texts and each of the eleven phrases #664 to #666 refuse, among them
//!   `Secrets are masked when` wrapped across two lines of
//!   `IntegrationLogPage.vue`, `coincide on a first submit` wrapped across
//!   two `///` lines of `submit.rs`, and `the checked address` in the
//!   published operation.
//! - **A replacement sentence reworded so it no longer says it** in the User
//!   Manual, `test-call-outcome.ts`, the published operation and JWSS's
//!   `OWNER` row: `every_text_says_what_the_code_does` named the first three,
//!   and `jwss_names_the_creator_as_the_owner` the row.
//!
//! [#621]: https://github.com/sujanto-gaws/kelir/issues/621
//! [#664]: https://github.com/sujanto-gaws/kelir/issues/664
//! [#665]: https://github.com/sujanto-gaws/kelir/issues/665
//! [#666]: https://github.com/sujanto-gaws/kelir/issues/666
//! [ADR-0043]: ../../docs/architectures/adr/0043.%20A%20Test%20Call%20Runs%20in%20the%20Request,%20Resolves%20Only%20env%20Secrets,%20and%20Connects%20Only%20to%20a%20Checked%20Address.md

use std::fs;
use std::path::PathBuf;

use kelir_backend::router::ApiDoc;
use utoipa::OpenApi;

const USER_MANUAL: &str = "docs/operations/03. User Manual.md";
const ADR_0043: &str = "docs/architectures/adr/0043. A Test Call Runs in the Request, Resolves Only env Secrets, and Connects Only to a Checked Address.md";
const JWSS: &str = "docs/schema/JSON Workflow Schema.md";

/// The files read, relative to the repository root, each with the sentences it
/// must carry. A file with none to carry must be over 10,000 bytes, so a
/// moved file is not read as an empty one.
const FILES: [(&str, &[&str]); 13] = [
    (
        USER_MANUAL,
        &[
            "no setting allows a loopback, link-local or cloud metadata address",
            "is judged public and is called, whatever IPv4 address it carries",
        ],
    ),
    ("docs/design/02. Database Schema.md", &[]),
    (
        "docs/architectures/03. Kelir Modules for Interfacing with External Systems.md",
        &[],
    ),
    // The SDD said it too, in §9.3.6.
    ("docs/design/01. System Design Document.md", &[]),
    ("CHANGELOG.md", &[]),
    (
        ADR_0043,
        &["is judged public whatever IPv4 address it carries"],
    ),
    // The `OWNER` row's sentences are held within the row, by its own test.
    (JWSS, &["a grant with no department satisfies every scope"]),
    (
        "kelir-frontend/src/features/integration/test-call-outcome.ts",
        &["No setting allows a loopback, link-local or cloud metadata address."],
    ),
    (
        "kelir-frontend/src/features/integration/IntegrationEndpointTestCallDialog.vue",
        &["only the spellings Kelir lists are masked"],
    ),
    (
        "kelir-frontend/src/features/integration/IntegrationLogPage.vue",
        &["masked only in the spellings Kelir lists"],
    ),
    (
        "kelir-frontend/src/features/integration/IntegrationLogDetailDialog.vue",
        &["masked only in the spellings Kelir lists"],
    ),
    ("kelir-backend/src/modules/document/service/submit.rs", &[]),
    (
        "kelir-backend/src/modules/document/repository/document.rs",
        &[],
    ),
];

const TEST_CALL: &str =
    "/api/v1/integration/external-systems/{id}/endpoints/{endpointId}/test-call";

/// What the published test call operation must carry.
const OPERATION_HOLDS: [&str; 3] = [
    "is judged public whatever IPv4 address it carries",
    "redacted only in the spellings ADR-0043 §R lists",
    "pinned to the checked addresses",
];

/// What the published `TestCallResponse` schema must carry.
const SCHEMA_HOLDS: [&str; 1] = ["in the spellings ADR-0043 §R lists, and no other"];

/// The phrases, compared without case and on word boundaries: `the checked
/// address` does not refuse `the checked addresses`.
const REFUSED: [&str; 17] = [
    // #621
    "never a secret",
    "shows no secret",
    "masked wherever it appears",
    "carries a resolved secret",
    "No row carries a resolved secret",
    "Nothing in Kelir can show the unmasked value",
    // #664
    "never are",
    "cannot be pointed at",
    // #665
    "in every form it was sent in",
    // Not `checked address`: ADR-0043's title and "every checked address" are true.
    "the checked address",
    "Any secret Kelir sent is masked",
    "Secrets are masked when",
    "masked when a call is logged",
    "Secrets were masked when this row was written",
    // #666
    "whoever submits the document",
    "not in both departments",
    "coincide on a first submit",
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

/// What `text` says: its struck spans removed, then every run of whitespace,
/// a line break included, made one space. A Rust comment's `//`, `///` or
/// `//!` at the start of a continued line goes with it, so a phrase wrapped
/// in a comment reads as one.
fn read(text: &str) -> String {
    without_struck_spans(text)
        .split_whitespace()
        .filter(|word| !matches!(*word, "//" | "///" | "//!"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `phrase` occurs in `text` with no letter or digit on either side.
fn says(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Every refused phrase `read` states.
fn refused_in(read: &str) -> Vec<&'static str> {
    let lower = read.to_lowercase();
    REFUSED
        .iter()
        .copied()
        .filter(|phrase| says(&lower, &phrase.to_lowercase()))
        .collect()
}

/// Every refused phrase `text` states outside a struck span.
fn claims(text: &str) -> Vec<&'static str> {
    refused_in(&read(text))
}

/// The published test call operation's summary and description, and the
/// `TestCallResponse` schema's description, each read as a file is.
fn published_texts() -> [(&'static str, String); 2] {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");

    let operation = &document["paths"][TEST_CALL]["post"];
    assert!(
        operation.is_object(),
        "the test call is missing from the document"
    );
    let operation = ["summary", "description"]
        .iter()
        .filter_map(|field| operation[field].as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let schema = document["components"]["schemas"]["TestCallResponse"]["description"]
        .as_str()
        .expect("TestCallResponse has a description");

    [
        ("the test call's OpenAPI operation", read(&operation)),
        ("the TestCallResponse OpenAPI schema", read(schema)),
    ]
}

fn read_file(file: &str) -> String {
    let path = repository_root().join(file);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

#[test]
fn no_text_says_more_than_the_code_does() {
    let mut texts: Vec<(&str, String)> = FILES
        .iter()
        .map(|(file, _)| (*file, read(&read_file(file))))
        .collect();
    texts.extend(published_texts());

    let mut found = Vec::new();
    for (name, text) in texts {
        for phrase in refused_in(&text) {
            found.push(format!("{name}: `{phrase}`"));
        }
    }

    assert!(
        found.is_empty(),
        "a text says again what the code does not do; in a document, strike it with ~~ ~~ \
         beside a sentence that says what it does, and elsewhere replace it:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_text_says_what_the_code_does() {
    let mut missing = Vec::new();

    for (file, holds) in FILES {
        let raw = read_file(file);
        assert!(
            !holds.is_empty() || raw.len() > 10_000,
            "{file} is too short to be the file; the layout moved"
        );

        let text = read(&raw);
        for sentence in holds {
            if !text.contains(sentence) {
                missing.push(format!("{file}: `{sentence}`"));
            }
        }
    }

    let [(operation_name, operation), (schema_name, schema)] = published_texts();
    for (name, text, holds) in [
        (operation_name, operation, &OPERATION_HOLDS[..]),
        (schema_name, schema, &SCHEMA_HOLDS[..]),
    ] {
        for sentence in holds {
            if !text.contains(sentence) {
                missing.push(format!("{name}: `{sentence}`"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "a text no longer says what replaced its claim:\n{}",
        missing.join("\n")
    );
}

#[test]
fn jwss_names_the_creator_as_the_owner() {
    // #666: the row said whoever submits; the code routes to the creator.
    let raw = read_file(JWSS);
    let rows: Vec<String> = raw
        .lines()
        .filter(|line| line.starts_with("| `OWNER` |"))
        .map(read)
        .collect();

    assert_eq!(rows.len(), 1, "JWSS §5.3 has one `OWNER` row");
    for sentence in [
        "the task goes to the creator, also when somebody else submitted the document",
        "a `RESUBMIT` edge `allowedBy: \"OWNER\"` refuses a submitter who is not the creator",
    ] {
        assert!(
            rows[0].contains(sentence),
            "JWSS §5.3's `OWNER` row no longer says `{sentence}`: {}",
            rows[0]
        );
    }
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

    // A screen wraps at 100 columns and indents the next line.
    let wrapped = "newest first. Secrets are\n        masked when a call is logged.";
    assert_eq!(
        claims(wrapped),
        ["Secrets are masked when", "masked when a call is logged"]
    );

    let commented = "/// They\n    /// coincide on a first\n    /// submit and";
    assert_eq!(claims(commented), ["coincide on a first submit"]);

    let plural = "The connection is pinned to the checked addresses.";
    assert!(claims(plural).is_empty());

    let singular = "The connection is pinned to the checked address, and";
    assert_eq!(claims(singular), ["the checked address"]);

    let inside = "whenever are";
    assert!(claims(inside).is_empty());
}

#[test]
fn every_document_still_carries_its_struck_sentence() {
    // The strike is the record (D-88). A document that dropped the old
    // sentence instead of striking it would pass the refusal above. The
    // screens, the comments and the OpenAPI texts are code, and are replaced.
    let root = repository_root();
    for (document, struck) in [
        (
            USER_MANUAL,
            "~~Any secret Kelir sent is masked wherever it appears",
        ),
        (
            USER_MANUAL,
            "Nothing in Kelir can show the unmasked value.~~",
        ),
        (
            USER_MANUAL,
            "Loopback, link-local and cloud metadata addresses never are.~~",
        ),
        (FILES[1].0, "~~**No row carries a resolved secret.**~~"),
        (FILES[1].0, "records what was sent, never a secret.~~"),
        (
            FILES[2].0,
            "`audit_events` row carries a resolved secret.~~",
        ),
        (ADR_0043, "The server ~~cannot be pointed at~~"),
        (JWSS, "~~whoever submits the document~~"),
        (JWSS, "~~who is not in both departments~~"),
    ] {
        let text = fs::read_to_string(root.join(document)).expect("the document");
        assert!(text.contains(struck), "{document} lost `{struck}`");
    }
}
