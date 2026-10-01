//! A sentence somebody reads carries no source indentation ([#558]).
//!
//! **What went wrong.** A string literal continued across source lines without
//! a trailing `\` keeps the next line's indentation in its value. #558 was the
//! `HANDLER_KIND_MISMATCH` refusal answering three runs of 34 spaces, and the
//! builder's sweep of `kelir-backend/src` found six more literals of the same
//! kind: a 409, a 422 detail, two OpenAPI descriptions and a log line. Each of
//! the seven is now read by a test of its own, **except the log line**, and a
//! new literal of the kind anywhere else was read by nothing: the builder's two
//! class guards cover the refusals `hook::service::check_entry` writes and the
//! descriptions of the served OpenAPI document, and said so.
//!
//! This file is the guard for the rest, written by the independent test
//! campaign on the PR. It does not start the server and needs no database.
//!
//! - [`no_sentence_in_the_source_carries_a_source_lines_indentation`] reads
//!   every ordinary string literal under `src/` and refuses the two shapes the
//!   defect takes: a run of two or more spaces inside a line of the value, and
//!   a line break followed by four or more spaces of indentation.
//! - [`every_code_check_entry_writes_is_drawn_and_reads_as_one_line`] takes the
//!   codes from `check_entry`'s source and requires one definition to draw each
//!   of them. The unit guard `every_registration_refusal_reads_as_one_line`
//!   asserts the list its own definition draws, which a sixth refusal written
//!   into `check_entry` does not change.
//! - [`no_published_description_carries_a_source_lines_indentation`] reads the
//!   OpenAPI document for what `router`'s
//!   `no_published_description_carries_a_run_of_spaces_inside_a_line` allows:
//!   that test passes any indentation at the start of a line, and a
//!   description continued with a typed `\n` puts its indentation exactly
//!   there. It counts what it read under each part of the document, which
//!   that test does not (its walk with arrays skipped stayed green), and it
//!   reads the two `identity` descriptions whole.
//!
//! # What this does not read
//!
//! - **Raw strings** (`r"…"`, `r#"…"#`). 341 of them under `src/` hold a line
//!   break and indentation, and every one is SQL.
//! - **An ordinary literal that opens with an SQL verb**: three under `src/`.
//! - **Words run together.** `"a veto or a\` joined to `change` reads
//!   `achange`, and no rule about blanks sees it. Only a sentence asserted
//!   whole does. Six of the seven literals #558 fixed have such an assertion
//!   now: the two kind-mismatch sentences in `hook::service`'s unit tests and
//!   `workflow_system_tasks`, the 409 in `workflow_engine`, the 422 in
//!   `documents_link`, and the two descriptions here. The log line has none.
//! - **Text composed at run time**: two fragments that each bring a blank to
//!   the join, or a `{}` that renders empty between two blanks.
//! - `kelir-backend/tests`, the frontend and `e2e`. Ten assertion messages
//!   under `tests/` carry the same runs of spaces; a developer reads those when
//!   a test fails, and no user does.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation was made alone on `d94ae68` plus this file, this file run,
//! the named tests observed red, and the mutation reverted. **Seen red,
//! 2026-10-01.** `source` is
//! `no_sentence_in_the_source_carries_a_source_lines_indentation`, `drawn` is
//! `every_code_check_entry_writes_is_drawn_and_reads_as_one_line` and
//! `published` is `no_published_description_carries_a_source_lines_indentation`.
//! "The unit guards" are `hook::service`'s and `router`'s, run as
//! `--lib -- modules::hook router::tests`, 59 tests.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | the before-only kind-mismatch literal back to three runs of 34 spaces | `source`, naming `hook/service.rs`; `drawn`, on the sentence |
//! | the `entityId` literal back to a typed `\n` and thirteen spaces | `source`, naming `document/domain/link.rs` |
//! | the `tracing::error!` line back to a run of 14 spaces (the builder's M15, which no test read) | `source`, naming `workflow/repository/definition.rs` |
//! | the reset email's body given a run of two spaces mid-sentence | `source`, naming `auth/reset.rs` |
//! | a sixth refusal, `HANDLER_CONFIG_INVALID`, written into `check_entry` with a run of spaces in its sentence | `source` on the run; `drawn` on the code it does not draw. **The unit guards stay green, 59 of 59** |
//! | the same sixth refusal with a clean sentence | `drawn` alone |
//! | the `HOOK_NAME_MISMATCH` sentence given a run of spaces | `source`, `drawn` |
//! | the handlers a refusal offers joined with `",  "` | `drawn` |
//! | the kind mismatch citing `LHCS-2` in place of `LHCS-3.2` | `drawn` |
//! | a response description continued with a typed `\n` and twelve spaces | `published`, `source`. **`router`'s guard does not report it** |
//! | a tag description ending in two spaces | `published`. **`router`'s guard does not report it** |
//! | a tag description given a run of two spaces mid-sentence | `published`, `source` |
//! | the roles list description with two words run together (`holding` and the permission) | `published`, on the description read whole. **The unit guards stay green, 59 of 59** |
//! | the one-role description with two words run together | `published`, the same way; the unit guards green again |
//!
//! [#558]: https://github.com/sujanto-gaws/kelir/issues/558

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kelir_backend::modules::hook::service::registration_errors;
use kelir_backend::router::ApiDoc;
use serde_json::{json, Value};
use utoipa::OpenApi;

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("the directory reads") {
        let path = entry.expect("an entry").path();

        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// One ordinary string literal: the line it opens on, and its **value** — the
/// text the program holds, with escapes resolved and `\` continuations joined.
#[derive(Debug, PartialEq, Eq)]
struct Literal {
    line: usize,
    value: String,
}

/// The ordinary (`"…"`) string literals of a Rust source text.
///
/// Comments, raw strings and character literals are stepped over, so that a
/// quote inside any of them does not open a literal: `'"'` is a character, and
/// `// "` is prose.
fn ordinary_literals(source: &str) -> Vec<Literal> {
    let text: Vec<char> = source.replace("\r\n", "\n").chars().collect();
    let at = |index: usize| text.get(index).copied();
    let mut literals = Vec::new();
    let mut index = 0;
    let mut line = 1;

    while let Some(character) = at(index) {
        match character {
            '\n' => {
                line += 1;
                index += 1;
            }
            '/' if at(index + 1) == Some('/') => {
                while at(index).is_some_and(|character| character != '\n') {
                    index += 1;
                }
            }
            '/' if at(index + 1) == Some('*') => {
                let mut depth = 1;
                index += 2;

                while depth > 0 && at(index).is_some() {
                    if at(index) == Some('/') && at(index + 1) == Some('*') {
                        depth += 1;
                        index += 2;
                    } else if at(index) == Some('*') && at(index + 1) == Some('/') {
                        depth -= 1;
                        index += 2;
                    } else {
                        if at(index) == Some('\n') {
                            line += 1;
                        }
                        index += 1;
                    }
                }
            }
            // A character literal, or a lifetime. `'\…'` and `'x'` are the
            // first; anything else is the second and is one character long.
            '\'' => {
                if at(index + 1) == Some('\\') {
                    index += 3;
                    while at(index).is_some_and(|character| character != '\'') {
                        index += 1;
                    }
                    index += 1;
                } else if at(index + 2) == Some('\'') {
                    index += 3;
                } else {
                    index += 1;
                }
            }
            'r' if raw_string_opens(&text, index) => {
                let mut hashes = 0;
                index += 1;

                while at(index) == Some('#') {
                    hashes += 1;
                    index += 1;
                }
                index += 1;

                loop {
                    match at(index) {
                        None => break,
                        Some('"') if (1..=hashes).all(|offset| at(index + offset) == Some('#')) => {
                            index += 1 + hashes;
                            break;
                        }
                        Some('\n') => {
                            line += 1;
                            index += 1;
                        }
                        Some(_) => index += 1,
                    }
                }
            }
            '"' => {
                let opened = line;
                let mut value = String::new();
                index += 1;

                loop {
                    match at(index) {
                        None => break,
                        Some('"') => {
                            index += 1;
                            break;
                        }
                        Some('\\') => match at(index + 1) {
                            // A continuation: the line break and every blank
                            // that follows it are not part of the value.
                            Some('\n') => {
                                index += 1;
                                while at(index).is_some_and(char::is_whitespace) {
                                    if at(index) == Some('\n') {
                                        line += 1;
                                    }
                                    index += 1;
                                }
                            }
                            Some('n') => {
                                value.push('\n');
                                index += 2;
                            }
                            Some('t') => {
                                value.push('\t');
                                index += 2;
                            }
                            Some('r') => {
                                value.push('\r');
                                index += 2;
                            }
                            // `\u{…}` and `\x..` are not blanks in any literal
                            // this reads for; a placeholder keeps the length.
                            Some('u') => {
                                while at(index).is_some_and(|character| character != '}') {
                                    index += 1;
                                }
                                value.push('?');
                                index += 1;
                            }
                            Some('x') => {
                                value.push('?');
                                index += 4;
                            }
                            Some(escaped) => {
                                value.push(escaped);
                                index += 2;
                            }
                            None => break,
                        },
                        Some(other) => {
                            if other == '\n' {
                                line += 1;
                            }
                            value.push(other);
                            index += 1;
                        }
                    }
                }

                literals.push(Literal {
                    line: opened,
                    value,
                });
            }
            _ => index += 1,
        }
    }

    literals
}

/// Whether the `r` at `index` opens a raw string: `r"`, `r#"`, and the same
/// behind a `b`. An `r` that ends an identifier (`for"`, were that written)
/// does not.
fn raw_string_opens(text: &[char], index: usize) -> bool {
    let before = index.checked_sub(1).and_then(|previous| text.get(previous));

    if before.is_some_and(|character| {
        (character.is_alphanumeric() || *character == '_') && *character != 'b'
    }) {
        return false;
    }

    let mut next = index + 1;

    while text.get(next) == Some(&'#') {
        next += 1;
    }

    text.get(next) == Some(&'"')
}

/// What is wrong with a literal's value, if it is one of the two shapes.
fn indentation_in(value: &str) -> Option<&'static str> {
    if value.lines().any(|line| line.trim().contains("  ")) {
        return Some("a run of two or more spaces inside a line");
    }

    let indented_after_a_break = value.split('\n').skip(1).any(|line| {
        let indent = line.len() - line.trim_start_matches(' ').len();

        indent >= 4 && !line.trim().is_empty()
    });

    indented_after_a_break.then_some("a line break followed by four or more spaces")
}

/// An ordinary literal that is a statement for the database, where a line
/// break and indentation are how it is laid out.
fn is_sql(value: &str) -> bool {
    ["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "WITH "]
        .iter()
        .any(|verb| value.trim_start().starts_with(verb))
}

/// **No sentence under `src/` carries its source line's indentation.**
///
/// The general guard #558's sweep did not leave behind: its two class guards
/// read the refusals of one function and the descriptions of one document, and
/// an `AppError` message, a validation detail, an email body or a log line
/// written anywhere else tomorrow was read by nothing.
#[test]
fn no_sentence_in_the_source_carries_a_source_lines_indentation() {
    let root = source_root();
    let mut files = Vec::new();

    rust_files(&root, &mut files);
    files.sort();

    let mut read = 0;
    let mut statements = 0;
    let mut found = Vec::new();

    for file in &files {
        let source = fs::read_to_string(file).expect("the source reads");

        for literal in ordinary_literals(&source) {
            read += 1;

            if is_sql(&literal.value) {
                statements += 1;

                continue;
            }

            if let Some(what) = indentation_in(&literal.value) {
                found.push(format!(
                    "{}:{}: {what}: {:?}",
                    file.strip_prefix(&root).expect("under the root").display(),
                    literal.line,
                    literal.value
                ));
            }
        }
    }

    assert!(
        found.is_empty(),
        "{} string literal(s) carry indentation in their value; continue the literal with a \
         trailing `\\`, which drops the line break and the blanks after it:\n{}",
        found.len(),
        found.join("\n")
    );
    // Not vacuous: the walk read the source, and the exemption stayed narrow.
    assert!(files.len() > 100, "{} files read", files.len());
    assert!(read > 9_000, "{read} literals read");
    assert!(
        statements < 50,
        "{statements} ordinary literals were passed over as SQL"
    );
}

/// **The reader itself**, on the shapes it has to tell apart. A guard that
/// reads source is only as good as what it takes for a literal.
#[test]
fn the_reader_takes_a_literal_for_what_the_program_holds() {
    let values = |source: &str| {
        ordinary_literals(source)
            .into_iter()
            .map(|literal| literal.value)
            .collect::<Vec<_>>()
    };

    // A continuation joins, and drops the next line's indentation.
    assert_eq!(
        values("x(\"a veto or a \\\n      change\")"),
        ["a veto or a change"]
    );
    // Without one, the line break and the indentation are in the value.
    assert_eq!(values("x(\"the two\n      are\")"), ["the two\n      are"]);
    // A typed `\n` before a continuation is a line break and nothing after it.
    assert_eq!(values("x(\"one\\n\\\n      two\")"), ["one\ntwo"]);
    // An escaped quote does not close the literal.
    assert_eq!(values(r#"x("say \"a  b\" twice")"#), ["say \"a  b\" twice"]);
    // Nothing in a comment, a raw string or a character opens one.
    assert_eq!(
        values("// \"a  b\"\n/* \"c  d\" /* nested */ */ let q = '\"'; let l: &'a str = \"kept\";"),
        ["kept"]
    );
    assert_eq!(
        values("let sql = r#\"SELECT 1\n      FROM \"t\"\"#; let b = br\"x  y\"; \"after\""),
        ["after"]
    );
    // The line a literal opens on is the line reported.
    assert_eq!(
        ordinary_literals("\n\n  \"third\"")[0],
        Literal {
            line: 3,
            value: "third".to_owned()
        }
    );

    // #558 as it was: one source line, the indentation inside it.
    assert_eq!(
        indentation_in("is a veto or a                                  change to the form"),
        Some("a run of two or more spaces inside a line")
    );
    // `document::domain::link` as it was: `\n` typed where `\` was meant.
    assert_eq!(
        indentation_in("and the two \n             are different records"),
        Some("a line break followed by four or more spaces")
    );
    // What is allowed: paragraphs, a two-space indent (the reset email's
    // link), and blanks at either end of a fragment that is joined to another.
    assert_eq!(
        indentation_in("one paragraph.\n\nAnother:\n\n  {link}\n"),
        None
    );
    assert_eq!(indentation_in(" — and a fragment "), None);
    assert_eq!(indentation_in("  acme  "), None);

    assert!(is_sql(
        "SELECT id FROM departments\n         WHERE tenant_id = $1"
    ));
    assert!(!is_sql("Select a role, or  type one"));
}

/// The codes `check_entry` can write, read from its source: every
/// `SCREAMING_SNAKE` literal between its signature and the function after it.
fn codes_check_entry_writes() -> BTreeSet<String> {
    let source = fs::read_to_string(source_root().join("modules/hook/service.rs"))
        .expect("the source reads")
        .replace("\r\n", "\n");
    let from = source
        .find("\nfn check_entry(")
        .expect("`check_entry` is where the registration refusals are written");
    let body = &source[from + 1..];
    let until = body
        .find("\n}\n")
        .expect("the function closes at the first brace in column one");

    ordinary_literals(&body[..until])
        .into_iter()
        .map(|literal| literal.value)
        .filter(|value| {
            value.contains('_')
                && value
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character == '_')
        })
        .collect()
}

/// **Every refusal `check_entry` can write is drawn here, and each is one
/// line.**
///
/// The codes come from the function's source, so a sixth written into it
/// without an entry below fails on the comparison. The definition draws each
/// code in the position that reaches it, and `HOOK_NAME_MISMATCH` in both
/// positions, because its sentence names the position.
#[test]
fn every_code_check_entry_writes_is_drawn_and_reads_as_one_line() {
    let details = registration_errors(&json!({
        "transitions": [{
            "guards": [
                { "handler": "core:continue_always", "hook": "after_workflow_transition" },
                { "handler": "Core:Not_A_Reference" },
                { "handler": "core:reserve_bugdet" },
                { "handler": "plugin:acme:reserve" }
            ],
            "actions": [
                { "handler": "core:set_form_field" },
                { "handler": "core:continue_always", "hook": "before_workflow_transition" },
                { "handler": "plugin:acme:notify", "hook": "before_workflow_transition" }
            ]
        }]
    }));

    let drawn: BTreeSet<String> = details.iter().map(|detail| detail.code.clone()).collect();

    assert_eq!(
        drawn,
        codes_check_entry_writes(),
        "`check_entry` writes a code this definition does not draw, or the reverse"
    );
    assert_eq!(
        drawn.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "HANDLER_KIND_MISMATCH",
            "HANDLER_NOT_FOUND",
            "HANDLER_PLUGIN_UNKNOWN",
            "HANDLER_REFERENCE_INVALID",
            "HOOK_NAME_MISMATCH"
        ]
    );
    // Eight details: one per guards entry, the wrong kind, the before-hook
    // name in `actions`, and an entry refused twice (its name and its plugin).
    assert_eq!(details.len(), 8, "{details:?}");

    for detail in &details {
        assert_eq!(indentation_in(&detail.message), None, "{detail:?}");
        assert!(!detail.message.contains('\n'), "a line break: {detail:?}");
        assert_eq!(detail.message, detail.message.trim(), "{detail:?}");
        assert!(
            detail.path.starts_with("definition.transitions.0."),
            "{detail:?}"
        );
        // The rule each refusal cites: the kind constraint is LHCS §3.2, the
        // handler reference LHCS §2.
        assert_eq!(
            detail.rule,
            match detail.code.as_str() {
                "HOOK_NAME_MISMATCH" | "HANDLER_KIND_MISMATCH" => "LHCS-3.2",
                _ => "LHCS-2",
            },
            "{detail:?}"
        );
    }

    // The sentence names the position it was written in, whichever that is.
    let named: Vec<&str> = details
        .iter()
        .filter(|detail| detail.code == "HOOK_NAME_MISMATCH")
        .map(|detail| detail.message.as_str())
        .collect();

    assert_eq!(named.len(), 3, "{named:?}");
    assert_eq!(
        named[0],
        "`after_workflow_transition` is not the hook this position registers — a `guards` \
         entry is `before_workflow_transition`, and the name may be omitted entirely"
    );

    // The article before `actions` is the sentence's own ("a `actions`
    // entry"), so the two halves around it are read rather than the whole.
    for sentence in &named[1..] {
        assert!(
            sentence.starts_with(
                "`before_workflow_transition` is not the hook this position registers — a"
            ) && sentence.ends_with(
                "`actions` entry is `after_workflow_transition`, and the name may be omitted \
                 entirely"
            ),
            "{sentence:?}"
        );
    }
}

/// Every `description` and `summary` of the document, with where it is.
fn published_texts(value: &Value, pointer: &str, texts: &mut Vec<(String, String)>) {
    match value {
        Value::Object(members) => {
            for (key, member) in members {
                let pointer = format!("{pointer}/{key}");

                match member.as_str() {
                    Some(text) if key == "description" || key == "summary" => {
                        texts.push((pointer, text.to_owned()));
                    }
                    _ => published_texts(member, &pointer, texts),
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                published_texts(item, &format!("{pointer}/{index}"), texts);
            }
        }
        _ => {}
    }
}

/// What is wrong with a published text, outside its fenced code blocks.
fn indentation_published_in(text: &str) -> Option<&'static str> {
    if text != text.trim() {
        return Some("blanks at its start or its end");
    }

    let mut fenced = false;

    for line in text.split('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;

            continue;
        }

        if fenced {
            continue;
        }

        if line != line.trim_end() {
            return Some("blanks at the end of a line");
        }

        if line.len() - line.trim_start_matches(' ').len() >= 4 {
            return Some("a line indented by four or more spaces");
        }

        if line.trim().contains("  ") {
            return Some("a run of two or more spaces inside a line");
        }

        if line.contains('\t') {
            return Some("a tab");
        }
    }

    None
}

/// **No published description carries a source line's indentation at the start
/// of a line, or blanks at an end.**
///
/// `router`'s guard refuses a run of spaces *inside* a line and allows any
/// indentation at the start of one, for a nested list in a doc comment. That is
/// exactly where a typed `\n` and a continued source line put thirteen, the
/// shape `document::domain::link` had, so a description written that way passed
/// it. Markdown reads four or more as a code block; nothing in the document
/// indents further than two today, and the bound is asserted so that stays
/// known.
///
/// The document is `ApiDoc::openapi()`, which is what `/api/docs/openapi.json`
/// serialises and nothing else.
#[test]
fn no_published_description_carries_a_source_lines_indentation() {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let mut texts = Vec::new();

    published_texts(&document, "", &mut texts);

    let found: Vec<String> = texts
        .iter()
        .filter_map(|(pointer, text)| {
            indentation_published_in(text).map(|what| format!("{pointer}: {what}: {text:?}"))
        })
        .collect();

    assert!(
        found.is_empty(),
        "{} published text(s):\n{}",
        found.len(),
        found.join("\n")
    );

    // Not vacuous: each kind of text the document carries was read.
    for (part, at_least) in [
        ("/tags/", 10),
        ("/info/", 1),
        ("/parameters/", 100),
        ("/responses/", 400),
        ("/properties/", 150),
        ("/summary", 30),
    ] {
        let read = texts
            .iter()
            .filter(|(pointer, _)| pointer.contains(part))
            .count();

        assert!(read >= at_least, "{read} texts under {part}");
    }

    assert_eq!(
        indentation_published_in(
            "a list:\n\n- one\n  - nested\n\n```json\n{\n      \"a\":   1\n}\n```"
        ),
        None
    );
    assert_eq!(
        indentation_published_in("Missing identity:role:read\n            for this tenant"),
        Some("a line indented by four or more spaces")
    );
    assert_eq!(
        indentation_published_in("Sign in, sign out, session refresh  "),
        Some("blanks at its start or its end")
    );
    assert_eq!(
        indentation_published_in("Users,  roles and permissions"),
        Some("a run of two or more spaces inside a line")
    );

    // The two #558 fixed, whole: a `\` continuation written without the blank
    // before it runs two words together, which no rule about blanks sees.
    let responses = |path: &str| &document["paths"][path]["get"]["responses"]["200"]["description"];

    assert_eq!(
        responses("/api/v1/identity/roles"),
        "Roles with their permissions. For a caller holding `workflow:task:reassign`, each also \
         carries `liveHolders` and `openTasks` (#508, D-91 (2)); both are omitted for anybody \
         else. `liveHolders` 0 beside a non-zero `openTasks` is a role whose last holder has left \
         while open tasks still need it: `GET /api/v1/identity/roles/{id}/open-tasks` lists them, \
         and `POST /api/v1/workflow/tasks/{id}/reassign` clears each one."
    );
    assert_eq!(
        responses("/api/v1/identity/roles/{id}"),
        "The role. `liveHolders` and `openTasks` as on the list: present only for a caller \
         holding `workflow:task:reassign` (#508)."
    );
}
