//! Every container image the deployment and the pipeline name is pinned
//! (**D-62**).
//!
//! # Why a test and not a convention
//!
//! PostgreSQL, Rust and Node have been pinned since Sprint 0. The four
//! infrastructure images Phase 6 depends on — MinIO, `mc`, ClamAV and Mailpit —
//! were `:latest` in the development compose, in **the staging compose a
//! release is deployed from**, and in two places in `ci.yml`. So two
//! deployments of one Kelir tag could run different object storage and a
//! different scanner, and `docker compose pull` could change the product
//! without changing the repository.
//!
//! **Nothing could have failed, which is the point.** The compose files are the
//! artefact no test reads — Sprint 12's exit demo found three defects there
//! that eleven hundred tests could not — and an image tag is the part of them
//! that changes while the file stands still. Raised by the [Sprint 13
//! independent pass](../../projects/verifications/13.%20Sprint%2013%20Independent%20Pass.md),
//! finding 3.
//!
//! # The rule
//!
//! In `deploy/` and `.github/workflows/`, outside comments:
//!
//! 1. No floating tag — `:latest`, `:edge` or `:stable`, which name whatever
//!    was published most recently rather than a version.
//! 2. Every `image:` key carries a tag at all. `image: postgres` is
//!    `postgres:latest` with the tag left off, which is the same defect wearing
//!    less.
//!
//! **Comments are stripped first**, because this file's own neighbours explain
//! the rule by naming `:latest`, and a check that forbids describing the thing
//! it forbids is one somebody works around.
//!
//! # What this deliberately does not check
//!
//! **That a pin is recent, or that it is the same pin everywhere.** Both are
//! judgement rather than a property, and a test that enforced them would be
//! read as noise the first time a bump is deliberate. What it holds is that
//! somebody chose.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Two mutations, run 2026-09-03:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `clamav/clamav:1.5` returned to `:latest` in the staging compose | *no deployment file names a floating image tag* |
//! | `image: postgres:16` reduced to `image: postgres` | *every image key names a tag* |
//!
//! # Rule 3: the release Dockerfiles, read since 2026-09-30 (#590)
//!
//! Rules 1 and 2 read YAML, and the images a release is **built from** are
//! named in Dockerfiles, which nothing read. `frontend.Dockerfile` built the
//! shipped image `FROM caddy:2-alpine`, which follows every Caddy 2 release, so
//! two builds of one Kelir tag could serve through different Caddy versions.
//!
//! **Every `FROM` in every Dockerfile in the repository** names one of:
//!
//! - a **patch-level** tag, a version with three numeric parts
//!   (`caddy:2.11.4-alpine`, `rust:1.89.0-slim-bookworm`);
//! - a **dated** tag (`debian:bookworm-20260918-slim`, MinIO's
//!   `RELEASE.<timestamp>`);
//! - a **digest** (`image@sha256:…`);
//! - an **earlier stage** of the same file (`FROM builder`), or `scratch`.
//!
//! **The Dockerfiles are found, not listed** (retrospective 6): the walk
//! starts at the repository root and takes every file named `Dockerfile`,
//! `Containerfile`, `*.Dockerfile` or `Dockerfile.*` (but not
//! `Dockerfile.dockerignore`, BuildKit's ignore file), skipping only `.git`,
//! `.claude` (other worktrees' checkouts), `node_modules`, `target` and `dist`.
//! A Dockerfile added anywhere else is read without anybody editing this file.
//!
//! **This is stricter than rules 1 and 2, deliberately, and the difference is
//! stated rather than hidden.** Rules 1 and 2 hold that *somebody chose a
//! tag*, and `caddy:2-alpine` passes them: it is neither `:latest` nor
//! untagged. The issue asked for the YAML rule, and the YAML rule cannot see
//! the defect it was raised for. So the Dockerfile rule asks for a release,
//! not a line. The compose files still name lines (`postgres:16`,
//! `node:24-alpine`, `rust:1.89`), which D-62 chose and which rules 1 and 2
//! accept; holding them to rule 3 is a change to D-62, not to this test.
//!
//! **What a patch-level tag still does not freeze**: Docker Official Images
//! re-push a tag when its base image is rebuilt, so `caddy:2.11.4-alpine` can
//! gain Alpine security fixes under the same name. The Caddy inside it cannot
//! change. A digest would freeze the rest, and would also stop those fixes
//! arriving; the tag is the granularity D-62 chose for everything else.
//!
//! **Parsing**: `#` comment lines are dropped (in a Dockerfile a `#` starts a
//! comment only at the start of a line), `\` continuations are joined, `FROM`
//! matches in any case, flags such as `--platform=` are skipped, and `AS name`
//! records a stage that a later `FROM name` may reference. A stage is known
//! only after it is declared, so `FROM builder` above `… AS builder` is an
//! image named `builder` and is refused. An image named through a build
//! argument (`FROM ${BASE}`) is refused too: its pin is not in the file. A
//! leading byte-order mark is dropped, as BuildKit drops it. An `escape`
//! parser directive is refused outright, because this parser reads only `\`
//! as the continuation and a backtick escape could hide a `FROM`.
//!
//! **Limits, stated**: a tag passes when **any** `-` or `_` separated segment
//! is patch-level or dated, so `node:24-alpine-3.22.1` would pass on its
//! variant. The skipped directory names are skipped at any depth, so a
//! Dockerfile under a `dist/` or `target/` directory is not read. A `RUN`
//! heredoc whose body starts a line with `FROM` is read as a `FROM`, which
//! errs toward refusing.
//!
//! ## Seen to fail (coding standard §2.9), 2026-09-30
//!
//! Rule 3 was run against the Dockerfiles as they stood, and all four `FROM`
//! lines were refused: `rust:1.89-slim-bookworm`, `debian:bookworm-slim`,
//! `node:24-alpine` and `caddy:2-alpine`. Each was then pinned to the release
//! its floating tag resolved to that day, **checked by index digest**, so the
//! pin froze the images rather than moving them. Each probe below was applied
//! on its own, run, seen red and reverted. Probes 1–6 change a Dockerfile;
//! 7–15 change this file:
//!
//! | # | Probe | Reddened |
//! |---|---|---|
//! | 1 | `frontend.Dockerfile`'s runtime back to `caddy:2-alpine` | *every dockerfile from names a pinned image* |
//! | 2 | A new `spikes/probe590/Dockerfile`, `FROM alpine:3.22`, outside `deploy/` | *every dockerfile from names a pinned image* |
//! | 3 | `FROM --platform=linux/amd64 caddy:2-alpine AS runtime` | *every dockerfile from names a pinned image* |
//! | 4 | `FROM \` continued onto `caddy:2-alpine AS runtime` | *every dockerfile from names a pinned image* |
//! | 5 | `backend.Dockerfile`'s runtime back to `debian:bookworm-slim` | *every dockerfile from names a pinned image* |
//! | 6 | A new `e2e/Containerfile`, `FROM node:24-alpine` | *every dockerfile from names a pinned image* |
//! | 7 | The walk skips `deploy/` | *the walk finds the release dockerfiles* (rule 3 itself passes, finding nothing, which is why the guard exists) |
//! | 8 | `is_patch_level` accepts two numeric parts | *the shapes rule 3 refuses are refused* |
//! | 9 | Stages are never recorded | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 10 | Comment lines are not dropped | *the shapes rule 3 refuses are refused* (the comment ending in `\`) |
//! | 11 | Flags are not skipped | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 12 | Continuations are not joined | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 13 | `is_dockerfile` misses `*.Dockerfile` | *a dockerfile is recognised by its name*, *the walk finds the release dockerfiles* |
//! | 14 | Anything after `@` is accepted as a digest | *the shapes rule 3 refuses are refused* |
//! | 15 | Stage names compared case-sensitively | *the shapes rule 3 accepts are accepted* |
//!
//! **The `test-engineer` gate then ran 21 probes of its own.** Four mutations
//! survived and three shapes got through or were misread, and each is now a
//! shape or a fix: the `$` refusal had no test of its own
//! (`caddy:2.11.4-${VARIANT}`), a 7-digit date, a non-numeric `a.b.c`,
//! `RELEASE.latest`, and the `_` separator are sent; a byte-order mark hid the
//! first `FROM` and is stripped; `Dockerfile.dockerignore` was read as a
//! Dockerfile and is not; an `escape` directive could hide a `FROM` and is
//! refused. The three limits above are what it left as limits. Probes 16–23
//! re-ran the eight survivors against the fixes, and each went red:
//!
//! | # | Probe | Reddened |
//! |---|---|---|
//! | 16 | `is_dated` accepts 7 digits | *the shapes rule 3 refuses are refused* |
//! | 17 | The `$` refusal removed | *the shapes rule 3 refuses are refused* |
//! | 18 | Patch-level parts need not be digits | *the shapes rule 3 refuses are refused* |
//! | 19 | Tag segments split on `-` only | *the shapes rule 3 accepts are accepted* |
//! | 20 | `RELEASE.` needs no digit after it | *the shapes rule 3 refuses are refused* |
//! | 21 | The byte-order mark is not stripped | *the shapes rule 3 refuses are refused* |
//! | 22 | `Dockerfile.dockerignore` read as a Dockerfile | *a dockerfile is recognised by its name* |
//! | 23 | The `escape` directive is not refused | *an escape directive is refused* |
//!
//! The positive control ran last: every probe reverted, the whole file green.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The repository root — one above the crate, which is where `deploy/` and
/// `.github/` live.
fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

/// Every YAML file under `deploy/` and `.github/workflows/`.
fn deployment_files() -> Vec<PathBuf> {
    let root = repository_root();
    let mut files = Vec::new();

    collect_yaml(&root.join("deploy"), &mut files);
    collect_yaml(&root.join(".github/workflows"), &mut files);

    // The same guard the other source walks carry: a walk that finds nothing
    // passes every assertion under it.
    assert!(
        files.len() >= 3,
        "the walk found {} deployment files, which is too few — the layout moved: {files:?}",
        files.len()
    );

    files
}

fn collect_yaml(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries {
        let path = entry.expect("a directory entry").path();

        if path.is_dir() {
            collect_yaml(&path, into);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml")
        {
            into.push(path);
        }
    }
}

/// The file with every `#` comment removed, so prose about `:latest` is not a
/// use of it.
fn without_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find('#') {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn relative(path: &Path) -> String {
    path.strip_prefix(repository_root())
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// **Rule 1** — a tag that names whatever was published last is not a pin.
#[test]
fn no_deployment_file_names_a_floating_image_tag() {
    let mut offences = Vec::new();

    for path in deployment_files() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        for (number, line) in without_comments(&source).lines().enumerate() {
            for floating in [":latest", ":edge", ":stable"] {
                if line.contains(floating) {
                    offences.push(format!(
                        "{}:{} names {floating} — {}",
                        relative(&path),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "a floating tag is whatever was published most recently, so the deployment changes \
         without the repository changing (D-62):\n  {}",
        offences.join("\n  ")
    );
}

/// **Rule 2** — an `image:` with no tag is `:latest` with the tag left off.
#[test]
fn every_image_key_names_a_tag() {
    let mut offences = Vec::new();

    for path in deployment_files() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        for (number, line) in without_comments(&source).lines().enumerate() {
            let trimmed = line.trim();

            let Some(reference) = trimmed.strip_prefix("image:") else {
                continue;
            };
            let reference = reference.trim();

            if reference.is_empty() {
                continue;
            }

            // A digest is a pin, and a stronger one than a tag.
            if reference.contains('@') {
                continue;
            }

            // `${KELIR_VERSION:?…}` is an interpolation the deployment supplies,
            // and `deploy.sh` refuses to start without it — which is the pin,
            // enforced somewhere a YAML check cannot see.
            let after_repository = reference.rsplit('/').next().unwrap_or(reference);

            if !after_repository.contains(':') {
                offences.push(format!(
                    "{}:{} — `{reference}` has no tag",
                    relative(&path),
                    number + 1
                ));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "an image with no tag resolves to `:latest` (D-62):\n  {}",
        offences.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Rule 3: the release Dockerfiles (#590)
// ---------------------------------------------------------------------------

/// Directories the Dockerfile walk does not enter: version control, other
/// worktrees' checkouts, installed dependencies and build output. None of
/// them holds a Dockerfile this repository builds from.
const NOT_WALKED: [&str; 5] = [".git", ".claude", "node_modules", "target", "dist"];

/// Whether a file name is one Docker or Podman builds from.
fn is_dockerfile(name: &str) -> bool {
    let name = name.to_ascii_lowercase();

    name == "dockerfile"
        || name == "containerfile"
        || name.ends_with(".dockerfile")
        || (name.starts_with("dockerfile.") && !name.ends_with(".dockerignore"))
}

/// Every Dockerfile in the repository, found by name from the root.
fn dockerfiles() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_dockerfiles(&repository_root(), &mut files);
    files.sort();
    files
}

fn collect_dockerfiles(directory: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        if path.is_dir() {
            if !NOT_WALKED.contains(&name.as_str()) {
                collect_dockerfiles(&path, into);
            }
        } else if is_dockerfile(&name) {
            into.push(path);
        }
    }
}

/// One `FROM` instruction: the physical line it starts on, the image or
/// stage it names, and the stage it declares, if any.
#[derive(Debug, PartialEq)]
struct FromInstruction {
    line: usize,
    base: String,
    declares: Option<String>,
}

/// Every `FROM` in a Dockerfile, in order.
///
/// A line whose first non-blank character is `#` is a comment; a `#`
/// anywhere else is not, so only whole lines are dropped. A line ending in
/// `\` continues onto the next, and a comment line inside a continuation is
/// dropped without ending it, which is what Docker does. A leading
/// byte-order mark is dropped too, as BuildKit drops it, or the first `FROM`
/// would read as `\u{feff}FROM` and never be seen.
fn from_instructions(source: &str) -> Vec<FromInstruction> {
    let source = source.trim_start_matches('\u{feff}');
    let mut instructions = Vec::new();
    let mut logical = String::new();
    let mut starts_on = 0;

    for (index, physical) in source.lines().enumerate() {
        if physical.trim_start().starts_with('#') {
            continue;
        }

        if logical.is_empty() {
            starts_on = index + 1;
        }

        let trimmed = physical.trim_end();
        match trimmed.strip_suffix('\\') {
            Some(continued) => {
                logical.push_str(continued);
                logical.push(' ');
            }
            None => {
                logical.push_str(trimmed);
                instructions.extend(read_from(&logical, starts_on));
                logical.clear();
            }
        }
    }

    instructions.extend(read_from(&logical, starts_on));
    instructions
}

fn read_from(logical: &str, line: usize) -> Option<FromInstruction> {
    let mut words = logical.split_whitespace();

    if !words.next()?.eq_ignore_ascii_case("FROM") {
        return None;
    }

    // `--platform=…` and any other flag come before the image.
    let mut words = words.skip_while(|word| word.starts_with("--"));
    let base = words.next()?.to_string();

    let declares = match (words.next(), words.next()) {
        (Some(keyword), Some(stage)) if keyword.eq_ignore_ascii_case("AS") => {
            Some(stage.to_ascii_lowercase())
        }
        _ => None,
    };

    Some(FromInstruction {
        line,
        base,
        declares,
    })
}

/// A version with at least three numeric parts, with or without a leading
/// `v`: `2.11.4`, `v1.31.0`, `24.21.0`.
fn is_patch_level(segment: &str) -> bool {
    let version = segment.strip_prefix('v').unwrap_or(segment);
    let parts: Vec<&str> = version.split('.').collect();

    parts.len() >= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// A date the publisher stamped the build with: `20260918`.
fn is_dated(segment: &str) -> bool {
    segment.len() == 8
        && segment.bytes().all(|byte| byte.is_ascii_digit())
        && (segment.starts_with("19") || segment.starts_with("20"))
}

/// Why an image reference is not a pin, or `None` if it is one.
fn pin_refusal(reference: &str) -> Option<String> {
    if reference.contains('$') {
        return Some("is named through a build argument, so its pin is not in the file".into());
    }

    if let Some((_, digest)) = reference.split_once('@') {
        let is_digest = digest
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()));

        return (!is_digest).then(|| format!("names `@{digest}`, which is not a sha256 digest"));
    }

    // A registry port (`localhost:5000/base`) is not a tag, so the tag is
    // looked for after the last `/`, as rule 2 does.
    let repository = reference.rsplit('/').next().unwrap_or(reference);

    let Some((_, tag)) = repository.split_once(':') else {
        return Some("has no tag, so it resolves to `:latest`".into());
    };

    let is_minio_release = tag
        .strip_prefix("RELEASE.")
        .is_some_and(|stamp| stamp.starts_with(|first: char| first.is_ascii_digit()));

    if is_minio_release
        || tag
            .split(['-', '_'])
            .any(|segment| is_patch_level(segment) || is_dated(segment))
    {
        return None;
    }

    Some(format!(
        "`:{tag}` names a line that moves, not a release; pin a patch-level or dated tag, or a \
         digest"
    ))
}

/// Every refusal in one Dockerfile's text, as `line — reason`.
fn dockerfile_refusals(source: &str) -> Vec<String> {
    let mut stages = HashSet::new();
    let mut refusals = Vec::new();

    // `# escape=` makes another character the continuation, and this parser
    // reads only `\`: under a backtick escape, `RUN dir C:\` would swallow the
    // `FROM` after it. Parser directives come first, before any blank line
    // or instruction, so only the leading comment lines are looked at.
    for (index, line) in source
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
        .take_while(|(_, line)| line.trim_start().starts_with('#'))
    {
        let directive = line.trim_start()[1..].trim().to_ascii_lowercase();

        if directive.starts_with("escape") && directive.contains('=') {
            refusals.push(format!(
                "{} — the `escape` directive changes the continuation character, which this \
                 test does not parse, so a FROM could be hidden from it",
                index + 1
            ));
        }
    }

    for instruction in from_instructions(source) {
        let base = instruction.base.to_ascii_lowercase();

        if base != "scratch" && !stages.contains(&base) {
            if let Some(reason) = pin_refusal(&instruction.base) {
                refusals.push(format!(
                    "{} — `FROM {}` {reason}",
                    instruction.line, instruction.base
                ));
            }
        }

        if let Some(stage) = instruction.declares {
            stages.insert(stage);
        }
    }

    refusals
}

/// The walk is only worth its assertions if it reaches the files a release is
/// built from.
#[test]
fn the_walk_finds_the_release_dockerfiles() {
    let found: Vec<String> = dockerfiles().iter().map(|path| relative(path)).collect();

    for expected in [
        "deploy/docker/backend.Dockerfile",
        "deploy/docker/frontend.Dockerfile",
    ] {
        assert!(
            found.iter().any(|path| path == expected),
            "the Dockerfile walk did not find {expected}, so the layout moved or the walk broke: \
             {found:?}"
        );
    }
}

/// **Rule 3** — a release image is built from a release, not from a line.
#[test]
fn every_dockerfile_from_names_a_pinned_image() {
    let mut offences = Vec::new();

    for path in dockerfiles() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        assert!(
            !from_instructions(&source).is_empty(),
            "{} has no FROM, so the parser missed it",
            relative(&path)
        );

        for refusal in dockerfile_refusals(&source) {
            offences.push(format!("{}:{refusal}", relative(&path)));
        }
    }

    assert!(
        offences.is_empty(),
        "a Dockerfile builds a release image from a base that moves, so two builds of one Kelir \
         tag can differ (D-62, #590):\n  {}",
        offences.join("\n  ")
    );
}

#[test]
fn a_dockerfile_is_recognised_by_its_name() {
    for name in [
        "Dockerfile",
        "dockerfile",
        "Containerfile",
        "backend.Dockerfile",
        "Dockerfile.dev",
    ] {
        assert!(is_dockerfile(name), "{name} is a Dockerfile");
    }

    for name in [
        "Dockerfile-notes.md",
        "docker-compose.yml",
        "Caddyfile",
        ".dockerignore",
        // BuildKit's per-Dockerfile ignore file, which has no FROM.
        "Dockerfile.dockerignore",
    ] {
        assert!(!is_dockerfile(name), "{name} is not a Dockerfile");
    }
}

/// The shapes rule 3 exists to let through, in one file.
#[test]
fn the_shapes_rule_3_accepts_are_accepted() {
    let accepted = r"
# syntax=docker/dockerfile:1
# FROM caddy:2-alpine is a comment naming a floating tag, not a use of it
ARG BUILDPLATFORM
FROM --platform=$BUILDPLATFORM rust:1.89.0-slim-bookworm AS builder
FROM debian:bookworm-20260918-slim as runtime
FROM builder AS tests
from runtime
FROM caddy:2.11.4-alpine AS Edge
FROM edge
FROM ghcr.io/sujanto-gaws/minio:RELEASE.2025-09-07T16-13-09Z
FROM localhost:5000/kelir/base:v1.31.0
FROM example/base:build_1.2.3
FROM node@sha256:ebfe2f90462722a7a4de65e91990e97fe0d401c70e0e762c5b53302f905ec1c1
FROM \
    node:24.21.0-alpine \
    AS continued
FROM scratch
";

    let instructions = from_instructions(accepted);
    assert_eq!(
        instructions.len(),
        12,
        "every FROM is read: {instructions:?}"
    );
    assert_eq!(
        instructions[0],
        FromInstruction {
            line: 5,
            base: "rust:1.89.0-slim-bookworm".into(),
            declares: Some("builder".into()),
        },
        "the platform flag is skipped and the stage recorded"
    );
    assert_eq!(
        instructions[10],
        FromInstruction {
            line: 15,
            base: "node:24.21.0-alpine".into(),
            declares: Some("continued".into()),
        },
        "a continued FROM is one instruction, on the line it starts"
    );

    assert_eq!(dockerfile_refusals(accepted), Vec::<String>::new());
}

/// An `escape` directive is refused outright: the parser reads only `\` as
/// the continuation, and under a backtick escape a `FROM` could be hidden.
#[test]
fn an_escape_directive_is_refused() {
    let hidden = "# escape=`\nRUN dir C:\\\nFROM caddy:2-alpine";
    let refusals = dockerfile_refusals(hidden);

    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.starts_with("1 — the `escape` directive")),
        "the escape directive is refused: {refusals:?}"
    );

    // `syntax` is a directive too, and changes nothing this test reads.
    assert_eq!(
        dockerfile_refusals("# syntax=docker/dockerfile:1\nFROM caddy:2.11.4-alpine"),
        Vec::<String>::new()
    );
}

/// Each shape rule 3 exists to refuse, sent on its own.
#[test]
fn the_shapes_rule_3_refuses_are_refused() {
    for (dockerfile, refused) in [
        ("FROM caddy:2-alpine AS runtime", "caddy:2-alpine"),
        ("FROM node:24-alpine", "node:24-alpine"),
        ("FROM rust:1.89-slim-bookworm", "rust:1.89-slim-bookworm"),
        ("FROM debian:bookworm-slim", "debian:bookworm-slim"),
        ("FROM node:24-alpine3.22", "node:24-alpine3.22"),
        ("FROM alpine:latest", "alpine:latest"),
        ("FROM postgres", "postgres"),
        (
            "FROM localhost:5000/kelir/base",
            "localhost:5000/kelir/base",
        ),
        ("ARG BASE=caddy:2.11.4-alpine\nFROM ${BASE}", "${BASE}"),
        ("FROM caddy@sha256:6aeddd44", "caddy@sha256:6aeddd44"),
        (
            "FROM --platform=linux/amd64 caddy:2-alpine AS runtime",
            "caddy:2-alpine",
        ),
        ("FROM \\\n    caddy:2-alpine AS runtime", "caddy:2-alpine"),
        // A comment ending in `\` does not continue, so it hides nothing.
        (
            "# a comment ending in a backslash \\\nFROM caddy:2-alpine",
            "caddy:2-alpine",
        ),
        // A build argument inside the tag hides the release as well.
        (
            "ARG VARIANT=alpine\nFROM caddy:2.11.4-${VARIANT}",
            "caddy:2.11.4-${VARIANT}",
        ),
        // A byte-order mark does not hide the first FROM.
        (
            "\u{feff}FROM caddy:2-alpine\nFROM scratch",
            "caddy:2-alpine",
        ),
        (
            "FROM debian:bookworm-2026091-slim",
            "debian:bookworm-2026091-slim",
        ),
        ("FROM example/base:a.b.c", "example/base:a.b.c"),
        (
            "FROM ghcr.io/sujanto-gaws/minio:RELEASE.latest",
            "ghcr.io/sujanto-gaws/minio:RELEASE.latest",
        ),
        // A stage is known only once declared, so this is an image.
        (
            "FROM builder\nFROM caddy:2.11.4-alpine AS builder",
            "builder",
        ),
        // A later stage is refused even when the earlier ones are pinned.
        (
            "FROM rust:1.89.0-slim-bookworm AS builder\nFROM builder\nFROM caddy:2",
            "caddy:2",
        ),
    ] {
        let refusals = dockerfile_refusals(dockerfile);

        assert_eq!(
            refusals.len(),
            1,
            "exactly one refusal for {dockerfile:?}: {refusals:?}"
        );
        assert!(
            refusals[0].contains(&format!("`FROM {refused}`")),
            "the refusal names {refused}: {refusals:?}"
        );
    }
}
