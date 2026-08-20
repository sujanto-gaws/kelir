//! A rolled-back binary starts against a database a newer release migrated (#76).
//!
//! [Release process](../../docs/standards/04.%20Release%20Process.md) §6 calls
//! application rollback "always possible because images are immutable", and
//! requires every migration to be backward-compatible with the previous release.
//! The `v0.2.0` rehearsal found that the rule was being met in the columns and
//! broken in the bookkeeping: the schema was additive and the `v0.1.0` source
//! compiled against it, but SQLx refused to start because `_sqlx_migrations`
//! held versions the binary could not resolve.
//!
//! Both halves of the trade are asserted here. Migrations from a newer release
//! are tolerated, and an *edited* migration is still refused — the tolerance is
//! for versions this binary has never heard of, not a licence to change one that
//! has already run.

mod common;

use common::TestApp;
use kelir_backend::db;
use sqlx::migrate::MigrateError;

/// Higher than any migration this project will plausibly write, so it can only
/// mean "applied by something newer than me".
const FROM_A_NEWER_RELEASE: i64 = 99_999_999_999_999;

#[tokio::test]
async fn a_database_migrated_by_a_newer_release_still_starts() {
    let app = TestApp::spawn().await;

    // What a rollback leaves behind: the newer release ran its migrations and
    // recorded them, then its image was replaced by the previous one.
    sqlx::query(
        r#"
        INSERT INTO _sqlx_migrations
            (version, description, installed_on, success, checksum, execution_time)
        VALUES ($1, 'applied by a newer release', now(), true, '\x00'::bytea, 0)
        "#,
    )
    .bind(FROM_A_NEWER_RELEASE)
    .execute(&app.pool)
    .await
    .expect("record a migration from a newer release");

    // Before #76 this returned MigrateError::VersionMissing and the process
    // exited: the previous image could not be redeployed without first deleting
    // rows out of _sqlx_migrations by hand.
    db::run_migrations(&app.pool)
        .await
        .expect("a rolled-back binary must start against a database a newer release migrated");
}

#[tokio::test]
async fn an_edited_migration_is_still_refused() {
    let app = TestApp::spawn().await;

    // Migrations are immutable once merged (coding standard §2.5), and the
    // checksum in _sqlx_migrations is what enforces it. Tolerating *unknown*
    // versions must not tolerate a *changed* one — without this case, #76's fix
    // could be widened to `ignore` everything and nothing would notice.
    sqlx::query("UPDATE _sqlx_migrations SET checksum = '\\x00'::bytea WHERE version = 1")
        .execute(&app.pool)
        .await
        .expect("corrupt a recorded checksum");

    let result = db::run_migrations(&app.pool).await;

    assert!(
        matches!(result, Err(MigrateError::VersionMismatch(1))),
        "an edited migration must still fail loudly, got {result:?}"
    );
}
