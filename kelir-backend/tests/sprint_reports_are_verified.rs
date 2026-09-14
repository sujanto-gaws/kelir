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
//! needs `git log`. When this was written `actions/checkout` ran at depth 1 in
//! CI and the history was not there to ask; **since [#453](https://github.com/sujanto-gaws/kelir/issues/453)
//! the backend job fetches all of it**, for rules 4–6 below. The trailer check
//! is still not attempted here — it is a claim about *who* read a range rather
//! than *what* changed in it, and that question is
//! [#448](https://github.com/sujanto-gaws/kelir/issues/448)'s. This is the
//! reachable half: the record exists, or the report says it does not.
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
//!
//! # Rules 4–6: a screen is driven by a browser, or its report says it was not
//!
//! [#453](https://github.com/sujanto-gaws/kelir/issues/453), Sprint 18 item 3b,
//! and [retrospective 15](../../projects/retrospectives/15.%20Sprint%2017%20Retrospective.md)'s
//! first action. [Sprint plan](../../projects/planning/01.%20Sprint%20Plan.md)
//! §5's seventh verification rule says *a frontend item's status row says
//! `verified by inspection only` unless a flow in `e2e/tests/` reaches the
//! surface it delivered*. **Sprint 17 shipped three screens, added no spec, and
//! wrote no such sentence until its close went looking** — and the phrase had
//! last reached a status report for Sprint 10. Rule 1 has a reader. Rule 7 had
//! none.
//!
//! For every status report at or above [`FIRST_BROWSER_GOVERNED_SPRINT`]:
//!
//! 4. **The sprint gained a browser spec, or the report's Scope Status says
//!    `verified by inspection only`.** *Gained* is read from git rather than
//!    from the report: a spec counts when the commit that added it is a squash
//!    merge whose subject ends in `(#N)`, and `#N` is a pull request the
//!    report's Scope Status cites.
//! 5. **The history is there to ask, and the walk found what exists.** A
//!    depth-1 clone does not fail to answer rule 4 — it answers it wrongly,
//!    because its single grafted commit *adds* every file in the tree. So a
//!    shallow clone is red and never skipped, and the log must attribute at
//!    least the thirteen specs that existed when the rule was written.
//! 6. **The pre-rule sprints are named rather than silently passed**, as rule 3
//!    names them for the label.
//!
//! # How a sprint's commit range is determined
//!
//! **By the pull requests its report cites.** #453 says in its own text that
//! this is a decision the row has to take rather than a detail, so the
//! alternatives are recorded with it:
//!
//! - *By date.* A report's header date is when it was written, not when the
//!   sprint began, and three closes running landed after the next sprint's plan
//!   ([retrospective 15](../../projects/retrospectives/15.%20Sprint%2017%20Retrospective.md)).
//!   A date window hands one sprint's specs to its neighbour.
//! - *By label.* `sprint:N` lives on GitHub issues, and a test that calls the
//!   API is a test that fails when the network does.
//! - *By the report naming a spec.* Checkable at depth 1 with no CI change, but
//!   it asks the report — the document that said nothing in Sprint 17 — to be
//!   its own evidence.
//!
//! The Scope Status table already cites each row's pull request, and the
//! squash subject already ends in its number, so **joining the two needs
//! nothing either side does not already carry.** The cost is CI's: the backend
//! job checks out with `fetch-depth: 0`, as `Commit messages` already did —
//! 275 commits and 5.4 MiB of pack on the day it changed.
//!
//! # What rules 4–6 will not notice
//!
//! - **A negation.** The phrase is matched, not parsed:
//!   [status report 10](../../projects/status/10.%20Sprint%209%20Status.md)'s
//!   Scope Status reads *its row does not say `verified by inspection only`*,
//!   and that satisfies rule 4. What is scoped is **where** the phrase may
//!   stand — Scope Status only — so a report cannot pass by quoting the rule in
//!   a later section.
//! - **A spec that reaches a different screen.** Rule 4 asks whether the sprint
//!   gained a spec, which is the retrospective's test. Whether it reaches
//!   *each* surface shipped is rule 7's full sentence, and needs a mapping from
//!   rows to routes that no file carries.
//! - **A pull request cited for context.** A Scope Status row that cites an
//!   older spec-adding pull request counts it for that sprint.
//! - **A spec extended rather than added.** A sprint that grows an existing
//!   flow to reach its new screen satisfies rule 7 and not rule 4, and must add
//!   a file or write the phrase. That is the retrospective's wording, kept.
//! - **A sprint that shipped no screen.** It is governed the same way and must
//!   still cite a spec or write the phrase. None has arisen since the floor;
//!   the first one is the time to add a third branch with its own mutation,
//!   rather than guessing its wording now.
//!
//! # Rules 4–6, seen red 2026-09-14 (#453)
//!
//! **Against the report that earned them, first.** Every `verified by
//! inspection only` in [status report 18](../../projects/status/18.%20Sprint%2017%20Status.md)
//! was rewritten to `verified by reading`, and
//! `a_governed_report_drives_its_screens_or_says_it_did_not` went red naming
//! *Sprint 17 — 18. Sprint 17 Status.md*. **The five other tests stayed
//! green.** A second run removed the phrase from Scope Status only, leaving
//! the one occurrence in a later section: the same test, red, which is what
//! says the scoping is load-bearing rather than decorative. The report was
//! restored byte-exact after each.
//!
//! **Seven synthetic reports, each written as `99. Sprint 99 Status.md` with
//! `author-verified` so rule 1 had nothing to say, run, and deleted —
//! positive controls last:**
//!
//! | The Scope Status | Rule 4 |
//! |---|---|
//! | Cites nothing, says nothing | **refused** |
//! | Says nothing; the phrase stands in a later section | **refused** |
//! | Cites `/pull/437` — merged, real, and it added no spec | **refused** |
//! | Cites `#385` as an `/issues/` link rather than a pull request | **refused** |
//! | **Control** — the phrase on a row | accepted |
//! | **Control** — `PR #385`, the Sprint 8 citation style | accepted |
//! | **Control** — `/pull/385`, the current style | accepted |
//!
//! **Each refused probe reddened rule 4 and nothing else.** The third is the
//! one that matters: a rule asking *does the report cite a pull request* would
//! have accepted it.
//!
//! **Three code mutations, each restored byte-exact:**
//!
//! | Mutation | Red |
//! |---|---|
//! | The shallow check expects `true` | rules 4, 5 and 6 — **every test that reads history refuses, none skips** |
//! | [`FIRST_BROWSER_GOVERNED_SPRINT`] raised to `99` | rule 5 alone |
//! | The squash pattern changed to `(PR #N)`, so nothing is attributed | rules 5 and 6 — rule 6's set grew by Sprints 14 and 15, the two whose reports cite their spec-adding pull requests |
//!
//! **The last one is also a finding about the floor.** With nothing attributed,
//! Sprints 7–10 still pass rule 6, because each of their Scope Status sections
//! contains the phrase — Sprint 7's as an honest label on two pages no flow
//! covered, and **Sprints 8, 9 and 10's only as negations**, each saying its
//! row does *not* read it because a named spec drives the screen. Those three
//! are right for the wrong reason. That is the first limit above, measured
//! rather than supposed.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use regex::Regex;

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

/// The first sprint governed by rules 4–6
/// ([#453](https://github.com/sujanto-gaws/kelir/issues/453)).
///
/// [Retrospective 15](../../projects/retrospectives/15.%20Sprint%2017%20Retrospective.md)
/// wrote the rule against Sprint 17 — three screens, no spec — and binds every
/// report from it on. Lowering this asks an earlier report for a sentence that
/// nothing read when it was written.
const FIRST_BROWSER_GOVERNED_SPRINT: u32 = 17;

/// The sprints below [`FIRST_BROWSER_GOVERNED_SPRINT`] whose report neither
/// cites a pull request that added a spec nor says [`INSPECTION_ONLY`] in its
/// Scope Status, named so the exemption is finite rather than implied.
///
/// **Eleven, and they are not one kind.** Sprints 0–6 predate the harness,
/// which [#153](https://github.com/sujanto-gaws/kelir/issues/153) built in
/// Sprint 7. Sprints 11, 12 and 16 added no spec. **Sprint 13 is the stated
/// limit met in a real report**: its Scope Status cites PR #297 and PR #302,
/// both of which *grew* existing specs, and rule 4 counts only a spec added.
/// The same walk shows [#369](https://github.com/sujanto-gaws/kelir/pull/369),
/// merged 2026-09-07 with a spec of its own, cited by no report at all.
const UNGOVERNED_AND_UNDRIVEN: [u32; 11] = [0, 1, 2, 3, 4, 5, 6, 11, 12, 13, 16];

/// What a frontend row says when no browser flow reaches its screen, in sprint
/// plan §5 rule 7's own words.
const INSPECTION_ONLY: &str = "verified by inspection only";

/// The specs `e2e/tests/` held when rules 4–6 were written. The log must
/// attribute at least this many, or it is not reading the history it claims.
const SPECS_WHEN_WRITTEN: usize = 13;

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

/// A report's `## Scope Status` section, up to the next `## ` heading.
///
/// Empty when the report has none, so a report that drops the section cites
/// nothing and says nothing — rule 4 red, rather than the whole body searched.
/// `### ` subsections stay inside it, which is where status report 18 explains
/// its three frontend rows.
fn scope_status(body: &str) -> &str {
    let Some(start) = body.find("\n## Scope Status") else {
        return "";
    };
    let section = &body[start + 1..];
    let end = section.find("\n## ").unwrap_or(section.len());

    &section[..end]
}

/// The pull requests a Scope Status cites: as a link, `/pull/436`, or as text,
/// `PR #212`, which is how the Sprint 8 report wrote them.
fn cited_pull_requests(section: &str) -> BTreeSet<u32> {
    let citation = Regex::new(r"/pull/(\d+)|PR #(\d+)").expect("the citation pattern compiles");

    citation
        .captures_iter(section)
        .filter_map(|found| found.get(1).or_else(|| found.get(2)))
        .filter_map(|number| number.as_str().parse().ok())
        .collect()
}

/// `git`, run at the repository root, with its standard output as text.
fn git(arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("git runs — rules 4–6 read history and have no answer without it");

    assert!(
        output.status.success(),
        "git {} failed:\n{}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("git prints UTF-8")
}

/// Every spec file ever added to `e2e/tests/`, with the pull request whose
/// squash merge added it — `None` when the subject names none, which is what a
/// pull request's own unsquashed commits look like while CI tests it.
///
/// **Refuses a shallow clone rather than answering from it.** A depth-1
/// checkout holds one grafted commit that adds every file in the tree, so the
/// log would credit all of `e2e/tests/` to whichever pull request is under
/// test — and rule 4 would be wrong in both directions while staying green.
///
/// `--no-renames`, so a renamed spec is an addition under its new name rather
/// than a rename that nothing attributes.
fn specs_added() -> Vec<(Option<u32>, String)> {
    assert_eq!(
        git(&["rev-parse", "--is-shallow-repository"]).trim(),
        "false",
        "this clone is shallow, and rules 4–6 ask a question a shallow clone answers wrongly: \
         its one grafted commit adds every file in the tree. Fetch full history — CI's backend \
         job checks out with `fetch-depth: 0` for this (#453), and `git fetch --unshallow` \
         does the same locally."
    );

    let log = git(&[
        "log",
        "--no-renames",
        "--diff-filter=A",
        "--name-only",
        "--format=%x00%s",
        "--",
        "e2e/tests",
    ]);
    let squash = Regex::new(r"\(#(\d+)\)\s*$").expect("the squash pattern compiles");
    let mut added = Vec::new();

    for commit in log.split('\0').skip(1) {
        let mut lines = commit.lines();
        let subject = lines.next().unwrap_or_default();
        let pull_request: Option<u32> = squash
            .captures(subject)
            .and_then(|found| found[1].parse().ok());

        for path in lines.map(str::trim) {
            if path.starts_with("e2e/tests/") && path.ends_with(".spec.ts") {
                added.push((pull_request, path.to_owned()));
            }
        }
    }

    added
}

/// The pull requests whose squash merge added at least one spec.
fn pull_requests_that_added_a_spec() -> BTreeSet<u32> {
    specs_added()
        .into_iter()
        .filter_map(|(pull_request, _)| pull_request)
        .collect()
}

/// Rule 4's predicate, shared with rule 6 so the two cannot drift apart.
fn drives_a_screen_or_says_it_did_not(body: &str, driven: &BTreeSet<u32>) -> bool {
    let section = scope_status(body);

    section.contains(INSPECTION_ONLY) || !cited_pull_requests(section).is_disjoint(driven)
}

/// Rule 4, and the point of #453: **a sprint that added no browser flow says
/// so.**
#[test]
fn a_governed_report_drives_its_screens_or_says_it_did_not() {
    let driven = pull_requests_that_added_a_spec();

    let silent: Vec<_> = status_reports()
        .into_iter()
        .filter(|(sprint, ..)| *sprint >= FIRST_BROWSER_GOVERNED_SPRINT)
        .filter(|(_, _, body)| !drives_a_screen_or_says_it_did_not(body, &driven))
        .map(|(sprint, name, _)| format!("Sprint {sprint} — {name}"))
        .collect();

    assert!(
        silent.is_empty(),
        "a status report cites no pull request that added a browser spec and does not say \
         {INSPECTION_ONLY:?} (sprint plan §5, verification rule 7; #453):\n  {}\n\n\
         Either cite, in the report's Scope Status, the pull request that added a spec to \
         e2e/tests/ — as a /pull/N link or `PR #N` — or write {INSPECTION_ONLY:?} on the \
         frontend rows no flow reaches. The second is a real answer: it is what Sprint 17's \
         report says.",
        silent.join("\n  ")
    );
}

/// Rule 5. **A shallow clone answers rule 4 wrongly rather than not at all**,
/// and a log that attributes nothing passes every assertion under it.
#[test]
fn the_history_is_there_to_ask_and_the_rule_governs_something() {
    let attributed: BTreeSet<String> = specs_added()
        .into_iter()
        .filter_map(|(pull_request, path)| pull_request.map(|_| path))
        .collect();

    assert!(
        attributed.len() >= SPECS_WHEN_WRITTEN,
        "the log attributed {} specs to a pull request, fewer than the {SPECS_WHEN_WRITTEN} \
         e2e/tests/ held when this rule was written — the specs moved, or squash subjects \
         stopped ending in (#N)",
        attributed.len()
    );

    let reports = status_reports();
    let governed = reports
        .iter()
        .filter(|(sprint, ..)| *sprint >= FIRST_BROWSER_GOVERNED_SPRINT)
        .count();

    assert!(
        governed > 0,
        "FIRST_BROWSER_GOVERNED_SPRINT is {FIRST_BROWSER_GOVERNED_SPRINT} and the last report \
         is Sprint {}, so rule 4 governs no report and passes without asserting anything",
        reports.last().map_or(0, |(sprint, ..)| *sprint)
    );
}

/// Rule 6. The floor exempts every report below it; this names the ones that
/// would fail rule 4, so the exemption is in the diff rather than in a
/// constant's shadow.
#[test]
fn the_pre_rule_sprints_are_named_rather_than_silently_passed() {
    let driven = pull_requests_that_added_a_spec();

    let undriven: Vec<u32> = status_reports()
        .into_iter()
        .filter(|(sprint, ..)| *sprint < FIRST_BROWSER_GOVERNED_SPRINT)
        .filter(|(_, _, body)| !drives_a_screen_or_says_it_did_not(body, &driven))
        .map(|(sprint, ..)| sprint)
        .collect();

    assert_eq!(
        undriven,
        UNGOVERNED_AND_UNDRIVEN.to_vec(),
        "the set of pre-rule sprints that neither cite a spec-adding pull request nor say \
         {INSPECTION_ONLY:?} in their Scope Status has changed.\nIt cannot grow — every sprint \
         from {FIRST_BROWSER_GOVERNED_SPRINT} on is governed by rule 4 — so a settled status \
         report was edited, or the commit that added a spec was rewritten. If that was \
         deliberate, update UNGOVERNED_AND_UNDRIVEN and say why in the commit."
    );
}
