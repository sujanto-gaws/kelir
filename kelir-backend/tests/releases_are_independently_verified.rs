//! A release does not tag without a pass that read the work in it
//! ([#392](https://github.com/sujanto-gaws/kelir/issues/392), Sprint 16 item 1;
//! hardened by [#412](https://github.com/sujanto-gaws/kelir/issues/412), Sprint
//! 17 item 2).
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
//! verification record, and at least one it cites is new** — written, since
//! [#412](https://github.com/sujanto-gaws/kelir/issues/412), *after every
//! record any earlier release cited*, which is a stronger reading of *new* than
//! the one this file shipped with and the reason rule 7 exists.
//!
//! The cost is stated rather than hidden: **this fires once per phase, not once
//! per sprint**, which is less than §2 asks for and more than §2 has ever got.
//! §2 is corrected in the same change to say so, because a rule that keeps
//! claiming the per-sprint dispatch would be the ninth restatement.
//!
//! # The rules
//!
//! Rules 1–4 are the gate as [#392](https://github.com/sujanto-gaws/kelir/issues/392)
//! and [#406](https://github.com/sujanto-gaws/kelir/pull/406) left it. Rules
//! 5–9 are [#412](https://github.com/sujanto-gaws/kelir/issues/412), and each
//! one shuts a door the [Sprint 16 independent pass](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
//! walked through with a synthetic record while the suite stayed green.
//!
//! 1. **A governed release record cites at least one verification record.**
//! 2. **At least one record it cites is cited by no earlier release.** Rule 1
//!    alone is satisfied by pointing at a pass from three phases ago; this is
//!    what makes each release bring a reading of its own.
//! 3. **The walk finds what exists and governs something**, because a walk over
//!    nothing passes every assertion above it.
//! 4. **Every release record states a status in its header**, so the gate
//!    cannot be escaped by deleting the line it reads.
//! 5. **Every file in `projects/releases/` is the template or a record in the
//!    house shape.** Door B: a name the walk cannot parse is governed by
//!    nothing *and nothing notices it is ungoverned*.
//! 6. **Every verification record a release cites exists on disk.** Door A: the
//!    citations are hand-typed percent-encoded relative paths, and a typo in
//!    one used to read as a new pass.
//! 7. **A governed release cites a record written after every record cited
//!    before it.** Finding 7: rule 2's pool of never-cited records grows with
//!    every sprint, and eight of them were old enough to have read nothing in
//!    the release naming them.
//! 8. **A release record's header status is a word the process defines.** Door
//!    C: a status the detector could not parse left the record ungoverned
//!    rather than red.
//! 9. **A release record states its status once.** Finding 8: the template's
//!    own header and the specimen header it carries are two `**Status:**`
//!    lines, so a verbatim copy was decided by the wrong one.
//!
//! Rule 10 is [#445](https://github.com/sujanto-gaws/kelir/issues/445), Sprint
//! 18 item 3, and it is the first rule here that reads a record's *body* rather
//! than its header, its name or its citations.
//!
//! 10. **A `Final` record names an issue for every follow-up its Aftermath
//!     filed**, or says `none`. [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md)
//!     went `Final` carrying a **reproduced** defect under the words
//!     `not yet filed` and stayed that way for four days, because nothing reads
//!     an Aftermath list. The row had to **open** with its issue link: the bad
//!     row cited the guard that was bypassed and the row above it cited another
//!     issue for context, so *mentions an issue* would have passed it. Governed
//!     from [`FIRST_AFTERMATH_GOVERNED_RELEASE`] rather than from the floor
//!     above, because the section shape is younger than most of the records.
//!
//! # The floor, and why it is not the next release
//!
//! [`FIRST_GOVERNED_RELEASE`] is `v0.3.0` rather than `v0.7.0`, and the
//! difference matters: a floor at the next release would govern **zero**
//! existing records, so the test could only ever be red against something
//! planted. At `v0.3.0` it governs five, and they pass — the practice has been
//! real since Sprint 6 and what was missing was the check. `v0.1.0` and
//! `v0.2.0` cite nothing and predate any verification record.
//!
//! # What a record can still do that this file will not notice
//!
//! Stated rather than implied, because a control whose edge is undrawn gets
//! trusted past it — which is how [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
//! found three doors in a file that had already been hardened twice.
//!
//! - **Claim a pass that never ran.** Every rule here reads records. The act is
//!   `sprint_reports_are_verified.rs`'s limit too, for the same reason: §2's
//!   own independence criterion needs `git log`, and CI cloned at depth 1
//!   ([finding 4](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)).
//!   Since [#453](https://github.com/sujanto-gaws/kelir/issues/453) the backend
//!   job fetches full history; reading a named reader's absence from trailers
//!   is still not asked, and sprint plan §2 has said so since
//!   [#448](https://github.com/sujanto-gaws/kelir/issues/448).
//! - **Cite a record that is new, resolves, is numbered above the high-water
//!   mark, and read none of this release's work.** Rule 7 sharpens rule 2's
//!   *new* and does not make it *relevant*. Matching a release to the sprints
//!   inside it needs a mapping no file carries, and a test that guessed it
//!   would fail on the project's real shape — record 09 is *Sprint 11* and
//!   record 13 is *Sprint 13*, numbered independently of both releases and
//!   sprints.
//! - **Cite a verification record that is empty, or is about something else.**
//!   Rule 6 resolves the path; judging the document is what the document is
//!   for.
//! - **Go `Final` without the tag ever being pushed.** The gate reads
//!   `projects/releases/`, not `git tag`.
//! - **Renumber itself.** The walk sorts by the version in the name, not by the
//!   `NN.` prefix, so `08. Release v0.7.1.md` landing before `07.` changes no
//!   answer. Rule 5 requires the prefix to be two digits; it does not require
//!   the series to be dense or ordered.
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
//!
//! **Five synthetic records on 2026-09-12, one per new rule (#412).** Each was
//! written into `projects/releases/` alone, run against
//! `cargo test --test releases_are_independently_verified`, and deleted. **The
//! positive control ran last** — [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)'s
//! own method, turned around to prove the doors are shut. Baseline first: **12
//! passed, nothing planted.**
//!
//! | The record | Red |
//! |---|---|
//! | **A** — `Final`, house shape, citing `99.%20A%20Pass%20That%20Was%20Never%20Written.md` | `a_cited_verification_record_exists` |
//! | **B** — the `v` dropped (`09. Release 9.9.10.md`), `Final`, citing nothing | `every_file_in_the_releases_directory_is_a_record_or_the_template` |
//! | **C** — the `·` separator replaced by a pipe, `Final`, citing nothing | `a_release_record_states_a_status_the_process_defines` |
//! | **D** — `Final`, house shape, citing record 01 and nothing newer | `each_release_cites_a_record_written_after_every_earlier_citation` |
//! | **E** — the template copied byte for byte, specimen header set to `Final` | `a_release_record_states_a_status_the_process_defines` **and** `a_release_record_states_its_status_once` |
//! | **Control**, last — `Final`, house shape, citing a record numbered 16 | **none. 12 passed** |
//!
//! **Each of A–D reddened its own rule and no other**, which is what says the
//! rules are independent rather than one rule wearing five names. **E reddened
//! two**, and that is recorded rather than tuned away: finding 8 offered two
//! remedies and this change took both, so a template copied whole is refused
//! for its status *and* for having two of them. Either alone would shut the
//! door; the pair is what keeps it shut if the template's preamble is ever
//! reworded.
//!
//! **The control is what makes the other six mean something.** It is `Final`,
//! in the house shape, cites a record above the high-water mark — a synthetic
//! `16. A Probe Pass.md`, removed with it — and passes all twelve. A gate that
//! refused it would be refusing releases rather than governing them.
//!
//! # Rule 10, seen red 2026-09-14 (#445)
//!
//! **Against the record that earned it, not against a stand-in.**
//! [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md)'s rollback row
//! was restored to its wording at `00a27b4` — `**not yet filed**`, where it
//! stood for four days — and `a_final_record_names_an_issue_for_every_follow_up`
//! went red naming the record and quoting the row. **The other twelve rules
//! stayed green**, so the mutation landed on the line rule 10 claims to cover
//! rather than somewhere beneath it. The record was restored byte-exact
//! afterwards; [#445](https://github.com/sujanto-gaws/kelir/issues/445) does
//! not amend it.
//!
//! **Six probes the same day, run against rule 10 alone and deleted, positive
//! control last.** Alone is deliberate: a synthetic `v0.8.0` cites no
//! verification record, so rules 1, 2, 6 and 7 would refuse it for reasons that
//! have nothing to do with what is being probed — and a probe that reddens for
//! the wrong reason is [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
//! §7's finding wearing a different hat.
//!
//! | The Aftermath | Gate |
//! |---|---|
//! | A row that **contains** an issue link but does not open with one | **refused** |
//! | A row opening with a `/pull/` link rather than `/issues/` | **refused** |
//! | No `- **Follow-ups filed:**` line at all | **refused** |
//! | The label promises follow-ups and no rows follow it | **refused** |
//! | A withdrawn follow-up opening with a struck-through issue link | accepted |
//! | **Control**, last — a row opening with an issue link | accepted |
//!
//! **The first probe is the one that matters.** It is the shape record 07's bad
//! row actually had — prose first, a link later — and a rule asking *does this
//! row mention an issue* would have accepted it. The two accepted rows are what
//! stop the rule from being *refuses everything*: one of them is a real shape
//! [record 01](../../projects/releases/01.%20Release%20v0.1.0.md) already uses.

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

/// The first release whose Aftermath is governed by rule 10, as
/// `(major, minor, patch)`.
///
/// **Higher than [`FIRST_GOVERNED_RELEASE`], and it has to be.** Rule 10 reads
/// a section shape that did not exist for most of this project's life:
///
/// - `05` has **no Aftermath section at all**.
/// - `04`'s Aftermath is a numbered list of prose findings, no issue links.
/// - `02` and `03` put their follow-ups inline on the `- **Follow-ups filed:**`
///   line, with no rows beneath it.
/// - `01` and `06` have rows, and most of them open with bold prose rather than
///   with an issue — `**A frontend image has no version identity.**` is a
///   follow-up that became [#362](https://github.com/sujanto-gaws/kelir/issues/362)
///   months later, and the row never learned.
///
/// **The template's own rule is that a settled record is not amended** — it
/// says so twice, about record 06 and about the status line — so a floor low
/// enough to reach those records would demand exactly the edit the house
/// forbids, and would be red on the tree as it stands rather than on anything
/// anybody did wrong.
///
/// `v0.7.0` is the first record written after
/// [#382](https://github.com/sujanto-gaws/kelir/pull/382) put *reproduced or
/// only observed* in the template, which is the convention this rule extends.
///
/// # The honest cost: this governs one record today
///
/// The module doc above says a floor at the next release would govern **zero**
/// records and could only ever be red against something planted. **One is not
/// zero, and the difference is the whole argument for this floor:** record 07
/// is the case that earned the rule, it was **red** at `00a27b4` where the row
/// read `not yet filed`, and it is **green** now that the row cites
/// [#440](https://github.com/sujanto-gaws/kelir/issues/440). A real red and a
/// real green on a real record, rather than a synthetic pair.
///
/// **Raising this is a claim that a settled record should be edited. Lowering
/// it is the same claim about an older one.**
const FIRST_AFTERMATH_GOVERNED_RELEASE: (u32, u32, u32) = (0, 7, 0);

/// The Aftermath heading rule 10 looks for.
const AFTERMATH_HEADING: &str = "## Aftermath";

/// The line beneath it that rule 10 reads, and the rows it governs are the ones
/// indented under this one.
const FOLLOW_UPS_LABEL: &str = "- **Follow-ups filed:**";

/// The prefix of an issue link, as the records write it.
///
/// **`/issues/` and not `/pull/`, deliberately.** A follow-up is an issue
/// somebody can be assigned and can close; a pull request is the change that
/// closed one. Record 07's own bad row cited
/// [#367](https://github.com/sujanto-gaws/kelir/pull/367) — *the guard that was
/// bypassed* — which is a useful thing to say and is not a filed follow-up.
const ISSUE_URL: &str = "https://github.com/sujanto-gaws/kelir/issues/";

/// A record is governed once it says the release happened.
///
/// **A record is `Draft` while the run is in flight and `Final` when it is
/// done**, so governing `Final` alone is what makes this a gate rather than an
/// obstruction: the checklist can be worked through with the pass still
/// outstanding, and the record cannot be closed until it is not.
const FINAL: &str = "Final";

/// Every status a release record's header may state.
///
/// # Why a vocabulary, and why these two words
///
/// Before [#412](https://github.com/sujanto-gaws/kelir/issues/412) the detector
/// read whatever the header said and compared it to `Final`. **Anything else
/// was ungoverned rather than wrong**, so a record that replaced the `·`
/// separator with a pipe left `Final | **Last updated:** …` in the status, did
/// not equal `Final`, and shipped outside the gate with every rule green. That
/// is door C of [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3, and rule 8 is the answer: a status this file cannot recognise is
/// a failure naming the record, not a free pass.
///
/// **The words are `Draft` and `Final`, and somebody had to choose.** The
/// checklist template said three different things at once — its own header read
/// `Template`, the specimen header it hands every copy read `In progress`, and
/// its note under the Pre-flight table said *`Draft` is where a release run
/// lives while it is worked*. [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md)
/// used `Draft`. **`In progress` was borrowed from the wrong vocabulary**: it
/// is [sprint plan](../../projects/planning/01.%20Sprint%20Plan.md) §2's word
/// for a sprint item whose author is also its only verifier, which is a
/// judgement about a piece of work rather than a stage of a release run. The
/// template was corrected to the note it already carried, in the change that
/// added this rule.
///
/// `Template` is deliberately outside the vocabulary. The template file itself
/// is never walked — rule 5 exempts it by name — so the only file this can
/// refuse is a *copy* that kept the preamble, which is exactly finding 8.
const RECORD_STATUSES: [&str; 2] = ["Draft", FINAL];

/// The label a record's header status line opens with.
const STATUS_LABEL: &str = "**Status:**";

/// The one file in `projects/releases/` that is not a release record.
const RELEASE_TEMPLATE: &str = "00. Release Checklist Template.md";

/// Where the records a release cites are looked for.
const VERIFICATIONS: &str = "projects/verifications";

/// A release as the walks below carry it: version, file name, and the
/// verification records it cites.
type Release = ((u32, u32, u32), String, BTreeSet<String>);

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

/// The version a release record's file name states, if the name is in the
/// house shape `NN. Release vX.Y.Z.md`.
///
/// # Why this is strict
///
/// It searched the name for the substring `Release v` until
/// [#412](https://github.com/sujanto-gaws/kelir/issues/412), and anything that
/// did not contain it answered `None` — which the walk read as *this file is
/// not a release record* and skipped in silence. **`09. Release 9.9.10.md`,
/// with the `v` dropped, was therefore in no walk at all**: not in
/// [`release_records`], so not in rules 1, 2 or 3, and not in rule 4 either, so
/// nothing observed that a release record had escaped every rule. That is door
/// B of [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3.
///
/// `None` still means *skip*; what changed is that rule 5 walks the directory
/// separately and refuses anything it finds that is neither the template nor a
/// name this function accepts. **A `None` is now a failure somewhere**, which
/// is the property door B exploited the absence of.
fn release_version(name: &str) -> Option<(u32, u32, u32)> {
    let stem = name.strip_suffix(".md")?;
    let (number, title) = stem.split_once(". ")?;

    if number.len() != 2 || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let digits = title.strip_prefix("Release v")?;
    let mut parts = digits.split('.');

    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;

    if parts.next().is_some() {
        return None;
    }

    Some((major, minor, patch))
}

/// The `NN` a numbered record's file name opens with.
///
/// Both folders number their records in the order they are written, which is
/// what rule 7 compares. The citation is percent-encoded and the number is in
/// front of the first encoded space, so this reads the same answer off
/// `15.%20Sprint%2016%20Independent%20Pass.md` and
/// `15. Sprint 16 Independent Pass.md`.
fn record_number(name: &str) -> Option<u32> {
    let (number, _) = name.split_once('.')?;

    if number.len() != 2 || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    number.parse().ok()
}

/// One hexadecimal digit as a number.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A percent-encoded link target as the file name it points at.
///
/// The links in the records are written by copying a path out of the editor,
/// so the spaces arrive as `%20` and nothing else is ever encoded. This decodes
/// any `%XX` all the same, and leaves a stray `%` alone rather than failing —
/// **a citation this cannot decode should be reported by rule 6 as a file that
/// does not exist, which names the path, rather than by a panic here, which
/// names this function.**
///
/// Bytes rather than `str` slicing on purpose: `&path[at + 1..at + 3]` panics
/// when the `%` is followed by a multi-byte character, and a malformed citation
/// is the one input this is guaranteed to meet.
fn percent_decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex_digit(bytes[at + 1]), hex_digit(bytes[at + 2])) {
                out.push(high * 16 + low);
                at += 3;
                continue;
            }
        }

        out.push(bytes[at]);
        at += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
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
/// **The template escaped and its copies did not**, because [`release_version`]
/// reads a version out of the file name and the template has none. So the
/// defect was invisible in the file that carried it.
///
/// # What matching the line buys
///
/// A record can now say anything it likes about the header — quote the rule,
/// explain the gate, carry a note like this one — and only the line that *is*
/// the header decides. A blockquoted mention (`> **Status:** Final`) does not
/// match either, because the marker must open the trimmed line and `>` is not
/// trimmed away. **That claim is now asserted rather than only written down**
/// ([finding 9](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)):
/// a mutation that also stripped a leading `>` left all six tests green,
/// because the regression test's blockquoted line failed the label match with
/// or without it.
///
/// # "The first matching line is the header" is now a rule rather than an
/// assumption
///
/// It used to be justified by construction — the header sits under the title,
/// above everything a record has to say. **A verbatim copy of the checklist
/// template has two**, its own and the specimen's, and this read the wrong one:
/// finding 8. [Rule 9](crate) refuses a record with more than one, so taking
/// the first is now safe because there is only ever one to take.
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

/// How many lines of a record open with the header label.
fn header_status_lines(body: &str) -> usize {
    body.lines()
        .filter(|line| line.trim_start().starts_with(STATUS_LABEL))
        .count()
}

/// Whether a record's header says the release is done.
fn says_final(body: &str) -> bool {
    header_status(body) == Some(FINAL)
}

/// Every name `projects/releases/` holds, the template included.
fn release_directory() -> Vec<String> {
    let directory = repository_root().join("projects/releases");

    fs::read_dir(&directory)
        .expect("the releases directory is readable")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// `projects/releases/NN. Release vX.Y.Z.md`, oldest first.
///
/// `00. Release Checklist Template.md` states no version and is skipped here;
/// **rule 5 is what makes that an exemption rather than a hole**, by naming the
/// template as the only file in the directory allowed to be skipped.
fn release_records() -> Vec<((u32, u32, u32), String, String)> {
    let directory = repository_root().join("projects/releases");
    let mut records = Vec::new();

    for name in release_directory() {
        let Some(version) = release_version(&name) else {
            continue;
        };

        let body =
            fs::read_to_string(directory.join(&name)).expect("the release record is readable");

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
///
/// # Why this still does not check that the file exists
///
/// [#412](https://github.com/sujanto-gaws/kelir/issues/412) asked for a
/// citation that does not resolve to be an error, and the tempting shape was to
/// drop it here. **That would have made a typo look like a different defect**:
/// a record whose one citation is misspelt would arrive at rule 1 citing
/// nothing, and rule 1's message tells the reader to dispatch a pass when the
/// pass ran and the link is wrong. So extraction stays verbatim and
/// [`a_cited_verification_record_exists`] does the resolving, with a message
/// that names the record and the path it could not find.
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

/// The Aftermath's follow-up block: the `- **Follow-ups filed:**` line and the
/// rows indented under it.
///
/// `None` means the record has no such line inside an `## Aftermath` section —
/// which rule 10 treats as a failure rather than an exemption, for door B's
/// reason: **a shape the walk cannot find must not be a shape the walk
/// ignores.**
///
/// The rows end at the first line that is not an indented list item, so the
/// block-quoted note every record carries after its Aftermath is not mistaken
/// for a follow-up.
fn follow_up_block(body: &str) -> Option<(String, Vec<String>)> {
    let after_heading = body.split_once(AFTERMATH_HEADING)?.1;

    // Stop at the next section, so a `- **Follow-ups filed:**` line further
    // down the document cannot stand in for a missing one here.
    let section = match after_heading.split_once("\n## ") {
        Some((section, _)) => section,
        None => after_heading,
    };

    let mut lines = section.lines();
    let label = lines.find(|line| line.trim_start().starts_with(FOLLOW_UPS_LABEL))?;

    let rows = lines
        .take_while(|line| {
            let trimmed = line.trim_start();
            line.starts_with(' ') && (trimmed.starts_with("- ") || trimmed.starts_with("~~"))
        })
        .map(|line| line.trim().to_owned())
        .collect();

    Some((label.trim().to_owned(), rows))
}

/// Whether a follow-up row **opens with** a link to an issue.
///
/// # Why opening with one, rather than containing one
///
/// **Record 07's bad row contained an issue-shaped link and was still the
/// defect this rule exists to catch.** As it stood at `00a27b4` the row read
///
/// ```text
///   - **`deploy.sh`'s rollback command exits 1 …** · **reproduced** ·
///     **not yet filed** · the guard is [#367](…/pull/367)'s `/version.json` assertion
/// ```
///
/// and the row above it in the same list ends `([#335](…/issues/335))`, citing
/// a *different* issue as context. **A rule that asked whether the row
/// mentioned an issue anywhere would pass both**: the first on a pull-request
/// link, the second on somebody else's issue number. What makes a row a filed
/// follow-up is that the issue is the row's subject, and the subject is what a
/// row opens with — which is the shape the template already prints.
///
/// This is the rule's load-bearing half, and it is the half a probe has to
/// attack. [Record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// §7 defeated three earlier probes with one-character edits precisely because
/// they attacked the half that was not carrying anything.
fn row_opens_with_issue_link(row: &str) -> bool {
    let row = row.trim_start().trim_start_matches("- ").trim_start();

    // The row may open with a struck-through link, which is how a withdrawn
    // follow-up is written. The strike is cosmetic and the link beneath it is
    // still the row's subject.
    let row = row.trim_start_matches("~~");

    let Some(rest) = row.strip_prefix('[') else {
        return false;
    };

    // The first `](` closes the link text. Link texts in these rows carry
    // backticks, em dashes and `#NNN`, but never a nested bracket pair.
    let Some((_text, target)) = rest.split_once("](") else {
        return false;
    };

    target.starts_with(ISSUE_URL)
        && target
            .trim_start_matches(ISSUE_URL)
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
}

/// Whether a line says there is nothing to file.
///
/// `none` is the template's own word for it and rule 10 accepts it on the
/// label line, which is how records 02 and 03 already write an empty Aftermath
/// — and how a future release with a clean run will.
fn says_none(line: &str) -> bool {
    line.to_ascii_lowercase().contains("none")
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
///
/// **Rule 7 implies this one** and the rule is kept anyway, because the two
/// report different defects and one of them is far more likely. *Cites only
/// records an earlier release cited* is what a copied Pre-flight row does;
/// *cites a record older than the high-water mark* is what reaching into the
/// uncited pool does. A reader who trips the first should be told the first.
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
///
/// The floors are minimums and are deliberately not raised to the current
/// counts. [Record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3 observed that they are *"exactly slack enough"* to let a planted
/// record through, which was true and is no longer the argument that matters:
/// **door B is shut at the file name by rule 5**, which refuses the planted
/// record outright rather than hoping a count notices it.
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

/// Rule 5. **A file the walk cannot parse is a failure, not an exemption.**
///
/// Door B of [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3: `09. Release 9.9.10.md`, the `v` dropped, was invisible to every
/// rule in this file *including rule 4*, because every one of them walks
/// [`release_records`] and that walk skips what it cannot read a version from.
/// **The record was governed by nothing and nothing said so.**
///
/// This is the only rule here that walks the directory rather than the records,
/// and that is the point: it is the rule that decides what a record *is*, so it
/// cannot ask the record-finder what it found.
///
/// The template is exempt by name rather than by the absence of a version.
/// Naming it is what makes the exemption finite — a second unparseable file
/// cannot join it by accident.
#[test]
fn every_file_in_the_releases_directory_is_a_record_or_the_template() {
    let stray: Vec<_> = release_directory()
        .into_iter()
        .filter(|name| name != RELEASE_TEMPLATE && release_version(name).is_none())
        .collect();

    assert!(
        stray.is_empty(),
        "projects/releases/ holds a file that is neither the template nor a release record \
         (#412, record 15 finding 3 door B):\n  {}\n\n\
         A release record is named `NN. Release vX.Y.Z.md` — two digits, the literal \
         `Release v`, and three dot-separated numbers. A name this walk cannot parse is \
         read by no rule in this file, so the record it holds is governed by nothing. \
         Rename it, or move it out of projects/releases/.",
        stray.join("\n  ")
    );
}

/// Rule 6. **A citation names a file that is there.**
///
/// Door A of [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3: a `Final` record citing
/// `../verifications/99.%20A%20Pass%20That%20Was%20Never%20Written.md` passed
/// every rule, because a novel string satisfied rule 2 by being novel rather
/// than by resolving.
///
/// **This is not a hypothetical evasion; it is the ordinary failure mode of a
/// hand-written link.** [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md)
/// cites record 15 by typing a percent-encoded relative path, and before this
/// rule a typo in it went green — the gate reporting that `v0.7.0` was read by
/// a pass that does not exist.
///
/// Every record is checked rather than only the governed ones. A broken link in
/// a `Draft` record is the same defect one day earlier, and catching it while
/// the run is still open is the whole value.
#[test]
fn a_cited_verification_record_exists() {
    let verifications = repository_root().join(VERIFICATIONS);
    let mut missing = Vec::new();

    for (_, name, body) in release_records() {
        for cited in citations(&body) {
            let path = verifications.join(percent_decoded(&cited));

            if !path.is_file() {
                missing.push(format!(
                    "{name} — cites {cited}, which is not in {VERIFICATIONS}/"
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "a release record cites a verification record that does not exist \
         (#412, record 15 finding 3 door A):\n  {}\n\n\
         The citations are hand-typed percent-encoded relative paths, so this is most \
         often a typo rather than an invention — check the spelling against \
         projects/verifications/. Until it resolves, the gate cannot tell a pass that \
         was written from one that was not.",
        missing.join("\n  ")
    );
}

/// Rule 7. **A release reaches forward for its reading, never back.**
///
/// [Record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// §7 finding 7: `projects/verifications/` holds fifteen records and **eight of
/// them are cited by no release**, so rule 2's *new to this release* was
/// satisfiable by reaching into a pool of old readings. `v0.7.0` could have
/// gone `Final` citing record 12, a Sprint 12 read of work that shipped in
/// `v0.6.0`, with no new reading at all. **Rule 6 does not reach this** — all
/// eight resolve — and the pool grows with every sprint record a release leaves
/// uncited.
///
/// The rule: *a governed release cites a record numbered above every record any
/// earlier release cited.* Records are numbered in the order they are written,
/// so a number above the high-water mark is a record written after the last
/// release read anything. It holds against all five governed releases —
/// `v0.3.0` → 03; `v0.4.0` → 06 > 03; `v0.5.0` → 09, 10 > 06; `v0.6.0` → 13, 14
/// > 10; `v0.7.0` → 15 > 14 — and it refuses the probe, which cites 01.
///
/// A release citing nothing at all is rule 1's, not this one's, so this stays
/// silent about it rather than reporting the same record twice with two
/// remedies.
#[test]
fn each_release_cites_a_record_written_after_every_earlier_citation() {
    let mut high_water: Option<u32> = None;
    let mut backward = Vec::new();

    for (_, name, cited) in governed() {
        let highest = cited
            .iter()
            .filter_map(|record| record_number(record))
            .max();

        let Some(highest) = highest else {
            continue;
        };

        if let Some(floor) = high_water {
            if highest <= floor {
                backward.push(format!(
                    "{name} — its highest citation is record {highest:02}, and releases before \
                     it had already read up to record {floor:02}"
                ));
            }
        }

        high_water = Some(high_water.map_or(highest, |floor| floor.max(highest)));
    }

    assert!(
        backward.is_empty(),
        "a release cites no verification record written since the last release read one \
         (#412, record 15 §7 finding 7):\n  {}\n\n\
         projects/verifications/ is numbered in the order its records are written, so a \
         release whose highest citation is below the high-water mark is resting on a \
         reading that predates the previous release. Rule 2 calls that new because no \
         release had cited it; it is not new to this release. Dispatch a pass and cite it.",
        backward.join("\n  ")
    );
}

/// Rule 8. **A status this file cannot recognise is red, not exempt.**
///
/// Door C of [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// finding 3: [`header_status`] truncates at the `·` separator, so a record
/// using any other separator left the whole rest of the line inside its status.
/// The comparison to `Final` then failed and the record was **ungoverned while
/// rule 4 passed**, because a status line was present. One character, and a
/// shipped release stepped outside the gate.
///
/// It is also the second half of finding 8's remedy. A verbatim copy of the
/// checklist template carries the template's own `**Status:** Template` first,
/// and `Template` is outside the vocabulary on purpose.
#[test]
fn a_release_record_states_a_status_the_process_defines() {
    let unknown: Vec<_> = release_records()
        .into_iter()
        .filter_map(|(_, name, body)| {
            let status = header_status(&body)?;

            (!RECORD_STATUSES.contains(&status)).then(|| format!("{name} — says {status:?}"))
        })
        .collect();

    assert!(
        unknown.is_empty(),
        "a release record's header states a status this gate does not define \
         (#412, record 15 finding 3 door C):\n  {}\n\n\
         A release record is {:?} while the run is in flight and {FINAL:?} when it is done, \
         and nothing else. A status the detector cannot match leaves the record governed by \
         no rule here, which is why an unrecognised one is a failure rather than a skip — \
         and the most likely cause is a separator other than the house `·`, which lands the \
         rest of the header line inside the status.",
        unknown.join("\n  "),
        RECORD_STATUSES[0]
    );
}

/// Rule 9. **A record states its status once, so the first line is the header.**
///
/// [Record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// §7 finding 8: the checklist template's first line says *"Copy this file to
/// `NN. Release vX.Y.Z.md`"*, and **the file has two header status lines** —
/// its own on line 3 and the specimen release's further down. [`header_status`]
/// takes the first. A byte-for-byte copy with the specimen set to `Final` and
/// citing nothing passed all six rules; the same record without the preamble
/// failed rule 1.
///
/// **This door was reached by following the template's instruction rather than
/// by editing anything**, which is why the template was corrected in the same
/// change to say what to copy. This rule is the backstop: the correction stops
/// the next copy being wrong, and this stops a wrong copy being silent.
///
/// It also turns [`header_status`]'s *"the first matching line is the header by
/// construction"* from an assumption into something asserted. Records 03–07 all
/// dropped the preamble, so nothing has gone through this door yet.
#[test]
fn a_release_record_states_its_status_once() {
    let ambiguous: Vec<_> = release_records()
        .into_iter()
        .filter(|(.., body)| header_status_lines(body) > 1)
        .map(|(_, name, body)| format!("{name} — {} header lines", header_status_lines(&body)))
        .collect();

    assert!(
        ambiguous.is_empty(),
        "a release record opens more than one line with {STATUS_LABEL:?} \
         (#412, record 15 §7 finding 8):\n  {}\n\n\
         This gate reads the first such line and calls it the header. Two of them means the \
         record has a header the reader sees and a header the gate reads, and they can \
         disagree. The usual cause is copying `{RELEASE_TEMPLATE}` whole rather than the \
         checklist below its divider — delete the preamble. To quote the header string \
         without stating it, put the line in a blockquote or indent it under a list item.",
        ambiguous.join("\n  ")
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

    // Record 15 finding 9: `header_status`'s doc comment has always claimed a
    // blockquoted mention does not match, and no test sent one — a mutation
    // that also stripped a leading `>` left all six tests green. The
    // blockquoted line above cannot tell the difference, because it fails the
    // label match either way.
    assert_eq!(
        header_status("> **Status:** Final\n"),
        None,
        "a blockquoted header line is a mention rather than a header, which is what lets a \
         record explain this gate without enrolling in it"
    );
}

/// **The names, the versions and the shapes this walk accepts and refuses.**
///
/// Rules 5 and 7 are enforced over the files that happen to be in the
/// repository today, and both would keep passing if [`release_version`] or
/// [`record_number`] quietly went back to being permissive — the present files
/// are all well-named. This pins the parsers themselves, against the three
/// spellings [record 15](../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md)
/// used and the ones next door to them.
#[test]
fn the_parsers_refuse_the_shapes_the_walk_must_not_skip() {
    assert_eq!(
        release_version("07. Release v0.7.0.md"),
        Some((0, 7, 0)),
        "the house shape is accepted, or rule 5 refuses every record at once"
    );

    assert_eq!(
        release_version("09. Release 9.9.10.md"),
        None,
        "door B: the `v` dropped. This must answer None so rule 5 names the file, rather \
         than a version so the record is governed under a name nobody writes"
    );
    assert_eq!(
        release_version("7. Release v0.7.0.md"),
        None,
        "the folder numbers with two digits, and a one-digit prefix sorts wrongly in every \
         listing the project reads"
    );
    assert_eq!(
        release_version("Release v0.7.0.md"),
        None,
        "a record with no number is not in the series"
    );
    assert_eq!(
        release_version("07. Release v0.7.md"),
        None,
        "a version is three parts; `v0.7` names no release this project has tagged"
    );
    assert_eq!(
        release_version("07. Release v0.7.0.0.md"),
        None,
        "four parts is not a version either, and `take_while` used to read the first three \
         and discard the rest in silence"
    );
    assert_eq!(
        release_version(RELEASE_TEMPLATE),
        None,
        "the template states no version — rule 5 exempts it by name, and this is why it \
         needs exempting"
    );

    assert_eq!(
        record_number("15.%20Sprint%2016%20Independent%20Pass.md"),
        Some(15),
        "rule 7 reads the number off the citation as it is written, percent-encoded"
    );
    assert_eq!(
        record_number("15. Sprint 16 Independent Pass.md"),
        Some(15),
        "and off the file name as it is on disk, so the two agree"
    );
    assert_eq!(
        record_number("99.%20A%20Pass%20That%20Was%20Never%20Written.md"),
        Some(99),
        "door A's invention parses — which is the point: rule 7 would let it through, and \
         rule 6 is what refuses it"
    );

    assert_eq!(
        percent_decoded("15.%20Sprint%2016%20Independent%20Pass.md"),
        "15. Sprint 16 Independent Pass.md",
        "rule 6 resolves the link against the disk, so the encoding has to come off"
    );
    assert_eq!(
        percent_decoded("100%.md"),
        "100%.md",
        "a stray percent is left alone rather than panicking, so rule 6 reports the path"
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

/// Rule 10. **A `Final` release record names an issue for every follow-up its
/// Aftermath filed**, or says `none`.
///
/// [Record 07](../../projects/releases/07.%20Release%20v0.7.0.md) went `Final`
/// on 2026-09-12 carrying a **reproduced** defect in the rollback path under
/// the words `not yet filed`, and stayed that way for four days. Nothing
/// noticed, because nothing reads an Aftermath list. It became
/// [#440](https://github.com/sujanto-gaws/kelir/issues/440) only when
/// [retrospective 14](../../projects/retrospectives/14.%20Sprint%2016%20Retrospective.md)
/// went looking at the record — and `v0.6.0` was `v0.7.0`'s only rollback
/// target, so the first operator to follow the script's own instruction after
/// that release would have got a red rollback that was not one.
///
/// # What the existing rule does, and what it does not
///
/// [Retrospective 12](../../projects/retrospectives/12.%20Sprint%2014%20Retrospective.md)'s
/// fifth action put **reproduced or only observed** in the template, carried
/// since [#382](https://github.com/sujanto-gaws/kelir/pull/382). **It worked**:
/// all three of record 07's rows carry the word. **It governs a follow-up's
/// description and not its existence** — an honest description of an unfiled
/// follow-up is still an unfiled follow-up, and `not yet filed` is the template
/// being obeyed.
///
/// # What this rule will not notice
///
/// Stated rather than implied, on the module doc's own terms.
///
/// - **An issue that is closed, or labelled to no sprint.** A follow-up that
///   was filed and then dealt with is not a defect, and requiring more than
///   existence is a separate judgement ([#445](https://github.com/sujanto-gaws/kelir/issues/445),
///   *Not in scope*).
/// - **An issue number that does not exist on GitHub.** Rule 6 resolves
///   verification citations because they are paths on disk; an issue link
///   resolves over the network, and a test that needed the network would be a
///   test that fails when GitHub does.
/// - **A follow-up nobody wrote down at all.** Every rule in this file reads
///   what a record says. A run that saw something and recorded nothing is
///   invisible here and always was.
#[test]
fn a_final_record_names_an_issue_for_every_follow_up() {
    let mut unfiled: Vec<String> = Vec::new();
    let mut shapeless: Vec<String> = Vec::new();

    for (version, name, body) in release_records() {
        if version < FIRST_AFTERMATH_GOVERNED_RELEASE || !says_final(&body) {
            continue;
        }

        let Some((label, rows)) = follow_up_block(&body) else {
            shapeless.push(name);
            continue;
        };

        // `none` on the label line settles the record: records 02 and 03
        // already write an empty Aftermath that way, and a clean run should not
        // have to invent a row in order to say that nothing happened.
        if says_none(&label) {
            continue;
        }

        // A label that neither says `none` nor has rows beneath it is the shape
        // record 07 would have had if its row had been deleted rather than left
        // unfiled — which must not be the cheap way out.
        if rows.is_empty() {
            shapeless.push(name);
            continue;
        }

        for row in rows {
            if !row_opens_with_issue_link(&row) && !says_none(&row) {
                let shown: String = row.chars().take(110).collect();
                unfiled.push(format!("{name}\n      {shown}"));
            }
        }
    }

    assert!(
        shapeless.is_empty(),
        "a `Final` release record from v{}.{}.{} on has no `{FOLLOW_UPS_LABEL}` line under \
         `{AFTERMATH_HEADING}` carrying either rows or the word `none` (#445):\n  {}\n\n\
         An Aftermath the walk cannot read is an Aftermath the walk does not govern, which is \
         door B one section down. Say `none` when a run filed nothing.",
        FIRST_AFTERMATH_GOVERNED_RELEASE.0,
        FIRST_AFTERMATH_GOVERNED_RELEASE.1,
        FIRST_AFTERMATH_GOVERNED_RELEASE.2,
        shapeless.join("\n  ")
    );

    assert!(
        unfiled.is_empty(),
        "a `Final` release record carries a follow-up with no issue (#445):\n  {}\n\n\
         Every row under `{FOLLOW_UPS_LABEL}` opens with a link to {ISSUE_URL}<number>, or says \
         `none`. **Opening with it, not merely containing it**: record 07's own bad row cited \
         the guard that was bypassed, and the row above it cited somebody else's issue for \
         context, so a rule asking whether the row mentioned an issue anywhere would have \
         passed the very defect it was written for.\n\n\
         `not yet filed` is not a follow-up that was filed. File it, then link it.",
        unfiled.join("\n  ")
    );
}
