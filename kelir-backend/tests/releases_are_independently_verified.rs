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
//!    what makes each release bring a reading of its own. A citation is the
//!    record it points at, not its spelling — see *Rules 2, 6 and 7, where a
//!    citation points, 2026-09-26* below.
//! 3. **The walk finds what exists and governs something**, because a walk over
//!    nothing passes every assertion above it.
//! 4. **Every release record states a status in its header**, so the gate
//!    cannot be escaped by deleting the line it reads.
//! 5. **Every file in `projects/releases/` is the template or a record in the
//!    house shape.** Door B: a name the walk cannot parse is governed by
//!    nothing *and nothing notices it is ungoverned*.
//! 6. **Every verification record a release cites exists on disk**, directly
//!    inside `projects/verifications/`. Door A: the citations are hand-typed
//!    percent-encoded relative paths, and a typo in one used to read as a new
//!    pass. Since [#483](https://github.com/sujanto-gaws/kelir/issues/483), a
//!    path that reaches a file somewhere else is refused too.
//! 7. **A governed release cites a record written after every record cited
//!    before it.** Finding 7: rule 2's pool of never-cited records grows with
//!    every sprint, and eight of them were old enough to have read nothing in
//!    the release naming them. Since [#483](https://github.com/sujanto-gaws/kelir/issues/483),
//!    a release that cites something and points at no numbered record is
//!    refused rather than skipped.
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
//!     **Hardened by [#467](https://github.com/sujanto-gaws/kelir/issues/467)**
//!     (Sprint 19 item 2b): `none` is the whole answer, and the walk reads every
//!     row in the block — see *Rule 10, hardened 2026-09-16* below. **And by
//!     [#486](https://github.com/sujanto-gaws/kelir/issues/486)** (Sprint 21
//!     row 2): a row starts at any CommonMark list marker, not only `- ` — see
//!     *Rule 10, every list marker, 2026-09-26* below.
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
//! - **Cite a record by a link this file does not read as a citation.** A
//!   citation is a path containing `verifications/` in lower case, bounded by
//!   `(`, `)`, a space, a tab or a line end, or a `:` before it, whose path
//!   part, before any `?query` or `#fragment`, ends `.md`. So
//!   `../Verifications/…`, `..\verifications\…`, a path in a code span and one
//!   ending `/` are not citations, as they were not before
//!   [#483](https://github.com/sujanto-gaws/kelir/issues/483). Nor are an HTML
//!   `href="…"`, an angle-bracket destination `(<…>)`, or any citation on a
//!   line ending CRLF, where the `\r` stays on the path. **That can only
//!   cost a release**: a citation not read cannot be rule 2's new record or
//!   rule 7's high one, and a release left citing nothing is refused by rule 1.
//! - **Point a symbolic link at an old record.** Rule 6 refuses a record that
//!   is itself a link, by the directory entry's own type, and [`normalised_citation`]
//!   resolves `..` lexically, so a linked folder on the path is not followed.
//!   **On this project's Windows checkouts the question does not arise**:
//!   `core.symlinks` is `false`, so git writes a link as a plain file holding
//!   its target, which rule 6 accepts as a file — and judging what a record
//!   holds is the record's job, not this file's. No link is tracked today.
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
//!
//! **Those six are committed now** (2026-09-16, #467): the four refusals in
//! `record_07_at_00a27b4_and_the_earlier_probes_are_still_refused`, the two
//! acceptances in `the_shapes_rule_10_accepts_are_still_accepted`. What the
//! 2026-09-14 table records is what was run then; the tests are what holds it.
//!
//! # Rule 10, hardened 2026-09-16 (#467)
//!
//! **Six probes and every one of them was a shape rule 10 already had an
//! answer for.** [Record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
//! finding 4 sent four it had not, each a synthetic `v0.8.0` carrying record
//! 07's unfiled row verbatim from `00a27b4`, and **all four passed** while the
//! control — the same row, alone under `one:` — failed:
//!
//! | # | The Aftermath | Before #467 | Now, and the test that sends it |
//! |---|---|---|---|
//! | A | Answer `one, and none of it filed yet:`, then the row | passed | **refused** — `an_answer_that_contains_none_is_not_the_answer_none` |
//! | B | Answer `one:`, the row ending `— recorded nonetheless` | passed | **refused** — `a_row_that_contains_none_is_still_a_follow_up` |
//! | C | Answer `two:`, a filed row, **a blank line**, the row | passed | **refused** — `a_blank_line_does_not_hide_the_rows_after_it` |
//! | D | Answer `two:`, a filed row, **a wrapped line**, the row | passed | **refused** — `a_wrapped_line_does_not_hide_the_rows_after_it` |
//!
//! **Two causes.** A and B: `says_none` was `contains("none")`, applied to the
//! label line and to every row, and *nonetheless* contains *none*. C and D: the
//! rows were a `take_while` over indented `- ` lines, so the first blank or
//! wrapped line ended the walk. The fixes are [`says_none`] matching the whole
//! answer and [`follow_up_block`] ending only at a line back at column 0. **A
//! third change came out of reading the second:** a `none` answer used to skip
//! the rows beneath it, so an *exact* `none` over record 07's row passed as
//! surely as shape A did. [`read_aftermath`] now judges the rows under any
//! answer, and no row is excused for saying `none`.
//!
//! **The shapes are committed text, not files.** [`read_aftermath`] is rule
//! 10's judgement of one body, apart from the walk that finds the records, so
//! a synthetic record is a string in this file and nothing is planted in
//! `projects/releases/`. Record 07 is read and never written: the `00a27b4`
//! control swaps its row in memory.
//!
//! **The controls still hold**: record 07 as it is on `main` passes; record 07
//! with its row restored to `00a27b4` is refused, quoting the row; a withdrawn
//! struck-through issue row, a row opening with an issue link, and the answer
//! `none` alone are accepted — as are C and D with both rows filed, which is
//! what shows a blank line or a wrapped line is not itself what is refused.
//!
//! **Seen red, 2026-09-16.** Ten mutations to the rule, each applied alone to
//! this file, the suite run, and the file restored:
//!
//! | # | Mutation | Red |
//! |---|---|---|
//! | 1 | [`says_none`] back to `contains("none")` | `an_answer_that_contains_none_is_not_the_answer_none` (A's answer alone), `says_none_matches_the_whole_answer` |
//! | 2 | A blank line ends the walk | `a_blank_line_does_not_hide_the_rows_after_it` (C) |
//! | 3 | A wrapped line ends the walk | `a_wrapped_line_does_not_hide_the_rows_after_it` (D), and the wrapped-`none` case |
//! | 4 | A `none` answer skips the rows again | `an_answer_of_none_does_not_excuse_the_rows_under_it` |
//! | 5 | Rows excused by `contains("none")` again | `a_row_that_contains_none_is_still_a_follow_up` (B) |
//! | 6 | Rows excused by a whole-answer `none` | `a_row_that_contains_none_is_still_a_follow_up` (a row of `none`) |
//! | 7 | A wrapped line before any row dropped rather than joined to the answer | `an_answer_that_contains_none_is_not_the_answer_none` (wrapped `none`) |
//! | 8 | A column-0 line no longer ends the block | `the_shapes_rule_10_accepts_are_still_accepted` (the next top-level item's rows) |
//! | 9 | 1 and 4 together | shape A itself, plus 1's and 4's reds |
//! | 10 | 1, 2, 3, 4 and 5 together — the rule as #455 shipped it | all six new tests; **15 passed**, the controls among them |
//!
//! **Mutation 1 does not redden shape A's own fixture, and that is recorded
//! rather than tuned away**: with the rows judged under any answer, A's row is
//! refused whatever [`says_none`] says. Mutation 9 turns both guards off and A
//! goes red, which is what shows the fixture is connected; the answer-alone
//! case is what pins the match by itself.
//!
//! **Two came back green on the first run and were closed, not replaced.**
//! Mutation 6 left all nineteen tests green, because shape B's row is not
//! `none` under either match, so nothing sent the row `- none` that
//! [`read_aftermath`]'s doc says is refused. Mutation 8 left them green,
//! because no fixture put an indented list under the *next* top-level item —
//! record 02's shape — so the column-0 edge [`follow_up_block`] draws was
//! written and not tested. Both inputs are now sent, and both mutations were
//! re-run red. Mutation 10 is the pre-#467 rule, and its reds are the four
//! shapes above going back to passing.
//!
//! # Rule 10, every list marker, 2026-09-26 (#486)
//!
//! [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md)
//! finding 5: [`follow_up_block`] started a row only at an indented `- ` or
//! `~~`, and joined every other indented line to the row above it. Under
//! `two:`, record 07's filed row and then its unfiled row written under `* `,
//! `+ `, `2. ` or `-` and a tab — four list items to CommonMark — **passed**,
//! while the same row under `- ` was refused. None of the four was among rule
//! 10's stated limits.
//!
//! **The fix is [`after_list_marker`]**: `-`, `*` or `+`, or one to nine
//! digits and `.` or `)`, followed by a space or a tab, starts a row, and
//! [`row_opens_with_issue_link`] strips any such marker before looking for the
//! link — without the second half, a filed row under `* ` would be refused.
//!
//! **Probes, committed** in `a_follow_up_under_any_list_marker_is_a_row`: the
//! unfiled row after a filed one under each of `- ` (the control the old rule
//! already refused), `* `, `+ `, `2. `, `2) `, `-`+tab, `*`+tab, `1. `, `10) `
//! and `3.`+tab — every one refused, quoting the row — and a filed row under
//! each, every one accepted; plus a `~~` row followed by a `* ` row, read as
//! two rows. **The false positives, committed** in
//! `a_line_that_only_looks_like_a_marker_continues_the_row`: a filed row's
//! wrapped line opening `**bold**`, `*emphasis*`, `2026-09-17`, `2.5`, `404 `,
//! a ten-digit `1234567890.`, `-1`, `--force` or `+1`, or carrying markers
//! later in the line, is joined to the row and the record passes. **A marker
//! with a space and nothing after it**, committed in
//! `a_marker_with_only_a_space_after_it_starts_a_row`: `* `, `- `, `2. ` or
//! `+`+tab ending its line starts a row, and the line below is its text — record
//! 07's unfiled row there is refused, a filed row accepted. Added after the
//! row's gate found this file's first account of that shape untrue.
//!
//! **Seen red.** Baseline before the change: 21 passed. After: 24. Seven
//! mutations, each applied alone to this file, the suite run, and the file
//! restored — every one red on the test it names and nothing else:
//!
//! | # | Mutation | Red |
//! |---|---|---|
//! | 1 | The start-of-row predicate back to `starts_with("- ")` — the rule as #479 left it | `a_follow_up_under_any_list_marker_is_a_row` (the `* ` row, joined and unread) |
//! | 2 | The row's marker stripped by `trim_start_matches("- ")` again | the same test (a filed row under `* ` refused) |
//! | 3 | No space or tab required after the marker | `a_line_that_only_looks_like_a_marker_continues_the_row` (`**bold**` read as a row) |
//! | 4 | A space after the marker, not a tab | `a_follow_up_under_any_list_marker_is_a_row` (`-`+tab) |
//! | 5 | Any number of digits | `a_line_that_only_looks_like_a_marker_continues_the_row` (`1234567890.`) |
//! | 6 | Bullet markers only | `a_follow_up_under_any_list_marker_is_a_row` (`2. `) |
//! | 7 | [`follow_up_block`] hands [`after_list_marker`] the trimmed line | `a_marker_with_only_a_space_after_it_starts_a_row` (`* ` then the unfiled row, unread) |
//!
//! Mutations 1 and 2 together are the pre-#486 rule entire, and redden the
//! same test.
//!
//! **Historic sweep.** `projects/releases/` and `projects/verifications/` were
//! restored from each of the 62 first-parent `main` commits that touched them,
//! and this file's suite run over each tree with the test binary before the
//! change and after it. **No test's verdict differed on any of the 62.** The
//! twelve trees since `f455986` are green under both; older trees are red under
//! both, for rules and fixtures younger than the tree — record 07 read from
//! disk, and at `879d497` and `00a27b4` record 07's own `not yet filed` row,
//! the defect rule 10 was written for.
//!
//! **Positive control, last.** A row `* **A probe follow-up …** · **reproduced**
//! · **not yet filed**` written under record 08's filed `#503` row on disk:
//! `a_final_record_names_an_issue_for_every_follow_up` red, quoting the row,
//! and the other 22 of the 23 then written green. The binary before the change
//! passed the same tree, 21 of 21 — finding 5 on the real walk, not only on a
//! string. Record 08 was restored byte-exact.
//!
//! # Rules 2, 6 and 7, where a citation points, 2026-09-26 (#483)
//!
//! [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md)
//! finding 2: rules 2 and 7 compared the text after `verifications/` as a raw
//! string and read the record number off it, while rule 6 decoded and resolved
//! it. So `../verifications/./15…`, `../verifications/%31%35…` and
//! `../verifications/../releases/07.%20Release%20v0.7.0.md`, each planted as a
//! `Final` `v0.8.0` when record 07 already cited record 15, **passed all 21
//! tests**. Rule 2 read each as a string no release had cited; rule 7's
//! [`record_number`] read no number from any of them, and the release was
//! skipped by a `continue`; rule 6 found each file on disk.
//!
//! **The fix is [`normalised_citation`], one reading of a citation that rules
//! 2, 3, 6 and 7 all take.** [`citations`] now keeps the whole path, since a
//! path cannot be resolved without its prefix, and starts it after a `:` too,
//! so a reference definition written `[r]:../verifications/…` is read. The
//! normaliser cuts at the first `?` or `#`, as a browser does, percent-decodes once with rule 6's own [`percent_decoded`],
//! resolves `.` and `..` from `projects/releases/`, and answers the file name
//! only when the result is directly inside `projects/verifications/`. Rule 6
//! matches that name against [`verification_files`], the folder's listing.
//! Rule 7 refuses a release that cites something and points at no numbered
//! record. **Compared byte for byte, as CI's Linux does**: `Verifications/`
//! and `15. sprint 16 …` are refused here, though Windows would open both.
//!
//! **Probes, committed.** In `a_citation_is_read_as_the_record_it_points_at`,
//! finding 2's `./15` and `%31%35`, plus `..%2F…`, `..%2f…`, `../../projects/…`,
//! `..//…`, a `#fragment`, a `?plain=1` query, a query shaped like a path to
//! record 19 and no encoding, all read as record 15. In
//! `a_citation_outside_the_verifications_folder_is_refused`, finding 2's
//! `../releases/07`, the same with `%2F`, a subfolder, `Verifications/`, above
//! the root, an absolute path, one with its `/` encoded, a URL, a drive and an
//! encoded backslash are all refused, `%2531%2535` decodes once to a name that
//! is not on disk, and rule 6 refuses a lower-case name.
//! `rules_2_and_7_refuse_a_record_already_cited_under_another_spelling` plants
//! each shape after a `v0.7.0` citing record 15, and a fourth,
//! `99.%20A%20Pass/../15…`: the old reading took 99 from it and the path
//! opens record 15. A fifth, `15…md?/../../verifications/19…md`, is record 15
//! with a query and is refused. Record 16, and record 16 beside `./15`, are
//! accepted. `a_citation_is_read_whole_from_the_link` reads `…md?plain=1` and
//! a reference definition `[r]:../verifications/19…` without its `[r]:`.
//! `a_release_with_no_numbered_citation_is_refused` sends a release whose only
//! citation is `../releases/07`, an unnumbered name, `%2531%2535` or a
//! subfolder: all four are red on rule 7, and the two outside the folder on
//! rule 2. A release citing nothing is left to rule 1.
//!
//! **Seen red.** Baseline before the change: 24 passed. After: 29. Twelve
//! mutations, each applied to this file, the suite run, and the file restored.
//! Every one was red, and only on this change's tests:
//!
//! | # | Mutation | Red |
//! |---|---|---|
//! | 1 | Rule 2 compares the raw strings | `rules_2_and_7_refuse_…_under_another_spelling`, `a_release_with_no_numbered_citation_is_refused` |
//! | 2 | Rule 7 reads [`record_number`] off the raw text after `verifications/` | `rules_2_and_7_refuse_…_under_another_spelling` (`99.%20A%20Pass/../15`) |
//! | 3 | The `continue` on no number, back | the same two as 1 |
//! | 4 | A path outside the folder answers its last component | `a_citation_outside_…_is_refused`, and the same two as 1 |
//! | 5 | The `#fragment` kept | `a_citation_is_read_as_the_record_it_points_at`, `a_citation_is_read_whole_from_the_link` |
//! | 6 | Decoded twice | `a_citation_outside_…_is_refused`, `a_release_with_no_numbered_citation_is_refused` (`%2531%2535` read as 15) |
//! | 7 | Rule 6 by `is_file()` rather than the listing | `a_citation_outside_…_is_refused` (the lower-case name). **Red only on a case-insensitive filesystem**, which this Windows checkout is; on Linux the two answers agree |
//! | 8 | A backslash not refused | `a_citation_outside_…_is_refused` |
//! | 9 | An absolute path, URL or drive not refused | `a_citation_outside_…_is_refused` |
//! | 10 | 1, 2 and 3 together: the comparisons as #412 shipped them | the same two as 1 |
//! | 11 | The path cut at `#` only, not at `?` | `rules_2_and_7_refuse_…_under_another_spelling` (the query read as record 19), `a_citation_is_read_as_the_record_it_points_at`, `a_citation_is_read_whole_from_the_link` |
//! | 12 | The start bound back to `(`, `)`, space and line end | `a_citation_is_read_whole_from_the_link` (`[r]:` kept on the path, which is then refused) |
//!
//! **Mutation 2 came back green on the first run and was closed, not
//! replaced.** Each of finding 2's shapes read no number under the old
//! reading, so the new refusal of a release with no number caught every one of
//! them, whatever rule 7 read. The fourth shape is one where the old reading
//! finds a *higher* number than the target's, and it is red. **So was mutation
//! 7**, because rule 6 walks the disk and no test reached its lookup;
//! [`unresolved`] is now rule 6's judgement of one citation, and the test sends
//! it the lower-case name.
//!
//! **Mutations 11 and 12 came from the row's gate**, which found both defects
//! in the first version of this change: a `?` query shaped like a path was
//! resolved as one, so `15…md?/../../verifications/19…md` read as record 19,
//! which a browser does not open; and `[r]:../verifications/19…`, valid
//! CommonMark that the reading before #483 accepted, kept `[r]:` on the path
//! and was refused by rules 2, 6 and 7. Both are fixed and both probes are
//! committed. The fix touches only the path cut and the start bound, and the
//! sweep below was re-run over it, with the same result.
//!
//! **Historic sweep.** `projects/releases/` and `projects/verifications/` were
//! restored from each of the 63 first-parent `main` commits that touched them,
//! up to `cb5ed37`, and the suite run over each tree with the test binary from
//! before the change and after it. **No test the two share changed verdict on
//! any of the 63.** The first sweep found seven: on the trees from `9d67e37`
//! back there is no `projects/verifications/`, and [`verification_files`]
//! panicked where the old rule 6 never opened the folder. A missing folder now
//! holds nothing, and the rerun is the one recorded. The thirteen trees since
//! `f455986` are green under both binaries, and older trees are red under both,
//! as #486's sweep found. Of the new tests,
//! `a_citation_outside_the_verifications_folder_is_refused` is red on the 43
//! trees before `42b5122`, which added record 15, because it reads that record
//! from disk.
//!
//! **Positive control, last.** Record 08 cites records 16 and 17, so the
//! high-water mark is 17. A `Final` `09. Release v0.9.0.md` was planted with an
//! Aftermath of `none` and one citation, run against both binaries, and
//! removed. First, the accepted control, record 19 in the house spelling: 24
//! of 24 before, 29 of 29 after. Then finding 2's shapes, pointed at record 17
//! and record 08:
//!
//! | Citation | Before | After |
//! |---|---|---|
//! | `../verifications/./17.%20…` | 24 of 24 | red: `each_release_brings_a_pass_no_earlier_release_cited`, `each_release_cites_a_record_written_after_every_earlier_citation` |
//! | `../verifications/%31%37.%20…` | 24 of 24 | red: the same two |
//! | `../verifications/../releases/08.%20Release%20v0.8.0.md` | 24 of 24 | red: the same two, and `a_cited_verification_record_exists` |
//!
//! Nothing else was red. The file was removed, and the tree was left as it was.

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

/// Where the release records live, and so what their relative links start
/// from.
const RELEASES: &str = "projects/releases";

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
/// what rule 7 compares. It reads the same answer off
/// `15.%20Sprint%2016%20Independent%20Pass.md` and
/// `15. Sprint 16 Independent Pass.md`, and since
/// [#483](https://github.com/sujanto-gaws/kelir/issues/483) rule 7 hands it
/// the second: the name [`normalised_citation`] resolved, not the text of the
/// link, which read nothing off `./15…` or `%31%35…`.
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

/// Every `projects/verifications/…` path a record links to, **as written**.
///
/// A citation is the whole path around a `verifications/` — from the `(`,
/// `)`, `:`, space, tab or line start before it to the `(`, `)`, space, tab or
/// line end after it — whose path part, before any `?query` or `#fragment`,
/// ends `.md`. The links are percent-encoded
/// relative paths (`../verifications/13.%20Sprint%2013%20Independent%20Pass.md`).
///
/// # The spelling is not the identity
///
/// Until [#483](https://github.com/sujanto-gaws/kelir/issues/483) this kept
/// only the text after `verifications/`, and rules 2 and 7 compared that text
/// as the record's identity, on the reasoning that *there is only ever one
/// spelling because the links are written by copying*. [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md)
/// finding 2 wrote three others — `./15…`, `%31%35…` and `../releases/07…` —
/// and each was a new record to rule 2 and no record to rule 7. **What a
/// citation is, is where it points**: [`normalised_citation`] says where, and
/// every rule that compares citations compares that.
///
/// So the prefix is kept now, because a path cannot be resolved without it.
/// **A token this does not keep can only cost a release**: it is not a
/// citation, so it cannot be the new one rule 2 wants or the high one rule 7
/// wants, and a release left citing nothing is refused by rule 1.
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
    let bounds = ['(', ')', ' ', '\t', '\n'];
    // A reference definition may put its destination straight after the
    // colon, `[r]:../verifications/…`, and a colon is never part of a relative
    // path; a URL's `https:` then leaves `//…`, which is still refused.
    let start_bounds = ['(', ')', ' ', '\t', '\n', ':'];

    for (at, _) in body.match_indices("verifications/") {
        let start = body[..at].rfind(start_bounds).map_or(0, |bound| bound + 1);
        let end = body[at..].find(bounds).map_or(body.len(), |len| at + len);
        let cited = &body[start..end];

        if path_part(cited).ends_with(".md") {
            found.insert(cited.to_owned());
        }
    }

    found
}

/// A link target without its `?query` or `#fragment`, cut at the first of
/// either, as a browser cuts it.
///
/// `15.%20…%20Pass.md#7-findings` and `15.%20…%20Pass.md?plain=1` cite record
/// 15 as surely as the bare path does; before
/// [#483](https://github.com/sujanto-gaws/kelir/issues/483) neither was a
/// citation, because neither ended `.md`. **And the cut is what the link
/// opens**: `15…md?/../../verifications/19…md` is record 15 with a query, not
/// record 19.
fn path_part(cited: &str) -> &str {
    cited.split(['?', '#']).next().unwrap_or(cited)
}

/// Why a citation names no record in `projects/verifications/`, whatever file
/// it reaches on disk.
#[derive(Debug, PartialEq)]
enum Refusal {
    /// An absolute path, a URL or a drive: not a path from the record.
    NotRelative,
    /// A backslash, which Windows reads as a separator and Linux as a character
    /// of the name — so the citation would name two different files.
    Backslash,
    /// It resolves somewhere other than directly inside
    /// `projects/verifications/`: another folder, a subfolder, or above the
    /// repository.
    OutsideVerifications,
}

/// The record a citation points at: the file name, directly inside
/// `projects/verifications/`, that the link resolves to from
/// `projects/releases/`, where every record it is read from lives
/// ([#483](https://github.com/sujanto-gaws/kelir/issues/483)).
///
/// **This is the one reading of a citation**, and rules 2, 3, 6 and 7 all take
/// it. The fragment comes off, the path is percent-decoded **once**, and `.`
/// and `..` are resolved against the path's own components — so `./15…`,
/// `%31%35…` and `..%2Fverifications%2F15…` are all record 15, and
/// `../verifications/../releases/07…` is refused rather than counted.
///
/// # What it decides, and what it leaves to rule 6
///
/// - **Once, not until it stops changing.** A browser decodes a link once, so
///   `%2531` is the name `%31…`, which exists nowhere, has no number, and is
///   refused by rules 6 and 7 rather than read as `15`.
/// - **Byte for byte.** `projects` and `verifications` are matched exactly and
///   the name is returned as spelt, because CI reads the tree on Linux, where
///   `Verifications/` is a different folder; Windows would open it. Rule 6
///   matches the name against the folder's listing for the same reason.
/// - **Lexically.** `..` removes the component before it, whether or not that
///   is a symbolic link on disk, which is what a Markdown renderer does too.
///   Rule 6 refuses a record that is itself a link.
/// - **Not whether it exists.** A well-formed name that is not there is rule
///   6's, so door A still reddens the rule that names it.
fn normalised_citation(cited: &str) -> Result<String, Refusal> {
    let decoded = percent_decoded(path_part(cited));

    if decoded.starts_with('/') || decoded.contains(':') {
        return Err(Refusal::NotRelative);
    }
    if decoded.contains('\\') {
        return Err(Refusal::Backslash);
    }

    let mut resolved: Vec<&str> = RELEASES.split('/').collect();

    for component in decoded.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                resolved.pop().ok_or(Refusal::OutsideVerifications)?;
            }
            name => resolved.push(name),
        }
    }

    match resolved.as_slice() {
        [projects, verifications, name]
            if [*projects, *verifications] == ["projects", "verifications"] =>
        {
            Ok((*name).to_owned())
        }
        _ => Err(Refusal::OutsideVerifications),
    }
}

/// Every regular file directly inside `projects/verifications/`, by exact name.
///
/// A listing rather than `is_file()`, because Windows answers `is_file()` for
/// `15. sprint 16 independent pass.md` and Linux, where CI runs, does not. And
/// the entry's own type rather than its target's, so a symbolic link named as
/// a new record and pointing at an old one is not a record.
///
/// **A folder that is not there holds nothing**, rather than panicking: rule 6
/// then names every citation as missing, and a tree that cites nothing — every
/// tree before the folder existed — passes it, as it did before
/// [#483](https://github.com/sujanto-gaws/kelir/issues/483).
fn verification_files() -> BTreeSet<String> {
    let directory = match fs::read_dir(repository_root().join(VERIFICATIONS)) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return BTreeSet::new(),
        Err(error) => panic!("the verifications directory is unreadable: {error}"),
    };

    directory
        .map(|entry| entry.expect("a directory entry"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// Rule 6's judgement of one citation against the folder's listing: `None`
/// when it names a record there, and otherwise why it does not.
fn unresolved(cited: &str, on_disk: &BTreeSet<String>) -> Option<String> {
    match normalised_citation(cited) {
        Ok(record) if on_disk.contains(&record) => None,
        Ok(_) => Some(format!("which is not in {VERIFICATIONS}/")),
        Err(refusal) => Some(format!(
            "which names no record in {VERIFICATIONS}/ ({refusal:?})"
        )),
    }
}

/// The Aftermath's follow-up block: the answer on the `- **Follow-ups filed:**`
/// line, and every row indented under it.
///
/// `None` means the record has no such line inside an `## Aftermath` section —
/// which rule 10 treats as a failure rather than an exemption, for door B's
/// reason: **a shape the walk cannot find must not be a shape the walk
/// ignores.**
///
/// # Where the block ends, and where it does not
///
/// **It ends at the first non-blank line back at column 0** — the next
/// top-level item, the block-quoted note every record carries after its
/// Aftermath, or prose — so that note is not mistaken for a follow-up.
///
/// **It does not end at a blank line or at an indented line that is not a list
/// item**, and until [#467](https://github.com/sujanto-gaws/kelir/issues/467) it
/// did. The rows were a `take_while` over indented `- ` and `~~` lines, so
/// [record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
/// finding 4 put a blank line (shape C) or a wrapped sentence (shape D) after a
/// filed row and every row beneath the break was out of view. Markdown renders
/// C as one loose list and D as one wrapped item, so **the break a reader never
/// sees was the break the walk stopped at.**
///
/// - An indented line opening with a list marker ([`after_list_marker`]) or
///   `~~` starts a row. **A nested item under a row is a row too**, and must
///   open with its own issue link: the strict reading, because an unfiled
///   follow-up tucked under a filed one is the shape C and D were.
/// - Any other indented line continues whatever came before it — the previous
///   row, or the answer when no row has started yet — so a wrapped answer is
///   judged whole, `none` and the words after it together.
///
/// **Until [#486](https://github.com/sujanto-gaws/kelir/issues/486) the only
/// marker was `- `.** [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md)
/// finding 5 wrote record 07's unfiled row under `* `, `+ `, `2. ` and `-` with
/// a tab, and each was joined to the filed row above it as a wrapped line and
/// passed — four list items to a reader, one row to the walk.
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

    let mut answer = label.trim_start()[FOLLOW_UPS_LABEL.len()..]
        .trim()
        .to_owned();
    let mut rows: Vec<String> = Vec::new();

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Shape C: a blank line makes the list loose, and does not end it.
            continue;
        }

        if !line.starts_with([' ', '\t']) {
            break;
        }

        if after_list_marker(line).is_some() || trimmed.starts_with("~~") {
            rows.push(trimmed.to_owned());
        } else {
            // Shape D: a wrapped line belongs to the item above it.
            let item = rows.last_mut().unwrap_or(&mut answer);
            item.push(' ');
            item.push_str(trimmed);
        }
    }

    Some((answer, rows))
}

/// The text after the CommonMark list marker `line` opens with, if it opens
/// with one ([#486](https://github.com/sujanto-gaws/kelir/issues/486)).
///
/// A marker is `-`, `*` or `+`, or one to nine digits and then `.` or `)` —
/// CommonMark's bullet and ordered markers — **followed by a space or a tab**.
/// The character after the marker is what keeps a wrapped line from being read
/// as a row: `**bold**` is not a `*` marker, `2026-09-17` and `2.5 hours` are
/// not ordered ones, and a marker later in the line is not at its start.
///
/// # What it does not do
///
/// - **It does not track indentation.** CommonMark lets an ordered item start
///   inside a paragraph only when its number is `1`, so a row's wrapped line
///   indented to its text and opening `2. ` is paragraph text to a reader and a
///   row here. **That costs a refusal, not a pass**: the walk quotes the line,
///   and rewrapping it ends the refusal.
/// - **It does not read a bare marker ending its line**, an empty item whose
///   text starts on the line below, because nothing follows the marker. A
///   marker followed by a space or a tab and then nothing *is* read:
///   [`follow_up_block`] hands this the untrimmed line, so `  * ` starts a row
///   and the line below joins it as the row's text.
fn after_list_marker(line: &str) -> Option<&str> {
    let line = line.trim_start();

    let rest = match line.strip_prefix(['-', '*', '+']) {
        Some(rest) => rest,
        None => {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            if !(1..=9).contains(&digits) {
                return None;
            }
            line[digits..].strip_prefix(['.', ')'])?
        }
    };

    rest.strip_prefix([' ', '\t'])
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
    // Every leading marker, as `trim_start_matches("- ")` stripped every `- `
    // before #486 — a row reaching its link through an empty nested item is
    // read as it was.
    let mut row = row.trim_start();
    while let Some(rest) = after_list_marker(row) {
        row = rest.trim_start();
    }

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

/// Whether an answer says there is nothing to file.
///
/// `none` is the template's own word for it, and rule 10 accepts it as the
/// label line's answer — how a future release with a clean run will write an
/// empty Aftermath.
///
/// # The whole answer, not a substring of it
///
/// This was `to_ascii_lowercase().contains("none")` until
/// [#467](https://github.com/sujanto-gaws/kelir/issues/467), and
/// [record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
/// finding 4 passed record 07's unfiled row through it twice: under the answer
/// `one, and none of it filed yet:` (shape A), and with `— recorded
/// nonetheless` appended to the row itself (shape B). *Nonetheless* contains
/// *none*, and so do *none yet* and *none of them*.
///
/// **So the answer is `none` and nothing else**, in any case, with two
/// allowances that cannot carry a word: the backticks the template prints
/// around it, and a closing full stop. `none from this rehearsal`, record 03's
/// own wording, is refused — record 03 is below
/// [`FIRST_AFTERMATH_GOVERNED_RELEASE`], and a governed record that wants to
/// explain an empty Aftermath can do it in a paragraph after the list. **Not
/// on an indented line under the answer**: [`follow_up_block`] joins that to
/// the answer, which is what stops `none` wrapping onto `of it filed yet`.
fn says_none(answer: &str) -> bool {
    let answer = answer.trim();
    let answer = answer.strip_suffix('.').unwrap_or(answer);
    let answer = answer
        .strip_prefix('`')
        .and_then(|inner| inner.strip_suffix('`'))
        .unwrap_or(answer);

    answer.eq_ignore_ascii_case("none")
}

/// What rule 10 finds in one record's Aftermath.
#[derive(Debug, PartialEq)]
enum Aftermath {
    /// No follow-up block, or an answer that is not `none` with no rows under
    /// it.
    Unreadable,
    /// The rows that do not open with an issue link. **Empty is a pass.**
    Unfiled(Vec<String>),
}

/// Rule 10's judgement of one record body, apart from the walk that finds the
/// records — so the shapes [record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
/// finding 4 probed can be sent to it as committed text rather than as files
/// planted in `projects/releases/` and deleted.
///
/// Two things changed here under [#467](https://github.com/sujanto-gaws/kelir/issues/467),
/// both on the reading that `none` is an answer to the label and nothing else:
///
/// - **A `none` answer does not excuse the rows under it.** The answer used to
///   be checked first and the record skipped, so an unfiled row under
///   `- **Follow-ups filed:** none` was never read. `none` now means *no rows
///   are required*, and every row there is still judged.
/// - **A row cannot say `none`.** Rows used to pass on `says_none` as well —
///   which is how shape B's `— recorded nonetheless` got through. A row is a
///   follow-up, and a follow-up opens with its issue.
fn read_aftermath(body: &str) -> Aftermath {
    let Some((answer, rows)) = follow_up_block(body) else {
        return Aftermath::Unreadable;
    };

    // An answer that neither says `none` nor has rows beneath it is the shape
    // record 07 would have had if its row had been deleted rather than left
    // unfiled — which must not be the cheap way out.
    if rows.is_empty() && !says_none(&answer) {
        return Aftermath::Unreadable;
    }

    Aftermath::Unfiled(
        rows.into_iter()
            .filter(|row| !row_opens_with_issue_link(row))
            .collect(),
    )
}

fn governed() -> Vec<Release> {
    release_records()
        .into_iter()
        .filter(|(version, _, body)| *version >= FIRST_GOVERNED_RELEASE && says_final(body))
        .map(|(version, name, body)| (version, name, citations(&body)))
        .collect()
}

/// The records a release's citations point at — each one [`normalised_citation`]
/// accepts.
fn cited_records(cited: &BTreeSet<String>) -> BTreeSet<String> {
    cited
        .iter()
        .filter_map(|record| normalised_citation(record).ok())
        .collect()
}

/// Rule 2's judgement over releases oldest first, apart from the walk that
/// finds them — so the shapes [#483](https://github.com/sujanto-gaws/kelir/issues/483)
/// names are sent to it as committed text.
///
/// A release that cites something is refused unless one record it points at
/// was pointed at by no earlier release.
fn recycled(releases: &[Release]) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut recycled = Vec::new();

    for (_, name, cited) in releases {
        let records = cited_records(cited);

        if !cited.is_empty() && records.iter().all(|record| seen.contains(record)) {
            recycled.push(format!(
                "{name} — cites only {}",
                cited.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        seen.extend(records);
    }

    recycled
}

/// Rule 7's judgement over releases oldest first, apart from the walk that
/// finds them.
///
/// A release that cites something is refused unless the highest-numbered
/// record it points at is above every record an earlier release pointed at —
/// **and refused when it points at no numbered record at all**, which until
/// [#483](https://github.com/sujanto-gaws/kelir/issues/483) was a `continue`:
/// a release whose citations were all spelt so [`record_number`] read nothing
/// was not checked.
fn backward(releases: &[Release]) -> Vec<String> {
    let mut high_water: Option<u32> = None;
    let mut backward = Vec::new();

    for (_, name, cited) in releases {
        // Citing nothing at all is rule 1's, so this stays silent about it
        // rather than reporting the same record twice with two remedies.
        if cited.is_empty() {
            continue;
        }

        let highest = cited_records(cited)
            .iter()
            .filter_map(|record| record_number(record))
            .max();

        let Some(highest) = highest else {
            backward.push(format!(
                "{name} — cites {}, and none of it is a numbered record in {VERIFICATIONS}/",
                cited.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
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

    backward
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
///
/// **Compared by where a citation points, since [#483](https://github.com/sujanto-gaws/kelir/issues/483)**:
/// through [`normalised_citation`], so `./15…` is record 15 and not a new
/// string, and a citation that points outside `projects/verifications/` is not
/// a new record either. A release whose every citation is refused is red here.
#[test]
fn each_release_brings_a_pass_no_earlier_release_cited() {
    let recycled = recycled(&governed());

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
        .flat_map(|(.., cited)| cited_records(cited))
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
///
/// **And it names one inside `projects/verifications/`**, since
/// [#483](https://github.com/sujanto-gaws/kelir/issues/483): record 17 finding 2
/// cited `../verifications/../releases/07.%20Release%20v0.7.0.md`, which is on
/// disk and is not a verification record. The path is read through
/// [`normalised_citation`] and the name matched against
/// [`verification_files`], byte for byte, as CI's filesystem would.
#[test]
fn a_cited_verification_record_exists() {
    let on_disk = verification_files();
    let mut missing = Vec::new();

    for (_, name, body) in release_records() {
        for cited in citations(&body) {
            if let Some(why) = unresolved(&cited, &on_disk) {
                missing.push(format!("{name} — cites {cited}, {why}"));
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
/// remedies. **A release that cites something and points at no numbered
/// record is this one's**, since [#483](https://github.com/sujanto-gaws/kelir/issues/483)
/// — see [`backward`].
#[test]
fn each_release_cites_a_record_written_after_every_earlier_citation() {
    let backward = backward(&governed());

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
        "the number reads the same off a percent-encoded name"
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
///
/// Three more, drawn 2026-09-16 when [#467](https://github.com/sujanto-gaws/kelir/issues/467)
/// closed shapes A–D, because a wider walk has a new edge and the edge should be
/// on the page:
///
/// - **A follow-up written back at column 0**, as a sibling of the
///   `- **Follow-ups filed:**` item rather than a row under it. The block ends
///   there, because a column-0 line is where the block-quoted note and the
///   next top-level item begin, and nothing in the line says which it is.
///   Markdown renders it outside the follow-up list too, so a reader sees the
///   same edge the walk does.
/// - **A follow-up under another label** — `- **Issues found post-release:**`,
///   or record 02's `- **Carried into the sprint plan:**`. Rule 10 reads the
///   one label the template gives follow-ups.
/// - **A row that opens with an issue and then describes a second follow-up
///   that has none.** The row's subject is filed; what else the row says is
///   prose, and judging prose is what the pass is for.
///
/// Two more, drawn 2026-09-26 when [#486](https://github.com/sujanto-gaws/kelir/issues/486)
/// let a row start at any list marker:
///
/// - **A bare list marker ending its line**, with no space or tab after it and
///   the follow-up's text on the line below. CommonMark renders an item there;
///   the walk reads the marker as a wrapped line of the row above, and the text
///   too. A marker followed by a space or a tab and then nothing *is* a row,
///   and the line below joins it. [`after_list_marker`] says so. The cost the
///   other way is stated there as well: a wrapped line opening `2. ` is read as
///   a row, and refused, where CommonMark would keep it in the paragraph.
/// - **A fenced code block inside the follow-up block** is not recognised as
///   one: its indented lines opening `-`, `*`, `+` or `N.` are read as rows.
///   Before #486 only its `- ` lines were. That costs a refusal, not a pass, and
///   no record `main` has held carries one.
#[test]
fn a_final_record_names_an_issue_for_every_follow_up() {
    let mut unfiled: Vec<String> = Vec::new();
    let mut shapeless: Vec<String> = Vec::new();

    for (version, name, body) in release_records() {
        if version < FIRST_AFTERMATH_GOVERNED_RELEASE || !says_final(&body) {
            continue;
        }

        match read_aftermath(&body) {
            Aftermath::Unreadable => shapeless.push(name),
            Aftermath::Unfiled(rows) => {
                for row in rows {
                    let shown: String = row.chars().take(110).collect();
                    unfiled.push(format!("{name}\n      {shown}"));
                }
            }
        }
    }

    assert!(
        shapeless.is_empty(),
        "a `Final` release record from v{}.{}.{} on has no `{FOLLOW_UPS_LABEL}` line under \
         `{AFTERMATH_HEADING}` carrying either rows or the answer `none` (#445, #467):\n  {}\n\n\
         An Aftermath the walk cannot read is an Aftermath the walk does not govern, which is \
         door B one section down. When a run filed nothing, the answer is `none` and nothing \
         else — `none yet` or `none of it filed` is a follow-up that exists and has no issue.",
        FIRST_AFTERMATH_GOVERNED_RELEASE.0,
        FIRST_AFTERMATH_GOVERNED_RELEASE.1,
        FIRST_AFTERMATH_GOVERNED_RELEASE.2,
        shapeless.join("\n  ")
    );

    assert!(
        unfiled.is_empty(),
        "a `Final` release record carries a follow-up with no issue (#445, #467, #486):\n  {}\n\n\
         Every row under `{FOLLOW_UPS_LABEL}` opens with a link to {ISSUE_URL}<number> — \
         including rows after a blank line, rows after a wrapped line, rows nested under \
         another row, rows under any list marker (`-`, `*`, `+`, `2.`, `2)`), and rows under \
         an answer of `none`. **Opening with it, not merely \
         containing it**: record 07's own bad row cited \
         the guard that was bypassed, and the row above it cited somebody else's issue for \
         context, so a rule asking whether the row mentioned an issue anywhere would have \
         passed the very defect it was written for.\n\n\
         `not yet filed` is not a follow-up that was filed. File it, then link it.",
        unfiled.join("\n  ")
    );
}

/// Record 07's rollback row **as it stood at `00a27b4`**, verbatim: the row
/// rule 10 was written for, and the row every shape in
/// [record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
/// finding 4 carried.
const RECORD_07_UNFILED_ROW: &str = "  - **`deploy.sh`'s rollback command exits 1 on a healthy rollback to `0.6.0`** · **reproduced** · **not yet filed** · the guard is [#367](https://github.com/sujanto-gaws/kelir/pull/367)'s `/version.json` assertion, which reads the single-page fallback every frontend image before it serves as an empty version — see *The failure this row asks to be recorded*";

/// Record 07's first row, verbatim: a filed follow-up, for the shapes that need
/// one above the break.
const RECORD_07_FILED_ROW: &str = r"  - [`#404` — §2.9 never gained the stated-versus-run mutation rule it was ordered to](https://github.com/sujanto-gaws/kelir/issues/404) · **reproduced** · found while building [#395](https://github.com/sujanto-gaws/kelir/issues/395); the missing sentence is verifiable by `grep -nE '\bdated\b|\bSeen red\b'` over the coding standard, which returns nothing, and [#376](https://github.com/sujanto-gaws/kelir/issues/376) asserts the rule exists";

/// Where record 07 is, for the two tests that read it as it is on disk.
const RECORD_07: &str = "projects/releases/07. Release v0.7.0.md";

/// A synthetic `Final` record for `v0.8.0` whose Aftermath carries `follow_ups`
/// — the shape of finding 4's probes, as text rather than as a file planted in
/// `projects/releases/`.
///
/// **The block-quoted note and the closing sentence are kept**, because they
/// are what a real record has after its Aftermath list and what the walk must
/// stop before.
fn record_with_follow_ups(follow_ups: &str) -> String {
    format!(
        "# Release v0.8.0 — 2099-01-01\n\
         \n\
         **Status:** Final · **Last updated:** 2099-01-01\n\
         \n\
         {AFTERMATH_HEADING}\n\
         \n\
         - **Issues found post-release:** none\n\
         {follow_ups}\n\
         \n\
         > **A follow-up filed from a release run says whether its mechanism was *reproduced* \
         or *only observed*.**\n\
         \n\
         Set this document's header status to `Final` when the release is verified in \
         production.\n"
    )
}

/// Whether rule 10 lets a record through.
fn accepted(body: &str) -> bool {
    read_aftermath(body) == Aftermath::Unfiled(Vec::new())
}

/// Whether rule 10 refuses a record **by quoting record 07's unfiled row** —
/// the right row, rather than some other refusal that happens to be red.
fn refuses_the_unfiled_row(body: &str) -> bool {
    matches!(
        read_aftermath(body),
        Aftermath::Unfiled(rows) if rows.len() == 1 && rows[0].contains("**not yet filed**")
    )
}

/// **Shape A: an answer that contains `none` is not the answer `none`**
/// ([#467](https://github.com/sujanto-gaws/kelir/issues/467)).
///
/// [Record 16](../../projects/verifications/16.%20Sprint%2018%20Independent%20Pass.md)
/// finding 4 found shape A passing rule 10. **It is now shut twice** — by the
/// whole-answer match in [`says_none`], and by [`read_aftermath`] judging the
/// rows under any answer — so shape A itself reddens only with both guards
/// off. The cases that pin the match alone are A's answer with its row taken
/// away, and a `none` wrapped onto the words after it.
#[test]
fn an_answer_that_contains_none_is_not_the_answer_none() {
    let shape_a = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** one, and none of it filed yet:\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&shape_a),
        "shape A: an answer containing `none` settled the Aftermath and record 07's unfiled \
         row went unread"
    );

    let shape_a_answer_alone =
        record_with_follow_ups("- **Follow-ups filed:** one, and none of it filed yet:");
    assert_eq!(
        read_aftermath(&shape_a_answer_alone),
        Aftermath::Unreadable,
        "shape A's answer with no row under it promises a follow-up and names none; only the \
         whole-answer match refuses it"
    );

    let wrapped =
        record_with_follow_ups("- **Follow-ups filed:** none\n    of it filed yet, see below");
    assert_eq!(
        read_aftermath(&wrapped),
        Aftermath::Unreadable,
        "an answer of `none` wrapped onto `of it filed yet` is one answer, and it is not `none`"
    );
}

/// **Shape B: a row that contains `none` is still a follow-up**
/// ([#467](https://github.com/sujanto-gaws/kelir/issues/467)).
///
/// Rows used to be excused by the same substring match as the answer, so record
/// 07's unfiled row with `— recorded nonetheless` appended passed.
#[test]
fn a_row_that_contains_none_is_still_a_follow_up() {
    let shape_b = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** one:\n{RECORD_07_UNFILED_ROW} — recorded nonetheless"
    ));
    assert!(
        refuses_the_unfiled_row(&shape_b),
        "shape B: `— recorded nonetheless` on the row passed it, because rows were also \
         excused by a substring `none`"
    );

    // Seen green 2026-09-16 and closed here: restoring the row excuse as a
    // *whole-answer* `none` left every test green, because B's row is not
    // `none` either way. `read_aftermath`'s doc says a row cannot say `none`,
    // and a sentence naming an input has a test that sends it (§2.9).
    let a_row_of_none = record_with_follow_ups("- **Follow-ups filed:** one:\n  - none");
    assert_eq!(
        read_aftermath(&a_row_of_none),
        Aftermath::Unfiled(vec!["- none".to_owned()]),
        "`none` answers the label; a row is a follow-up, and a follow-up opens with its issue"
    );
}

/// **[`says_none`] matches the whole answer**, with the two allowances its doc
/// names and nothing else.
#[test]
fn says_none_matches_the_whole_answer() {
    for answer in ["none", "None", "NONE", "`none`", "none.", "  none  "] {
        assert!(says_none(answer), "{answer:?} is the answer `none`");
    }

    for answer in [
        "one, and none of it filed yet:",
        "none yet",
        "none of them",
        "nonetheless",
        "none from this rehearsal.",
        "`none` yet",
        "none..",
    ] {
        assert!(
            !says_none(answer),
            "{answer:?} contains `none` and is not the answer `none` — the substring match \
             #467 replaced accepted it"
        );
    }
}

/// **Shape C: a blank line does not end the follow-up block**
/// ([#467](https://github.com/sujanto-gaws/kelir/issues/467)).
///
/// Markdown renders a filed row, a blank line and an unfiled row as one loose
/// list. The walk stopped at the blank line, and passed.
#[test]
fn a_blank_line_does_not_hide_the_rows_after_it() {
    let shape_c = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&shape_c),
        "shape C: the walk stopped at the blank line and record 07's unfiled row, one list \
         item further down, went unread"
    );

    let blank_lines_everywhere = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n\n{RECORD_07_FILED_ROW}\n\n\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&blank_lines_everywhere),
        "a blank line under the answer and two between the rows are still one list"
    );
}

/// **Shape D: a wrapped line does not end the follow-up block**
/// ([#467](https://github.com/sujanto-gaws/kelir/issues/467)).
///
/// Markdown renders a filed row, an indented line that is not a list item, and
/// an unfiled row as a wrapped item followed by another. The walk stopped at
/// the wrapped line, and passed.
#[test]
fn a_wrapped_line_does_not_hide_the_rows_after_it() {
    let shape_d = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n    \
         and the issue carries the reproduction in full\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&shape_d),
        "shape D: the walk stopped at the wrapped line and record 07's unfiled row, the next \
         list item, went unread"
    );

    let nested = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n  {RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&nested),
        "an unfiled row nested under a filed one is a row, and it opens with no issue"
    );
}

/// The CommonMark list markers a row can open with, each followed by the
/// space or tab that makes it one. `- ` is the control: the one marker the walk
/// read before [#486](https://github.com/sujanto-gaws/kelir/issues/486).
const LIST_MARKERS: [&str; 10] = [
    "- ", "* ", "+ ", "2. ", "2) ", "-\t", "*\t", "1. ", "10) ", "3.\t",
];

/// `row`, a record 07 row opening `  - `, written under `marker` instead.
fn under_marker(row: &str, marker: &str) -> String {
    let text = row
        .strip_prefix("  - ")
        .expect("record 07's rows open `  - `");
    format!("  {marker}{text}")
}

/// **A follow-up under any list marker is a row**
/// ([#486](https://github.com/sujanto-gaws/kelir/issues/486)).
///
/// [Record 17](../../projects/verifications/17.%20Sprint%2019%20and%20Sprint%2017%20Independent%20Pass.md)
/// finding 5: under `two:`, record 07's filed row and then its unfiled row
/// written with `* `, `+ `, `2. ` or `-` and a tab. The walk started a row only
/// at `- `, joined the unfiled row to the filed one as a wrapped line, and
/// passed all four. CommonMark renders each as a list item of its own.
#[test]
fn a_follow_up_under_any_list_marker_is_a_row() {
    for marker in LIST_MARKERS {
        let unfiled = record_with_follow_ups(&format!(
            "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n{}",
            under_marker(RECORD_07_UNFILED_ROW, marker)
        ));
        assert!(
            refuses_the_unfiled_row(&unfiled),
            "record 07's unfiled row under {marker:?}, after a filed row, was joined to it and \
             went unread: {:?}",
            read_aftermath(&unfiled)
        );

        let filed = record_with_follow_ups(&format!(
            "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n{}",
            under_marker(RECORD_07_FILED_ROW, marker)
        ));
        assert!(
            accepted(&filed),
            "a row under {marker:?} opening with its issue link is a filed follow-up: {:?}",
            read_aftermath(&filed)
        );
    }

    // The struck-through row still starts a row with no marker at all.
    let struck = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n  \
         ~~[#999](https://github.com/sujanto-gaws/kelir/issues/999) — a duplicate~~\n{}",
        under_marker(RECORD_07_UNFILED_ROW, "* ")
    ));
    assert!(
        refuses_the_unfiled_row(&struck),
        "a `~~` row and a `* ` row under it are two rows, and only the second is unfiled: {:?}",
        read_aftermath(&struck)
    );
}

/// **A marker with a space and nothing after it starts a row, and the line
/// below is its text** ([#486](https://github.com/sujanto-gaws/kelir/issues/486)).
///
/// [`follow_up_block`] hands [`after_list_marker`] the untrimmed line, so the
/// trailing space is still there to make `  * ` a marker. Only a bare marker
/// ending its line is rule 10's stated limit.
#[test]
fn a_marker_with_only_a_space_after_it_starts_a_row() {
    for marker in ["* ", "- ", "2. ", "+\t"] {
        let unfiled = record_with_follow_ups(&format!(
            "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n  {marker}\n    {}",
            RECORD_07_UNFILED_ROW.trim_start_matches("  - ")
        ));
        assert!(
            refuses_the_unfiled_row(&unfiled),
            "{marker:?} and nothing else, with record 07's unfiled row on the line below, is \
             one unfiled row: {:?}",
            read_aftermath(&unfiled)
        );

        let filed = record_with_follow_ups(&format!(
            "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n  {marker}\n    {}",
            RECORD_07_FILED_ROW.trim_start_matches("  - ")
        ));
        assert!(
            accepted(&filed),
            "{marker:?} and nothing else, with a filed row's text on the line below, is one \
             filed row: {:?}",
            read_aftermath(&filed)
        );
    }
}

/// **A wrapped line that only looks like a marker is still a wrapped line**
/// ([#486](https://github.com/sujanto-gaws/kelir/issues/486)).
///
/// The other side of [`a_follow_up_under_any_list_marker_is_a_row`]: a marker
/// is one only with a space or a tab after it, and only at the line's start.
/// Each line here is joined to the filed row above it, so the row stays filed.
#[test]
fn a_line_that_only_looks_like_a_marker_continues_the_row() {
    for wrapped in [
        "**bold**, which opens with `*` and is not a `*` marker",
        "*emphasis* opens with `*` too",
        "2026-09-17, a date",
        "2.5 hours, a number and a stop",
        "404 is an issue number, not an ordered marker",
        "1234567890. ten digits, one more than CommonMark allows",
        "-1 is the exit code",
        "--force is a flag",
        "+1 from the pass",
        "and a marker later in the line: - * + 2. 2)",
    ] {
        let body = record_with_follow_ups(&format!(
            "- **Follow-ups filed:** one:\n{RECORD_07_FILED_ROW}\n    {wrapped}"
        ));
        let (_, rows) = follow_up_block(&body).expect("the block is readable");
        assert_eq!(
            rows.len(),
            1,
            "{wrapped:?} is a wrapped line of the row above, not a row: {rows:?}"
        );
        assert!(
            accepted(&body),
            "{wrapped:?} under a filed row: {:?}",
            read_aftermath(&body)
        );
    }
}

/// **An answer of `none` does not excuse the rows under it**
/// ([#467](https://github.com/sujanto-gaws/kelir/issues/467)).
///
/// Not one of finding 4's shapes: it is the door behind shape A. Before this
/// change a `none` answer skipped the rows entirely, so an exact `none` with
/// record 07's row beneath it passed as surely as `none of it filed yet` did.
#[test]
fn an_answer_of_none_does_not_excuse_the_rows_under_it() {
    let contradicted = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** none\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&contradicted),
        "the answer said `none` and a follow-up with no issue sat under it, unread"
    );
}

/// **What rule 10 accepts is still accepted**, so the shapes above are refused
/// for what they carry rather than because the rule now refuses everything.
///
/// Record 07 as it is, the answer `none` alone, a row opening with an issue
/// link, and a withdrawn struck-through one — plus C and D with the unfiled
/// row filed, which is what shows a blank line or a wrapped line is not itself
/// the thing refused.
#[test]
fn the_shapes_rule_10_accepts_are_still_accepted() {
    let record_07 =
        fs::read_to_string(repository_root().join(RECORD_07)).expect("record 07 is readable");
    assert!(
        accepted(&record_07),
        "record 07 as it is, with #440 opening the row that was unfiled, passes rule 10: {:?}",
        read_aftermath(&record_07)
    );

    for answer in ["none", "`none`", "None."] {
        let clean = record_with_follow_ups(&format!("- **Follow-ups filed:** {answer}"));
        assert!(
            accepted(&clean),
            "the answer {answer:?} alone says a clean run filed nothing: {:?}",
            read_aftermath(&clean)
        );
    }

    let filed = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** one:\n{RECORD_07_FILED_ROW}"
    ));
    assert!(
        accepted(&filed),
        "a row opening with an issue link is a filed follow-up"
    );

    let withdrawn = record_with_follow_ups(
        "- **Follow-ups filed:** one, withdrawn:\n  \
         - ~~[#999](https://github.com/sujanto-gaws/kelir/issues/999) — a duplicate~~ · \
         **only observed** · withdrawn in favour of #404",
    );
    assert!(
        accepted(&withdrawn),
        "a withdrawn follow-up opening with a struck-through issue link is still a filed one"
    );

    let c_filed = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n\n{RECORD_07_FILED_ROW}"
    ));
    assert!(
        accepted(&c_filed),
        "shape C with both rows filed: a loose list is not a defect"
    );

    let d_filed = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** two:\n{RECORD_07_FILED_ROW}\n    \
         and the issue carries the reproduction in full\n{RECORD_07_FILED_ROW}"
    ));
    assert!(
        accepted(&d_filed),
        "shape D with both rows filed: a wrapped row is not a defect"
    );

    // Seen green 2026-09-16 and closed here: letting the walk run past a
    // column-0 line left every test green, because no fixture put an indented
    // list under the *next* top-level item. Record 02's Aftermath has exactly
    // that shape, and its rows are not follow-ups.
    let next_item = record_with_follow_ups(
        "- **Follow-ups filed:** none\n\
         - **Carried into the sprint plan:**\n  \
         - the Sprint 5 exit debt, closed by items 1–4",
    );
    assert!(
        accepted(&next_item),
        "the block ends at the next top-level item, so rows under another label are not read \
         as follow-ups: {:?}",
        read_aftermath(&next_item)
    );
}

/// **What rule 10 refused before [#467](https://github.com/sujanto-gaws/kelir/issues/467)
/// is still refused** — record 07's own row first, then the four refusals of
/// 2026-09-14, committed here rather than run once and deleted.
#[test]
fn record_07_at_00a27b4_and_the_earlier_probes_are_still_refused() {
    let control = record_with_follow_ups(&format!(
        "- **Follow-ups filed:** one:\n{RECORD_07_UNFILED_ROW}"
    ));
    assert!(
        refuses_the_unfiled_row(&control),
        "finding 4's control: record 07's unfiled row, alone under `one:`"
    );

    // Record 07 read from disk and never written: the row is swapped in memory.
    let record_07 =
        fs::read_to_string(repository_root().join(RECORD_07)).expect("record 07 is readable");
    let filed_row = "  - [#440](https://github.com/sujanto-gaws/kelir/issues/440) — ";
    assert_eq!(
        record_07
            .lines()
            .filter(|line| line.starts_with(filed_row))
            .count(),
        1,
        "record 07 carries its #440 row exactly once — a settled record is not amended, so \
         this failing means somebody amended it"
    );
    let at_00a27b4 = record_07
        .lines()
        .map(|line| {
            if line.starts_with(filed_row) {
                RECORD_07_UNFILED_ROW
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        refuses_the_unfiled_row(&at_00a27b4),
        "record 07 with its rollback row restored to `00a27b4` is the defect rule 10 exists for"
    );

    let contains_but_does_not_open = record_with_follow_ups(
        "- **Follow-ups filed:** one:\n  \
         - the rollback exits 1 · **reproduced** · \
         see [#440](https://github.com/sujanto-gaws/kelir/issues/440)",
    );
    assert!(
        !accepted(&contains_but_does_not_open),
        "a row that contains an issue link but does not open with one"
    );

    let pull_request = record_with_follow_ups(
        "- **Follow-ups filed:** one:\n  \
         - [#367](https://github.com/sujanto-gaws/kelir/pull/367) — the guard that was bypassed",
    );
    assert!(
        !accepted(&pull_request),
        "a row opening with a pull request rather than an issue"
    );

    let no_label = record_with_follow_ups("- **Carried into the sprint plan:** the rollback fix");
    assert_eq!(
        read_aftermath(&no_label),
        Aftermath::Unreadable,
        "an Aftermath with no follow-ups label"
    );

    let promised = record_with_follow_ups("- **Follow-ups filed:** one:");
    assert_eq!(
        read_aftermath(&promised),
        Aftermath::Unreadable,
        "an answer that promises a follow-up with no row under it"
    );
}

// ---------------------------------------------------------------------------
// Where a citation points (#483)
// ---------------------------------------------------------------------------

/// Record 15, as record 07 cites it.
const RECORD_15_CITED: &str = "../verifications/15.%20Sprint%2016%20Independent%20Pass.md";

/// Record 15, as it is named on disk.
const RECORD_15: &str = "15. Sprint 16 Independent Pass.md";

/// Record 16, which no release before `v0.8.0` cited.
const RECORD_16_CITED: &str = "../verifications/16.%20Sprint%2018%20Independent%20Pass.md";

/// Record 19, which no release has cited.
const RECORD_19_CITED: &str = "../verifications/19.%20Sprint%2020%20Independent%20Pass.md";

/// Record 15 with a query that reads like a path to record 19. A browser opens
/// record 15.
const QUERY_OVER_15: &str = "../verifications/15.%20Sprint%2016%20Independent%20Pass.md?/../../verifications/19.%20Sprint%2020%20Independent%20Pass.md";

/// Record 17 finding 2's three shapes: record 15 spelt twice more, and a path
/// through `verifications/` to a file that is not a verification record.
const FINDING_2_SHAPES: [&str; 3] = [
    "../verifications/./15.%20Sprint%2016%20Independent%20Pass.md",
    "../verifications/%31%35.%20Sprint%2016%20Independent%20Pass.md",
    "../verifications/../releases/07.%20Release%20v0.7.0.md",
];

/// A governed release, as the rules carry it.
fn release(version: (u32, u32, u32), name: &str, cited: &[&str]) -> Release {
    (
        version,
        name.to_owned(),
        cited.iter().map(|&record| record.to_owned()).collect(),
    )
}

/// `v0.7.0` citing record 15, then `v0.8.0` citing `cited` — the history
/// record 17 finding 2 planted its probes into.
fn after_v0_7_0(cited: &[&str]) -> Vec<Release> {
    vec![
        release((0, 7, 0), "07. Release v0.7.0.md", &[RECORD_15_CITED]),
        release((0, 8, 0), "08. Release v0.8.0.md", cited),
    ]
}

/// **Every spelling of record 15 is record 15.** The accepted shapes, each
/// sent through the one function rules 2, 3, 6 and 7 read a citation by.
#[test]
fn a_citation_is_read_as_the_record_it_points_at() {
    let record_15 = Ok(RECORD_15.to_owned());

    for (cited, how) in [
        (RECORD_15_CITED, "the house spelling"),
        (FINDING_2_SHAPES[0], "finding 2: a `./` in the path"),
        (FINDING_2_SHAPES[1], "finding 2: the number percent-encoded"),
        (
            "../verifications/..%2Fverifications%2F15.%20Sprint%2016%20Independent%20Pass.md",
            "an encoded `%2F`",
        ),
        (
            "../verifications/..%2fverifications%2f15.%20Sprint%2016%20Independent%20Pass.md",
            "an encoded `%2f`, lower case",
        ),
        (
            "../../projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            "the long way round, through the repository root",
        ),
        (
            "..//verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            "a doubled `/`",
        ),
        (
            "../verifications/15.%20Sprint%2016%20Independent%20Pass.md#7-findings",
            "a `#fragment`",
        ),
        (
            "../verifications/15.%20Sprint%2016%20Independent%20Pass.md?plain=1",
            "a `?query`",
        ),
        (
            QUERY_OVER_15,
            "a `?query` shaped like a path to record 19, which a browser does not follow",
        ),
        (
            "../verifications/15. Sprint 16 Independent Pass.md",
            "no encoding at all",
        ),
    ] {
        assert_eq!(normalised_citation(cited), record_15, "{how}: {cited}");
    }
}

/// **A citation that points anywhere but directly into
/// `projects/verifications/` names no record**, whatever it reaches on disk.
#[test]
fn a_citation_outside_the_verifications_folder_is_refused() {
    for (cited, refusal, how) in [
        (
            FINDING_2_SHAPES[2],
            Refusal::OutsideVerifications,
            "finding 2: through `verifications/` to a release record",
        ),
        (
            "../verifications/..%2Freleases%2F07.%20Release%20v0.7.0.md",
            Refusal::OutsideVerifications,
            "the same, with the separators encoded",
        ),
        (
            "../verifications/sub/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::OutsideVerifications,
            "a subfolder",
        ),
        (
            "../verifications/../../projects/Verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::OutsideVerifications,
            "`Verifications/`, which Windows opens and Linux does not",
        ),
        (
            "../../../../verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::OutsideVerifications,
            "above the repository root",
        ),
        (
            "/projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::NotRelative,
            "an absolute path",
        ),
        (
            "%2Fprojects/verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::NotRelative,
            "an absolute path, its `/` encoded",
        ),
        (
            "https://github.com/sujanto-gaws/kelir/blob/main/projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::NotRelative,
            "a URL",
        ),
        (
            "C:/kelir/projects/verifications/15.%20Sprint%2016%20Independent%20Pass.md",
            Refusal::NotRelative,
            "a drive",
        ),
        (
            "../verifications/..%5Creleases%5C07.%20Release%20v0.7.0.md",
            Refusal::Backslash,
            "an encoded backslash",
        ),
    ] {
        assert_eq!(normalised_citation(cited), Err(refusal), "{how}: {cited}");
    }

    assert_eq!(
        normalised_citation("../verifications/%2531%2535.%20Sprint%2016%20Independent%20Pass.md"),
        Ok("%31%35. Sprint 16 Independent Pass.md".to_owned()),
        "a double-encoded number is decoded once, as a browser does — to a name with no \
         number, which is on no disk"
    );

    let on_disk = verification_files();
    assert!(
        unresolved(
            "../verifications/%2531%2535.%20Sprint%2016%20Independent%20Pass.md",
            &on_disk
        )
        .is_some(),
        "so rule 6 refuses it"
    );
    assert!(
        unresolved(
            "../verifications/15.%20sprint%2016%20independent%20pass.md",
            &on_disk
        )
        .is_some(),
        "rule 6 matches a name against the listing byte for byte, as CI's Linux would, \
         even where this filesystem would open it"
    );
    assert!(
        unresolved(FINDING_2_SHAPES[2], &on_disk).is_some(),
        "rule 6 refuses finding 2's release record, which is on disk"
    );
    assert!(
        unresolved(RECORD_15_CITED, &on_disk).is_none()
            && unresolved(FINDING_2_SHAPES[0], &on_disk).is_none(),
        "and accepts record 15 in the house spelling and another, so the refusals above are \
         about where the citations point"
    );
}

/// **The whole path is the citation, and a `?query` or `#fragment` does not
/// stop it being one.**
#[test]
fn a_citation_is_read_whole_from_the_link() {
    let body = format!(
        "[a]({}) [b]({}#7) [c](../verifications/) `{}` [d]({}/) [e]({}?plain=1)\n\
         [r]:{}\n",
        FINDING_2_SHAPES[0],
        FINDING_2_SHAPES[2],
        RECORD_16_CITED,
        RECORD_15_CITED,
        RECORD_15_CITED,
        RECORD_19_CITED
    );

    assert_eq!(
        citations(&body),
        BTreeSet::from([
            FINDING_2_SHAPES[0].to_owned(),
            format!("{}#7", FINDING_2_SHAPES[2]),
            format!("{RECORD_15_CITED}?plain=1"),
            RECORD_19_CITED.to_owned(),
        ]),
        "the prefix is kept so the path can be resolved, a query or fragment is allowed, and a \
         reference definition with no space after its colon is read without the `[r]:`; the \
         folder link, a code span and a trailing `/` are not citations, as they were not \
         before #483"
    );
}

/// **Rules 2 and 7 refuse each of finding 2's shapes**, planted after
/// `v0.7.0` exactly as record 17 planted them, and accept the control.
#[test]
fn rules_2_and_7_refuse_a_record_already_cited_under_another_spelling() {
    for shape in FINDING_2_SHAPES {
        let releases = after_v0_7_0(&[shape]);

        assert_eq!(
            recycled(&releases).len(),
            1,
            "rule 2 counted {shape} as a record no earlier release cited"
        );
        assert_eq!(
            backward(&releases).len(),
            1,
            "rule 7 did not refuse {shape}"
        );
    }

    let control = after_v0_7_0(&[RECORD_15_CITED]);
    assert_eq!(
        recycled(&control).len(),
        1,
        "the house spelling of record 15"
    );
    assert_eq!(
        backward(&control).len(),
        1,
        "the house spelling of record 15"
    );

    // A number the path passes through and leaves: the text after the last
    // `verifications/` reads 99, and the link opens record 15. Windows opens
    // it even though no folder `99. A Pass` exists, because it resolves `..`
    // before looking.
    let passes_through_99 =
        "../verifications/99.%20A%20Pass/../15.%20Sprint%2016%20Independent%20Pass.md";
    let releases = after_v0_7_0(&[passes_through_99]);
    assert_eq!(
        recycled(&releases).len(),
        1,
        "rule 2 counted {passes_through_99} as a new record"
    );
    assert_eq!(
        backward(&releases).len(),
        1,
        "rule 7 read 99 off {passes_through_99}, which is record 15"
    );

    let releases = after_v0_7_0(&[QUERY_OVER_15]);
    assert!(
        recycled(&releases).len() == 1 && backward(&releases).len() == 1,
        "rules 2 and 7 read {QUERY_OVER_15} as record 19; a browser opens record 15"
    );

    let new = after_v0_7_0(&[RECORD_16_CITED]);
    assert!(
        recycled(&new).is_empty() && backward(&new).is_empty(),
        "record 16, which v0.7.0 did not cite, is a new reading"
    );

    let new_and_old = after_v0_7_0(&[RECORD_16_CITED, FINDING_2_SHAPES[0]]);
    assert!(
        recycled(&new_and_old).is_empty() && backward(&new_and_old).is_empty(),
        "an old record cited beside a new one takes nothing away"
    );
}

/// **A governed release that points at no numbered record is red, not
/// skipped** — the `continue` that let all three of finding 2's shapes past
/// rule 7 under the old reading.
#[test]
fn a_release_with_no_numbered_citation_is_refused() {
    for cited in [
        FINDING_2_SHAPES[2],
        "../verifications/A%20Pass%20With%20No%20Number.md",
        "../verifications/%2531%2535.%20Sprint%2016%20Independent%20Pass.md",
        "../verifications/sub/16.%20Sprint%2018%20Independent%20Pass.md",
    ] {
        let alone = vec![release((0, 8, 0), "08. Release v0.8.0.md", &[cited])];

        assert_eq!(
            backward(&alone).len(),
            1,
            "rule 7 skipped a release whose only citation is {cited}"
        );
        assert_eq!(
            recycled(&alone).len(),
            usize::from(normalised_citation(cited).is_err()),
            "rule 2 refuses {cited} exactly when it points outside the folder: a name inside              it is new to rule 2 whether or not it has a number, and rule 6 decides whether it              is there"
        );
    }

    let silent = vec![release((0, 8, 0), "08. Release v0.8.0.md", &[])];
    assert!(
        backward(&silent).is_empty() && recycled(&silent).is_empty(),
        "a release citing nothing is rule 1's alone"
    );
}
