//! Every architecture decision record agrees with the index, and says where it
//! stands ([#346](https://github.com/sujanto-gaws/kelir/issues/346)).
//!
//! # Why a test and not a convention
//!
//! [Standards/06](../../docs/standards/06.%20Architecture%20Decision%20Records.md)
//! §5 has said since Sprint 0 that a record becomes `Adopted` when its pull
//! request merges. **It had fired once in seven merges.** Five records —
//! ADR-0029 through 0032 and ADR-0034 — shipped in `v0.6.0` and still read
//! `Draft`, so a reader opening `docs/README.md` to ask *what governs comments
//! today* was told the answer was under review. A sixth, ADR-0033, was flipped
//! in the record and not in the index, which is the same drift from the other
//! side.
//!
//! **A convention followed once in seven is not a convention**, and this is
//! the shape [`deployment_images_are_pinned`](deployment_images_are_pinned.rs)
//! already has for image tags (**D-62**), for the same stated reason: a pin
//! nothing regenerates ages the way a list nothing regenerates does.
//!
//! # The rules
//!
//! For every `docs/architectures/adr/NNNN. *.md`, read from the **first**
//! `**Status:**` line — the template carries a second one as the form to copy,
//! and a walk that took the last would read every record as the template:
//!
//! 1. **The status is in the vocabulary** — `Draft`, `Adopted`, `Rejected`,
//!    `Superseded`, `Retired`, or `Template` for `0000`.
//! 2. **The index and the record agree.** `docs/README.md`'s
//!    `architectures/adr/` table is the only index (§3), so a status that lives
//!    in one and not the other is a reader being told two things.
//! 3. **An `Adopted` record carries its decision date.** `Adopted` with the
//!    date still `—` is a flip that stopped half way, which is what ADR-0033
//!    was.
//! 4. **A `Draft` record names what is holding it** — `**Blocked by:** #NNN`,
//!    standard §5.1. This is the rule that catches the recurrence: without it a
//!    merged record can sit at `Draft` indefinitely and look exactly like one
//!    written yesterday. **And the blocker is not one this tree delivers**
//!    ([#545](https://github.com/sujanto-gaws/kelir/issues/545)): a `Draft`
//!    is refused when every issue its `Blocked by:` names is **cited** in
//!    `CHANGELOG.md`'s `[Unreleased]` section.
//!
//! # Rule 4's second half: what "cited" means
//!
//! ADR-0041 merged in #537 as `Draft`, `**Blocked by:** #519`, and that merge
//! closed #519. The first half accepted it, because it named a blocker; #539
//! flipped it by hand. The tree's own statement that an issue is delivered is
//! its changelog entry, so that is what the second half reads.
//!
//! **Cited is narrower than named.** An issue is cited when it appears — as
//! `#519`, `/issues/519` or `/pull/519` — in an entry's **reference**: the
//! balanced parenthesis right after the entry's bold headline, where the house
//! style says what the entry delivers. Anywhere else it is *mentioned*: a
//! follow-up at the end of a body (`[Unreleased]` gives #511 that shape
//! today), a nested item, the reason a record stays `Draft`.
//!
//! **The narrow reading is not a nicety.** #372 merged ADR-0037 `Draft`
//! behind #371 in a tree whose `[Unreleased]` named #371 twice, and #371 is
//! still open. Read as *any `#371` in `[Unreleased]`*, the rule refuses that
//! tree (mutation M1 with probe 3, below); read as a reference, it does not.
//!
//! **Only `[Unreleased]`**, as #545 asks: every entry is written there before
//! a release moves it, so from this rule on a delivery is seen while it is
//! unreleased. **Every blocker, not any**: a record held by two issues is
//! still held by the one not delivered.
//!
//! # What this deliberately does not check
//!
//! **Whether the file is on `main`.** That is the property the rule is really
//! about, and it needs `git log`. When this was written CI did not have it —
//! `actions/checkout` ran at depth 1 — and since
//! [#453](https://github.com/sujanto-gaws/kelir/issues/453) the backend job
//! fetches full history, so the question is now reachable and simply not
//! asked here. Rule 4 is the substitute — it does not know *when* a record merged, and it does not need
//! to, because a `Draft` that names an undelivered blocker is honest whenever
//! it merged and one that does not is not.
//!
//! **Whether the impacted documents in §4.1 are actually done.** That is
//! judgement over prose, and §7 puts it on the author and the reviewer.
//!
//! **A delivery the changelog does not record.** The second half reads the
//! changelog, not GitHub, so **it would not have caught #537 itself**: #537
//! closed #519 and wrote no entry, and #519 first appeared under `[Unreleased]`
//! in #543 (`5791681`). Probe 1 is that tree, and it passes. The rule fires on
//! the first tree that records the delivery — #543's, had #539 not come first.
//! Nor does it see a reference that does not directly follow a bold headline.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Four mutations, run 2026-09-06 — one per rule, each against a real record,
//! all four red:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | ADR-0029 returned to `**Status:** Draft`, **the exact state #346 found** | *a Draft record names what is holding it*, and *the index and the record agree* |
//! | ADR-0031's index row returned to `**Draft**` | *the index and the record agree* |
//! | ADR-0034's decision date returned to `—` | *an Adopted record carries its decision date* |
//! | ADR-0030's status set to `Accepted`, a plausible near-miss for `Adopted` | *every status is in the vocabulary*, and *the index and the record agree* |
//!
//! **The first is the one that matters**, and it is #346's AC4: the rule has
//! been seen red against a merged record left at `Draft`. It reddens two tests
//! rather than one, which is the property worth having — a record that drifts
//! is caught from both the record's side and the index's.
//!
//! ## Rule 4's second half, 2026-09-25
//!
//! **Seen red against ADR-0041 as #537 left it**: in memory by
//! `adr_0041_as_537_left_it_is_refused`, on disk by probe 2. Each probe
//! changed the tree, ran this file and was restored; the positive control ran
//! last, so a green probe is the rule not firing rather than the probe not
//! arriving.
//!
//! | Probe | Tree | Result |
//! |---|---|---|
//! | 1 | The literal #537 tree: ADR-0041, `CHANGELOG.md` and `docs/README.md` from `4c4e6a3` | **Green** — the limit above |
//! | 2 | ADR-0041 from `4c4e6a3`, its index row at `Draft`, `main`'s changelog (#519 under *Added*) | **Red**, this test only, naming #519 |
//! | 3 | Cited elsewhere: `CHANGELOG.md` from `26a2bdb` (#372), ADR-0037 `Draft` behind #371 | **Green** |
//! | 4 | Cited in a released section: `main`, whose `[0.7.0]` names #371 | **Green** |
//! | 5 | **Positive control**: a synthetic `9999` record and index row, `Draft` behind #520, FR-INT-001's reference in `[Unreleased]` | **Red**, this test only, naming #520 |
//!
//! Six mutations of the rule, each alone, all red:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | M1 — every number in `[Unreleased]` is cited | *mentioned but not cited*; with probe 3's tree, the tree test too |
//! | M2 — the whole changelog is read | *cited only in a released section*, *the readers* |
//! | M3 — any blocker, not every | *held by two issues* |
//! | M4 — the `Blocked by:` description's issues are blockers | *the readers* |
//! | M5 — a `#` after a letter is read | *the readers*, once `z.md#525)` was added; green before it |
//! | M6 — the second half removed | *as #537 left it*, *held by two issues* |

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// The statuses [standards/06](../../docs/standards/06.%20Architecture%20Decision%20Records.md)
/// §5 defines, plus the template's own.
const VOCABULARY: [&str; 6] = [
    "Draft",
    "Adopted",
    "Rejected",
    "Superseded",
    "Retired",
    "Template",
];

struct Record {
    number: String,
    name: String,
    status: String,
    /// The `**Decision date:**` field, verbatim — `—` when unset.
    decision_date: String,
    /// The whole metadata block, for the `Blocked by:` search.
    header: String,
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

/// The value of the first `**Field:**` occurrence, up to the ` · ` that ends it.
///
/// **First, not last.** `0000. ADR Template.md` carries a second metadata block
/// as the form to copy, and reading that one would report every record as the
/// template's placeholder.
fn first_field(source: &str, field: &str) -> Option<String> {
    let marker = format!("**{field}:**");
    let at = source.find(&marker)? + marker.len();
    let rest = &source[at..];
    let end = rest
        .find(" · ")
        .or_else(|| rest.find('\n'))
        .unwrap_or(rest.len());

    Some(rest[..end].trim().to_owned())
}

/// One record's metadata, from its file name and its text.
///
/// Separate from the walk so that a record as an earlier commit left it can be
/// judged from text held in this file (the fixtures under *Rule 4, second
/// half*), by the same code that judges the folder.
fn parse_record(name: String, source: &str) -> Record {
    // The metadata block is everything before the first section heading.
    let header = source
        .split("\n## ")
        .next()
        .expect("a split yields one part")
        .to_owned();

    Record {
        number: name.chars().take(4).collect(),
        status: first_field(&header, "Status")
            .unwrap_or_else(|| panic!("{name} has no **Status:** field")),
        decision_date: first_field(&header, "Decision date").unwrap_or_default(),
        header,
        name,
    }
}

fn records() -> Vec<Record> {
    let directory = repository_root().join("docs/architectures/adr");
    let mut records = Vec::new();

    for entry in fs::read_dir(&directory).expect("the adr directory is readable") {
        let path = entry.expect("a directory entry").path();

        if path.extension().is_none_or(|extension| extension != "md") {
            continue;
        }

        let name = path
            .file_name()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();
        let source = fs::read_to_string(&path).expect("the record is readable");

        records.push(parse_record(name, &source));
    }

    // The guard every source walk in this suite carries: a walk that finds
    // nothing passes every assertion under it.
    assert!(
        records.len() >= 30,
        "the walk found {} records, which is too few — the folder moved",
        records.len()
    );

    records.sort_by(|left, right| left.number.cmp(&right.number));
    records
}

/// The `architectures/adr/` table in `docs/README.md`, as number → status.
///
/// The status is the row's last cell, with emphasis stripped: the table writes
/// some of them `**Superseded** by 0010`, and what is being compared is the
/// word rather than the formatting around it.
fn index() -> Vec<(String, String)> {
    let readme = fs::read_to_string(repository_root().join("docs/README.md"))
        .expect("docs/README.md is readable");
    let mut rows = Vec::new();

    for line in readme.lines() {
        let Some(rest) = line.strip_prefix("| [") else {
            continue;
        };

        if !line.contains("(architectures/adr/") {
            continue;
        }

        let number: String = rest.chars().take(4).collect();

        if !number.chars().all(|character| character.is_ascii_digit()) {
            continue;
        }

        let last = line
            .trim_end()
            .trim_end_matches('|')
            .rsplit('|')
            .next()
            .expect("a row has cells")
            .replace('*', "");
        let status = last
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();

        rows.push((number, status));
    }

    assert!(
        rows.len() >= 30,
        "the index has {} adr rows, which is too few — the table moved",
        rows.len()
    );

    rows
}

#[test]
fn every_status_is_in_the_vocabulary() {
    let wrong: Vec<_> = records()
        .into_iter()
        .filter(|record| !VOCABULARY.contains(&record.status.as_str()))
        .map(|record| format!("{} reads {:?}", record.name, record.status))
        .collect();

    assert!(
        wrong.is_empty(),
        "a record's status is not one standards/06 §5 defines — {VOCABULARY:?}:\n  {}",
        wrong.join("\n  ")
    );
}

/// **The drift ADR-0033 actually had**: flipped in the record, left in the
/// index, so the folder's own index disagreed with the folder.
#[test]
fn the_index_and_the_record_agree() {
    let index = index();
    let records = records();

    let mismatched: Vec<_> = records
        .iter()
        .map(|record| {
            let indexed = index
                .iter()
                .find(|(number, _)| number == &record.number)
                .map(|(_, status)| status.clone());

            (record, indexed)
        })
        .filter(|(record, indexed)| indexed.as_deref() != Some(record.status.as_str()))
        .map(|(record, indexed)| {
            format!(
                "{} — the record says {:?}, docs/README.md says {}",
                record.name,
                record.status,
                indexed.map_or("nothing at all".to_owned(), |status| format!("{status:?}"))
            )
        })
        .collect();

    assert!(
        mismatched.is_empty(),
        "docs/README.md is the only index (standards/06 §3) and it disagrees with the records:\n  {}",
        mismatched.join("\n  ")
    );
}

/// A flip that changed the status and not the date is a flip that stopped half
/// way, and the date is what says *when this became the thing the code follows*.
#[test]
fn an_adopted_record_carries_its_decision_date() {
    let undated: Vec<_> = records()
        .into_iter()
        .filter(|record| record.status == "Adopted")
        .filter(|record| record.decision_date.is_empty() || record.decision_date == "—")
        .map(|record| record.name)
        .collect();

    assert!(
        undated.is_empty(),
        "an Adopted record's decision date is its merge date, not '—' (standards/06 §5.1):\n  {}",
        undated.join("\n  ")
    );
}

/// **The rule that catches the recurrence.**
///
/// A record merged and left at `Draft` is indistinguishable from one written
/// yesterday unless it says what is holding it. Standard §5.1 is the field;
/// this is what refuses a record without it.
#[test]
fn a_draft_record_names_what_is_holding_it() {
    let silent: Vec<_> = records()
        .into_iter()
        .filter(|record| record.status == "Draft")
        .filter(|record| !record.header.contains("**Blocked by:**"))
        .map(|record| record.name)
        .collect();

    assert!(
        silent.is_empty(),
        "a Draft record carries '**Blocked by:** #NNN — <what is unfinished>' \
         (standards/06 §5.1), so a merged record left at Draft is visible \
         rather than looking like one written yesterday:\n  {}",
        silent.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Rule 4, second half: a `Draft` whose blocker the same tree delivers
// ---------------------------------------------------------------------------

/// `CHANGELOG.md`'s `[Unreleased]` section, heading included, up to the next
/// `## ` heading or the end of the file. `None` when there is no such heading.
///
/// **Only `[Unreleased]`, because every entry is there first.** A pull
/// request writes its entry under `[Unreleased]`, and a release only moves it,
/// so from this rule on a delivery is seen before it is released. What a
/// released section cites is history the rule did not govern, and it may be
/// part of an issue that stayed open — `[0.7.0]` names
/// [#371](https://github.com/sujanto-gaws/kelir/issues/371) twice, and #371 is
/// what legitimately holds ADR-0037 at `Draft` today.
fn unreleased_section(changelog: &str) -> Option<&str> {
    let heading = "## [Unreleased]";
    let start = if changelog.starts_with(heading) {
        0
    } else {
        changelog.find(&format!("\n{heading}"))? + 1
    };
    let section = &changelog[start..];
    let heading_end = section.find('\n').map_or(section.len(), |at| at + 1);
    let end = section[heading_end..]
        .find("\n## ")
        .map_or(section.len(), |at| heading_end + at + 1);

    Some(&section[..end])
}

/// The leading run of ASCII digits in `text`, as a number, and what follows it.
fn leading_number(text: &str) -> Option<(u32, &str)> {
    let digits = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());

    if digits == 0 {
        return None;
    }

    Some((text[..digits].parse().ok()?, &text[digits..]))
}

/// Every issue or pull request number `text` names: `#519`, `/issues/519` or
/// `/pull/519`. GitHub numbers issues and pull requests from one sequence, so
/// the three spellings name one thing.
///
/// A `#` preceded by a letter or digit is an anchor (`….md#9-troubleshooting`),
/// and one followed by a letter or `-` is not a reference either. `#5190` is
/// 5190, never 519.
fn issue_numbers(text: &str) -> Vec<u32> {
    let bytes = text.as_bytes();
    let mut numbers = Vec::new();

    for (at, _) in text.match_indices('#') {
        if at > 0 && bytes[at - 1].is_ascii_alphanumeric() {
            continue;
        }

        let Some((number, after)) = leading_number(&text[at + 1..]) else {
            continue;
        };

        if after.starts_with(|character: char| character.is_alphanumeric() || character == '-') {
            continue;
        }

        numbers.push(number);
    }

    for marker in ["/issues/", "/pull/"] {
        for (at, _) in text.match_indices(marker) {
            if let Some((number, _)) = leading_number(&text[at + marker.len()..]) {
                numbers.push(number);
            }
        }
    }

    numbers
}

/// The issues a record's `**Blocked by:**` field names, in order, once each.
///
/// Read up to the field's first ` — `: what follows describes what is
/// unfinished, and it may mention other issues without being held by them.
fn blockers(header: &str) -> Vec<u32> {
    let Some(value) = first_field(header, "Blocked by") else {
        return Vec::new();
    };
    let named = value.split(" — ").next().unwrap_or_default();
    let mut blockers = Vec::new();

    for number in issue_numbers(named) {
        if !blockers.contains(&number) {
            blockers.push(number);
        }
    }

    blockers
}

/// A changelog section's list items, each with its wrapped lines joined by a
/// space and its marker removed. An item ends at the next item, a blank line
/// or a heading, so a nested item is an entry of its own.
fn entries(section: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current: Option<String> = None;

    for line in section.lines() {
        let trimmed = line.trim_start();
        let heading = line.starts_with('#') && line.trim_start_matches('#').starts_with(' ');

        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            entries.extend(current.take());
            current = Some(item.to_owned());
        } else if trimmed.is_empty() || heading {
            entries.extend(current.take());
        } else if let Some(entry) = current.as_mut() {
            entry.push(' ');
            entry.push_str(trimmed);
        }
    }

    entries.extend(current);
    entries
}

/// The parenthesis that follows an entry's bold headline, balanced: the
/// entry's **reference**, as in `**JWSS actions run, …** ([#519](…), [ADR-0041](…))`.
///
/// That parenthesis is where the house style says what an entry delivers. An
/// issue named anywhere else in the entry is *mentioned*: a follow-up, the
/// issue a query came from, the reason a record stays `Draft`.
fn reference_of(entry: &str) -> Option<&str> {
    let headline = entry.strip_prefix("**")?;
    let close = headline.find("**")?;
    let after = headline[close + 2..].trim_start();

    if !after.starts_with('(') {
        return None;
    }

    let mut depth = 0_usize;

    for (at, character) in after.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;

                if depth == 0 {
                    return Some(&after[..=at]);
                }
            }
            _ => {}
        }
    }

    None
}

/// The issues a changelog section **cites**: those named in an entry's
/// reference (see [`reference_of`]).
fn cited_issues(section: &str) -> BTreeSet<u32> {
    entries(section)
        .iter()
        .filter_map(|entry| reference_of(entry))
        .flat_map(issue_numbers)
        .collect()
}

/// Rule 4's second half, for one record: `Some(blockers)` when the record is
/// `Draft`, names at least one blocker, and **every** blocker it names is cited
/// by `unreleased`.
///
/// **Every, not any.** A record held by two issues, one of them delivered, is
/// still held by the other. A `Draft` naming no blocker is the first half's
/// refusal, not this one's.
fn delivered_blockers(record: &Record, unreleased: &str) -> Option<Vec<u32>> {
    if record.status != "Draft" {
        return None;
    }

    let blockers = blockers(&record.header);

    if blockers.is_empty() {
        return None;
    }

    let cited = cited_issues(unreleased);

    blockers
        .iter()
        .all(|number| cited.contains(number))
        .then_some(blockers)
}

fn listed(numbers: &[u32]) -> String {
    numbers
        .iter()
        .map(|number| format!("#{number}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// **The drift ADR-0041 had** ([#545](https://github.com/sujanto-gaws/kelir/issues/545)):
/// `Draft`, `**Blocked by:** #519`, in a tree whose changelog records #519 as
/// delivered. The first half accepts it, because it names a blocker.
#[test]
fn a_draft_records_blocker_is_not_delivered_by_the_same_tree() {
    let changelog = fs::read_to_string(repository_root().join("CHANGELOG.md"))
        .expect("CHANGELOG.md is readable");
    let unreleased = unreleased_section(&changelog)
        .expect("CHANGELOG.md has a `## [Unreleased]` heading — the heading moved");

    let delivered: Vec<_> = records()
        .iter()
        .filter_map(|record| {
            delivered_blockers(record, unreleased).map(|blockers| {
                format!(
                    "{} — Blocked by {}, which CHANGELOG.md's [Unreleased] cites as delivered",
                    record.name,
                    listed(&blockers)
                )
            })
        })
        .collect();

    assert!(
        delivered.is_empty(),
        "a Draft record's blocker is delivered by this same tree, so nothing holds it: \
         flip it to Adopted with its decision date and index row, or name what \
         still holds it (standards/06 §5.1):\n  {}",
        delivered.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Rule 4, second half: fixtures
// ---------------------------------------------------------------------------

/// ADR-0041's metadata block **as #537 left it**: `git show
/// 4c4e6a3:"docs/architectures/adr/0041. …"`, lines 1–7 verbatim, and the
/// heading that ends the block.
const ADR_0041_AT_4C4E6A3: &str = r"# ADR-0041 — Every Workflow Transition Writes an Outbox Event, and After-Hooks Are Its First Consumer

**Status:** Draft · **Last updated:** 2026-09-24

**Blocked by:** #519 — the row this record is a criterion of; it is `Adopted` at that row's merge, which waits for record 18 (plan 16 §1.3)

**Decision date:** — · **Deciders:** Product owner · **Supersedes:** [ADR-0036](0036.%20The%20Hook%20Chain%20Ships%20Its%20Before%20Half%20First.md) in part: its after-hook clauses (standard 06 §6.3) · **Superseded by:** — · **Related:** [ADR-0034](0034.%20One%20Delivery%20Attempt,%20Recorded%20Rather%20Than%20Retried.md), [ADR-0036](0036.%20The%20Hook%20Chain%20Ships%20Its%20Before%20Half%20First.md), [ADR-0037](0037.%20ERP%20Posting%20Runs%20Inside%20the%20Document%20Transaction.md), `D-79`, `D-81`, FR-INT-005, FR-INT-007, issue [#519](https://github.com/sujanto-gaws/kelir/issues/519)

## 1. Context
";

/// `CHANGELOG.md` at `5791681` (#543), the first commit whose `[Unreleased]`
/// records #519 under *Added*: its head, its first upgrade note shortened, the
/// #519 entry verbatim, and the next release heading. #537 itself added no
/// entry (see the module doc).
const CHANGELOG_AT_5791681: &str = r"# Changelog

All notable changes to Kelir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as applied by the
[release process](docs/standards/04.%20Release%20Process.md).

While the major version is `0`, the public API may change in any release.

## [Unreleased]

### Upgrade notes

- **Only the system tenant's administrators can see external systems until a
  permission is granted** (FR-INT-001).

### Added

- **JWSS `actions` run, delivered after the transition commits**
  ([#519](https://github.com/sujanto-gaws/kelir/issues/519),
  [ADR-0041](docs/architectures/adr/0041.%20Every%20Workflow%20Transition%20Writes%20an%20Outbox%20Event,%20and%20After-Hooks%20Are%20Its%20First%20Consumer.md)).
  Every committed workflow transition writes one `Workflow.Transitioned` event
  to a new `outbox_events` table (migration `0045_outbox.sql`). A worker
  delivers it to the `after_workflow_transition` chain. A failure is retried
  at 30 s, 1, 2, 4 and 8 minutes, and dead-lettered on the sixth. A handler
  that fails five times in a row is paused, the tenant's administrators are
  told once, and it is tried again after ten minutes.
  - **Dead letters have no screen yet.** Query `outbox_events` where
    `status = 'DEAD_LETTER'`.

## [0.8.0] — 2026-09-17
";

/// ADR-0037's metadata block as it stands on `46d78c5`, verbatim through its
/// `Blocked by:` line: `Draft`, held by #371, which is open.
const ADR_0037_ON_46D78C5: &str = r"# ADR-0037 — ERP Posting Runs Inside the Document Transaction

**Status:** Draft · **Last updated:** 2026-09-24

**Blocked by:** [#371](https://github.com/sujanto-gaws/kelir/issues/371) — ~~the ERP scope decision is not taken; no `D-n` adopts an ERP layer~~ **`D-73` was answered on 2026-09-18: no ERP layer before `v1.0.0`**, and this record stays as written for a later decision on whether one follows. No `D-n` adopts an ERP layer, and no `FR-*` covers one

## 1. Context
";

/// A changelog whose `[Unreleased]` cites #519, and whose `[0.7.0]` cites
/// #371 as an entry's reference: the shape a release takes when part of an
/// issue ships and the issue stays open. (#372's own two `[0.7.0]` entries
/// only *mention* #371, so they would not test this.)
const CHANGELOG_WITH_371_RELEASED: &str = r"# Changelog

## [Unreleased]

### Added

- **JWSS `actions` run, delivered after the transition commits**
  ([#519](https://github.com/sujanto-gaws/kelir/issues/519)).

## [0.7.0] — 2026-09-10

### Added

- **The ERP layering, documentation only**
  ([#371](https://github.com/sujanto-gaws/kelir/issues/371), **D-73**). Nothing
  in it is scheduled, and #371 stays open.
";

/// An `[Unreleased]` that names #519 everywhere except an entry's reference:
/// at the end of a body (the shape `[Unreleased]` gives #511 on `46d78c5`), in
/// a nested item's body, as an anchor, and as the prefix of a longer number.
const UNRELEASED_MENTIONING_519: &str = r"## [Unreleased]

### Changed

- **A role that an open task still needs cannot be deleted**
  ([#487](https://github.com/sujanto-gaws/kelir/issues/487), decision **D-89**).
  A role deleted before this release has a query that finds it and the steps
  to clear one by hand
  ([#519](https://github.com/sujanto-gaws/kelir/issues/519)).
  - **Dead letters have no screen yet**, and #519 says why.
- **A refused pattern is told a reason that is true for it**
  ([#5190](https://github.com/sujanto-gaws/kelir/issues/5190)). See
  [the troubleshooting section](docs/operations/01.md#519-troubleshooting).
";

fn fixture(name: &str, source: &str) -> Record {
    parse_record(name.to_owned(), source)
}

fn unreleased_of(changelog: &str) -> &str {
    unreleased_section(changelog).expect("the fixture has an [Unreleased] heading")
}

/// **#545's seen-red.** ADR-0041 exactly as #537 left it, against the first
/// `[Unreleased]` that recorded #519 under *Added*. The first half of rule 4
/// accepts it; the second refuses it, naming #519.
#[test]
fn adr_0041_as_537_left_it_is_refused() {
    let record = fixture("0041. as 4c4e6a3 left it.md", ADR_0041_AT_4C4E6A3);

    assert_eq!(record.status, "Draft");
    assert!(
        record.header.contains("**Blocked by:**"),
        "the first half of rule 4 accepts this record, which is the gap"
    );
    assert_eq!(
        delivered_blockers(&record, unreleased_of(CHANGELOG_AT_5791681)),
        Some(vec![519])
    );
}

/// The same record, flipped the way #539 flipped it, is not this rule's.
#[test]
fn adr_0041_as_539_left_it_is_accepted() {
    let adopted = ADR_0041_AT_4C4E6A3
        .replace("**Status:** Draft", "**Status:** Adopted")
        .replace("**Decision date:** —", "**Decision date:** 2026-09-24");
    let record = fixture("0041. as 6006cb3 left it.md", &adopted);

    assert_eq!(record.status, "Adopted");
    assert_eq!(
        delivered_blockers(&record, unreleased_of(CHANGELOG_AT_5791681)),
        None
    );
}

/// **A blocker cited in a released section.** ADR-0037 is legitimately
/// `Draft` behind #371, and `[0.7.0]` cites #371 as an entry's reference.
/// Reading the whole file would refuse it; reading `[Unreleased]` does not.
#[test]
fn a_blocker_cited_only_in_a_released_section_is_not_refused() {
    let record = fixture("0037. on 46d78c5.md", ADR_0037_ON_46D78C5);

    assert_eq!(blockers(&record.header), vec![371]);
    assert!(
        cited_issues(CHANGELOG_WITH_371_RELEASED).contains(&371),
        "the fixture must cite #371 somewhere, or this test proves nothing"
    );
    assert_eq!(
        delivered_blockers(&record, unreleased_of(CHANGELOG_WITH_371_RELEASED)),
        None
    );
}

/// **A blocker cited elsewhere.** #519 in a body, a nested body, an anchor
/// and a longer number is mentioned, not cited, and ADR-0041 as #537 left it
/// passes against it.
#[test]
fn a_blocker_mentioned_but_not_cited_is_not_refused() {
    let record = fixture("0041. as 4c4e6a3 left it.md", ADR_0041_AT_4C4E6A3);
    let unreleased = unreleased_of(UNRELEASED_MENTIONING_519);

    assert!(
        issue_numbers(unreleased).contains(&519),
        "the fixture must mention #519, or this test proves nothing"
    );
    assert_eq!(cited_issues(unreleased), BTreeSet::from([487, 5190]));
    assert_eq!(delivered_blockers(&record, unreleased), None);
}

/// A record held by two issues is still held while one is undelivered.
#[test]
fn a_record_held_by_two_issues_is_refused_only_when_both_are_cited() {
    let held_twice = ADR_0041_AT_4C4E6A3.replace(
        "**Blocked by:** #519 —",
        "**Blocked by:** #519, [#546](https://github.com/sujanto-gaws/kelir/issues/546) —",
    );
    let record = fixture("0041. held twice.md", &held_twice);

    assert_eq!(blockers(&record.header), vec![519, 546]);
    assert_eq!(
        delivered_blockers(&record, unreleased_of(CHANGELOG_AT_5791681)),
        None
    );

    let both = CHANGELOG_AT_5791681.replace(
        "### Added\n",
        "### Added\n\n- **The second half** ([#546](https://github.com/sujanto-gaws/kelir/issues/546)).\n",
    );
    assert_eq!(
        delivered_blockers(&record, unreleased_of(&both)),
        Some(vec![519, 546])
    );
}

/// The readers the rule stands on, one edge each.
#[test]
fn the_readers_under_the_rule_read_what_they_say() {
    assert_eq!(
        issue_numbers("#519, [#520](x/issues/520), y/pull/521) z.md#522-a z.md#525) #523b #524"),
        vec![519, 520, 524, 520, 521]
    );
    assert_eq!(
        blockers("**Blocked by:** #1 — waits for #2\n"),
        vec![1],
        "an issue in the description is not a blocker"
    );
    assert_eq!(
        reference_of("**A** (#1 (nested) #2) and (#3)"),
        Some("(#1 (nested) #2)")
    );
    assert_eq!(reference_of("**A.** It says (#1)"), None);
    assert_eq!(reference_of("Plain (#1)"), None);

    let section = unreleased_of(CHANGELOG_AT_5791681);
    assert!(section.starts_with("## [Unreleased]\n"));
    assert!(
        !section.contains("[0.8.0]"),
        "the section ends at the next release"
    );
    assert!(unreleased_section("# Changelog\n\n## [0.8.0]\n").is_none());
}
