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
//!    written yesterday.
//!
//! # What this deliberately does not check
//!
//! **Whether the file is on `main`.** That is the property the rule is really
//! about, and it needs `git log`, which CI does not have: `actions/checkout`
//! runs at depth 1 and the history is not there to ask. Rule 4 is the reachable
//! substitute — it does not know *when* a record merged, and it does not need
//! to, because a `Draft` that names its blocker is honest whenever it merged
//! and one that does not is not.
//!
//! **Whether the impacted documents in §4.1 are actually done.** That is
//! judgement over prose, and §7 puts it on the author and the reviewer.
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
        // The metadata block is everything before the first section heading.
        let header = source
            .split("\n## ")
            .next()
            .expect("a split yields one part")
            .to_owned();

        records.push(Record {
            number: name.chars().take(4).collect(),
            status: first_field(&header, "Status")
                .unwrap_or_else(|| panic!("{name} has no **Status:** field")),
            decision_date: first_field(&header, "Decision date").unwrap_or_default(),
            header,
            name,
        });
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
