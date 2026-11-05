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
//!    was published most recently rather than a version. Read in every YAML
//!    file, and since #671 in every shell script under `deploy/`, whose
//!    `docker run` lines name images as a workflow's `run:` lines do.
//! 2. Every `image:` key carries a tag at all. `image: postgres` is
//!    `postgres:latest` with the tag left off, which is the same defect wearing
//!    less. Read in the YAML files.
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
//! `Dockerfile.dockerignore`, BuildKit's ignore file), and since #671
//! `*.containerfile`, `Containerfile.*` (but not `.containerignore`) and
//! `Dockerfile-<word>` or `Containerfile-<word>` with no further `.` (so
//! `Dockerfile-prod`, not `Dockerfile-notes.md`), skipping only `.git`,
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
//! comment only at the start of a line), `\` continuations are joined, and an
//! empty or blank line inside a continuation is skipped without ending it, as
//! BuildKit skips it (#671). `FROM` matches in any case, flags such as
//! `--platform=` are skipped, and `AS name` records a stage that a later
//! `FROM name` may reference. A stage is known only after it is declared, so
//! `FROM builder` above `… AS builder` is an image named `builder` and is
//! refused. A `FROM` that names no image is refused, not skipped (#671). An
//! image named through a build argument (`FROM ${BASE}`) is refused too: its
//! pin is not in the file. A leading byte-order mark is dropped, as BuildKit
//! drops it. An `escape` parser directive is refused outright, because this
//! parser reads only `\` as the continuation and a backtick escape could hide
//! a `FROM`. Since #671 a `COPY --from=`, a `RUN --mount=…,from=` and the
//! `syntax` directive are held to the rule as a `FROM` is; the last section
//! says why, and what is still not read.
//!
//! **Limits, stated**: a tag passes when **any** `-` or `_` separated segment
//! is patch-level or dated, so `node:24-alpine-3.22.1` would pass on its
//! variant. The skipped directory names are skipped at any depth, so a
//! Dockerfile under a `dist/` or `target/` directory is not read. A `RUN`
//! heredoc whose body starts a line with `FROM`, `COPY --from=` or `RUN
//! --mount=` is read as that instruction, which errs toward refusing. A
//! `COPY --from=` or `from=` of digits is accepted as a stage index whatever
//! the number, which BuildKit checks.
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
//! refused. The first three limits above are what it left as limits; the
//! fourth, the stage index, came with #671. Probes 16–23 re-ran the eight
//! survivors against the fixes, and each went red:
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
//!
//! # Every route an image is named by, read or named as unread (#671)
//!
//! **Record 21's probe M3 passed all eight tests, and should have failed
//! one.** It continued `frontend.Dockerfile`'s runtime `FROM \` over an empty
//! line onto `caddy:2-alpine AS runtime`. The logical line ended at the blank,
//! the parser found a `FROM` with no image and skipped it, and the *has no
//! FROM* guard fires only on a file with none at all. BuildKit joins over the
//! empty line, with a warning that it will one day be an error, so the image
//! would have been built from `caddy:2-alpine`. **Both halves are fixed**: an
//! empty line inside a continuation is skipped as BuildKit skips it, and a
//! `FROM` that names no image is refused.
//!
//! The issue then asked for every other route an image takes into a build or
//! a run to be read, or named here as unread with the reason. None of them
//! occurs in the tree on 2026-11-05; each read one is held from now on.
//!
//! | Route | Read? | How, or why not |
//! |---|---|---|
//! | `COPY --from=<image>` | **Read**, rule 3 | The flag names an earlier stage, a stage index (`--from=0`) or an image, and an image is held as a `FROM`'s is |
//! | `RUN --mount=…,from=<image>` | **Read**, rule 3 | Every `from=` option of every `--mount=` flag, the same way; its key in any case |
//! | `# syntax=<image>` | **Read**, rule 3 | It names the frontend BuildKit parses the file with, so `docker/dockerfile:1` follows every 1.x release as `caddy:2` follows Caddy's. *The shapes rule 3 accepts* accepted it until now (where it sat after a blank line, which is no directive) |
//! | `Dockerfile-prod`, `*.containerfile`, `Containerfile.*` | **Read**, by name | `is_dockerfile`'s patterns, above |
//! | Compose's `dockerfile_inline:` | **Read**, rule 3 | In every YAML file rules 1 and 2 read, the key plain or quoted and anywhere on a line that is not a comment, so in a flow mapping too: a block scalar to the end of its indentation, a one-line value as compose reads it, with `\n` as a line break. Not read: a quoted value folded over several lines, past its first; a value through a YAML alias (`*name`) fails the *no FROM* guard instead |
//! | A floating tag in a `docker run` line | **Read**, rule 1 | A workflow's `run:` was always read as YAML; `deploy/`'s shell scripts are walked too now |
//! | An **untagged** image in a `docker run` line | **Not read** | Finding the image word means knowing which of `docker run`'s options take a value, and the tree's scripts name their images through variables (`"${POSTGRES_IMAGE}"`, `"${mc_image}"`) set elsewhere or by the operator. Rule 1 still refuses a floating word anywhere on the line |
//! | A Dockerfile under any other name (`build/prod.df`) | **Not read** | `docker build -f` and compose's `dockerfile:` key accept any file, and reading them means resolving `-f` arguments through shell variables and `dockerfile:` through its `context:`. A name outside the patterns is a reviewer's line |
//! | A Dockerfile on standard input (`docker build -f- <<EOF`) | **Not read** | A heredoc in a script or a `run:`, which no Dockerfile walk reaches; rule 1 still reads its floating words |
//! | An image supplied from outside the file (`--build-context name=docker-image://…`, the `BUILDKIT_SYNTAX` argument) | **Not read** | The pin is not in the file; a `FROM ${BASE}` that takes one is refused, as before |
//!
//! ## Seen to fail (coding standard §2.9), 2026-11-05
//!
//! Probes 24–32 change the tree. Each was run against the test as it stood
//! before #671, and **all nine passed all eight tests**, which is each gap
//! reproduced. Each was then run against this file, and went red. Probes
//! 33–44 change this file, each on its own, and each went red. Every probe
//! was reverted with `git checkout`, and the whole file, ten tests, is green.
//!
//! | # | Probe | Reddened |
//! |---|---|---|
//! | 24 | Record 21's M3: the runtime `FROM \`, an empty line, `caddy:2-alpine AS runtime` | *every dockerfile from names a pinned image*: `frontend.Dockerfile:46 — FROM caddy:2-alpine` |
//! | 25 | The runtime `FROM` reduced to `FROM` | the same: `:46 — FROM names no image` |
//! | 26 | `COPY --from=alpine:3.22 /etc/alpine-release /srv/` in the runtime stage | the same: `:56 — COPY --from=alpine:3.22` |
//! | 27 | `RUN --mount=type=bind,from=alpine:3.22,target=/probe true` there | the same: `:56 — RUN --mount=…from=alpine:3.22` |
//! | 28 | `# syntax=docker/dockerfile:1` atop `frontend.Dockerfile` | the same: `:1 — # syntax=docker/dockerfile:1` |
//! | 29 | A new `spikes/probe671/Dockerfile-prod`, `FROM alpine:3.22` | the same |
//! | 30 | A new `spikes/probe671/app.containerfile`, `FROM node:24-alpine` | the same |
//! | 31 | A `dockerfile_inline: \|` service, `FROM caddy:2-alpine`, in the development compose | the same: `docker-compose.yml:6` |
//! | 32 | `docker run --rm alpine:latest true` in `deploy-local.sh` | *no deployment file names a floating image tag* |
//! | 33 | An empty line inside a continuation ends it again | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 34 | A `FROM` that names no image is skipped | *the shapes rule 3 refuses are refused* |
//! | 35 | `COPY --from=` is not read | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 36 | A mount's `from=` is not read | *the shapes rule 3 accepts are accepted*, *… refuses are refused* |
//! | 37 | The `syntax` directive is not read | *the shapes rule 3 refuses are refused* |
//! | 38 | A stage index (`--from=0`) is not recognised | *the shapes rule 3 accepts are accepted* |
//! | 39 | `is_dockerfile` misses `Dockerfile-<word>` | *a dockerfile is recognised by its name* |
//! | 40 | `is_dockerfile` misses every `Containerfile` name | *a dockerfile is recognised by its name* |
//! | 41 | `dockerfile_inline:` is not read | *an inline dockerfile is read from compose* |
//! | 42 | `deploy/`'s shell scripts are not walked | *the walk reads the deployment scripts* |
//! | 43 | An inline block does not end at its indentation | *an inline dockerfile is read from compose* |
//! | 44 | A mount's `from=` key matched case-sensitively | *the shapes rule 3 refuses are refused* |
//!
//! **The `test-engineer` gate's mutants 45 and 46 survived all ten tests**,
//! and each went red against the eleventh, added for them:
//!
//! | # | Probe | Reddened |
//! |---|---|---|
//! | 45 | A directive's key matched case-sensitively (`# SYNTAX=` unread) | *a directive is read in any case and only where it is one* |
//! | 46 | Directives read from every comment line, not the leading block alone | *a directive is read in any case and only where it is one* |
//!
//! **The gate's shapes N7 and N8 passed all eleven tests**, a flow mapping
//! and a quoted key, and are read since. Probes 47 and 48 passed the eleven
//! before and went red after; 49–51 change this file:
//!
//! | # | Probe | Reddened |
//! |---|---|---|
//! | 47 | `build: { context: ., dockerfile_inline: "FROM caddy:2-alpine" }` in the development compose | *every dockerfile from names a pinned image*: `docker-compose.yml:3` |
//! | 48 | `"dockerfile_inline": \|` there, `FROM caddy:2-alpine` | the same: `docker-compose.yml:6` |
//! | 49 | The key read only at a line's start, unquoted | *an inline dockerfile is read in every form* |
//! | 50 | The quoted keys not looked for | *an inline dockerfile is read in every form* |
//! | 51 | A single-quoted value ends at any quote, `''` included | *an inline dockerfile is read in every form* |

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

/// Every YAML file under `deploy/` and `.github/workflows/`, and every shell
/// script under `deploy/` (#671), whose `docker run` lines name images too.
fn deployment_files() -> Vec<PathBuf> {
    let root = repository_root();
    let mut files = Vec::new();

    collect_by_extension(&root.join("deploy"), &["yml", "yaml", "sh"], &mut files);
    collect_by_extension(
        &root.join(".github/workflows"),
        &["yml", "yaml"],
        &mut files,
    );

    // The same guard the other source walks carry: a walk that finds nothing
    // passes every assertion under it.
    assert!(
        files.len() >= 3,
        "the walk found {} deployment files, which is too few — the layout moved: {files:?}",
        files.len()
    );

    files
}

fn collect_by_extension(directory: &Path, extensions: &[&str], into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries {
        let path = entry.expect("a directory entry").path();

        if path.is_dir() {
            collect_by_extension(&path, extensions, into);
        } else if path
            .extension()
            .is_some_and(|extension| extensions.iter().any(|wanted| extension == *wanted))
        {
            into.push(path);
        }
    }
}

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "yml" || extension == "yaml")
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

    for path in deployment_files().into_iter().filter(|path| is_yaml(path)) {
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

/// Whether a file name is one Docker or Podman builds from: `Dockerfile` or
/// `Containerfile`, either as a suffix (`backend.Dockerfile`,
/// `app.containerfile`), or followed by `.` (`Dockerfile.dev`, but not
/// `Dockerfile.dockerignore` or `Containerfile.containerignore`, the ignore
/// files) or by `-` and a word with no further `.` (`Dockerfile-prod`, but not
/// `Dockerfile-notes.md`).
fn is_dockerfile(name: &str) -> bool {
    let name = name.to_ascii_lowercase();

    ["dockerfile", "containerfile"].iter().any(|stem| {
        name == *stem
            || name.ends_with(&format!(".{stem}"))
            || name
                .strip_prefix(&format!("{stem}."))
                .is_some_and(|rest| !rest.ends_with("ignore"))
            || name
                .strip_prefix(&format!("{stem}-"))
                .is_some_and(|rest| !rest.is_empty() && !rest.contains('.'))
    })
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

/// How a Dockerfile instruction names an image (#671).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Via {
    /// `FROM image`.
    From,
    /// `COPY --from=image`, which copies out of an image or an earlier stage.
    CopyFrom,
    /// `RUN --mount=…,from=image`, which mounts one.
    MountFrom,
}

/// One image a Dockerfile names: the physical line its instruction starts on,
/// how it is named, the image or stage (`None` for a `FROM` that names
/// nothing), and the stage a `FROM` declares, if any.
#[derive(Debug, PartialEq)]
struct ImageReference {
    line: usize,
    via: Via,
    base: Option<String>,
    declares: Option<String>,
}

impl ImageReference {
    /// The instruction as a refusal quotes it.
    fn quoted(&self) -> String {
        let base = self.base.as_deref().unwrap_or_default();

        match self.via {
            Via::From if base.is_empty() => "FROM".into(),
            Via::From => format!("FROM {base}"),
            Via::CopyFrom => format!("COPY --from={base}"),
            Via::MountFrom => format!("RUN --mount=…from={base}"),
        }
    }
}

/// Every instruction in a Dockerfile, as `(the physical line it starts on,
/// the logical line)`.
///
/// A line whose first non-blank character is `#` is a comment; a `#`
/// anywhere else is not, so only whole lines are dropped. A line ending in
/// `\` continues onto the next, and a comment line inside a continuation is
/// dropped without ending it, which is what Docker does. **So is an empty
/// line** (#671): BuildKit joins over one, warning that empty continuation
/// lines will become errors, so `FROM \`, a blank line and `caddy:2-alpine`
/// is one `FROM caddy:2-alpine`. A leading byte-order mark is dropped too, as
/// BuildKit drops it, or the first `FROM` would read as `\u{feff}FROM` and
/// never be seen.
fn instructions(source: &str) -> Vec<(usize, String)> {
    let source = source.trim_start_matches('\u{feff}');
    let mut instructions = Vec::new();
    let mut logical = String::new();
    let mut starts_on = 0;
    let mut continuing = false;

    for (index, physical) in source.lines().enumerate() {
        if physical.trim_start().starts_with('#') || (continuing && physical.trim().is_empty()) {
            continue;
        }

        if !continuing {
            starts_on = index + 1;
        }

        let trimmed = physical.trim_end();
        match trimmed.strip_suffix('\\') {
            Some(continued) => {
                logical.push_str(continued);
                logical.push(' ');
                continuing = true;
            }
            None => {
                logical.push_str(trimmed);
                instructions.push((starts_on, std::mem::take(&mut logical)));
                continuing = false;
            }
        }
    }

    if continuing {
        instructions.push((starts_on, logical));
    }

    instructions
}

/// Every image a Dockerfile names, in order: each `FROM`, each `COPY
/// --from=`, and each `from=` of a `RUN --mount=`.
fn image_references(source: &str) -> Vec<ImageReference> {
    instructions(source)
        .iter()
        .flat_map(|(line, logical)| read_instruction(logical, *line))
        .collect()
}

fn read_instruction(logical: &str, line: usize) -> Vec<ImageReference> {
    let mut words = logical.split_whitespace();
    let Some(keyword) = words.next() else {
        return Vec::new();
    };

    // Flags come before the arguments, always in the `--name=value` form.
    let flags: Vec<&str> = words
        .clone()
        .take_while(|word| word.starts_with("--"))
        .collect();
    let mut arguments = words.skip_while(|word| word.starts_with("--"));
    let reference = |via, base: &str| ImageReference {
        line,
        via,
        base: Some(base.to_string()),
        declares: None,
    };

    if keyword.eq_ignore_ascii_case("FROM") {
        // A `FROM` that names nothing is refused rather than skipped (#671):
        // skipping it is how an instruction this parser misread went unseen.
        let base = arguments.next().map(str::to_string);
        let declares = match (arguments.next(), arguments.next()) {
            (Some(keyword), Some(stage)) if keyword.eq_ignore_ascii_case("AS") => {
                Some(stage.to_ascii_lowercase())
            }
            _ => None,
        };

        vec![ImageReference {
            line,
            via: Via::From,
            base,
            declares,
        }]
    } else if keyword.eq_ignore_ascii_case("COPY") {
        flags
            .iter()
            .filter_map(|flag| flag.strip_prefix("--from="))
            .map(|base| reference(Via::CopyFrom, base))
            .collect()
    } else if keyword.eq_ignore_ascii_case("RUN") {
        flags
            .iter()
            .filter_map(|flag| flag.strip_prefix("--mount="))
            .flat_map(|mount| mount.split(','))
            .filter_map(|option| option.split_once('='))
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case("from"))
            .map(|(_, base)| reference(Via::MountFrom, base.trim()))
            .collect()
    } else {
        Vec::new()
    }
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
    dockerfile_refusals_at(source, 0)
}

/// The same, for a Dockerfile whose first line is line `offset + 1` of the
/// file it sits in, as a compose file's `dockerfile_inline:` does.
fn dockerfile_refusals_at(source: &str, offset: usize) -> Vec<String> {
    let mut stages = HashSet::new();
    let mut refusals = Vec::new();

    // Parser directives come first, before any blank line or instruction, so
    // only the leading comment lines are looked at. Every one of them is,
    // where BuildKit stops at the first comment that is not a directive; the
    // difference can only refuse more.
    for (index, line) in source
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
        .take_while(|(_, line)| line.trim_start().starts_with('#'))
    {
        let Some((key, value)) = line.trim_start()[1..].split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());

        // `# escape=` makes another character the continuation, and this
        // parser reads only `\`: under a backtick escape, `RUN dir C:\` would
        // swallow the `FROM` after it.
        if key == "escape" {
            refusals.push(format!(
                "{} — the `escape` directive changes the continuation character, which this \
                 test does not parse, so a FROM could be hidden from it",
                offset + index + 1
            ));
        }

        // `# syntax=` names the image BuildKit parses the rest of the file
        // with (#671), so `docker/dockerfile:1` follows every 1.x frontend
        // release the way `caddy:2-alpine` follows every Caddy 2.
        if key == "syntax" {
            if let Some(reason) = pin_refusal(value) {
                refusals.push(format!(
                    "{} — `# syntax={value}` {reason}",
                    offset + index + 1
                ));
            }
        }
    }

    for reference in image_references(source) {
        let line = offset + reference.line;

        let Some(base) = reference.base.as_deref() else {
            refusals.push(format!(
                "{line} — `FROM` names no image, so whatever this parser missed went unchecked"
            ));
            continue;
        };
        let named = base.to_ascii_lowercase();

        // `COPY --from=0` names a stage by its index.
        let is_stage_index =
            reference.via != Via::From && named.bytes().all(|byte| byte.is_ascii_digit());

        if named != "scratch" && !stages.contains(&named) && !is_stage_index {
            if let Some(reason) = pin_refusal(base) {
                refusals.push(format!("{line} — `{}` {reason}", reference.quoted()));
            }
        }

        if let Some(stage) = reference.declares {
            stages.insert(stage);
        }
    }

    refusals
}

/// Every `dockerfile_inline:` in a compose file (#671), as `(offset, body)`:
/// the body's first line is line `offset + 1` of the file. The key is found
/// plain or quoted (`"dockerfile_inline":`), and anywhere on a line that is
/// not a comment, so a flow mapping (`build: { dockerfile_inline: "…" }`) is
/// read too (both found by row 21's `test-engineer` gate). A block scalar
/// (`|`, `>`) runs while its lines are blank or indented past the key's line;
/// any other value is one line, its lines numbered on from the key's, so a
/// refusal past its first names a line after the key.
fn inline_dockerfiles(source: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut found = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let Some(value) = inline_value(line) else {
            continue;
        };

        if value.starts_with('|') || value.starts_with('>') {
            let body: Vec<&str> = lines[index + 1..]
                .iter()
                .take_while(|body| body.trim().is_empty() || indent(body) > indent(line))
                .copied()
                .collect();

            found.push((index + 1, body.join("\n")));
        } else {
            found.push((index, one_line_scalar(value)));
        }
    }

    found
}

/// The text after a `dockerfile_inline` key's colon, trimmed, if the line has
/// one outside a comment.
fn inline_value(line: &str) -> Option<&str> {
    if line.trim_start().starts_with('#') {
        return None;
    }

    [
        "\"dockerfile_inline\"",
        "'dockerfile_inline'",
        "dockerfile_inline",
    ]
    .iter()
    .find_map(|key| {
        let rest = line[line.find(key)? + key.len()..].trim_start();

        rest.strip_prefix(':').map(str::trim)
    })
}

/// A one-line YAML scalar as compose reads it. A double-quoted one ends at its
/// closing quote, with `\n` a line break; a single-quoted one at its closing
/// quote; a plain one at a comment, or at the `,` or `}` that ends it inside a
/// flow mapping. In the last two a literal `\n` is read as a line break too,
/// which BuildKit would not do, and which can only find more to refuse.
fn one_line_scalar(value: &str) -> String {
    if let Some(quoted) = value.strip_prefix('"') {
        let mut text = String::new();
        let mut characters = quoted.chars();

        while let Some(character) = characters.next() {
            match character {
                '"' => break,
                '\\' => match characters.next() {
                    Some('n') => text.push('\n'),
                    Some(escaped) => text.push(escaped),
                    None => {}
                },
                _ => text.push(character),
            }
        }

        return text;
    }

    let text = match value.strip_prefix('\'') {
        // `''` is a quote inside a single-quoted scalar; one alone ends it.
        Some(quoted) => {
            let mut text = String::new();
            let mut characters = quoted.chars().peekable();

            while let Some(character) = characters.next() {
                if character == '\'' && characters.next_if_eq(&'\'').is_none() {
                    break;
                }
                text.push(character);
            }

            text
        }
        None => {
            let plain = value.split(" #").next().unwrap_or_default();

            plain
                .split([',', '}'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        }
    };

    text.replace("\\n", "\n")
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
    let has_from = |source: &str| {
        image_references(source)
            .iter()
            .any(|reference| reference.via == Via::From)
    };

    for path in dockerfiles() {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        assert!(
            has_from(&source),
            "{} has no FROM, so the parser missed it",
            relative(&path)
        );

        for refusal in dockerfile_refusals(&source) {
            offences.push(format!("{}:{refusal}", relative(&path)));
        }
    }

    // A compose file can carry a Dockerfile of its own (#671).
    for path in deployment_files().into_iter().filter(|path| is_yaml(path)) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} could not be read: {error}", path.display()));

        for (offset, body) in inline_dockerfiles(&source) {
            assert!(
                has_from(&body),
                "{}:{} has a dockerfile_inline with no FROM, so the parser missed it",
                relative(&path),
                offset + 1
            );

            for refusal in dockerfile_refusals_at(&body, offset) {
                offences.push(format!("{}:{refusal}", relative(&path)));
            }
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
        // #671: the names `docker build -f` is usually given besides these.
        "Dockerfile-prod",
        "app.containerfile",
        "Containerfile.arm64",
        "CONTAINERFILE-dev",
    ] {
        assert!(is_dockerfile(name), "{name} is a Dockerfile");
    }

    for name in [
        "Dockerfile-notes.md",
        "Dockerfile-",
        "docker-compose.yml",
        "Caddyfile",
        ".dockerignore",
        // BuildKit's per-Dockerfile ignore files, which have no FROM.
        "Dockerfile.dockerignore",
        "Containerfile.containerignore",
    ] {
        assert!(!is_dockerfile(name), "{name} is not a Dockerfile");
    }
}

/// The walk is only worth its assertions if it reaches the scripts that run
/// images (#671): rule 1 reads their `docker run` lines.
#[test]
fn the_walk_reads_the_deployment_scripts() {
    let found: Vec<String> = deployment_files()
        .iter()
        .map(|path| relative(path))
        .collect();

    for expected in [
        "deploy/staging/deploy.sh",
        "deploy/staging/kelir-backup.sh",
        "deploy/docker/docker-compose.yml",
        ".github/workflows/ci.yml",
    ] {
        assert!(
            found.iter().any(|path| path == expected),
            "the deployment walk did not find {expected}: {found:?}"
        );
    }
}

/// The shapes rule 3 exists to let through, in one file.
#[test]
fn the_shapes_rule_3_accepts_are_accepted() {
    let accepted = r"# syntax=docker/dockerfile:1.7.0
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
FROM \

    # a comment inside one continuation, after an empty line
    caddy:2.11.4-alpine AS joined
COPY --from=builder /build/kelir-backend /usr/local/bin/
COPY --chown=1000:1000 --from=0 /build/dist /srv
COPY --from=alpine:3.22.2 /etc/alpine-release /srv/
COPY /local /srv/local
RUN --mount=type=cache,target=/root/.cargo,from=Builder --mount=type=bind,source=/x,target=/y,from=alpine:3.22.2 true
RUN --network=none echo from=alpine
FROM scratch
";

    let references = image_references(accepted);
    let froms: Vec<&ImageReference> = references
        .iter()
        .filter(|reference| reference.via == Via::From)
        .collect();

    assert_eq!(froms.len(), 13, "every FROM is read: {froms:?}");
    assert_eq!(
        *froms[0],
        ImageReference {
            line: 4,
            via: Via::From,
            base: Some("rust:1.89.0-slim-bookworm".into()),
            declares: Some("builder".into()),
        },
        "the platform flag is skipped and the stage recorded"
    );
    assert_eq!(
        *froms[10],
        ImageReference {
            line: 14,
            via: Via::From,
            base: Some("node:24.21.0-alpine".into()),
            declares: Some("continued".into()),
        },
        "a continued FROM is one instruction, on the line it starts"
    );
    assert_eq!(
        *froms[11],
        ImageReference {
            line: 17,
            via: Via::From,
            base: Some("caddy:2.11.4-alpine".into()),
            declares: Some("joined".into()),
        },
        "a FROM continued over an empty line and a comment is one instruction (#671)"
    );

    let others: Vec<(Via, &str)> = references
        .iter()
        .filter(|reference| reference.via != Via::From)
        .map(|reference| (reference.via, reference.base.as_deref().unwrap_or_default()))
        .collect();
    assert_eq!(
        others,
        [
            (Via::CopyFrom, "builder"),
            (Via::CopyFrom, "0"),
            (Via::CopyFrom, "alpine:3.22.2"),
            (Via::MountFrom, "Builder"),
            (Via::MountFrom, "alpine:3.22.2"),
        ],
        "every COPY --from and RUN --mount from= is read, and nothing after the flags (#671)"
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

    // `syntax` is a directive too, and names an image (#671): a release of
    // the frontend passes, and a line of them is in the refused shapes.
    assert_eq!(
        dockerfile_refusals("# syntax = docker/dockerfile:1.7.0\nFROM caddy:2.11.4-alpine"),
        Vec::<String>::new()
    );
}

/// A compose file's `dockerfile_inline:` is a Dockerfile (#671), and a
/// refusal in it names the line of the compose file.
#[test]
fn an_inline_dockerfile_is_read_from_compose() {
    let compose = "services:
  probe:
    build:
      context: .
      dockerfile_inline: |
        FROM node:24.21.0-alpine AS builder

        FROM caddy:2-alpine
      args:
        PROBE: 1
  other:
    build:
      dockerfile_inline: \"FROM rust:1.89.0-slim-bookworm\\nFROM debian:bookworm-slim\"
";

    let inline = inline_dockerfiles(compose);
    assert_eq!(
        inline.len(),
        2,
        "both inline Dockerfiles are found: {inline:?}"
    );

    let (offset, body) = &inline[0];
    assert!(
        !body.contains("PROBE"),
        "the block ends where its indentation does: {body:?}"
    );
    assert_eq!(
        dockerfile_refusals_at(body, *offset),
        [
            "8 — `FROM caddy:2-alpine` `:2-alpine` names a line that moves, not a release; pin a \
          patch-level or dated tag, or a digest"
        ],
    );

    let (offset, body) = &inline[1];
    let refusals = dockerfile_refusals_at(body, *offset);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(
        refusals[0].starts_with("14 — `FROM debian:bookworm-slim`"),
        "a one-line value is read with its \\n as a line break, its lines numbered on from \
         its key: {refusals:?}"
    );
}

/// A `dockerfile_inline:` is read in each form compose accepts (#671): the
/// `test-engineer` gate's flow mapping (N7) and quoted key (N8) passed every
/// test until they were read.
#[test]
fn an_inline_dockerfile_is_read_in_every_form() {
    for (compose, line) in [
        // N7: a flow mapping.
        (
            "services:\n  probe:\n    build: { context: ., dockerfile_inline: \"FROM caddy:2-alpine\" }\n",
            3,
        ),
        // N8: a quoted key, double and single.
        (
            "services:\n  probe:\n    build:\n      \"dockerfile_inline\": |\n        FROM caddy:2-alpine\n",
            5,
        ),
        (
            "services:\n  probe:\n    build:\n      'dockerfile_inline': |-\n        FROM caddy:2-alpine\n",
            5,
        ),
        // One-line values: plain with a comment, plain ending a flow mapping,
        // and single-quoted with a `''` before the FROM it carries.
        (
            "    build:\n      dockerfile_inline: FROM caddy:2-alpine # the floating one\n",
            2,
        ),
        ("    build: {dockerfile_inline: FROM caddy:2-alpine}\n", 1),
        (
            "    build:\n      dockerfile_inline: 'RUN echo ''x''\\nFROM caddy:2-alpine'\n",
            3,
        ),
    ] {
        let inline = inline_dockerfiles(compose);
        assert_eq!(inline.len(), 1, "one inline Dockerfile in {compose:?}: {inline:?}");

        let (offset, body) = &inline[0];
        let refusals = dockerfile_refusals_at(body, *offset);
        assert_eq!(
            refusals.len(),
            1,
            "exactly one refusal for {compose:?}: {refusals:?}"
        );
        assert!(
            refusals[0].starts_with(&format!("{line} — `FROM caddy:2-alpine`")),
            "the refusal names line {line} and the image: {refusals:?}"
        );
    }

    // A comment naming the key is not a Dockerfile.
    assert!(inline_dockerfiles("    # dockerfile_inline: is read too\n").is_empty());
}

/// Each shape rule 3 exists to refuse, sent on its own, with the instruction
/// as the refusal quotes it.
#[test]
fn the_shapes_rule_3_refuses_are_refused() {
    for (dockerfile, refused) in [
        ("FROM caddy:2-alpine AS runtime", "FROM caddy:2-alpine"),
        ("FROM node:24-alpine", "FROM node:24-alpine"),
        (
            "FROM rust:1.89-slim-bookworm",
            "FROM rust:1.89-slim-bookworm",
        ),
        ("FROM debian:bookworm-slim", "FROM debian:bookworm-slim"),
        ("FROM node:24-alpine3.22", "FROM node:24-alpine3.22"),
        ("FROM alpine:latest", "FROM alpine:latest"),
        ("FROM postgres", "FROM postgres"),
        (
            "FROM localhost:5000/kelir/base",
            "FROM localhost:5000/kelir/base",
        ),
        ("ARG BASE=caddy:2.11.4-alpine\nFROM ${BASE}", "FROM ${BASE}"),
        ("FROM caddy@sha256:6aeddd44", "FROM caddy@sha256:6aeddd44"),
        (
            "FROM --platform=linux/amd64 caddy:2-alpine AS runtime",
            "FROM caddy:2-alpine",
        ),
        (
            "FROM \\\n    caddy:2-alpine AS runtime",
            "FROM caddy:2-alpine",
        ),
        // A comment ending in `\` does not continue, so it hides nothing.
        (
            "# a comment ending in a backslash \\\nFROM caddy:2-alpine",
            "FROM caddy:2-alpine",
        ),
        // A build argument inside the tag hides the release as well.
        (
            "ARG VARIANT=alpine\nFROM caddy:2.11.4-${VARIANT}",
            "FROM caddy:2.11.4-${VARIANT}",
        ),
        // A byte-order mark does not hide the first FROM.
        (
            "\u{feff}FROM caddy:2-alpine\nFROM scratch",
            "FROM caddy:2-alpine",
        ),
        (
            "FROM debian:bookworm-2026091-slim",
            "FROM debian:bookworm-2026091-slim",
        ),
        ("FROM example/base:a.b.c", "FROM example/base:a.b.c"),
        (
            "FROM ghcr.io/sujanto-gaws/minio:RELEASE.latest",
            "FROM ghcr.io/sujanto-gaws/minio:RELEASE.latest",
        ),
        // A stage is known only once declared, so this is an image.
        (
            "FROM builder\nFROM caddy:2.11.4-alpine AS builder",
            "FROM builder",
        ),
        // A later stage is refused even when the earlier ones are pinned.
        (
            "FROM rust:1.89.0-slim-bookworm AS builder\nFROM builder\nFROM caddy:2",
            "FROM caddy:2",
        ),
        // #671, record 21's probe M3: an empty line inside a continuation
        // does not end it, as BuildKit reads it, whitespace or not.
        (
            "FROM \\\n\ncaddy:2-alpine AS runtime",
            "FROM caddy:2-alpine",
        ),
        (
            "FROM --platform=linux/amd64 \\\n  \t\n# a comment\n\ncaddy:2-alpine",
            "FROM caddy:2-alpine",
        ),
        // A FROM that names nothing is refused, not skipped.
        ("FROM rust:1.89.0-slim-bookworm AS builder\nFROM", "FROM"),
        ("FROM --platform=linux/amd64", "FROM"),
        // The images a stage copies from or mounts.
        (
            "FROM caddy:2.11.4-alpine\nCOPY --from=alpine:3.22 /etc/alpine-release /srv/",
            "COPY --from=alpine:3.22",
        ),
        (
            "FROM caddy:2.11.4-alpine\nCOPY --from=builder /x /y\n\
             FROM rust:1.89.0-slim-bookworm AS builder",
            "COPY --from=builder",
        ),
        (
            "FROM caddy:2.11.4-alpine\nCOPY --chmod=755 --from=node /x /y",
            "COPY --from=node",
        ),
        (
            "FROM caddy:2.11.4-alpine\n\
             RUN --mount=type=bind,from=alpine:3.22,target=/probe true",
            "RUN --mount=…from=alpine:3.22",
        ),
        (
            "FROM caddy:2.11.4-alpine\n\
             RUN --network=none --mount=type=cache,target=/x,FROM=node:24-alpine true",
            "RUN --mount=…from=node:24-alpine",
        ),
        // The frontend BuildKit parses the file with is an image as well.
        (
            "# syntax=docker/dockerfile:1\nFROM caddy:2.11.4-alpine",
            "# syntax=docker/dockerfile:1",
        ),
        (
            "\u{feff}#syntax=docker/dockerfile:1.7-labs\nFROM caddy:2.11.4-alpine",
            "# syntax=docker/dockerfile:1.7-labs",
        ),
    ] {
        let refusals = dockerfile_refusals(dockerfile);

        assert_eq!(
            refusals.len(),
            1,
            "exactly one refusal for {dockerfile:?}: {refusals:?}"
        );
        assert!(
            refusals[0].contains(&format!("`{refused}`")),
            "the refusal names {refused}: {refusals:?}"
        );
    }
}

/// A parser directive is read as BuildKit reads one (#671): its key in any
/// case, and only in the comment lines the file opens with. A `# syntax=`
/// after an instruction or a blank line is a comment, not a directive, and
/// BuildKit never fetches the image it names.
#[test]
fn a_directive_is_read_in_any_case_and_only_where_it_is_one() {
    for (dockerfile, refused) in [
        (
            "# SYNTAX=docker/dockerfile:1\nFROM caddy:2.11.4-alpine",
            "# syntax=docker/dockerfile:1",
        ),
        (
            "# Syntax = docker/dockerfile:1\nFROM caddy:2.11.4-alpine",
            "# syntax=docker/dockerfile:1",
        ),
        (
            "# Escape=`\nFROM caddy:2.11.4-alpine",
            "the `escape` directive",
        ),
    ] {
        let refusals = dockerfile_refusals(dockerfile);

        assert_eq!(
            refusals.len(),
            1,
            "exactly one refusal for {dockerfile:?}: {refusals:?}"
        );
        assert!(
            refusals[0].contains(refused),
            "the refusal names {refused}: {refusals:?}"
        );
    }

    for not_a_directive in [
        "FROM caddy:2.11.4-alpine\n# syntax=docker/dockerfile:1",
        "\n# syntax=docker/dockerfile:1\nFROM caddy:2.11.4-alpine",
    ] {
        assert_eq!(
            dockerfile_refusals(not_a_directive),
            Vec::<String>::new(),
            "a `# syntax=` that is not in the leading comment block is a comment: \
             {not_a_directive:?}"
        );
    }
}
