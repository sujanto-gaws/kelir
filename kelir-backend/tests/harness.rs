//! Verification of the integration harness itself.
//!
//! A harness that silently shares state between tests, or silently skips
//! migrations, makes every test above it worthless — and does so quietly. These
//! check the properties the rest of the suite assumes.

mod common;

use common::{fixtures, redact, with_database, TestApp};

// ---------------------------------------------------------------------------
// Connection-string handling (pure, no database)
// ---------------------------------------------------------------------------

#[test]
fn replaces_the_database_in_a_connection_string() {
    assert_eq!(
        with_database("postgres://u:p@localhost:5432/kelir", "kelir_test_1"),
        "postgres://u:p@localhost:5432/kelir_test_1"
    );
}

#[test]
fn keeps_query_parameters_when_replacing_the_database() {
    // sslmode is normal in a managed-PostgreSQL URL. Dropping it would make the
    // test database unreachable for a reason nobody would guess from the error.
    assert_eq!(
        with_database("postgres://u:p@host/kelir?sslmode=require", "t"),
        "postgres://u:p@host/t?sslmode=require"
    );
}

#[test]
fn adds_a_database_to_a_url_that_names_none() {
    assert_eq!(
        with_database("postgres://u:p@localhost:5432", "t"),
        "postgres://u:p@localhost:5432/t"
    );
}

#[test]
fn hides_the_password_in_harness_diagnostics() {
    // The harness banner is printed on failure, and CI logs are widely readable.
    let redacted = redact("postgres://kelir:hunter2@localhost:5432/kelir");

    assert!(!redacted.contains("hunter2"), "got {redacted}");
    assert_eq!(redacted, "postgres://kelir:***@localhost:5432/kelir");
}

#[test]
fn leaves_a_connection_string_without_credentials_alone() {
    assert_eq!(
        redact("postgres://localhost:5432/kelir"),
        "postgres://localhost:5432/kelir"
    );
}

// ---------------------------------------------------------------------------
// Provisioning
// ---------------------------------------------------------------------------

#[tokio::test]
async fn applies_every_migration_in_the_migrations_directory() {
    let app = TestApp::spawn().await;

    // Counted from the directory rather than hard-coded, so adding a migration
    // does not require editing this test — and forgetting to ship one does.
    let on_disk = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
        .expect("the migrations directory is readable")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sql"))
        .count() as i64;

    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&app.pool)
        .await
        .expect("the migration bookkeeping table exists");

    assert_eq!(
        applied, on_disk,
        "the harness must apply every committed migration"
    );
    assert!(on_disk > 0, "guard against an empty migrations directory");
}

#[tokio::test]
async fn seeds_the_permission_catalogue_and_the_admin_role() {
    let app = TestApp::spawn().await;

    fixtures::assert_admin_role_exists(&app.pool).await;

    let permissions: i64 = sqlx::query_scalar("SELECT count(*) FROM permissions")
        .fetch_one(&app.pool)
        .await
        .expect("permissions are queryable");

    assert!(
        permissions >= 10,
        "0002_identity.sql seeds the Phase 2 catalogue; found {permissions}"
    );

    // ROLE-ADMIN holding every permission is the premise of every "an
    // administrator can do X" assertion in this suite.
    let ungranted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM permissions p
         WHERE NOT EXISTS (
             SELECT 1 FROM role_permissions rp
             WHERE rp.permission_id = p.id AND rp.role_id = $1 AND rp.deleted_at IS NULL
         )",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .fetch_one(&app.pool)
    .await
    .expect("role_permissions is queryable");

    assert_eq!(ungranted, 0, "ROLE-ADMIN must hold every permission");
}

#[tokio::test]
async fn gives_each_test_its_own_database() {
    // The property the whole suite rests on: two instances alive at once must
    // not see each other's rows, or every assertion becomes order-dependent.
    let first = TestApp::spawn().await;
    let second = TestApp::spawn().await;

    assert_ne!(first.database_name, second.database_name);

    fixtures::create_user(
        &first.pool,
        fixtures::SYSTEM_TENANT_ID,
        "user.only.in.first",
        "only.in.first@kelir.test",
        "harness-isolation-password",
        &[],
    )
    .await;

    let in_first: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'user.only.in.first'")
            .fetch_one(&first.pool)
            .await
            .expect("queryable");
    let in_second: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'user.only.in.first'")
            .fetch_one(&second.pool)
            .await
            .expect("queryable");

    assert_eq!(in_first, 1);
    assert_eq!(
        in_second, 0,
        "a row written by one test leaked into another"
    );
}

#[tokio::test]
async fn drops_its_database_when_the_test_ends() {
    let name = {
        let app = TestApp::spawn().await;
        app.database_name.clone()
    };

    // A fresh instance is only used for a connection to the server; the
    // database under test is already gone by the time this runs.
    let probe = TestApp::spawn().await;
    let leftover: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_database WHERE datname = $1")
        .bind(&name)
        .fetch_one(&probe.pool)
        .await
        .expect("pg_database is queryable");

    assert_eq!(
        leftover, 0,
        "test database {name} was left behind; a full suite run would accumulate them"
    );
}

// ---------------------------------------------------------------------------
// First-run bootstrap
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_first_run_bootstrap_creates_one_administrator_holding_every_permission() {
    let app = TestApp::spawn().await;

    let (id, status): (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, status FROM users WHERE username = $1")
            .bind(common::ADMIN_USERNAME)
            .fetch_one(&app.pool)
            .await
            .expect("the bootstrap created the administrator");

    assert_eq!(status, "ACTIVE");

    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT r.role_code FROM user_roles ur
         JOIN roles r ON r.id = ur.role_id
         WHERE ur.user_id = $1 AND ur.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("queryable");

    assert_eq!(roles, vec!["ROLE-ADMIN".to_owned()]);
}

#[tokio::test]
async fn the_bootstrap_does_nothing_once_a_user_exists() {
    // Called on every start (main.rs), so a second run must not create a second
    // administrator — nor resurrect one that was deliberately removed.
    let app = TestApp::spawn().await;

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&app.pool)
        .await
        .expect("queryable");

    kelir_backend::modules::auth::bootstrap::ensure_administrator(&app.pool, &app.state.config)
        .await
        .expect("a repeat bootstrap is not an error");

    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&app.pool)
        .await
        .expect("queryable");

    assert_eq!(before, 1);
    assert_eq!(
        after, before,
        "the bootstrap created a second administrator"
    );
}
