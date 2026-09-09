//! A decision answered in place is a decision in the wrong table
//! ([#394](https://github.com/sujanto-gaws/kelir/issues/394), Sprint 16 item 5).
//!
//! [Product Backlog](../../projects/planning/02.%20Product%20Backlog.md) §6.2 is
//! headed **Open** and its closing rule says an answered decision moves to §6.1
//! *with the date and what was applied where*. On 2026-09-09 **three of its four
//! rows were not open**: **D-39** and **D-40** had been answered on 2026-08-28
//! and applied in `workflow::repository::task::holds_role` and
//! `workflow::service::engine::fire`, and had sat for eleven days with their
//! resolutions written into the *question* column; **D-15** was four sprints old.
//!
//! **The rule existed, was known, and was not performed.** That is the shape
//! this project has three working controls for — `adr_records_are_current.rs`
//! for a record that stayed `Draft` past its merge,
//! `sprint_reports_are_verified.rs` for a report claiming neither a verification
//! record nor `author-verified`, `releases_are_independently_verified.rs` for a
//! release nobody read — and §6.2 had none.
//!
//! # Two rules, and the second is why nobody noticed the first
//!
//! 1. **No row in §6.2 carries a dated resolution.** `**(YYYY-MM-DD)` is the
//!    form every resolved row uses, so a row in the Open table wearing one is a
//!    decision that was answered where it stood.
//! 2. **Every row in both tables has its own header's cells.** D-39 and D-40
//!    carried two against a four-column header and D-15 three, which is *why*
//!    the answers were invisible: a two-cell row renders as a question with no
//!    columns after it, and a reader skimming a table headed *Open* sees a list
//!    of what is undecided.
//!
//! # The detector has a second subject, and it is the other table
//!
//! Rule 1 passing proves nothing on its own — a detector that matched nothing
//! anywhere would pass it for ever. So the same detector is required to fire in
//! §6.1, where **70 of 75 rows** carry a dated resolution. The five that do not
//! predate the convention; §6's own preamble says *resolved 2026-08-11 unless
//! the row says otherwise*.
//!
//! # What this deliberately does not check
//!
//! **Whether a decision is actually resolved.** A row in §6.2 with no date is
//! open as far as this test can tell; judging that is what reading it is for.
//! What is checkable is the contradiction — a table headed *Open* holding a row
//! that says when it was answered.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Three mutations, run 2026-09-09, all three red. This is a contract test over
//! a documented promise, so two of them land on the document:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **D-73** is answered where it stands — the historical defect, replanted | *no row in the open table carries a dated resolution* |
//! | A resolved row loses a cell | *every decision row has its header's cells* |
//! | `carries_a_dated_resolution` never matches | *the same detector finds the dates in the resolved table* |
//!
//! **The third is the one worth having.** Without that test, a dead detector
//! passes rule 1 silently for ever — which is the failure this whole file
//! exists to catch, one level up.

use std::fs;
use std::path::PathBuf;

/// The heading each table sits under, and the heading that ends it.
const RESOLVED: (&str, &str) = ("### 6.1 Resolved", "### 6.2 Open");
const OPEN: (&str, &str) = ("### 6.2 Open", "### 6.3");

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository root")
        .to_path_buf()
}

fn backlog() -> String {
    fs::read_to_string(repository_root().join("projects/planning/02. Product Backlog.md"))
        .expect("the product backlog is readable")
}

/// One section of §6, between its own heading and the next.
fn section(document: &str, bounds: (&str, &str)) -> String {
    let (from, to) = bounds;

    let after = document
        .split_once(from)
        .unwrap_or_else(|| panic!("{from} is a heading in the product backlog"))
        .1;

    after
        .split_once(to)
        .unwrap_or_else(|| panic!("{to} follows {from}"))
        .0
        .to_owned()
}

/// The decision rows in a section: every line that opens `| **D-`.
fn decision_rows(section: &str) -> Vec<String> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("| **D-"))
        .map(str::to_owned)
        .collect()
}

/// The `D-n` a row is about, for a failure message that names it.
fn identifier(row: &str) -> String {
    row.split_once("**D-")
        .and_then(|(_, rest)| rest.split_once("**"))
        .map_or_else(|| "D-?".to_owned(), |(number, _)| format!("D-{number}"))
}

/// A table row's cells, respecting the escaped pipes several rows carry inside
/// code spans (`grep -rln "rad/lists\|ListDefinition"`).
///
/// **An unescaped `|` inside a cell would split it**, which is a real defect in
/// the row rather than a limitation here: GitHub renders it the same broken way.
fn cells(row: &str) -> usize {
    let inner = row.trim().trim_matches('|');
    let mut count = 1;
    let mut previous = '\0';

    for character in inner.chars() {
        if character == '|' && previous != '\\' {
            count += 1;
        }
        previous = character;
    }

    count
}

/// The first `|` line of a section is its header, and its width is the width
/// every row under it must have.
fn header_cells(section: &str) -> usize {
    let header = section
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('|'))
        .expect("the section has a table");

    cells(header)
}

/// `**(YYYY-MM-DD)` — the form a resolved row's answer opens with.
fn carries_a_dated_resolution(row: &str) -> bool {
    row.match_indices("**(").any(|(at, _)| {
        let rest = &row[at + "**(".len()..];
        let date: Vec<char> = rest.chars().take(11).collect();

        date.len() == 11
            && date[..4].iter().all(char::is_ascii_digit)
            && date[4] == '-'
            && date[5..7].iter().all(char::is_ascii_digit)
            && date[7] == '-'
            && date[8..10].iter().all(char::is_ascii_digit)
            && date[10] == ')'
    })
}

/// Rule 1, and the whole point.
#[test]
fn no_row_in_the_open_table_carries_a_dated_resolution() {
    let document = backlog();
    let answered: Vec<String> = decision_rows(&section(&document, OPEN))
        .iter()
        .filter(|row| carries_a_dated_resolution(row))
        .map(|row| identifier(row))
        .collect();

    assert!(
        answered.is_empty(),
        "these rows sit in §6.2 *Open* and carry a dated resolution, so they were answered \
         where they stood: {}\n\n\
         §6's own closing rule is that an answered decision moves to §6.1 with the date and \
         what was applied where. Move the row, keeping its question in the Decision column \
         and its answer in the Resolution column — D-39 and D-40 are the worked example, and \
         they sat here for eleven days because nothing checked.",
        answered.join(", ")
    );
}

/// **The detector must fire somewhere**, or rule 1 is a test that passes because
/// it matches nothing.
///
/// §6.1 is the second subject: the same function, the same document, the
/// opposite expectation.
#[test]
fn the_same_detector_finds_the_dates_in_the_resolved_table() {
    let document = backlog();
    let rows = decision_rows(&section(&document, RESOLVED));
    let dated = rows
        .iter()
        .filter(|row| carries_a_dated_resolution(row))
        .count();

    assert!(
        rows.len() >= 60,
        "the walk found {} rows in §6.1, which is too few — the table moved",
        rows.len()
    );

    assert!(
        dated * 2 > rows.len(),
        "only {} of §6.1's {} rows carry a dated resolution, so the detector rule 1 relies on \
         is not finding what it is meant to find",
        dated,
        rows.len()
    );
}

/// Rule 2. **A malformed row is why an answered one is invisible.**
#[test]
fn every_decision_row_has_its_headers_cells() {
    let document = backlog();
    let mut wrong = Vec::new();

    for (name, bounds) in [("§6.1", RESOLVED), ("§6.2", OPEN)] {
        let section = section(&document, bounds);
        let expected = header_cells(&section);

        for row in decision_rows(&section) {
            let found = cells(&row);

            if found != expected {
                wrong.push(format!(
                    "{name} {} has {found} cells against a {expected}-column header",
                    identifier(&row)
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "these decision rows do not match their table's header:\n  {}\n\n\
         A row with too few cells renders as a question with its answer missing, which is how \
         D-39 and D-40 sat in the Open table with their resolutions already written. \
         A `|` inside a cell must be escaped as `\\|`.",
        wrong.join("\n  ")
    );
}

/// **A walk that finds nothing passes every assertion above it.**
#[test]
fn the_walk_finds_both_tables_and_governs_something() {
    let document = backlog();

    let resolved = decision_rows(&section(&document, RESOLVED));
    let open = decision_rows(&section(&document, OPEN));

    assert!(
        resolved.len() >= 60,
        "the walk found {} rows in §6.1, which is too few",
        resolved.len()
    );

    assert!(
        !open.is_empty(),
        "the walk found no rows in §6.2. An empty Open table would pass rule 1 for ever — \
         if every decision really is resolved, this assertion is the one to change, \
         deliberately."
    );

    assert_eq!(
        header_cells(&section(&document, RESOLVED)),
        3,
        "§6.1's header is `| ID | Decision | Resolution |`"
    );
    assert_eq!(
        header_cells(&section(&document, OPEN)),
        4,
        "§6.2's header is `| # | Question | Raised | Needed by |`"
    );
}
