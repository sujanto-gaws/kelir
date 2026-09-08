//! Every sprint report either has a verification record or says plainly that
//! it has not ([#375](https://github.com/sujanto-gaws/kelir/issues/375)).
//!
//! # Why a test and not a sentence
//!
//! [Sprint plan](../../projects/planning/01.%20Sprint%20Plan.md) §2 says *a
//! sprint whose pass did not run closes `author-verified`, and the status
//! report carries the label*. That clause was written on 2026-09-04 (`cdcd704`)
//! to end six sprints of independent passes that fired only when somebody was
//! already worried — **and it failed on the first sprint it governed.** Sprint
//! 14's mid-sprint refinement passed, nothing was dispatched, and there is no
//! record 15 in `projects/verifications/`.
//!
//! [Retrospective 12](../../projects/retrospectives/12.%20Sprint%2014%20Retrospective.md)
//! withdrew permission to write the action a sixth time: *the sixth
//! restatement is not available and this row does not restate it — it converts
//! the label into something that fails.*
//!
//! **This test is deliberately weaker than the rule it guards.** It cannot make
//! a pass happen and does not try. What it makes impossible is the *silent*
//! version: a status report that says nothing at all about whether anybody
//! outside the work read it. Sprint 14's report carries the label because its
//! author chose to, and nothing would have caught one that quietly did not.
//!
//! # The rules
//!
//! For every `projects/status/NN. Sprint <N> Status.md` at or above
//! [`FIRST_GOVERNED_SPRINT`]:
//!
//! 1. **A verification record for sprint `<N>` exists**, or the report contains
//!    `author-verified`. One or the other, never neither.
//! 2. **The walk found what exists, and governs something.** Every source walk
//!    in this suite carries the first half — *a walk that finds nothing passes
//!    every assertion under it* — and this one needs the second as well: a
//!    floor raised past the last report would leave rule 1 asserting over an
//!    empty set, which passes and means nothing.
//! 3. **The pre-rule sprints are named rather than silently skipped.**
//!    [`UNGOVERNED_AND_UNLABELLED`] is the exact set below the floor that
//!    satisfies neither branch, so the exemption is finite, countable and in
//!    the diff when it changes.
//!
//! # The floor, and why there is one
//!
//! **Six reports on today's tree satisfy neither branch** — Sprints 0 to 5 have
//! no verification record and do not carry the label. That is not drift. The
//! label was invented in Sprint 14, and a test demanding it of a report written
//! in Sprint 0 asserts a rule against a document that could not have complied
//! with it.
//!
//! **The two ways out that were not taken:**
//!
//! - *Backfill the label into the six.* That is editing settled history to make
//!   a table green, which is what **D-71** declined for ADR-0033's decision date
//!   on the same reasoning: a record says what was true when it was written.
//! - *Set the floor at 6, where the tree happens to go quiet.* Sprints 6–13
//!   nearly all have verification records, so a floor there would also pass
//!   today. It would pass **by accident** rather than by rule, and a threshold
//!   chosen to fit the data is a threshold that says nothing about the next
//!   report.
//!
//! So the floor is the sprint the rule was written for, and rule 3 keeps the
//! exemption visible rather than letting the floor hide it.
//!
//! # What this deliberately does not check
//!
//! **Whether the pass actually ran.** §2's own criterion is *a session
//! appearing in no construction commit trailer for the sprint's range*, which
//! needs `git log`; `actions/checkout` runs at depth 1 in CI and the history is
//! not there to ask. This is the reachable half: the record exists, or the
//! report says it does not.
//!
//! **Whether a verification record is any good.** A record naming a sprint
//! satisfies rule 1 by existing. Judging it is what the record itself is for.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Four mutations, run 2026-09-08, all four red:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `author-verified` removed from `15. Sprint 14 Status.md`, **the exact state the rule exists to catch** | *a governed report is verified or says it is not* |
//! | `FIRST_GOVERNED_SPRINT` raised to `99`, so rule 1 asserts over nothing | *the walk finds what exists and governs something* |
//! | `author-verified` added to `04. Sprint 3 Status.md`, a plausible well-meant backfill | *the pre-rule sprints are named rather than silently skipped* |
//! | `01. Sprint 0 Status.md` renamed out of the walk | *the walk finds what exists*, and *the pre-rule sprints are named* |
//!
//! **The first is the one that matters** — it is #375's AC3, and it is the
//! state Sprint 14 would have been in had its author not chosen the label.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// The first sprint governed by [sprint plan](../../projects/planning/01.%20Sprint%20Plan.md)
/// §2's label rule.
///
/// The clause landed at `cdcd704` on 2026-09-04, during Sprint 14, and Sprint
/// 14 is the first sprint it governed. Lowering this is a claim that an earlier
/// report should have carried a label that did not exist when it was written.
const FIRST_GOVERNED_SPRINT: u32 = 14;

/// The sprints below the floor with neither a verification record nor the
/// label, named so the exemption is finite rather than implied by the floor.
///
/// These six predate the rule. The set is expected to be exactly this — it
/// cannot grow, because every sprint from [`FIRST_GOVERNED_SPRINT`] on is
/// governed, and if it *shrinks* somebody has edited a settled report.
const UNGOVERNED_AND_UNLABELLED: [u32; 6] = [0, 1, 2, 3, 4, 5];

/// The label a report carries when no independent pass read the sprint.
const LABEL: &str = "author-verified";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

/// The sprint number a file name refers to, from the `Sprint <N>` in it.
///
/// **The two folders are numbered independently and do not line up**: record 09
/// is *Sprint 11 Independent Pass* and record 13 is *Sprint 13 Independent
/// Pass*, so matching a status report to a verification record by position
/// would pair most of them with the wrong sprint and still pass. The sprint
/// number in the name is the only thing that means the same in both.
fn sprint_in(name: &str) -> Option<u32> {
    let at = name.find("Sprint ")? + "Sprint ".len();
    let digits: String = name[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();

    digits.parse().ok()
}

/// `projects/status/NN. Sprint <N> Status.md`, as sprint number → file name.
///
/// `00. Status Report Template.md` is the form the reports are copied from and
/// names no sprint, so it is skipped by the same rule that skips a verification
/// record naming none.
fn status_reports() -> Vec<(u32, String, String)> {
    let directory = repository_root().join("projects/status");
    let mut reports = Vec::new();

    for entry in fs::read_dir(&directory).expect("the status directory is readable") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();

        if !name.ends_with(" Status.md") {
            continue;
        }

        let Some(sprint) = sprint_in(&name) else {
            continue;
        };

        let body = fs::read_to_string(&path).expect("the status report is readable");

        reports.push((sprint, name, body));
    }

    reports.sort_by_key(|(sprint, ..)| *sprint);
    reports
}

/// The sprints `projects/verifications/` holds a record for.
///
/// A record whose name states no sprint is not a defect and is ignored: `10.
/// Phase 5 Exit Demo.md`, `14. MVP Verification.md`, `01. Party Surface
/// Verification.md` and `02. Role View and Fix Verification.md` are all real
/// verification work that answers to a phase, a release or a surface rather
/// than to a sprint.
fn verified_sprints() -> BTreeSet<u32> {
    let directory = repository_root().join("projects/verifications");
    let mut sprints = BTreeSet::new();

    for entry in fs::read_dir(&directory).expect("the verifications directory is readable") {
        let path = entry.expect("a directory entry").path();

        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }

        let name = path
            .file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();

        if let Some(sprint) = sprint_in(&name) {
            sprints.insert(sprint);
        }
    }

    sprints
}

/// Rule 1, and the whole point: **a report says which of the two it is.**
#[test]
fn a_governed_report_is_verified_or_says_it_is_not() {
    let verified = verified_sprints();

    let silent: Vec<_> = status_reports()
        .into_iter()
        .filter(|(sprint, ..)| *sprint >= FIRST_GOVERNED_SPRINT)
        .filter(|(sprint, _, body)| !verified.contains(sprint) && !body.contains(LABEL))
        .map(|(sprint, name, _)| format!("Sprint {sprint} — {name}"))
        .collect();

    assert!(
        silent.is_empty(),
        "a status report says neither that a pass read the sprint nor that none did \
         (sprint plan §2):\n  {}\n\nEither add a verification record for that sprint to \
         projects/verifications/ — its name must contain 'Sprint <N>' — or write \
         {LABEL:?} into the report. The second is a real answer, not a fallback: it is \
         what Sprint 14's report says.",
        silent.join("\n  ")
    );
}

/// Rule 2. **A walk that finds nothing passes every assertion under it**, and a
/// floor above the last report is the same failure wearing a constant.
#[test]
fn the_walk_finds_what_exists_and_governs_something() {
    let reports = status_reports();
    let verified = verified_sprints();

    assert!(
        reports.len() >= 15,
        "the walk found {} status reports, which is too few — projects/status/ moved",
        reports.len()
    );

    assert!(
        verified.len() >= 8,
        "the walk found records for {} sprints, which is too few — \
         projects/verifications/ moved, or its names stopped saying 'Sprint <N>'",
        verified.len()
    );

    let governed = reports
        .iter()
        .filter(|(sprint, ..)| *sprint >= FIRST_GOVERNED_SPRINT)
        .count();

    assert!(
        governed > 0,
        "FIRST_GOVERNED_SPRINT is {FIRST_GOVERNED_SPRINT} and the last report is Sprint {}, \
         so the rule governs no report and passes without asserting anything",
        reports.last().map_or(0, |(sprint, ..)| *sprint)
    );
}

/// Rule 3. The floor exempts six reports; this names them, so the exemption is
/// in the diff rather than in a constant's shadow.
#[test]
fn the_pre_rule_sprints_are_named_rather_than_silently_skipped() {
    let verified = verified_sprints();

    let unlabelled: Vec<u32> = status_reports()
        .into_iter()
        .filter(|(sprint, ..)| *sprint < FIRST_GOVERNED_SPRINT)
        .filter(|(sprint, _, body)| !verified.contains(sprint) && !body.contains(LABEL))
        .map(|(sprint, ..)| sprint)
        .collect();

    assert_eq!(
        unlabelled,
        UNGOVERNED_AND_UNLABELLED.to_vec(),
        "the set of pre-rule sprints with neither a record nor the label has changed.\n\
         It cannot grow — every sprint from {FIRST_GOVERNED_SPRINT} on is governed by the \
         test above — so this means a settled status report or verification record was \
         edited. If that was deliberate, update UNGOVERNED_AND_UNLABELLED and say why in \
         the commit; if it was a backfill of {LABEL:?} into an old report, it is D-71's \
         rule: a record says what was true when it was written."
    );
}
