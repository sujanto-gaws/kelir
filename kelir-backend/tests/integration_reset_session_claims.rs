//! The password-reset texts say what a reset does to a session, and no more
//! ([#697], decision D-107 A).
//!
//! # Why a test
//!
//! A self-service reset (`auth/reset.rs`) and an administrator's set-password
//! (`identity/service.rs`) each revoke every refresh token for the account,
//! and nothing else. The check `middleware::auth` makes on every request reads
//! whether the token's tenant and user are deleted, not a password, so an
//! access token issued before the reset works until it expires, at most
//! `ACCESS_TOKEN_TTL_MINUTES` (15) after issue. `auth_password_reset.rs` and
//! `identity_users.rs` measure that.
//!
//! **Five texts said otherwise**: the reset page, the two OpenAPI 204 texts,
//! the reset email and `CHANGELOG.md`'s `v0.4.0` entry each said a reset signs
//! the account out. Verification record 21 §4 read all five as false. The
//! `CHANGELOG` is a document, so its sentence is struck `~~ ~~` beside its
//! replacement (D-88); the others are code, and are replaced.
//!
//! This file reads each text with its struck spans removed and whitespace run
//! together, as `integration_redaction_claims.rs` does, since the page wraps
//! and the email is a `format!` string continued with `\` across lines. It
//! refuses each old phrase in every text, compared without case and on word
//! boundaries, and it requires each sentence that replaced one.
//!
//! # What it does not read
//!
//! - **The published OpenAPI document.** The two 204 texts are read here from
//!   their handlers' source; `router.rs`'s
//!   `the_two_reset_contracts_do_not_promise_more_than_they_deliver` reads
//!   them from `ApiDoc::openapi()`.
//! - **The delivered email.** `reset.rs` is read as source, with
//!   `{ACCESS_TOKEN_TTL_MINUTES}` uninterpolated; `auth_password_reset.rs`'s
//!   `the_reset_email_says_what_a_reset_does_to_a_session` reads the body a
//!   person receives, with its "15 minutes".
//! - **The tests**, `ResetPasswordPage.spec.ts` among them, which quote the
//!   old sentences in order to refuse them, and this file.
//! - **The planning documents and verification records** that name the five
//!   texts: they are history, and record 21 is not edited.
//! - **The User Manual, the SDD and `api/auth.ts`**, which made no such claim.
//!   The manual and the SDD were widened in this change to name a reset, and
//!   are not pinned; `router.rs` checks the 15 minutes they repeat.
//!
//! # Seen to fail (coding standard 2.9)
//!
//! 2026-10-16, two runs, each restored to green. `CHANGELOG.md` and the page
//! belong to other rows' authors, so their mutated copies were bind-mounted
//! over the two paths inside the build container, and the files on disk were
//! not touched.
//!
//! - **Four old texts put back**: the page's sentence in place of its
//!   replacement, the `CHANGELOG`'s strike removed (`~~redeeming it signs the
//!   account out everywhere~~` made plain, its replacement kept), and the old
//!   email sentence and set-password 204 text in their source.
//!   `no_reset_text_says_a_reset_signs_the_account_out` named all four files,
//!   each with its phrase; `every_reset_text_says_what_a_reset_does` named
//!   the page, `reset.rs` and `identity/handlers.rs`; and
//!   `the_changelog_still_carries_its_struck_sentence` reddened on the lost
//!   strike.
//! - **The reset-password 204 text put back** in `auth/handlers.rs`: the
//!   refusal and the requirement each named that file alone.
//!
//! [#697]: https://github.com/sujanto-gaws/kelir/issues/697

use std::fs;
use std::path::PathBuf;

const CHANGELOG: &str = "CHANGELOG.md";
const PAGE: &str = "kelir-frontend/src/features/auth/ResetPasswordPage.vue";
const EMAIL: &str = "kelir-backend/src/modules/auth/reset.rs";
const RESET_HANDLER: &str = "kelir-backend/src/modules/auth/handlers.rs";
const SET_PASSWORD_HANDLER: &str = "kelir-backend/src/modules/identity/handlers.rs";

/// The files read, relative to the repository root, each with the sentences it
/// must carry.
const FILES: [(&str, &[&str]); 5] = [
    (
        CHANGELOG,
        &[
            "redeeming it revokes every refresh token for the account, so an access token \
           already issued works until it expires, at most 15 minutes",
        ],
    ),
    (
        PAGE,
        &[
            "Your other sessions can no longer be renewed, and any still open ends within 15 \
           minutes. Sign in again with the new password.",
        ],
    ),
    (
        EMAIL,
        &[
            "It stops working in {TOKEN_TTL_MINUTES} minutes. Using it stops your other \
           sessions from being renewed, and any still open ends within \
           {ACCESS_TOKEN_TTL_MINUTES} minutes.",
        ],
    ),
    (
        RESET_HANDLER,
        &[
            "The password is changed; every refresh token for the account is revoked. An \
           access token already issued remains valid until it expires, at most 15 minutes \
           after issue.",
        ],
    ),
    (
        SET_PASSWORD_HANDLER,
        &[
            "Password set; every refresh token for that user is revoked. An access token \
           already issued remains valid until it expires, at most 15 minutes after issue.",
        ],
    ),
];

/// The five phrases, one from each text, compared without case and on word
/// boundaries.
const REFUSED: [&str; 5] = [
    // The reset page.
    "Everywhere you were signed in has been signed out",
    // The administrator's set-password, OpenAPI 204.
    "every session for that user ends",
    // The self-service reset, OpenAPI 204.
    "every other session for that account is signed out",
    // The reset email.
    "signs you out everywhere else",
    // `CHANGELOG.md`, `v0.4.0`.
    "signs the account out everywhere",
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

/// Every run of whitespace, a line break included, made one space. A Rust
/// comment's `//`, `///` or `//!` at the start of a continued line goes with
/// it, and so does a string's `\` continuation, so a phrase wrapped in either
/// reads as one.
fn joined(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| !matches!(*word, "//" | "///" | "//!" | "\\"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What `text` says: its struck spans removed, then its whitespace joined.
fn read(text: &str) -> String {
    joined(&without_struck_spans(text))
}

/// Whether `phrase` occurs in `text` with no letter or digit on either side.
fn says(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Every refused phrase `text` states outside a struck span.
fn claims(text: &str) -> Vec<&'static str> {
    let lower = read(text).to_lowercase();
    REFUSED
        .iter()
        .copied()
        .filter(|phrase| says(&lower, &phrase.to_lowercase()))
        .collect()
}

fn read_file(file: &str) -> String {
    let path = repository_root().join(file);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()))
}

#[test]
fn no_reset_text_says_a_reset_signs_the_account_out() {
    let mut found = Vec::new();
    for (file, _) in FILES {
        for phrase in claims(&read_file(file)) {
            found.push(format!("{file}: `{phrase}`"));
        }
    }

    assert!(
        found.is_empty(),
        "a text says again that a reset signs the account out, where it revokes refresh \
         tokens and an access token already issued works until it expires; in the \
         CHANGELOG, strike it with ~~ ~~ beside a sentence that says what it does, and \
         elsewhere replace it:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_reset_text_says_what_a_reset_does() {
    let mut missing = Vec::new();

    for (file, holds) in FILES {
        let text = read(&read_file(file));
        for sentence in holds {
            if !text.contains(sentence) {
                missing.push(format!("{file}: `{sentence}`"));
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
fn the_changelog_still_carries_its_struck_sentence() {
    // The strike is the record (D-88). A CHANGELOG that dropped the old
    // sentence instead of striking it would pass the refusal above.
    let text = joined(&read_file(CHANGELOG));
    let struck = "~~redeeming it signs the account out everywhere~~";

    assert!(text.contains(struck), "{CHANGELOG} lost `{struck}`");
}

#[test]
fn a_struck_claim_is_history_and_an_unstruck_one_is_refused() {
    let struck = "~~redeeming it signs the account out everywhere~~ redeeming it revokes";
    assert!(claims(struck).is_empty());

    let unstruck = "~~An old sentence.~~ redeeming it signs the account out everywhere and";
    assert_eq!(claims(unstruck), ["signs the account out everywhere"]);

    let unpaired = "~~ a stray marker, and then every session for that user ends";
    assert_eq!(claims(unpaired), ["every session for that user ends"]);

    // The page wraps and indents its next line.
    let wrapped = "<p>\n          Everywhere you were signed in\n          has been signed out.";
    assert_eq!(
        claims(wrapped),
        ["Everywhere you were signed in has been signed out"]
    );

    // A `format!` string continued with `\` across lines.
    let continued = "minutes, and using it \\\n             signs you out everywhere else.\\n";
    assert_eq!(claims(continued), ["signs you out everywhere else"]);

    let cased = "Every Other Session For That Account Is Signed Out";
    assert_eq!(
        claims(cased),
        ["every other session for that account is signed out"]
    );

    let inside = "resigns the account out everywhere";
    assert!(claims(inside).is_empty());
}
