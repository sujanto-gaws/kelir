//! The two `minio-init` scripts are one script, and the workflow makes its
//! buckets as that script does (#807, plan 21 row 41, AC-41-7 and AC-41-8).
//!
//! # Why a test
//!
//! `minio-init` is the one place a Kelir store is told to encrypt by default
//! (ADR-0049 §2), and it is written out twice: in
//! `deploy/staging/docker-compose.staging.yml`, which a release is deployed
//! from, and in `deploy/docker/docker-compose.yml`, which every developer and
//! the browser job's predecessor ran. **No test read either.** A fix made in
//! one and not the other is a development stack that stops proving anything
//! about the release, and nothing would have failed.
//!
//! The third copy is `.github/workflows/ci.yml`, whose MinIO steps make a
//! bucket for the integration tests and one for the encryption probe. They
//! are not the same script and are not held to be: the alias, the bucket and
//! the waiting differ. What they must not do is make a bucket **without**
//! encrypting it, or with a command `minio-init` does not use.
//!
//! # The rule
//!
//! 1. `minio-init`'s `command:` script is extracted from each compose file,
//!    by its literal block's indentation, and the two are compared **byte for
//!    byte**. Nothing is normalised, because no difference is documented: the
//!    credentials that do differ (a `.env`'s pair, `minioadmin`) are in
//!    `environment:`, outside the script. A difference that becomes
//!    deliberate is normalised here, in [`normalised`], with its reason.
//! 2. Every workflow step that runs `mc mb` also runs `mc encrypt set
//!    sse-s3`, and each `mc mb` and `mc encrypt` command it runs, read up to
//!    its target, is one the compose script runs: a subset check. At least
//!    two such steps are found, the backend tests' store and the probe's.
//!
//! **Comments are stripped from the workflow first**, so prose naming a
//! command is not a use of it. The probe's own `mc mb` and `mc encrypt`, in
//! `encryption-at-rest.probe.sh`, make a scratch store and are not read.
//!
//! # Seen to fail (coding standard §2.9), 2026-11-08
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `sleep 1` changed to `sleep 2` in the staging compose's wait loop | *the two minio init scripts are the same*, at script line 2 |
//! | `kelir/kelir` changed to `kelir/kelirr` on the development compose's `mc mb` line | *the two minio init scripts are the same*, at script line 3 |
//! | `&& mc encrypt set sse-s3 ci/kelir-test` dropped from the workflow | *every workflow bucket is made as minio init makes it*, naming the backend tests' step |

use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate has a parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repository_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .replace("\r\n", "\n")
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The `minio-init` service's `command:` script: the literal block (`- |`)
/// under `command:`, each line with the block's indentation removed.
/// `None` when the service, its `command:` or the block is not found.
fn minio_init_script(compose: &str) -> Option<String> {
    let lines: Vec<&str> = compose.lines().collect();
    let service = lines
        .iter()
        .position(|line| line.trim_end() == "  minio-init:")?;

    let mut index = service + 1;
    let command = loop {
        let line = *lines.get(index)?;
        if !line.trim().is_empty() && indentation(line) <= 2 {
            return None; // the next service, and no command
        }
        if line.trim_end() == "    command:" {
            break index;
        }
        index += 1;
    };

    let block_start = lines.get(command + 1)?;
    if block_start.trim() != "- |" {
        return None;
    }
    let first = lines.get(command + 2)?;
    let indent = indentation(first);
    if indent <= indentation(block_start) {
        return None;
    }

    let mut script = Vec::new();
    for line in &lines[command + 2..] {
        if line.trim().is_empty() {
            script.push("");
        } else if indentation(line) >= indent {
            script.push(&line[indent..]);
        } else {
            break;
        }
    }
    while script.last() == Some(&"") {
        script.pop();
    }
    Some(script.join("\n"))
}

/// The documented differences between the two scripts, removed. There are
/// none: see the module's rule 1.
fn normalised(script: &str) -> String {
    script.to_string()
}

/// Each `mc mb …` and `mc encrypt …` command in `source`, read up to its
/// target: the words before the first one naming an alias path (holding
/// `/`), a variable or a quote. `mc mb --ignore-existing ci/kelir-test` is
/// `mc mb --ignore-existing`.
fn bucket_commands(source: &str) -> Vec<String> {
    let words: Vec<&str> = source
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '\\'))
        .filter(|word| !word.is_empty())
        .collect();

    let mut commands = Vec::new();
    for (at, pair) in words.windows(2).enumerate() {
        if pair[0] == "mc" && matches!(pair[1], "mb" | "encrypt") {
            let command: Vec<&str> = words[at..]
                .iter()
                .take_while(|word| !word.contains(['/', '$', '"', '\'']))
                .copied()
                .collect();
            commands.push(command.join(" "));
        }
    }
    commands
}

/// The workflow with every `#` comment removed. A `#` inside a `run:` script
/// that is not a comment would be cut too; none of this file's MinIO steps
/// has one, and a cut command would fail rule 2 rather than pass it.
fn without_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.find('#').map_or(line, |at| &line[..at]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each step of the workflow as (its name, its text), split at `- name:`.
fn steps(workflow: &str) -> Vec<(String, String)> {
    let mut steps: Vec<(String, String)> = Vec::new();
    for line in workflow.lines() {
        if let Some(name) = line.trim_start().strip_prefix("- name: ") {
            steps.push((name.trim().to_string(), String::new()));
        } else if let Some((_, text)) = steps.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    steps
}

const STAGING: &str = "deploy/staging/docker-compose.staging.yml";
const DEVELOPMENT: &str = "deploy/docker/docker-compose.yml";

fn script_of(relative: &str) -> String {
    let script = minio_init_script(&read(relative))
        .unwrap_or_else(|| panic!("no minio-init `command:` literal block in {relative}"));
    // The guard every walk here carries: an extraction that finds an empty
    // or truncated script would agree with another empty one.
    assert!(
        script.contains("mc mb --ignore-existing") && script.contains("mc encrypt set sse-s3"),
        "the minio-init script read from {relative} lacks its bucket commands; the extraction is wrong:\n{script}"
    );
    script
}

#[test]
fn the_two_minio_init_scripts_are_the_same() {
    let staging = normalised(&script_of(STAGING));
    let development = normalised(&script_of(DEVELOPMENT));

    if staging != development {
        let difference = staging
            .lines()
            .zip(development.lines())
            .enumerate()
            .find(|(_, (left, right))| left != right)
            .map_or_else(
                || "one script is longer than the other".to_string(),
                |(line, (left, right))| {
                    format!(
                        "script line {}:\n  {STAGING}: {left}\n  {DEVELOPMENT}: {right}",
                        line + 1
                    )
                },
            );
        panic!("the two minio-init scripts differ at {difference}");
    }
}

#[test]
fn every_workflow_bucket_is_made_as_minio_init_makes_it() {
    let compose_commands = bucket_commands(&script_of(STAGING));
    let workflow = without_comments(&read(".github/workflows/ci.yml"));

    let mut makers = Vec::new();
    let mut refusals = Vec::new();
    for (name, text) in steps(&workflow) {
        let commands = bucket_commands(&text);
        if !commands.iter().any(|command| command.starts_with("mc mb")) {
            continue;
        }
        if !commands
            .iter()
            .any(|command| command == "mc encrypt set sse-s3")
        {
            refusals.push(format!(
                "`{name}` makes a bucket and never runs `mc encrypt set sse-s3`"
            ));
        }
        for command in &commands {
            if !compose_commands.contains(command) {
                refusals.push(format!(
                    "`{name}` runs `{command}`, which minio-init does not: {compose_commands:?}"
                ));
            }
        }
        makers.push(name);
    }

    assert!(
        makers.len() >= 2,
        "found {} workflow step(s) that make a bucket, expected the backend tests' store and the probe's: {makers:?}",
        makers.len()
    );
    assert!(refusals.is_empty(), "{}", refusals.join("\n"));
}

#[test]
fn a_command_is_read_up_to_its_target() {
    assert_eq!(
        bucket_commands(
            "until mc alias set ci x; do sleep 1; done \\\n && mc mb --ignore-existing ci/kelir-test \\\n && mc encrypt set sse-s3 ci/kelir-test"
        ),
        ["mc mb --ignore-existing", "mc encrypt set sse-s3"]
    );
    assert_eq!(
        bucket_commands("[[ \"$$(mc encrypt info --json kelir/kelir)\" == x ]]"),
        ["mc encrypt info --json"]
    );
}

#[test]
fn a_literal_block_is_read_to_its_end() {
    let compose = "services:\n  minio-init:\n    image: x\n    command:\n      - |\n        one\n\n          two\n        three\n\n  next:\n    image: y\n";
    assert_eq!(
        minio_init_script(compose).as_deref(),
        Some("one\n\n  two\nthree")
    );
    assert_eq!(minio_init_script("services:\n  minio-init:\n    image: x\n  next:\n    command:\n      - |\n        one\n"), None);
}
