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
