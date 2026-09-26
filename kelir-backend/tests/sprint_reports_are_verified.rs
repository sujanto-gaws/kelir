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
//! than *what* changed in it, and sprint plan §2 records that nothing asks it
//! ([#448](https://github.com/sujanto-gaws/kelir/issues/448)). This is the
//! reachable half: the record exists, or the report says it does not.
//!
//! **Whether a verification record is any good.** A record naming a sprint
//! satisfies rule 1 by existing. Judging it is what the record itself is for.
//!
//! **A label nobody can see.** Rule 1 matches `author-verified` in the raw
//! Markdown, so the label inside an HTML comment satisfies it. #468 made rule
//! 4 read a report as it renders; rule 1 was outside that issue and is not
//! changed by it.
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
//!    merge whose subject ends in `(#N)`, the spec is still in `e2e/tests/` at
//!    `HEAD` under that name or one it was renamed to, and `#N` is a pull
//!    request the report's Scope Status cites — as a link whose target is
//!    `https://github.com/sujanto-gaws/kelir/pull/N`, or as `PR #N` in prose.
//!    **The report is read as it renders**
//!    ([#468](https://github.com/sujanto-gaws/kelir/issues/468)): what a
//!    reader cannot see — an HTML comment, a link reference definition, a
//!    link's title or destination, an image, a tag — says nothing, and a
//!    link's label is not where it points.
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
//! - **`PR #N` in prose is read as Kelir's.** Text has no repository in it, so
//!   *unovis PR #457* cites Kelir's #457. Only a link says where it points,
//!   and only a link whose target is this repository counts. A reference
//!   definition that puts its target on the line after the label is not read
//!   as one, so `[PR #457][u]` over it is left as prose and counts the same
//!   way.
//! - **Text a class hides.** An element marked `hidden` or given a `style` is
//!   dropped with its contents, whether or not GitHub keeps the attribute —
//!   either answer is safe (see [`without_hidden_elements`]). An element
//!   hidden only by a class the page's stylesheet defines is not, and its
//!   text counts. No report uses HTML, on `main` or in any tree it has held.
//! - **`HEAD`, not the working tree, for specs.** Presence is asked of the
//!   commit the log reads, so an uncommitted deletion is not seen until it is
//!   committed, and an uncommitted move is not mistaken for one. The reports
//!   themselves are read from disk, so an edit is checked before it is
//!   committed.
//! - **A move git does not recognise as one.** Renames are followed as
//!   `git log --find-renames` finds them: at least half the file unchanged.
//!   A pull request that moves a spec *and* rewrites most of it squashes to a
//!   deletion and an addition, and a settled report citing the pull request
//!   that first added it goes red. Move in one pull request and rewrite in
//!   another.
//!
//! **Fails safe, and named rather than fixed.** Each refuses a report that
//! does render the phrase or a citation, so its author sees a red test rather
//! than a quiet pass:
//!
//! - **Code is read as Markdown.** A code span is not set apart, so
//!   `` `<!--` `` in inline code starts a comment that runs to its `-->` or to
//!   the end, and `` `[x](y)` `` loses its brackets. No report has either.
//! - **The phrase is matched in plain words.** `verified by *inspection* only`
//!   renders the phrase and does not match it. Sprint plan §5 asks for it *in
//!   those words*.
//! - **An HTML link.** `<a href>` is a tag, and is dropped with its target.
//!   Write the link in Markdown, as every report does.
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
//!
//! # A hidden phrase, a foreign pull request and a deleted spec, refused 2026-09-26 (#468)
//!
//! [Verification record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)'s
//! finding 5 found rule 4 satisfiable three ways without a flow:
//!
//! - **A.** The phrase inside `<!-- … -->`, which renders as nothing.
//! - **B.** `https://github.com/f5/unovis/pull/457`, which `/pull/(\d+)` read
//!   as Kelir's #457.
//! - **C.** A spec added under `(#9999)` and deleted by a later commit, which
//!   `--diff-filter=A` went on counting.
//!
//! **The first fix was sent back by its gate**, which found each shape had a
//! neighbour: a link reference definition, a link title and a `<…>`
//! destination hid the phrase as a comment does; `[PR #457](…f5/unovis…)`
//! passed because the label was read as prose; and asking whether a spec's
//! path still existed turned [status report 20](../../projects/status/20.%20Sprint%2019%20Status.md)
//! red when #481's spec was moved into a subdirectory — a settled report
//! failing for a move, which D-71 does not allow to be fixed by editing it.
//! Its full table added three more: `[//]: # (PR #457)`, a citation that
//! renders as nothing; a bare Kelir URL inside another,
//! `https://evil.example/?u=https://github.com/sujanto-gaws/kelir/pull/457`;
//! and `<span hidden>`, which nobody had checked GitHub keeps. **So the rule
//! now reads what renders, where a link points, and where a spec went:**
//!
//! - **A.** HTML comments are removed before Scope Status is found, an
//!   unclosed one running to the end, as `adr_records_are_current.rs` does
//!   since #561. [`rendered`] then drops what shows nothing else, citations
//!   included, and an element marked `hidden` or styled with its contents.
//! - **B.** A link counts by its target, which must *be* a Kelir pull request
//!   URL from its first character — any scheme, `www.` or case. Its label is
//!   not prose. `PR #N`, `PR#N` and a bare Kelir URL count in prose outside a
//!   label, the URL only where it starts a word.
//! - **C.** Each spec is followed oldest first through its renames to where
//!   it is now; a deletion ends it, and a path added again starts afresh.
//!   What survives counts while its path is in `HEAD`.
//!
//! **Each fix seen red.** Each predicate below was put back alone against the
//! final file and the file run, then restored byte-exact:
//!
//! | Put back | Red |
//! |---|---|
//! | The raw body read, comments kept | *a phrase inside an HTML comment*, *a commented-out heading*, and *a pull request in another repository* — its commented-out citation |
//! | The phrase matched in the raw section, not [`rendered`] | *a phrase that renders as nothing* alone |
//! | `/pull/(\d+)` anywhere in a link's target | *a pull request in another repository* alone |
//! | `PR #N` read in labels as well as prose | *a pull request in another repository* alone |
//! | Every spec ever added counted | *a spec no longer in the tree* and *a moved spec* |
//! | Renames not followed — the first fix's presence check | *a moved spec* alone |
//!
//! **No report main has held is refused.** Main's file and this one were run
//! against `projects/status/` and `projects/verifications/` as each of the 66
//! first-parent commits that touched them left them, from the first report to
//! `13e1e76`, each folder restored where that commit had it. **Both gave the
//! same result on every tree, and rule 4 was green on all 66**: every test
//! green on the 14 from `b3127de` (#443, the first to hold a Sprint 17 report)
//! on, and the same other tests red under both on the 52 before it, which
//! predate the reports this file's floors and constants were written against.
//! No status report in any of the 66 contains `<!--`, a reference definition,
//! an image, an HTML tag, a `/pull/` link outside this repository, or a
//! `PR #N` label on one.
//!
//! **C is swept by its history rather than by tree.** The sweep reads today's
//! log. That is faithful for C because **no commit on main has deleted or
//! renamed a file in `e2e/tests/`**, and none that touched it is a merge: at
//! every commit, every spec added before it was still there under its first
//! name, and C's fix counts exactly what the old predicate counted.
//!
//! **Positive controls last**, run against main's file, the first fix and
//! this one. The first seven are a synthetic `99. Sprint 99 Status.md`
//! carrying `author-verified` and one Scope Status row; the rest change
//! `e2e/tests/` in local commits and add no report. Each was undone before the
//! next:
//!
//! | The probe | Main | First fix | Now |
//! |---|---|---|---|
//! | `Done`, citing nothing — the issue's control | refused | refused | **refused** |
//! | `Done`, then `[//]: # (verified by inspection only)` | accepted | accepted | **refused** |
//! | `Done [x](https://example.com "verified by inspection only")` | accepted | accepted | **refused** |
//! | `[Done](<verified by inspection only>)` | accepted | accepted | **refused** |
//! | `Done - verified by inspection only` | accepted | accepted | accepted |
//! | `Done - [PR #481](https://github.com/f5/unovis/pull/481)` | accepted | accepted | **refused** |
//! | `Done - [#481](https://github.com/sujanto-gaws/kelir/pull/481)` | accepted | accepted | accepted |
//! | #481's spec moved into `e2e/tests/dashboard/` and committed | green | Sprint 19 red | **green** |
//! | The same move, staged and not committed | green | Sprint 19 red | **green** |
//! | The moved spec then deleted and committed | green | Sprint 19 red | **Sprint 19 red** |
//! | A spec at `e2e/tests/zz-é.spec.ts` under `(#9998)`, cited as `PR #9998` | refused | refused | **accepted** |
//! | `PR #9999`, its spec added under `(#9999)` and deleted | accepted | refused | **refused** |
//! | The tree as it stands | green | green | green |
//!
//! **Each red reddened rule 4 and nothing else.** The three rows for #481's
//! spec are the ones that decide C: a moved flow is the same flow, and a
//! deleted one is not.
//! The non-ASCII row was refused before because git quotes such a path, and
//! the log is now read with `core.quotePath=false`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use regex::{Captures, Regex};

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

/// `text` with every `<!-- … -->` removed. An unclosed comment runs to the end.
///
/// A renderer shows none of it, so a phrase or a citation inside one is said
/// to nobody ([#468](https://github.com/sujanto-gaws/kelir/issues/468) A).
/// The same as `adr_records_are_current.rs`'s, which #561 wrote for a
/// commented-out blocker.
fn without_comments(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;

    while let Some(open) = rest.find("<!--") {
        kept.push_str(&rest[..open]);
        rest = rest[open..]
            .find("-->")
            .map_or("", |close| &rest[open + close + 3..]);
    }

    kept.push_str(rest);
    kept
}

/// Wrapped around a link's label while a section is rendered, so the label can
/// be kept for the phrase and dropped for a citation.
const LABEL_OPEN: char = '\u{1}';
const LABEL_CLOSE: char = '\u{2}';

/// What a Scope Status shows a reader, as far as rule 4 asks.
struct Rendered {
    /// The text a reader sees, link labels included.
    visible: String,
    /// The same without link labels: where `PR #N` in prose may cite.
    unlabelled: String,
    /// Where each link points.
    targets: Vec<String>,
}

/// `text` without any element whose opening tag carries `hidden` or `style`,
/// contents and all, up to its closing tag — or to the end, if it has none.
///
/// **Whether GitHub keeps either attribute is not asked, because the answer
/// does not change what this should do.** If GitHub keeps them, the contents
/// are hidden and rightly dropped. If it strips them, the contents show and
/// are dropped anyway, which refuses a report that did say the phrase: a red
/// test, not a quiet pass. An element of the same name nested inside one
/// ends it early, and the rest is left to the tag pattern, which keeps text.
fn without_hidden_elements(text: &str) -> String {
    let opening = Regex::new(r"(?i)<([a-z][a-z0-9-]*)\b[^>]*\b(?:hidden|style)\b[^>]*>")
        .expect("the hidden element pattern compiles");
    let mut kept = String::new();
    let mut rest = text;

    while let Some(found) = opening.captures(rest) {
        let whole = found.get(0).expect("group 0 always takes part");
        let closing = format!("</{}", found[1].to_ascii_lowercase());

        kept.push_str(&rest[..whole.start()]);
        rest = &rest[whole.end()..];
        // ASCII lowercasing keeps every byte offset, so `at` indexes `rest`.
        rest = rest
            .to_ascii_lowercase()
            .find(&closing)
            .and_then(|at| rest[at..].find('>').map(|close| &rest[at + close + 1..]))
            .unwrap_or("");
    }

    kept.push_str(rest);
    kept
}

/// `section`, with its HTML comments already removed, reduced to what
/// renders ([#468](https://github.com/sujanto-gaws/kelir/issues/468) A and B).
///
/// **What renders as nothing is dropped**: a link reference definition, such
/// as `[//]: # (…)`; an element marked `hidden` or styled, with its contents;
/// an image; a link's title and destination; an HTML tag and its attributes.
/// Hidden elements go before links are read, so a link inside one cites
/// nothing. **A link's label is visible but is not where it points**,
/// so `[PR #457](https://github.com/f5/unovis/pull/457)` shows `PR #457` and
/// cites another repository.
///
/// A reduction, not a parser: [`Rendered`] is what these patterns leave, and
/// the module doc names what they do not see.
fn rendered(section: &str) -> Rendered {
    let definition = Regex::new(r"(?m)^ {0,3}\[([^\]\n]+)\]:[ \t]*(?:<([^>\n]*)>|(\S+))[^\n]*$")
        .expect("the definition pattern compiles");
    let image = Regex::new(r"!\[[^\]\n]*\]\([^)\n]*\)").expect("the image pattern compiles");
    let inline = Regex::new(
        r#"\[([^\]\n]*)\]\(\s*(?:<([^>\n]*)>|([^\s)]*))(?:\s+(?:"[^"\n]*"|'[^'\n]*'|\([^)\n]*\)))?\s*\)"#,
    )
    .expect("the inline link pattern compiles");
    let reference =
        Regex::new(r"\[([^\]\n]*)\](?:\[([^\]\n]*)\])?").expect("the reference pattern compiles");
    let autolink = Regex::new(r"<((?i:https?)://[^>\s]+)>").expect("the autolink pattern compiles");
    let tag = Regex::new(r"</?[A-Za-z][^>]*>").expect("the tag pattern compiles");
    let label = Regex::new("\u{1}[^\u{2}]*\u{2}").expect("the label pattern compiles");

    let mut definitions = BTreeMap::new();
    for found in definition.captures_iter(section) {
        let target = found
            .get(2)
            .or_else(|| found.get(3))
            .map_or("", |m| m.as_str());
        definitions
            .entry(found[1].to_lowercase())
            .or_insert_with(|| target.to_owned());
    }

    let mut targets = Vec::new();
    let text = definition.replace_all(section, "");
    let text = without_hidden_elements(&text);
    let text = image.replace_all(&text, "");
    let text = inline.replace_all(&text, |found: &Captures| {
        let target = found
            .get(2)
            .or_else(|| found.get(3))
            .map_or("", |m| m.as_str());
        targets.push(target.to_owned());
        format!("{LABEL_OPEN}{}{LABEL_CLOSE}", &found[1])
    });
    let text = reference.replace_all(&text, |found: &Captures| {
        let key = found
            .get(2)
            .filter(|m| !m.as_str().is_empty())
            .unwrap_or_else(|| found.get(1).expect("group 1 always takes part"));

        match definitions.get(&key.as_str().to_lowercase()) {
            Some(target) => {
                targets.push(target.clone());
                format!("{LABEL_OPEN}{}{LABEL_CLOSE}", &found[1])
            }
            None => found[0].to_owned(),
        }
    });
    let text = autolink.replace_all(&text, |found: &Captures| {
        targets.push(found[1].to_owned());
        format!("{LABEL_OPEN}{}{LABEL_CLOSE}", &found[1])
    });
    let text = tag.replace_all(&text, "");

    Rendered {
        visible: text.replace([LABEL_OPEN, LABEL_CLOSE], ""),
        unlabelled: label.replace_all(&text, "").into_owned(),
        targets,
    }
}

/// The pull requests a Scope Status cites: as a link to one of Kelir's,
/// `https://github.com/sujanto-gaws/kelir/pull/436`, or as text, `PR #212`,
/// which is how the Sprint 8 report wrote them.
///
/// **Only this repository's pull requests, read from where a link points.**
/// `/pull/N` anywhere in a URL once counted, so
/// `https://github.com/f5/unovis/pull/457` cited Kelir's #457, and a label
/// reading `PR #457` counted whatever it linked to
/// ([#468](https://github.com/sujanto-gaws/kelir/issues/468) B). A link's
/// target must *be* a Kelir pull request URL, from its first character; `PR
/// #N` and a bare Kelir URL count in prose outside a label. **A bare URL must
/// start a word**, after a line start, a space, a table's `|` or an opening
/// parenthesis that itself starts one, so one inside another URL, as in
/// `https://evil.example/?u=https://github.com/sujanto-gaws/kelir/pull/457`,
/// is part of that URL and cites nothing.
fn cited_pull_requests(section: &Rendered) -> BTreeSet<u32> {
    let kelir_link = Regex::new(
        r"^(?i:(?:https?://)?(?:www\.)?github\.com/sujanto-gaws/kelir/pull/)(\d+)(?:[/?#]|$)",
    )
    .expect("the link pattern compiles");
    let in_prose = Regex::new(
        r"(?m)(?:^|[\s|])\(?(?i:(?:https?://)?(?:www\.)?github\.com/sujanto-gaws/kelir/pull/)(\d+)|PR ?#(\d+)",
    )
    .expect("the prose pattern compiles");

    let linked = section
        .targets
        .iter()
        .filter_map(|target| kelir_link.captures(target.trim()))
        .filter_map(|found| found[1].parse().ok());
    let written = in_prose
        .captures_iter(&section.unlabelled)
        .filter_map(|found| found.get(1).or_else(|| found.get(2)))
        .filter_map(|number| number.as_str().parse().ok());

    linked.chain(written).collect()
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

/// Every spec file added to `e2e/tests/` whose flow is still there, under
/// whatever name, with the pull request whose squash merge added it — `None`
/// when the subject names none, which is what a pull request's own unsquashed
/// commits look like while CI tests it.
///
/// **Refuses a shallow clone rather than answering from it.** A depth-1
/// checkout holds one grafted commit that adds every file in the tree, so the
/// log would credit all of `e2e/tests/` to whichever pull request is under
/// test — and rule 4 would be wrong in both directions while staying green.
///
/// **Oldest first, renames followed.** A renamed spec keeps the pull request
/// that added it and also credits the one that moved it, which is what
/// `--no-renames` credited before #468. **Presence is asked of `HEAD`**, the
/// same commits the log reads, so an uncommitted move is not a deletion.
/// `core.quotePath=false`, so a path outside ASCII is printed as it is.
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
        "-c",
        "core.quotePath=false",
        "log",
        "--reverse",
        "--topo-order",
        "--find-renames",
        "--name-status",
        "--format=%x00%s",
        "--",
        "e2e/tests",
    ]);
    let tree: BTreeSet<String> = git(&[
        "-c",
        "core.quotePath=false",
        "ls-tree",
        "-r",
        "--name-only",
        "HEAD",
        "--",
        "e2e/tests",
    ])
    .lines()
    .map(str::to_owned)
    .collect();

    attributed_specs(&log, |path| tree.contains(path))
}

/// [`specs_added`]'s reading of `git log --reverse --find-renames
/// --name-status --format=%x00%s`, apart from git so the rule can be tested
/// without writing history.
///
/// **Each spec is followed from its addition to where it is now.** A rename
/// carries its additions to the new path; a deletion ends them; a path added
/// again after a deletion starts afresh. What survives counts only while
/// `still_there` says its path exists.
///
/// `--diff-filter=A` alone found every spec ever added, so a pull request
/// whose spec a later commit deleted went on driving a screen that no flow
/// reaches any more ([#468](https://github.com/sujanto-gaws/kelir/issues/468)
/// C) — and presence alone would refuse a spec that was only moved.
fn attributed_specs(log: &str, still_there: impl Fn(&str) -> bool) -> Vec<(Option<u32>, String)> {
    let squash = Regex::new(r"\(#(\d+)\)\s*$").expect("the squash pattern compiles");
    let is_spec = |path: &str| path.starts_with("e2e/tests/") && path.ends_with(".spec.ts");
    let mut lineages: BTreeMap<String, Vec<(Option<u32>, String)>> = BTreeMap::new();

    for commit in log.split('\0').skip(1) {
        let mut lines = commit.lines();
        let subject = lines.next().unwrap_or_default();
        let pull_request: Option<u32> = squash
            .captures(subject)
            .and_then(|found| found[1].parse().ok());

        for line in lines {
            let fields: Vec<&str> = line.trim_end().split('\t').collect();

            match fields.as_slice() {
                [status, path] if status.starts_with('A') && is_spec(path) => {
                    lineages.insert((*path).to_owned(), vec![(pull_request, (*path).to_owned())]);
                }
                [status, path] if status.starts_with('D') => {
                    lineages.remove(*path);
                }
                [status, from, to] if status.starts_with('R') => {
                    let mut lineage = lineages.remove(*from).unwrap_or_default();

                    if is_spec(to) {
                        lineage.push((pull_request, (*to).to_owned()));
                        lineages.insert((*to).to_owned(), lineage);
                    }
                }
                _ => {}
            }
        }
    }

    lineages
        .into_iter()
        .filter(|(path, _)| still_there(path))
        .flat_map(|(_, lineage)| lineage)
        .collect()
}

/// The pull requests whose squash merge added at least one spec.
fn pull_requests_that_added_a_spec() -> BTreeSet<u32> {
    specs_added()
        .into_iter()
        .filter_map(|(pull_request, _)| pull_request)
        .collect()
}

/// Rule 4's predicate, shared with rule 6 so the two cannot drift apart.
///
/// Read with its HTML comments removed **before** the section is found, so a
/// commented-out heading neither starts nor ends it.
fn drives_a_screen_or_says_it_did_not(body: &str, driven: &BTreeSet<u32>) -> bool {
    let uncommented = without_comments(body);
    let section = rendered(scope_status(&uncommented));

    section.visible.contains(INSPECTION_ONLY) || !cited_pull_requests(&section).is_disjoint(driven)
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
         Either cite, in the report's Scope Status, the pull request that added a spec still \
         in e2e/tests/ — as a https://github.com/sujanto-gaws/kelir/pull/N link or `PR #N` — \
         or write {INSPECTION_ONLY:?} on the frontend rows no flow reaches. Neither counts \
         inside an HTML comment. The second is a real answer: it is what Sprint 17's report \
         says.",
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

/// A report with `scope` as its Scope Status, and a later section after it.
fn report_whose_scope_status_says(scope: &str) -> String {
    format!(
        "# Sprint 99 Status\n\nauthor-verified\n\n## Scope Status\n\n{scope}\n\n## Risks\n\nNone.\n"
    )
}

/// #468 A. **A phrase in an HTML comment renders as nothing**, so it is not
/// said — however the comment is written, and when it never closes.
#[test]
fn a_phrase_inside_an_html_comment_is_not_said() {
    let nothing_driven = BTreeSet::new();

    for hidden in [
        "| 1 | The screen | Done <!-- verified by inspection only --> |",
        "| 1 | The screen | Done |\n<!--\nverified by inspection only\n-->",
        "| 1 | The screen | Done <!-- verified by inspection only",
    ] {
        assert!(
            !drives_a_screen_or_says_it_did_not(
                &report_whose_scope_status_says(hidden),
                &nothing_driven
            ),
            "a commented-out phrase satisfied rule 4:\n{hidden}"
        );
    }

    assert!(
        drives_a_screen_or_says_it_did_not(
            &report_whose_scope_status_says(
                "| 1 | The screen | Done <!-- a note --> — verified by inspection only |"
            ),
            &nothing_driven
        ),
        "the phrase beside a comment is visible, and says it"
    );
}

/// #468 A, the gate's second round. **A comment is not the only Markdown that
/// renders as nothing**: a link reference definition, a link's title or
/// destination, an image, and an HTML tag's attributes all hide the phrase.
#[test]
fn a_phrase_that_renders_as_nothing_is_not_said() {
    let nothing_driven = BTreeSet::new();

    for hidden in [
        "| 1 | The screen | Done |\n\n[//]: # (verified by inspection only)",
        "| 1 | The screen | Done |\n\n[note]: https://example.com \"verified by inspection only\"",
        "| 1 | The screen | [Done](https://example.com \"verified by inspection only\") |",
        "| 1 | The screen | [Done](https://example.com 'verified by inspection only') |",
        "| 1 | The screen | [Done](<verified by inspection only>) |",
        "| 1 | The screen | ![verified by inspection only](shot.png) |",
        "| 1 | The screen | <span title=\"verified by inspection only\">Done</span> |",
        "| 1 | The screen | Done <verified by inspection only> |",
        "| 1 | The screen | Done <span hidden>verified by inspection only</span> |",
        "| 1 | The screen | Done <SPAN HIDDEN>verified by inspection only</SPAN> |",
        "| 1 | The screen | Done <div style=\"display:none\">verified by inspection only</div> |",
        "| 1 | The screen | Done <span hidden>verified by inspection only",
    ] {
        assert!(
            !drives_a_screen_or_says_it_did_not(
                &report_whose_scope_status_says(hidden),
                &nothing_driven
            ),
            "a phrase that renders as nothing satisfied rule 4:\n{hidden}"
        );
    }

    for shown in [
        "| 1 | The screen | [verified by inspection only](https://example.com) |",
        "| 1 | The screen | `verified by inspection only` |",
        "| 1 | The screen | <b>verified by inspection only</b> |",
        "| 1 | The screen | <span class=\"note\">verified by inspection only</span> |",
        "| 1 | The screen | <span hidden>a note</span> verified by inspection only |",
        "| 1 | The screen | Done — verified by inspection only |\n\n[//]: # (a note)",
    ] {
        assert!(
            drives_a_screen_or_says_it_did_not(
                &report_whose_scope_status_says(shown),
                &nothing_driven
            ),
            "a phrase a reader sees did not satisfy rule 4:\n{shown}"
        );
    }
}

/// #468 A, the other way round: a commented-out heading neither ends Scope
/// Status early nor starts it.
#[test]
fn a_commented_out_heading_is_not_a_heading() {
    let nothing_driven = BTreeSet::new();

    let ended_early = report_whose_scope_status_says(
        "<!--\n## Next\n-->\n| 1 | The screen | verified by inspection only |",
    );
    assert!(drives_a_screen_or_says_it_did_not(
        &ended_early,
        &nothing_driven
    ));

    let started_in_a_comment = "# Sprint 99 Status\n\n<!--\n## Scope Status\n\n\
         verified by inspection only\n-->\n\n## Scope Status\n\n| 1 | The screen | Done |\n";
    assert!(!drives_a_screen_or_says_it_did_not(
        started_in_a_comment,
        &nothing_driven
    ));
}

/// #468 B. **A pull request in another repository is not one of Kelir's**,
/// though its number matches one that added a spec.
#[test]
fn a_pull_request_in_another_repository_is_not_cited() {
    let driven = BTreeSet::from([457]);

    for foreign in [
        "| 1 | The chart | Done — https://github.com/f5/unovis/pull/457 |",
        "| 1 | The chart | Done — [upstream](https://github.com/sujanto-gaws/kelir-fork/pull/457) |",
        "| 1 | The chart | Done — [relative](../../pull/457) |",
        "| 1 | The chart | Done — [PR #457](https://github.com/f5/unovis/pull/457) |",
        "| 1 | The chart | Done — [https://github.com/sujanto-gaws/kelir/pull/457](https://github.com/f5/unovis/pull/457) |",
        "| 1 | The chart | Done — [PR #457][u] |\n\n[u]: https://github.com/f5/unovis/pull/457",
        "| 1 | The chart | Done — [PR #457] |\n\n[PR #457]: https://github.com/f5/unovis/pull/457",
        "| 1 | The chart | Done — [x](https://example.com/?u=https://github.com/sujanto-gaws/kelir/pull/457) |",
        "| 1 | The chart | Done — https://example.com/github.com/sujanto-gaws/kelir/pull/457 |",
        "| 1 | The chart | Done — https://evil.example/?u=https://github.com/sujanto-gaws/kelir/pull/457 |",
        "| 1 | The chart | Done — https://evil.example/(https://github.com/sujanto-gaws/kelir/pull/457) |",
        "| 1 | The chart | Done |\n\n[//]: # (PR #457)",
        "| 1 | The chart | Done |\n\n[//]: # (https://github.com/sujanto-gaws/kelir/pull/457)",
        "| 1 | The chart | Done <span hidden>[#457](https://github.com/sujanto-gaws/kelir/pull/457)</span> |",
        "| 1 | The chart | Done <span hidden>PR #457</span> |",
    ] {
        assert!(
            !drives_a_screen_or_says_it_did_not(&report_whose_scope_status_says(foreign), &driven),
            "a foreign pull request satisfied rule 4:\n{foreign}"
        );
    }

    for kelir in [
        "| 1 | The chart | Done — [#457](https://github.com/sujanto-gaws/kelir/pull/457) |",
        "| 1 | The chart | Done — PR #457 |",
        "| 1 | The chart | Done — PR#457 |",
        "| 1 | The chart | Done — https://github.com/sujanto-gaws/kelir/pull/457 |",
        "| 1 | The chart | Done (https://github.com/sujanto-gaws/kelir/pull/457) |",
        "|https://github.com/sujanto-gaws/kelir/pull/457|",
        "| 1 | The chart | Done — [files](https://github.com/sujanto-gaws/kelir/pull/457/files) |",
        "| 1 | The chart | Done — [#457](http://github.com/sujanto-gaws/kelir/pull/457) |",
        "| 1 | The chart | Done — [#457](https://www.github.com/sujanto-gaws/kelir/pull/457) |",
        "| 1 | The chart | Done — [#457](https://github.com/Sujanto-Gaws/Kelir/pull/457) |",
        "| 1 | The chart | Done — [#457](<https://github.com/sujanto-gaws/kelir/pull/457> \"the pull\") |",
        "| 1 | The chart | Done — <https://github.com/sujanto-gaws/kelir/pull/457> |",
        "| 1 | The chart | Done — [#457][k] |\n\n[k]: https://github.com/sujanto-gaws/kelir/pull/457",
    ] {
        assert!(
            drives_a_screen_or_says_it_did_not(&report_whose_scope_status_says(kelir), &driven),
            "a Kelir pull request that added a spec did not satisfy rule 4:\n{kelir}"
        );
    }

    assert!(
        !drives_a_screen_or_says_it_did_not(
            &report_whose_scope_status_says(
                "| 1 | The chart | Done — <!-- https://github.com/sujanto-gaws/kelir/pull/457 --> |"
            ),
            &driven
        ),
        "a commented-out citation is cited to nobody"
    );
}

/// #468 C. **A spec a later commit deleted drives nothing**, so the pull
/// request that added it is not credited with a flow that no longer exists.
#[test]
fn a_spec_no_longer_in_the_tree_is_not_counted() {
    let log =
        "\0feat: a screen (#457)\n\nA\te2e/tests/a-screen.spec.ts\nA\te2e/tests/fixtures.ts\n\
               \0test: probe the rule (#9999)\n\nA\te2e/tests/zz-gone.spec.ts\n\
               \0wip: unsquashed\n\nA\te2e/tests/a-draft.spec.ts\n\
               \0test: the probe goes\n\nD\te2e/tests/zz-gone.spec.ts\n";
    let tree = ["e2e/tests/a-screen.spec.ts", "e2e/tests/a-draft.spec.ts"];

    let attributed = attributed_specs(log, |path| tree.contains(&path));

    assert_eq!(
        attributed,
        vec![
            (None, "e2e/tests/a-draft.spec.ts".to_owned()),
            (Some(457), "e2e/tests/a-screen.spec.ts".to_owned()),
        ],
        "only a spec still in the tree is attributed, and a non-spec file never is"
    );

    // Presence is also asked of the tree, so a deletion the log does not show,
    // such as one made while resolving a merge, cannot keep a spec alive.
    assert!(
        attributed_specs(log, |path| path == "e2e/tests/a-draft.spec.ts")
            .iter()
            .all(|(pull_request, _)| pull_request.is_none())
    );

    let driven: BTreeSet<u32> = attributed
        .into_iter()
        .filter_map(|(pull_request, _)| pull_request)
        .collect();

    assert!(!drives_a_screen_or_says_it_did_not(
        &report_whose_scope_status_says("| 1 | The screen | Done — PR #9999 |"),
        &driven
    ));
    assert!(drives_a_screen_or_says_it_did_not(
        &report_whose_scope_status_says("| 1 | The screen | Done — PR #457 |"),
        &driven
    ));
}

/// #468 C, the gate's second round. **A moved spec is the same flow**, so the
/// pull request that added it keeps its credit and a settled report that cites
/// it stays green (D-71). Moved and then deleted, it is gone; added again
/// after a deletion, it belongs to whoever added it again.
#[test]
fn a_moved_spec_is_still_counted() {
    let added = "\0feat: sign out (#481)\n\nA\te2e/tests/sign-out.spec.ts\n";
    let moved = "\0refactor: group the specs (#600)\n\n\
                 R100\te2e/tests/sign-out.spec.ts\te2e/tests/roles/sign-out.spec.ts\n";
    let deleted = "\0test: drop the roles flow (#601)\n\nD\te2e/tests/roles/sign-out.spec.ts\n";
    let added_again = "\0test: a new roles flow (#700)\n\nA\te2e/tests/roles/sign-out.spec.ts\n";
    let moved_tree = |path: &str| path == "e2e/tests/roles/sign-out.spec.ts";

    assert_eq!(
        attributed_specs(&format!("{added}{moved}"), moved_tree),
        vec![
            (Some(481), "e2e/tests/sign-out.spec.ts".to_owned()),
            (Some(600), "e2e/tests/roles/sign-out.spec.ts".to_owned()),
        ],
        "a moved spec keeps the pull request that added it"
    );

    assert!(
        attributed_specs(&format!("{added}{moved}{deleted}"), moved_tree).is_empty(),
        "a spec moved and then deleted drives nothing"
    );

    assert_eq!(
        attributed_specs(&format!("{added}{moved}{deleted}{added_again}"), moved_tree),
        vec![(Some(700), "e2e/tests/roles/sign-out.spec.ts".to_owned())],
        "a path added again after a deletion does not revive the first pull request"
    );

    assert!(
        attributed_specs(
            &format!(
                "{added}\0refactor: not a spec any more (#602)\n\n\
                 R090\te2e/tests/sign-out.spec.ts\te2e/tests/sign-out.ts\n"
            ),
            |_| true
        )
        .is_empty(),
        "a spec renamed to something that is not a spec drives nothing"
    );
}

#[test]
fn without_comments_drops_each_comment_and_an_unclosed_one_to_the_end() {
    assert_eq!(without_comments("a<!-- b -->c<!-- d"), "ac");
    assert_eq!(without_comments("no comment"), "no comment");
}
