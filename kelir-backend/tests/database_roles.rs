//! The backend migrates as the schema's owner and serves as a role that owns
//! nothing (ADR-0051, [#842]).
//!
//! `audit_append_only.rs` holds the serving role against `audit_events`. This
//! file holds the startup that puts it there: one connection as the owner
//! migrates and narrows, and `staging` and `production` check the serving role's posture
//! before the server binds, refusing a URL that names the owner, a grant the
//! narrowing does not take back, and a superuser. The refusal of one URL in
//! `staging` and `production` is `config.rs`'s, and is tested there.
//!
//! Each test drives `db::prepare_serving_pool`, the sequence `main.rs` runs,
//! or its halves, `db::StartupPools::open` and `prepare`, as `main.rs` runs
//! them, against a database the harness provisioned as a host is provisioned.
//!
//! **The owner holds no connection once startup is prepared** ([#991]): the
//! owner's connection is named `db::MIGRATION_APPLICATION_NAME`, and none may
//! be left in `pg_stat_activity`.
//!
//! [#842]: https://github.com/sujanto-gaws/kelir/issues/842
//! [#991]: https://github.com/sujanto-gaws/kelir/issues/991

mod common;

use std::time::Duration;

use common::{TestApp, SERVING_ROLE};
use kelir_backend::config::{AppConfig, AppEnv};
use kelir_backend::db::{self, DatabaseStartupError, ServingRoleError, StartupPools};
use sqlx::PgPool;
use tokio::time::Instant;

/// The owner's URL the harness gave the application.
fn owner_url(app: &TestApp) -> String {
    app.state
        .config
        .database_migration_url
        .clone()
        .expect("the harness configures the owner's URL")
}

/// The application's configuration, as `staging` would load it.
fn staging(app: &TestApp) -> AppConfig {
    AppConfig {
        app_env: AppEnv::Staging,
        ..(*app.state.config).clone()
    }
}

/// The role a pool connects as.
async fn role_of(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(pool)
        .await
        .expect("the pool names its role")
}

/// The two narrowed tables' access lists, as the catalogue holds them.
async fn the_narrowed_grants(app: &TestApp) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT relname::text, coalesce(relacl::text, '')
         FROM pg_class
         WHERE oid IN ('audit_events'::regclass, '_sqlx_migrations'::regclass)
         ORDER BY 1",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the catalogue reads back")
}

/// Grants the serving role something, through the owner's pool.
async fn grant(app: &TestApp, grant: &str) {
    sqlx::query(&format!("GRANT {grant} TO {SERVING_ROLE}"))
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("the owner's `GRANT {grant}`: {error}"));
}

/// The deployed shape: both URLs, in `staging`. The backend migrates as the
/// owner and serves as the serving role.
#[tokio::test]
async fn staging_serves_as_the_serving_role_with_both_urls() {
    let app = TestApp::spawn().await;

    let serving = db::prepare_serving_pool(&staging(&app))
        .await
        .expect("both URLs, the serving role as provisioned, starts");

    assert_eq!(role_of(&serving).await, SERVING_ROLE);
    serving.close().await;
}

/// ADR-0051 §5, the fourth bullet: **`KELIR_DATABASE_URL` naming the owner** is
/// refused, naming what was found. One URL naming the owner twice is the same
/// shape, and is refused the same way.
#[tokio::test]
async fn staging_refuses_a_serving_url_that_names_the_owner() {
    let app = TestApp::spawn().await;

    for migration_url in [Some(owner_url(&app)), None] {
        let config = AppConfig {
            database_url: owner_url(&app),
            database_migration_url: migration_url,
            ..staging(&app)
        };

        let error = db::prepare_serving_pool(&config)
            .await
            .expect_err("the owner may not serve");

        let (role, found) = match &error {
            DatabaseStartupError::ServingRole(ServingRoleError::Overprivileged { role, found }) => {
                (role, found)
            }
            other => panic!("refused, but not for the role's posture: {other}"),
        };
        let owner = role_of(&app.pool).await;

        assert_eq!(role, &owner);
        assert!(
            found
                .iter()
                .any(|finding| finding.contains("the owner of audit_events")),
            "the refusal does not name the owner: {error}"
        );
        assert!(
            error.to_string().contains("KELIR_DATABASE_URL"),
            "the refusal does not name the setting: {error}"
        );
    }
}

/// ADR-0051 §2: **the narrowing runs on every start**, so a grant widened since
/// the last one is taken back before the posture is read, and the backend
/// starts with the serving role as it should be.
#[tokio::test]
async fn a_widened_grant_on_the_two_tables_is_narrowed_again_at_startup() {
    let app = TestApp::spawn().await;
    let narrowed = the_narrowed_grants(&app).await;

    grant(&app, "UPDATE, DELETE, TRUNCATE ON audit_events").await;
    grant(&app, "INSERT, UPDATE, DELETE, TRUNCATE ON _sqlx_migrations").await;
    assert_ne!(the_narrowed_grants(&app).await, narrowed, "nothing widened");

    let serving = db::prepare_serving_pool(&staging(&app))
        .await
        .expect("the narrowing takes the grant back, and the backend starts");
    serving.close().await;

    assert_eq!(the_narrowed_grants(&app).await, narrowed);
}

/// ADR-0051 §5, the fourth bullet: **with the narrowing mutated out**, a
/// widened grant reaches the posture check, which refuses it, naming each
/// privilege. No setting turns the narrowing off, so the check is driven
/// directly here.
#[tokio::test]
async fn the_posture_check_refuses_a_serving_role_that_may_change_audit_events() {
    let app = TestApp::spawn().await;

    db::check_serving_posture(&app.serving_pool)
        .await
        .expect("the harness's serving role passes as provisioned");

    grant(&app, "UPDATE, DELETE, TRUNCATE ON audit_events").await;

    let error = db::check_serving_posture(&app.serving_pool)
        .await
        .expect_err("a serving role that may change audit_events is refused");

    let ServingRoleError::Overprivileged { role, found } = &error else {
        panic!("refused, but not for the role's posture: {error}");
    };
    assert_eq!(role, SERVING_ROLE);
    assert_eq!(
        found,
        &["it holds UPDATE, DELETE, TRUNCATE on audit_events".to_owned()]
    );
}

/// ADR-0051 §2: `CREATE` on schema `public` is not the narrowing's to take
/// back, because a deployment that granted it decided something. The posture
/// check refuses it at startup instead.
#[tokio::test]
async fn staging_refuses_a_serving_role_that_may_create_in_public() {
    let app = TestApp::spawn().await;

    grant(&app, "CREATE ON SCHEMA public").await;

    let error = db::prepare_serving_pool(&staging(&app))
        .await
        .expect_err("a serving role that may create objects is refused");

    assert!(
        error.to_string().contains("CREATE on schema public"),
        "the refusal does not name the privilege: {error}"
    );
}

/// ADR-0051 §5, the fifth bullet: **two starts leave the same grants**.
#[tokio::test]
async fn the_narrowing_is_idempotent() {
    let app = TestApp::spawn().await;
    let after_one = the_narrowed_grants(&app).await;

    let mut owner = app.pool.acquire().await.expect("an owner's connection");
    for _ in 0..2 {
        let narrowed = db::narrow_serving_role(&mut owner, &app.serving_pool)
            .await
            .expect("the narrowing runs again");
        assert!(narrowed, "two roles, so the narrowing runs");
    }

    assert_eq!(the_narrowed_grants(&app).await, after_one);
}

/// ADR-0051 §2, the development shape: **one URL migrates and serves**, the
/// narrowing is skipped because the owner would revoke from itself, and no
/// posture is checked outside `staging` and `production`.
#[tokio::test]
async fn one_url_migrates_and_serves_outside_a_deployment() {
    let app = TestApp::spawn().await;

    let mut owner = app.pool.acquire().await.expect("an owner's connection");
    let skipped = !db::narrow_serving_role(&mut owner, &app.pool)
        .await
        .expect("the narrowing reads both roles");
    assert!(skipped, "one role, so the narrowing is skipped");
    drop(owner);

    let config = AppConfig {
        app_env: AppEnv::Test,
        database_url: owner_url(&app),
        database_migration_url: None,
        ..(*app.state.config).clone()
    };

    let serving = db::prepare_serving_pool(&config)
        .await
        .expect("one URL starts outside a deployment");

    assert_eq!(role_of(&serving).await, role_of(&app.pool).await);
    serving.close().await;
}

/// One of the owner's connections to the test's database, as
/// `pg_stat_activity` shows it.
#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code, reason = "read through Debug, in a failure's message")]
struct OwnerConnection {
    pid: i32,
    usename: String,
    client_addr: String,
    backend_start: String,
    state: String,
    wait_event_type: String,
    state_change: String,
    query: String,
}

/// The owner's connections to the test's database: those named
/// [`db::MIGRATION_APPLICATION_NAME`]. The harness's own pool is named
/// otherwise, so it is not among them.
async fn owner_connections(app: &TestApp) -> Vec<OwnerConnection> {
    sqlx::query_as(
        "SELECT pid,
                coalesce(usename::text, '') AS usename,
                coalesce(host(client_addr), 'local') AS client_addr,
                backend_start::text AS backend_start,
                coalesce(state, '') AS state,
                coalesce(wait_event_type, '') AS wait_event_type,
                coalesce(state_change::text, '') AS state_change,
                left(coalesce(query, ''), 200) AS query
         FROM pg_stat_activity
         WHERE datname = $1 AND application_name = $2 AND backend_type = 'client backend'
         ORDER BY backend_start",
    )
    .bind(&app.database_name)
    .bind(db::MIGRATION_APPLICATION_NAME)
    .fetch_all(&app.pool)
    .await
    .expect("pg_stat_activity reads back")
}

/// How long a backend whose connection was closed may take to leave
/// `pg_stat_activity`. Only a connection left open reaches it.
const BACKEND_EXIT: Duration = Duration::from_secs(10);

/// No connection of the owner's is left in `pg_stat_activity`.
///
/// **This waits, and says so**: a closed connection's backend exits a moment
/// after `close()` returns, so its row is waited out, up to [`BACKEND_EXIT`].
/// A connection left open stays idle, and only that reaches the deadline.
async fn the_owner_holds_no_connection(app: &TestApp, when: &str) {
    let deadline = Instant::now() + BACKEND_EXIT;
    loop {
        let left = owner_connections(app).await;
        if left.is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{when}: the owner still holds connections {BACKEND_EXIT:?} on: {left:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// **The owner opens one connection, and leaves none** (#991, ADR-0051 §2).
///
/// `prepare` is held part way: the test locks `_sqlx_migrations`, so the
/// migrator, which takes its advisory lock and then reads the table, waits on
/// the lock. While it waits, the owner's connections are counted: there is
/// exactly one, and it is the one waiting. Then the lock is released, and
/// once `prepare` returns that connection is gone too.
///
/// Before #991 the owner had a pool, whose `close()` could leave a connection
/// it was still returning (launchbadge/sqlx#3217, `tests/pool_close.rs`).
/// Now it has one connection, which `prepare` closes itself, so there is no
/// return in the background to race. The stall and the count are
/// deterministic; the waits are bounded and only a failure reaches them.
#[tokio::test]
async fn the_owner_opens_one_connection_and_leaves_none() {
    let app = TestApp::spawn().await;
    let config = staging(&app);
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");

    let mut lock = app.pool.begin().await.expect("a transaction to lock with");
    sqlx::query("LOCK TABLE _sqlx_migrations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .expect("_sqlx_migrations locks");

    let held_part_way = async {
        let deadline = Instant::now() + BACKEND_EXIT;
        let waiting = loop {
            let owner = owner_connections(&app).await;
            if owner
                .iter()
                .any(|connection| connection.wait_event_type == "Lock")
            {
                break owner;
            }
            assert!(
                Instant::now() < deadline,
                "prepare never waited on the lock as the owner: {owner:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        assert_eq!(
            waiting.len(),
            1,
            "the owner holds more than one connection while it migrates: {waiting:#?}"
        );

        lock.rollback().await.expect("the lock is released");
    };

    let (prepared, ()) = tokio::join!(pools.prepare(&config), held_part_way);
    prepared.expect("both URLs, the serving role as provisioned, starts");

    the_owner_holds_no_connection(&app, "once prepared").await;
    pools.serving.close().await;
}

/// **Once startup is prepared, the owner holds no connection** (#991,
/// ADR-0051 §2): `StartupPools::open`, then `prepare` with both URLs, as
/// `main.rs` runs them, and on a multi-threaded runtime, as `main.rs` runs:
/// the shape in which the owner's pool could leave a connection behind, while
/// it had one. Repeated, because that was a race.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owner_holds_no_connection_once_startup_is_prepared() {
    let app = TestApp::spawn().await;
    let config = staging(&app);

    for round in 1..=30 {
        let pools = StartupPools::open(&config).expect("the pool and the owner's options build");
        pools
            .prepare(&config)
            .await
            .expect("both URLs, the serving role as provisioned, starts");

        the_owner_holds_no_connection(&app, &format!("round {round}, once prepared")).await;

        pools.serving.close().await;
    }
}
