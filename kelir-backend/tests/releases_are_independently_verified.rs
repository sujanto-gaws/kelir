//! A release does not tag without a pass that read the work in it
//! ([#392](https://github.com/sujanto-gaws/kelir/issues/392), Sprint 16 item 1).
//!
//! [Sprint plan](../../projects/planning/01.%20Sprint%20Plan.md) §2 has said
//! since 2026-09-04 that the project manager dispatches an independent pass at
//! mid-sprint refinement and that it does not wait to be asked. **It has never
//! once happened on that rule.** Eight sprints, eight failures, and
//! [retrospective 12](../../projects/retrospectives/12.%20Sprint%2014%20Retrospective.md)
//! withdrew permission to restate it a sixth time.
//!
//! `sprint_reports_are_verified.rs` was Sprint 15's answer and it works exactly
//! as designed: a report now either cites a record or says `author-verified`.
//! **What it cannot do is make the pass run**, and it says so in its own module
//! doc. That is not a defect in it — the thing it checks is a record, and the
//! missing thing is an act.
//!
//! # Why this one can
//!
//! **Every control this project keeps enforces an artefact that already
//! exists.** A pass that never runs produces none, so no test of that shape
//! reaches it. This test does not try: it attaches the requirement to something
//! the project cannot skip and already wants — **the tag**.
//!
//! [Sprint plan](../../projects/planning/01.%20Sprint%20Plan.md) §2's last
//! bullet: *every phase ends with a tagged release*. A release is written down
//! as a record in `projects/releases/`, and a record goes `Final` when the
//! release is done. **So the gate is: a `Final` release record cites a
//! verification record, and at least one it cites is new.**
//!
//! The cost is stated rather than hidden: **this fires once per phase, not once
//! per sprint**, which is less than §2 asks for and more than §2 has ever got.
//! §2 is corrected in the same change to say so, because a rule that keeps
//! claiming the per-sprint dispatch would be the ninth restatement.
//!
//! # The three rules
//!
//! 1. **A governed release record cites at least one verification record.**
//! 2. **At least one record it cites is cited by no earlier release.** Rule 1
//!    alone is satisfied by pointing at a pass from three phases ago; this is
//!    what makes each release bring a reading of its own.
//! 3. **The walk finds what exists and governs something**, because a walk over
//!    nothing passes every assertion above it.
//!
//! # The floor, and why it is not the next release
//!
//! [`FIRST_GOVERNED_RELEASE`] is `v0.3.0` rather than `v0.7.0`, and the
//! difference matters: a floor at the next release would govern **zero**
//! existing records, so the test could only ever be red against something
//! planted. At `v0.3.0` it governs four, and they pass — the practice has been
//! real since Sprint 6 and what was missing was the check. `v0.1.0` and
//! `v0.2.0` cite nothing and predate any verification record.
//!
//! # What this deliberately does not check
//!
//! **Whether the pass actually ran, or was any good.** Both are
//! `sprint_reports_are_verified.rs`'s limits too, for its reasons: §2's own
//! criterion needs `git log`, which CI checks out at depth 1, and judging a
//! record is what the record is for.
//!
//! **Whether the cited record read *this* release's work.** Rule 2 makes it
//! new, not relevant. Matching a release to the sprints inside it needs a
//! mapping no file carries, and a test that guessed it would fail on the
//! project's real shape — record 09 is *Sprint 11* and record 13 is *Sprint
//! 13*, numbered independently of both releases and sprints.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Three mutations, run 2026-09-09, all three red — recorded in the pull
//! request that adds this file.
//!
//! **Three more on 2026-09-10, when the detector was hardened (#406):**
//!
//! - `says_final` reverted to the body substring → `a_record_that_only_quotes_
//!   the_header_is_not_governed` red. This is the pre-#406 behaviour, so the
//!   mutation is the defect itself rather than a stand-in for it.
//! - The `**Status:**` line deleted from record 05 → `every_release_record_
//!   states_a_status_in_its_header` red, **and rule 3 red with it**: taking a
//!   record out of the gate drops the governed count below its floor.
//! - `says_final` accepts any status the header states → the same regression
//!   test red, which is what pins `Draft` from `Final` rather than merely
//!   pinning *has a header*.
//!
//! **One came back green and is recorded rather than replaced**: removing the
//! `trim_start` after the label changed no answer, because the return already
//! trims. The dead call is gone and the note sits on [`header_status`].

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// The first release governed, as `(major, minor, patch)`.
///
/// `v0.3.0` is the first release whose record cites a verification record, and
/// every release since has cited one no earlier release did. **Lowering this is
/// a claim that `v0.1.0` or `v0.2.0` should have cited a record that did not
/// exist when they were tagged.**
const FIRST_GOVERNED_RELEASE: (u32, u32, u32) = (0, 3, 0);

/// A record is governed once it says the release happened.
///
/// **A record is `Draft` while the run is in flight and `Final` when it is
/// done**, so governing `Final` alone is what makes this a gate rather than an
/// obstruction: the checklist can be worked through with the pass still
/// outstanding, and the record cannot be closed until it is not.
const FINAL: &str = "Final";

/// The label a record's header status line opens with.
const STATUS_LABEL: &str = "**Status:**";

/// A release as the walks below carry it: version, file name, and the
/// verification records it cites.
type Release = ((u32, u32, u32), String, BTreeSet<String>);

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

/// The version a release file name states, from the `Release vX.Y.Z` in it.
fn version_in(name: &str) -> Option<(u32, u32, u32)> {
    let at = name.find("Release v")? + "Release v".len();
    let rest = &name[at..];
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;

    Some((major, minor, patch))
}

/// The status a record's **header line** states, if it states one.
///
/// # Why the line rather than the body
///
/// This asked whether the body *contained* `**Status:** Final` until
/// [#406](https://github.com/sujanto-gaws/kelir/pull/406). A record that quoted
/// that string in prose — as every copy of the checklist template did, in the
/// closing line telling the reader to set it — **enrolled itself in this gate
/// before the release happened**, and then failed rule 2 against citations it
/// was only listing. [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md)
/// is where that was found, on the first record copied since this file landed.
///
/// **The template escaped and its copies did not**, because [`version_in`]
/// reads a version out of the file name and the template has none. So the
/// defect was invisible in the file that carried it.
///
/// # What matching the line buys
///
/// A record can now say anything it likes about the header — quote the rule,
/// explain the gate, carry a note like this one — and only the line that *is*
/// the header decides. A blockquoted mention (`> **Status:** Final`) does not
/// match either, because the marker must open the trimmed line and `>` is not
/// trimmed away.
///
/// The first matching line is the header by construction: it sits under the
/// title, above everything a record has to say.
///
/// **The label is not trimmed after stripping and does not need to be**: the
/// return trims both ends, which a mutation found by coming back green — the
/// inner `trim_start` it removed changed no answer, because it was dead. That
/// is the finding §2.9 asks to be recorded rather than tidied away.
fn header_status(body: &str) -> Option<&str> {
    let line = body
        .lines()
        .find(|line| line.trim_start().starts_with(STATUS_LABEL))?;

    let rest = &line.trim_start()[STATUS_LABEL.len()..];
    let end = rest.find('\u{b7}').unwrap_or(rest.len());

    Some(rest[..end].trim())
}

/// Whether a record's header says the release is done.
fn says_final(body: &str) -> bool {
    header_status(body) == Some(FINAL)
}

/// `projects/releases/NN. Release vX.Y.Z.md`, oldest first.
///
/// `00. Release Checklist Template.md` states no version and is skipped by the
/// same rule that skips a verification record naming no sprint.
fn release_records() -> Vec<((u32, u32, u32), String, String)> {
    let directory = repository_root().join("projects/releases");
    let mut records = Vec::new();

    for entry in fs::read_dir(&directory).expect("the releases directory is readable") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();

        let Some(version) = version_in(&name) else {
            continue;
        };

        let body = fs::read_to_string(&path).expect("the release record is readable");

        records.push((version, name, body));
    }

    records.sort_by_key(|record| record.0);
    records
}

/// Every `projects/verifications/…` path a record links to.
///
/// The links are percent-encoded relative paths (`../verifications/13.%20Sprint%2013%20Independent%20Pass.md`),
/// so the file name is taken verbatim as the identity rather than decoded — two
/// spellings of one path would be two citations, and there is only ever one
/// spelling because the links are written by copying.
fn citations(body: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let marker = "verifications/";

    for (at, _) in body.match_indices(marker) {
        let rest = &body[at + marker.len()..];
        let end = rest.find([')', ' ', '\n']).unwrap_or(rest.len());
        let cited = &rest[..end];

        if cited.ends_with(".md") {
            found.insert(cited.to_owned());
        }
    }

    found
}

fn governed() -> Vec<Release> {
    release_records()
        .into_iter()
        .filter(|(version, _, body)| *version >= FIRST_GOVERNED_RELEASE && says_final(body))
        .map(|(version, name, body)| (version, name, citations(&body)))
        .collect()
}

/// Rule 1. **A release that shipped says which pass read it.**
#[test]
fn a_governed_release_cites_a_verification_record() {
    let silent: Vec<_> = governed()
        .into_iter()
        .filter(|(.., cited)| cited.is_empty())
        .map(|(_, name, _)| name)
        .collect();

    assert!(
        silent.is_empty(),
        "a Final release record cites no verification record (sprint plan §2, #392):\n  {}\n\n\
         Add the record the release rests on to projects/verifications/ and link it from the \
         Pre-flight table's `Independent pass` row. A release record goes Final when the \
         release is done, and it is not done while nobody outside the work has read it.",
        silent.join("\n  ")
    );
}

/// Rule 2, and the whole point: **each release brings a reading of its own.**
///
/// Without it, rule 1 is satisfied for ever by citing the same record — which
/// is the failure mode of every version of this rule so far: a control that can
/// be discharged by pointing at something that already happened.
#[test]
fn each_release_brings_a_pass_no_earlier_release_cited() {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut recycled = Vec::new();

    for (_, name, cited) in governed() {
        if !cited.is_empty() && cited.iter().all(|record| seen.contains(record)) {
            recycled.push(format!(
                "{name} — cites only {}",
                cited.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        seen.extend(cited);
    }

    assert!(
        recycled.is_empty(),
        "a release cites only verification records an earlier release already cited \
         (sprint plan §2, #392):\n  {}\n\n\
         Every record it names was written for work that shipped before it, so nothing \
         read this release. Dispatch a pass and cite it.",
        recycled.join("\n  ")
    );
}

/// Rule 3. **A walk that finds nothing passes every assertion under it**, and a
/// floor above the last release is the same failure wearing a constant.
#[test]
fn the_walk_finds_what_exists_and_governs_something() {
    let all = release_records();

    assert!(
        all.len() >= 6,
        "the walk found {} release records, which is too few — projects/releases/ moved, \
         or its names stopped saying 'Release vX.Y.Z'",
        all.len()
    );

    let governed = governed();

    assert!(
        governed.len() >= 4,
        "the walk governs {} releases, which is too few — either the floor rose or Final \
         records stopped opening with a {STATUS_LABEL:?} line saying {FINAL:?}",
        governed.len()
    );

    let cited: BTreeSet<_> = governed
        .iter()
        .flat_map(|(.., records)| records.iter().cloned())
        .collect();

    assert!(
        cited.len() >= 4,
        "the governed releases cite {} distinct verification records between them, which is \
         too few for rule 2 to be asserting anything",
        cited.len()
    );
}

/// Rule 4. **Every release record states a status in its header**, so the gate
/// cannot be escaped by omitting the line it reads.
///
/// The detector answers `None` for a record with no `**Status:**` line, and a
/// `None` is ungoverned — which is right for a record still being drafted and
/// wrong as a way out. **Before this rule, deleting one line took a shipped
/// release out of the gate**, and the old body-substring detector had the same
/// hole in the same shape. This is the rule that makes the header line
/// mandatory rather than merely read.
#[test]
fn every_release_record_states_a_status_in_its_header() {
    let silent: Vec<_> = release_records()
        .into_iter()
        .filter(|(.., body)| header_status(body).is_none())
        .map(|(_, name, _)| name)
        .collect();

    assert!(
        silent.is_empty(),
        "a release record states no status in its header (#392, #406):\n  {}\n\n\
         The header line under the title reads `{STATUS_LABEL} <status>`, and this walk \
         reads it to decide whether the record is governed. A record without one is \
         governed by nothing.",
        silent.join("\n  ")
    );
}

/// **A record may say what it likes about the header; only the header decides.**
///
/// This is the regression test for the defect [#406](https://github.com/sujanto-gaws/kelir/pull/406)
/// found: the checklist template's closing line quoted the header string, so
/// every record copied from it enrolled itself in this gate while still being
/// drafted. The template escaped its own trap because it carries no version in
/// its file name; the copies did not.
///
/// Synthetic bodies rather than files, because the point is the detector's
/// rule and not any record's present contents — a record edited tomorrow must
/// not be able to quietly retire this.
#[test]
fn a_record_that_only_quotes_the_header_is_not_governed() {
    let drafting = "\
# Release v9.9.9 — 2099-01-01\n\
\n\
**Status:** Draft · **Last updated:** 2099-01-01\n\
\n\
Set this document's header to `**Status:** Final` when the release is verified.\n\
\n\
> The gate reads `**Status:** Final` and refuses a record citing nothing new.\n";

    assert_eq!(
        header_status(drafting),
        Some("Draft"),
        "the header line decides, and this record's header says Draft"
    );
    assert!(
        !says_final(drafting),
        "a Draft record quoting the header string in prose enrolled itself in the gate \
         before #406 — that is the defect this test exists to keep closed"
    );

    let shipped = "\
# Release v9.9.9 — 2099-01-01\n\
\n\
**Status:** Final · **Last updated:** 2099-01-01\n\
\n\
Nothing here quotes the header at all.\n";

    assert!(
        says_final(shipped),
        "a record whose header says Final is governed — the fix must not have narrowed \
         the gate to nothing"
    );

    assert_eq!(
        header_status("# Release v9.9.9\n\nNo status line at all.\n"),
        None,
        "a record with no header status line states no status, which rule 4 refuses"
    );

    assert_eq!(
        header_status("**Status:** Final\n"),
        Some("Final"),
        "a status line with no trailing separator still states its status"
    );
}

/// The releases below the floor are named rather than implied by it.
///
/// `v0.1.0` and `v0.2.0` cite nothing, and the exemption is finite: it cannot
/// grow, because every release from the floor on is governed, and if it
/// *shrinks* somebody has edited a settled record.
#[test]
fn the_pre_rule_releases_are_named_rather_than_silently_skipped() {
    let ungoverned: Vec<_> = release_records()
        .into_iter()
        .filter(|(version, ..)| *version < FIRST_GOVERNED_RELEASE)
        .map(|(version, name, body)| (version, name, citations(&body)))
        .collect();

    assert_eq!(
        ungoverned.len(),
        2,
        "the set of releases below the floor is expected to be exactly v0.1.0 and v0.2.0, \
         and the walk found {}",
        ungoverned.len()
    );

    for (version, name, cited) in ungoverned {
        assert!(
            cited.is_empty(),
            "{name} (v{}.{}.{}) is below the floor and cites {} verification record(s) — \
             if a pre-rule release did cite one, the floor is wrong rather than the record",
            version.0,
            version.1,
            version.2,
            cited.len()
        );
    }
}
