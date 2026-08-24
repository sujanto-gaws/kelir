//! Bakes the build commit into the binary so `/version` can report it.
//!
//! Release process §4 step 7 makes the staging smoke test check that `/version`
//! reports the expected version *and SHA*, so this cannot be resolved at
//! runtime — the deployed artifact has to carry it.
//!
//! Resolution order:
//!   1. `KELIR_BUILD_SHA` from the environment — how CI and the Docker build
//!      supply it, since neither has a usable `.git` directory.
//!   2. `git rev-parse --short HEAD` — the local development case.
//!   3. `unknown` — never fails the build; a missing SHA is a smoke-test
//!      finding, not a compile error.
//!
//! It also declares `migrations/` as an input, which is nothing to do with the
//! SHA and everything to do with `sqlx::migrate!` embedding that directory at
//! compile time. Nothing else tells Cargo so: adding a migration left `db.rs`
//! untouched, so an incremental build kept running the previous set and the
//! new file was applied by nothing. `applies_every_migration_in_the_migrations_directory`
//! is what noticed, one migration after the fact.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=KELIR_BUILD_SHA");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=migrations");

    let sha = std::env::var("KELIR_BUILD_SHA")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(git_short_sha)
        .unwrap_or_else(|| "unknown".to_owned());

    println!("cargo:rustc-env=KELIR_BUILD_SHA={sha}");
}

fn git_short_sha() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sha = String::from_utf8(output.stdout).ok()?.trim().to_owned();

    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}
