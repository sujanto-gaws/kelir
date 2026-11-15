//! `test-engineer`'s campaign against the serving role (ADR-0051, [#842]).
//!
//! `audit_append_only.rs` holds the serving role against `audit_events`, and
//! `database_roles.rs` holds the startup that puts it there. This file attacks
//! both from the side a deployment can get wrong and a builder's suite does not
//! reach:
//!
//! - **the posture check, in `staging` and in `production`**, against each
//!   shape a serving role can be given too much in: an attribute, a
//!   membership, a grant through `PUBLIC` or through another role, a
//!   predefined role, a column grant, an object it owns;
//! - **the roads out of the database**: `COPY … PROGRAM`, server files, large
//!   objects, `SET ROLE`, role and default-privilege changes, rules, policies
//!   and triggers on `audit_events`, and `SECURITY DEFINER` functions;
//! - **a later migration**, run by sqlx as the owner, as a release would: the
//!   default privileges grant what it creates, and the narrowing holds against
//!   a table it recreates;
//! - **the N−1 question at the migrator** (ADR-0051 §4): `v0.9.0`'s migration
//!   set, on `v0.9.0`'s sqlx, started as the serving role and as the owner.
//!
//! A per-cluster change (an attribute, a membership, a predefined role) is
//! made to a [`common::ScratchServingRole`], never to the shared
//! [`common::SERVING_ROLE`], so the tests running beside these never see it.
//!
//! [#842]: https://github.com/sujanto-gaws/kelir/issues/842

mod common;

use common::TestApp;
use kelir_backend::config::{AppConfig, AppEnv};
use kelir_backend::db;
use sqlx::migrate::{MigrateError, Migration, MigrationType, Migrator};
use sqlx::{Connection, PgConnection, PgPool};

/// What PostgreSQL answers a statement the role holds no privilege or
/// ownership for: `insufficient_privilege`.
const INSUFFICIENT_PRIVILEGE: &str = "42501";

/// The two environments that check the serving role's posture.
const DEPLOYMENTS: [AppEnv; 2] = [AppEnv::Staging, AppEnv::Production];

/// The owner's URL the harness gave the application.
fn owner_url(app: &TestApp) -> String {
    app.state
        .config
        .database_migration_url
        .clone()
        .expect("the harness configures the owner's URL")
}

/// The owner's role name, quoted for SQL.
async fn the_owner(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT quote_ident(current_user)")
        .fetch_one(&app.pool)
        .await
        .expect("the owner names itself")
}

/// Starts the database as `main.rs` does in `env`, with both URLs, serving as
/// `serving_url`, and returns the refusal's text, or `None` when it started.
async fn start(app: &TestApp, env: AppEnv, serving_url: &str) -> Option<String> {
    let config = AppConfig {
        app_env: env,
        database_url: serving_url.to_owned(),
        database_migration_url: Some(owner_url(app)),
        ..(*app.state.config).clone()
    };

    match db::prepare_serving_pool(&config).await {
        Ok(pool) => {
            pool.close().await;
            None
        }
        Err(error) => Some(error.to_string()),
    }
}

/// Each deployment in which the startup did not refuse `serving_url` with a
/// refusal naming `naming`, as a line saying what it did instead.
async fn not_refused(app: &TestApp, serving_url: &str, naming: &str) -> Vec<String> {
    let mut wrong = Vec::new();

    for env in DEPLOYMENTS {
        match start(app, env, serving_url).await {
            None => wrong.push(format!("{env:?} started")),
            Some(refusal) if refusal.contains(naming) => {}
            Some(refusal) => wrong.push(format!(
                "{env:?} refused, without naming `{naming}`: {refusal}"
            )),
        }
    }

    wrong
}

/// Runs `sql` on `pool` in a transaction that is always rolled back, and
/// returns the refusal's SQLSTATE and text, or `None` when it landed.
async fn as_role(pool: &PgPool, sql: &str) -> Option<(String, String)> {
    let mut transaction = pool.begin().await.expect("the pool opens a transaction");

    let outcome = sqlx::query(sql).execute(&mut *transaction).await;
    let _ = transaction.rollback().await;

    match outcome {
        Ok(_) => None,
        Err(error) => {
            let code = error
                .as_database_error()
                .and_then(|database| database.code())
                .map(|code| code.into_owned())
                .unwrap_or_default();

            Some((code, error.to_string()))
        }
    }
}

/// Each of `cases` that `pool`'s role is not refused on privilege.
async fn not_refused_on_privilege(pool: &PgPool, cases: &[String]) -> Vec<String> {
    let mut wrong = Vec::new();

    for sql in cases {
        match as_role(pool, sql).await {
            None => wrong.push(format!("landed: {sql}")),
            Some((code, _)) if code == INSUFFICIENT_PRIVILEGE => {}
            Some((code, message)) => {
                wrong.push(format!(
                    "refused, not on privilege ({code} {message}): {sql}"
                ));
            }
        }
    }

    wrong
}

/// Runs a statement through the owner's pool.
async fn as_the_owner(app: &TestApp, sql: &str) {
    sqlx::query(sql)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("the owner's `{sql}`: {error}"));
}

// ---------------------------------------------------------------------------
// The posture check, in staging and in production (ADR-0051 §2 and §5)
// ---------------------------------------------------------------------------

/// The control for every posture case below: a scratch role, provisioned and
/// narrowed as the serving role is, **starts in both deployments**. Each case
/// changes one thing from this, so a refusal there is that thing's.
#[tokio::test]
async fn a_scratch_serving_role_as_provisioned_starts_in_both_deployments() {
    let app = TestApp::spawn().await;
    let scratch = app.scratch_serving_role().await;

    for env in DEPLOYMENTS {
        let refusal = start(&app, env, &scratch.url).await;
        assert_eq!(refusal, None, "{env:?} refused the role as provisioned");
    }

    scratch.drop_from(&app).await;
}

/// `KELIR_DATABASE_URL` naming the owner, in **production** as well as the
/// `staging` `database_roles.rs` covers.
#[tokio::test]
async fn both_deployments_refuse_a_serving_url_that_names_the_owner() {
    let app = TestApp::spawn().await;

    let wrong = not_refused(&app, &owner_url(&app), "the owner of audit_events").await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A superuser serving URL that is **not** the owner: it passes every
/// privilege check, so only the attribute can refuse it.
#[tokio::test]
async fn both_deployments_refuse_a_superuser_serving_role() {
    let app = TestApp::spawn().await;
    let scratch = app.scratch_serving_role().await;

    as_the_owner(&app, &format!("ALTER ROLE {} SUPERUSER", scratch.name)).await;

    let wrong = not_refused(&app, &scratch.url, "it is a superuser").await;
    scratch.drop_from(&app).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The serving role made a member of the owner, so it **inherits** the
/// owner's ownership, and, with `INHERIT FALSE`, so it can only `SET ROLE` to
/// it. Either is the owner's credential by another name.
#[tokio::test]
async fn both_deployments_refuse_a_serving_role_that_is_a_member_of_the_owner() {
    let app = TestApp::spawn().await;
    let owner = the_owner(&app).await;
    let mut wrong = Vec::new();

    for inherit in ["TRUE", "FALSE"] {
        let scratch = app.scratch_serving_role().await;
        as_the_owner(
            &app,
            &format!("GRANT {owner} TO {} WITH INHERIT {inherit}", scratch.name),
        )
        .await;

        for line in not_refused(&app, &scratch.url, "the owner of audit_events").await {
            wrong.push(format!("INHERIT {inherit}: {line}"));
        }
        scratch.drop_from(&app).await;
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A grant to `PUBLIC`** reaches the serving role, and the narrowing, which
/// revokes from the serving role by name, does not take it back. So it is the
/// posture check that must refuse it, for each verb alone.
#[tokio::test]
async fn both_deployments_refuse_a_verb_on_audit_events_granted_to_public() {
    let app = TestApp::spawn().await;
    let serving_url = app.state.config.database_url.clone();
    let mut wrong = Vec::new();

    for verb in ["UPDATE", "DELETE", "TRUNCATE"] {
        as_the_owner(&app, &format!("GRANT {verb} ON audit_events TO PUBLIC")).await;

        for line in not_refused(&app, &serving_url, &format!("holds {verb}")).await {
            wrong.push(format!("{verb} to PUBLIC: {line}"));
        }

        as_the_owner(&app, &format!("REVOKE {verb} ON audit_events FROM PUBLIC")).await;
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A grant through another role**: the serving role is a member of a role
/// that may change `audit_events`, and the narrowing, by name, leaves it.
#[tokio::test]
async fn both_deployments_refuse_a_verb_on_audit_events_held_through_a_membership() {
    let app = TestApp::spawn().await;
    let scratch = app.scratch_serving_role().await;
    let writer = format!("{}_writer", scratch.name);

    as_the_owner(&app, &format!("CREATE ROLE {writer} NOLOGIN")).await;
    as_the_owner(
        &app,
        &format!("GRANT UPDATE, DELETE ON audit_events TO {writer}"),
    )
    .await;
    as_the_owner(&app, &format!("GRANT {writer} TO {}", scratch.name)).await;

    let wrong = not_refused(&app, &scratch.url, "holds UPDATE, DELETE").await;

    scratch.drop_from(&app).await;
    as_the_owner(&app, &format!("DROP OWNED BY {writer}")).await;
    as_the_owner(&app, &format!("DROP ROLE {writer}")).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **`pg_write_all_data`** writes every table without a grant on any of them.
/// The privilege functions count it, so the posture check sees `UPDATE` and
/// `DELETE` on `audit_events`.
#[tokio::test]
async fn both_deployments_refuse_a_serving_role_that_writes_all_data() {
    let app = TestApp::spawn().await;
    let scratch = app.scratch_serving_role().await;

    as_the_owner(
        &app,
        &format!("GRANT pg_write_all_data TO {}", scratch.name),
    )
    .await;

    let wrong = not_refused(&app, &scratch.url, "UPDATE").await;
    scratch.drop_from(&app).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `CREATE` on schema `public` through `PUBLIC`, in **production** as well as
/// the `staging` `database_roles.rs` covers, and through `PUBLIC` rather than
/// to the role.
#[tokio::test]
async fn both_deployments_refuse_create_in_public_granted_to_public() {
    let app = TestApp::spawn().await;
    let serving_url = app.state.config.database_url.clone();

    as_the_owner(&app, "GRANT CREATE ON SCHEMA public TO PUBLIC").await;
    let wrong = not_refused(&app, &serving_url, "CREATE on schema public").await;
    as_the_owner(&app, "REVOKE CREATE ON SCHEMA public FROM PUBLIC").await;

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A column grant **to the serving role** is taken back by the narrowing: a
/// table-level `REVOKE` revokes the column privileges with it. After the
/// start, `UPDATE` of that column is refused on privilege.
#[tokio::test]
async fn a_column_grant_on_audit_events_to_the_serving_role_is_narrowed_away() {
    let app = TestApp::spawn().await;
    let role: String = sqlx::query_scalar("SELECT quote_ident(current_user)")
        .fetch_one(&app.serving_pool)
        .await
        .expect("the serving role names itself");

    as_the_owner(
        &app,
        &format!("GRANT UPDATE (reason) ON audit_events TO {role}"),
    )
    .await;

    for env in DEPLOYMENTS {
        let serving = db::prepare_serving_pool(&AppConfig {
            app_env: env,
            ..(*app.state.config).clone()
        })
        .await
        .unwrap_or_else(|error| panic!("{env:?} did not start: {error}"));

        let refusal = as_role(&serving, "UPDATE audit_events SET reason = 'x'").await;
        serving.close().await;

        assert!(
            matches!(&refusal, Some((code, _)) if code == INSUFFICIENT_PRIVILEGE),
            "{env:?}: the column grant survived the narrowing: {refusal:?}"
        );
    }
}

/// **DEFECT, red until fixed.** A column grant on `audit_events` **to
/// `PUBLIC`** survives the narrowing, which revokes from the serving role by
/// name, and the posture check reads `has_table_privilege`, which counts no
/// column grant. So both deployments start, and the serving role's
/// `UPDATE audit_events SET reason = …` passes privilege and is stopped only by
/// the trigger: the probe's first bullet is red in a deployment that started.
/// `has_any_column_privilege` counts it.
#[tokio::test]
async fn both_deployments_refuse_a_column_grant_on_audit_events_to_public() {
    let app = TestApp::spawn().await;
    let serving_url = app.state.config.database_url.clone();

    as_the_owner(&app, "GRANT UPDATE (reason) ON audit_events TO PUBLIC").await;

    let wrong = not_refused(&app, &serving_url, "UPDATE").await;
    assert!(
        wrong.is_empty(),
        "a column UPDATE on audit_events, granted to PUBLIC:\n{}",
        wrong.join("\n")
    );
}

/// **DEFECT, red until fixed.** The narrowing revokes `INSERT`, `UPDATE`,
/// `DELETE` and `TRUNCATE` on `_sqlx_migrations` from the serving role by
/// name; the posture check never reads `_sqlx_migrations`. Granted through
/// `PUBLIC`, or through a membership, the writes survive both, and the
/// deployment starts with a serving role that can mark a later migration
/// applied (so the owner skips it at the next start) or delete a row (so the
/// owner runs it again).
#[tokio::test]
async fn both_deployments_refuse_a_serving_role_that_may_write_the_migrations_table() {
    let app = TestApp::spawn().await;
    let serving_url = app.state.config.database_url.clone();

    as_the_owner(
        &app,
        "GRANT INSERT, UPDATE, DELETE ON _sqlx_migrations TO PUBLIC",
    )
    .await;

    let mut wrong = Vec::new();

    for env in DEPLOYMENTS {
        match start(&app, env, &serving_url).await {
            Some(refusal) if refusal.contains("_sqlx_migrations") => {}
            Some(refusal) => wrong.push(format!(
                "{env:?} refused, without naming _sqlx_migrations: {refusal}"
            )),
            None => {
                let landed = as_role(
                    &app.serving_pool,
                    "INSERT INTO _sqlx_migrations
                         (version, description, success, checksum, execution_time)
                     VALUES (99999, 'forged', true, '\\x00', 0)",
                )
                .await;
                wrong.push(format!(
                    "{env:?} started, and the serving role's forged migration row {}",
                    if landed.is_none() {
                        "landed"
                    } else {
                        "was refused"
                    }
                ));
            }
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **DEFECT, red until fixed.** The predefined roles that reach the server's
/// files and programs are not read by the posture check. A serving role in
/// `pg_execute_server_program` runs `COPY … TO PROGRAM` as the database
/// server's operating-system user, which reads the data directory and signs in
/// as anyone; the file roles read and write it. Each deployment starts with
/// any of them.
#[tokio::test]
async fn both_deployments_refuse_a_serving_role_that_reaches_the_server() {
    let app = TestApp::spawn().await;
    let mut wrong = Vec::new();

    for predefined in [
        "pg_execute_server_program",
        "pg_read_server_files",
        "pg_write_server_files",
    ] {
        let scratch = app.scratch_serving_role().await;
        as_the_owner(&app, &format!("GRANT {predefined} TO {}", scratch.name)).await;

        for line in not_refused(&app, &scratch.url, predefined).await {
            wrong.push(format!("{predefined}: {line}"));
        }
        scratch.drop_from(&app).await;
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **DEFECT, red until fixed.** ADR-0051's serving role *owns nothing*, and
/// the posture check reads ownership of `audit_events` alone. A serving role
/// that owns another table (one a migration was run as it by hand, say) may
/// disable that table's triggers, drop it, or grant it away, and each
/// deployment starts.
#[tokio::test]
async fn both_deployments_refuse_a_serving_role_that_owns_a_table() {
    let app = TestApp::spawn().await;
    let scratch = app.scratch_serving_role().await;

    as_the_owner(
        &app,
        "CREATE TABLE a_table_the_serving_role_owns (id integer)",
    )
    .await;
    as_the_owner(
        &app,
        &format!(
            "ALTER TABLE a_table_the_serving_role_owns OWNER TO {}",
            scratch.name
        ),
    )
    .await;

    let wrong = not_refused(&app, &scratch.url, "a_table_the_serving_role_owns").await;
    scratch.drop_from(&app).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// The roads out of the database
// ---------------------------------------------------------------------------

/// Everything a role that owns nothing must be refused **on privilege**: the
/// server's programs and files, large objects to and from them, another
/// role's identity, its own attributes, default privileges, and the objects on
/// `audit_events` that would hide or swallow a row without touching the
/// trigger (a rule, a policy, a trigger of its own, a lock that stalls every
/// write). Each runs in a rolled-back transaction.
#[tokio::test]
async fn the_serving_role_cannot_reach_the_server_another_role_or_the_audit_table() {
    let app = TestApp::spawn().await;
    let owner = the_owner(&app).await;
    let database = &app.database_name;

    let cases = [
        "COPY (SELECT 1) TO PROGRAM 'true'".to_owned(),
        "COPY audit_events FROM PROGRAM 'true'".to_owned(),
        "COPY (SELECT 1) TO '/tmp/kelir_serving_role_probe'".to_owned(),
        "COPY audit_events FROM '/etc/hostname'".to_owned(),
        "SELECT lo_import('/etc/hostname')".to_owned(),
        "SELECT lo_export(0, '/tmp/kelir_serving_role_probe')".to_owned(),
        "SELECT pg_read_file('/etc/hostname')".to_owned(),
        "SELECT pg_read_binary_file('/etc/hostname')".to_owned(),
        "SELECT pg_ls_dir('.')".to_owned(),
        "SELECT pg_stat_file('postgresql.conf')".to_owned(),
        "SELECT pg_reload_conf()".to_owned(),
        format!("SET ROLE {owner}"),
        format!("SET SESSION AUTHORIZATION {owner}"),
        format!("GRANT {owner} TO CURRENT_USER"),
        "GRANT pg_execute_server_program TO CURRENT_USER".to_owned(),
        "ALTER ROLE CURRENT_USER SUPERUSER".to_owned(),
        "ALTER ROLE CURRENT_USER CREATEROLE".to_owned(),
        "ALTER ROLE CURRENT_USER BYPASSRLS".to_owned(),
        "ALTER ROLE CURRENT_USER SET session_replication_role = replica".to_owned(),
        "SELECT set_config('session_replication_role', 'replica', false)".to_owned(),
        "CREATE ROLE kelir_serving_role_probe".to_owned(),
        "CREATE SCHEMA kelir_serving_role_probe".to_owned(),
        format!(r#"ALTER DATABASE "{database}" SET session_replication_role = replica"#),
        format!("ALTER DEFAULT PRIVILEGES FOR ROLE {owner} GRANT ALL ON TABLES TO CURRENT_USER"),
        format!("REASSIGN OWNED BY {owner} TO CURRENT_USER"),
        "CREATE RULE kelir_serving_role_probe AS ON INSERT TO audit_events DO INSTEAD NOTHING"
            .to_owned(),
        "CREATE TRIGGER kelir_serving_role_probe BEFORE INSERT ON audit_events
         FOR EACH STATEMENT EXECUTE FUNCTION audit_events_refuse_change()"
            .to_owned(),
        "ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY".to_owned(),
        "CREATE POLICY kelir_serving_role_probe ON audit_events USING (false)".to_owned(),
        "ALTER TABLE audit_events ALTER COLUMN reason SET DEFAULT 'x'".to_owned(),
        "LOCK TABLE audit_events IN ACCESS EXCLUSIVE MODE".to_owned(),
        "SELECT id FROM audit_events FOR UPDATE".to_owned(),
    ];

    let wrong = not_refused_on_privilege(&app.serving_pool, &cases).await;
    assert!(
        wrong.is_empty(),
        "the serving role is not refused:\n{}",
        wrong.join("\n")
    );
}

/// The `SECURITY DEFINER` functions the serving role may execute, outside
/// the system's own schemas: each runs as its owner, so each is a way to the
/// owner's privileges.
async fn definer_functions_the_serving_role_may_run(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT p.oid::regprocedure::text
         FROM pg_proc p
         JOIN pg_namespace n ON n.oid = p.pronamespace
         WHERE p.prosecdef
           AND n.nspname NOT IN ('pg_catalog', 'information_schema')
           AND has_function_privilege(p.oid, 'EXECUTE')
         ORDER BY 1",
    )
    .fetch_all(&app.serving_pool)
    .await
    .expect("the serving role reads the catalogue")
}

/// No function the serving role may call runs as somebody else. With its
/// control: a definer function the owner creates, as a later migration might,
/// is found, because `EXECUTE` goes to `PUBLIC` by default.
#[tokio::test]
async fn the_serving_role_may_run_no_security_definer_function() {
    let app = TestApp::spawn().await;

    assert_eq!(
        definer_functions_the_serving_role_may_run(&app).await,
        Vec::<String>::new()
    );

    as_the_owner(
        &app,
        "CREATE FUNCTION a_definer_function() RETURNS integer
         LANGUAGE sql SECURITY DEFINER AS 'SELECT 1'",
    )
    .await;

    assert_eq!(
        definer_functions_the_serving_role_may_run(&app).await,
        vec!["a_definer_function()".to_owned()],
        "the control: a definer function is not found"
    );
}

// ---------------------------------------------------------------------------
// A later migration (ADR-0051 §2, *future tables are granted by default*)
// ---------------------------------------------------------------------------

/// What a later release's migration might do: a table with an identity, a
/// sequence, and `audit_events` rebuilt under its own name, which the default
/// privileges grant in full.
const A_LATER_MIGRATION: &str = "
CREATE TABLE a_later_table (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    note text
);
CREATE SEQUENCE a_later_sequence;
ALTER TABLE audit_events RENAME TO audit_events_before_rebuild;
CREATE TABLE audit_events (LIKE audit_events_before_rebuild INCLUDING ALL);
";

/// One effective privilege, as the serving role holds it.
async fn holds(pool: &PgPool, check: &str) -> bool {
    sqlx::query_scalar(&format!("SELECT {check}"))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("`{check}`: {error}"))
}

/// **A migration run by sqlx as the owner**, as a release runs it, and then
/// the startup as `staging` runs it. The serving role reads and writes the new
/// table and draws from the new sequence without anybody naming it; it may
/// not set the sequence; and the rebuilt `audit_events`, granted in full by
/// the default privileges, is narrowed again before the server would bind.
#[tokio::test]
async fn a_later_migration_is_granted_by_default_and_narrowed_at_the_next_start() {
    let app = TestApp::spawn().await;

    let mut later = sqlx::migrate!("./migrations");
    let mut migrations = later.migrations.to_vec();
    migrations.push(Migration::new(
        9_001,
        "a later migration".into(),
        MigrationType::Simple,
        A_LATER_MIGRATION.into(),
        false,
    ));
    later.migrations = migrations.into();
    later
        .run(&app.pool)
        .await
        .expect("the owner runs the later migration");

    assert!(
        holds(
            &app.serving_pool,
            "has_table_privilege('audit_events', 'UPDATE')"
        )
        .await,
        "the rebuilt table is not granted by default, so this proves nothing"
    );

    let serving = db::prepare_serving_pool(&AppConfig {
        app_env: AppEnv::Staging,
        ..(*app.state.config).clone()
    })
    .await
    .expect("staging starts after the later migration");

    let mut wrong = Vec::new();

    for (check, expected) in [
        ("has_table_privilege('a_later_table', 'SELECT')", true),
        ("has_table_privilege('a_later_table', 'INSERT')", true),
        ("has_table_privilege('a_later_table', 'UPDATE')", true),
        ("has_table_privilege('a_later_table', 'DELETE')", true),
        ("has_table_privilege('a_later_table', 'TRUNCATE')", false),
        ("has_sequence_privilege('a_later_sequence', 'USAGE')", true),
        (
            "has_sequence_privilege('a_later_sequence', 'UPDATE')",
            false,
        ),
        ("has_table_privilege('audit_events', 'INSERT')", true),
        ("has_table_privilege('audit_events', 'UPDATE')", false),
        ("has_table_privilege('audit_events', 'DELETE')", false),
        ("has_table_privilege('audit_events', 'TRUNCATE')", false),
        ("has_table_privilege('_sqlx_migrations', 'DELETE')", false),
    ] {
        if holds(&serving, check).await != expected {
            wrong.push(format!("{check} is not {expected}"));
        }
    }

    for sql in [
        "INSERT INTO a_later_table (note) VALUES ('x')",
        "SELECT nextval('a_later_sequence')",
    ] {
        if let Some((code, message)) = as_role(&serving, sql).await {
            wrong.push(format!("refused ({code} {message}): {sql}"));
        }
    }

    wrong.extend(
        not_refused_on_privilege(
            &serving,
            &[
                "SELECT setval('a_later_sequence', 1000)".to_owned(),
                "ALTER SEQUENCE a_later_sequence RESTART".to_owned(),
                "UPDATE audit_events SET reason = 'x'".to_owned(),
                "DELETE FROM _sqlx_migrations WHERE version = 9001".to_owned(),
            ],
        )
        .await,
    );

    serving.close().await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// The N−1 question, at the migrator (ADR-0051 §4)
// ---------------------------------------------------------------------------

/// `v0.9.0`'s last migration, `0050_integration_log_read_permission.sql`.
/// `0001` to `0050` are byte-for-byte as `v0.9.0` shipped them, so their
/// checksums are its checksums.
const THE_PREVIOUS_RELEASES_LAST_MIGRATION: i64 = 50;

/// `v0.9.0`'s migrator, as its `db::run_migrations` builds it: its migration
/// set, `ignore_missing`, and the same sqlx, 0.8.6, in both lock files.
fn the_previous_releases_migrator() -> Migrator {
    let mut previous = sqlx::migrate!("./migrations");
    previous.migrations = previous
        .migrations
        .iter()
        .filter(|migration| migration.version <= THE_PREVIOUS_RELEASES_LAST_MIGRATION)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    previous.set_ignore_missing(true);
    previous
}

/// Runs the previous release's migrator on a connection of its own, which is
/// closed after, so the advisory lock it takes goes with it.
async fn the_previous_releases_start(url: &str) -> Result<(), MigrateError> {
    let mut connection = PgConnection::connect(url)
        .await
        .expect("a connection for the previous release's migrator");

    let outcome = the_previous_releases_migrator().run(&mut connection).await;
    let _ = connection.close().await;
    outcome
}

/// **The rollback's fallback, at the migrator**: started as the owner, as
/// ADR-0051 §4 has the rollback run if the serving role cannot, `v0.9.0`'s
/// migrator passes against this release's schema. Its fifty migrations are
/// applied with their checksums, and the two it does not know are ignored.
#[tokio::test]
async fn the_previous_releases_migrator_starts_as_the_owner() {
    let app = TestApp::spawn().await;

    the_previous_releases_start(&owner_url(&app))
        .await
        .expect("v0.9.0's migrator, as the owner, against this release's schema");
}

/// **The N−1 rehearsal's third step, at the migrator, recorded**
/// (ADR-0051 §4; plan 22 row 2). `v0.9.0`'s migrator, started as the serving
/// role against this release's schema, is refused on privilege at sqlx's
/// `CREATE TABLE IF NOT EXISTS _sqlx_migrations`: PostgreSQL checks `CREATE`
/// on the schema before it looks for the table. So **the rollback to `v0.9.0`
/// cannot start as `kelir_app`**, and runs in `v0.9.0`'s posture, as the
/// upgrade note's fallback says.
///
/// **If this goes red because the migrator started**, the rollback needs no
/// step: ADR-0051 §4's other branch applies, and the upgrade note changes.
/// What it does not cover is the rest of `v0.9.0`'s binary, which the
/// container rehearsal still runs.
#[tokio::test]
async fn the_previous_releases_migrator_is_refused_as_the_serving_role() {
    let app = TestApp::spawn().await;

    let outcome = the_previous_releases_start(&app.state.config.database_url).await;

    let error = match outcome {
        Ok(()) => panic!(
            "v0.9.0's migrator started as the serving role: the rollback needs no step, \
             and ADR-0051 §4 and the upgrade note must say so"
        ),
        Err(MigrateError::Execute(error)) => error,
        Err(other) => panic!("refused, but not by the database: {other}"),
    };

    let code = error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_default();
    assert_eq!(
        code, INSUFFICIENT_PRIVILEGE,
        "refused, but not on privilege: {error}"
    );
    assert!(
        error.to_string().contains("schema public"),
        "refused on privilege, but not for CREATE on the schema: {error}"
    );
}
