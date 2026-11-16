//! `test-engineer`'s campaign against the owner's one connection at startup
//! ([#991], ADR-0051 §2).
//!
//! `database_roles.rs` is the builder's: `StartupPools::prepare` succeeding,
//! once held part way and thirty times over. **This file is every other way
//! out of it**, and each test names what it pins:
//!
//! | Path out of `prepare` | Test |
//! |---|---|
//! | A migration error | *a migration error closes the owner's connection* |
//! | A narrowing error | *a narrowing error closes the owner's connection* |
//! | A stop while the owner is idle | *a stop while the owner is idle hangs it up* |
//! | A stop mid-migration | *a stop mid migration leaves no owner connection once its statement ends* |
//! | A stop mid-narrowing | *a stop mid narrowing leaves no owner connection once its statement ends* |
//! | SIGTERM mid-migration, to the binary | *the binary stopped mid migration exits zero and leaves no owner connection* |
//! | Refused, then listening, inside the window | *a refused owner is retried until the server listens* |
//! | Refused past the window | *a refused owner is given up on after the acquire timeout* |
//! | Wrong credentials, and the exit they give | *the binary refused as the owner exits one at once* |
//! | The TLS rule | *the owner's connection keeps the tls rule* |
//!
//! **A stop is a dropped future**, as `main.rs`'s `select!` drops `start`:
//! each in-process stop test drops `prepare` at a point staged by a lock, not
//! by a sleep.
//!
//! **A statement in flight outlives a hang-up.** PostgreSQL notices a client
//! that has gone when it next reads from it, so a backend waiting on a lock
//! when its client hangs up waits on, holding the migrator's advisory lock,
//! until the statement ends. The stop tests release their lock and then
//! require the owner's connection to be gone; they print what was left
//! before the release, and do not assert it.
//!
//! [#991]: https://github.com/sujanto-gaws/kelir/issues/991

mod common;

use std::time::Duration;

use common::TestApp;
use kelir_backend::config::AppConfig;
use kelir_backend::db::{self, DatabaseStartupError, DatabaseTlsSettings, StartupPools};
use sqlx::{Connection, Postgres, Transaction};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

/// How long a staged step may take before the test says which one did not
/// happen.
const PATIENCE: Duration = Duration::from_secs(10);

/// How long a backend whose client hung up may take to leave
/// `pg_stat_activity`. Only a connection left open reaches it.
const BACKEND_EXIT: Duration = Duration::from_secs(10);

/// A password that must never reach a log.
const WRONG_PASSWORD: &str = "not-the-owners-pw";

/// One of the owner's connections, as `pg_stat_activity` shows it.
#[derive(Debug, sqlx::FromRow)]
struct OwnerConnection {
    state: String,
    wait_event_type: String,
    query: String,
}

/// The owner's connections to the test's database: those named
/// [`db::MIGRATION_APPLICATION_NAME`]. The harness's pools are named
/// otherwise.
async fn owner_connections(app: &TestApp) -> Vec<OwnerConnection> {
    sqlx::query_as(
        "SELECT coalesce(state, '') AS state,
                coalesce(wait_event_type, '') AS wait_event_type,
                coalesce(query, '') AS query
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

/// Waits until one of the owner's connections is as `seen` says.
async fn owner_seen(app: &TestApp, what: &str, seen: impl Fn(&OwnerConnection) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let owner = owner_connections(app).await;
        if owner.iter().any(&seen) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the owner was never seen {what}: {owner:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Waits out a closed connection's backend, up to [`BACKEND_EXIT`]: no
/// connection of the owner's may be left.
async fn no_owner_connection(app: &TestApp, when: &str) {
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

/// The owner's URL the harness gave the application.
fn owner_url(app: &TestApp) -> String {
    app.state
        .config
        .database_migration_url
        .clone()
        .expect("the harness configures the owner's URL")
}

/// The application's configuration, with `migration_url` as the owner's.
fn with_owner(app: &TestApp, migration_url: Option<String>) -> AppConfig {
    AppConfig {
        database_migration_url: migration_url,
        ..(*app.state.config).clone()
    }
}

/// `url`, signed in as `user` with `password`.
fn signed_in_as(url: &str, user: &str, password: &str) -> String {
    let mut url = url::Url::parse(url).expect("the owner's URL parses");
    url.set_username(user).expect("the URL takes a user");
    url.set_password(Some(password))
        .expect("the URL takes a password");
    url.into()
}

/// A loopback port nothing listens on.
fn closed_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("the bound address")
        .port()
}

/// **The migration gate**: `_sqlx_migrations` locked, so the migrator, which
/// takes its advisory lock and then reads the table, waits on it.
async fn lock_the_migrations_table(app: &TestApp) -> Transaction<'static, Postgres> {
    let mut lock = app.pool.begin().await.expect("a transaction to lock with");
    sqlx::query("LOCK TABLE _sqlx_migrations IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .expect("_sqlx_migrations locks");
    lock
}

/// **The narrowing gate**: the first narrowed table's catalogue row updated
/// and not committed, so the narrowing's `REVOKE` on it waits for this
/// transaction.
async fn hold_the_first_narrowed_grant(app: &TestApp) -> Transaction<'static, Postgres> {
    let (table, _) = db::SERVING_ROLE_NARROWING[0];
    let mut gate = app.pool.begin().await.expect("a transaction to gate with");
    sqlx::query(&format!("GRANT TRIGGER ON {table} TO PUBLIC"))
        .execute(&mut *gate)
        .await
        .unwrap_or_else(|error| panic!("the gate's GRANT on {table}: {error}"));
    gate
}

/// Whether the owner's connection is the narrowing's `REVOKE`, waiting.
fn revoking_on_a_lock(connection: &OwnerConnection) -> bool {
    connection.wait_event_type == "Lock" && connection.query.trim_start().starts_with("REVOKE")
}

/// **A migration error closes the owner's connection** (#991): an applied
/// migration's checksum is changed, so the migrator, holding its advisory
/// lock, refuses with `VersionMismatch`. `prepare` returns that error, and the
/// connection that held the lock is gone.
#[tokio::test]
async fn a_migration_error_closes_the_owners_connection() {
    let app = TestApp::spawn().await;
    sqlx::query(
        "UPDATE _sqlx_migrations SET checksum = decode('00', 'hex')
         WHERE version = (SELECT max(version) FROM _sqlx_migrations)",
    )
    .execute(&app.pool)
    .await
    .expect("the newest migration's checksum changes");

    let config = with_owner(&app, Some(owner_url(&app)));
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");
    let refused = pools
        .prepare(&config)
        .await
        .expect_err("a changed migration is refused");

    assert!(
        matches!(
            refused,
            DatabaseStartupError::Migration(sqlx::migrate::MigrateError::VersionMismatch(_))
        ),
        "refused, but not by the migrator's checksum: {refused:?}"
    );
    no_owner_connection(&app, "after a migration error").await;
}

/// **A narrowing error closes the owner's connection** (#991): the test's
/// database is given a `lock_timeout`, and the narrowing's `REVOKE` is held
/// by the gate until it times out. `prepare` returns `Narrowing`, and the
/// connection, alive when the error came, is gone.
#[tokio::test]
async fn a_narrowing_error_closes_the_owners_connection() {
    let app = TestApp::spawn().await;
    let database: String = sqlx::query_scalar("SELECT quote_ident(current_database())")
        .fetch_one(&app.pool)
        .await
        .expect("the database names itself");
    sqlx::query(&format!(
        "ALTER DATABASE {database} SET lock_timeout = '200ms'"
    ))
    .execute(&app.pool)
    .await
    .expect("the test's database takes a lock_timeout");

    let gate = hold_the_first_narrowed_grant(&app).await;
    let config = with_owner(&app, Some(owner_url(&app)));
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");
    let refused = tokio::time::timeout(PATIENCE, pools.prepare(&config))
        .await
        .expect("the REVOKE times out on its lock")
        .expect_err("a REVOKE that cannot lock is refused");

    let DatabaseStartupError::Narrowing(error) = &refused else {
        panic!("refused, but not by the narrowing: {refused:?}");
    };
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03"),
        "the narrowing failed, but not on its lock_timeout: {error:?}"
    );
    no_owner_connection(&app, "after a narrowing error").await;

    gate.rollback().await.expect("the gate rolls back");
    sqlx::query(&format!("ALTER DATABASE {database} RESET lock_timeout"))
        .execute(&app.pool)
        .await
        .expect("the lock_timeout resets");
}

/// **A stop while the owner is idle hangs it up** (#991, #856): the serving
/// URL is a server that accepts and never answers, so `prepare`, migrated as
/// the owner, waits on the serving pool to learn the serving role. The owner's
/// connection is idle then. `prepare` is dropped there, as a stop drops it,
/// and nothing of `prepare` runs after: only the drop can end the connection,
/// and it must.
#[tokio::test]
async fn a_stop_while_the_owner_is_idle_hangs_it_up() {
    let app = TestApp::spawn().await;

    let silent = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let port = silent.local_addr().expect("the bound address").port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });

    let config = AppConfig {
        database_url: format!("postgres://kelir_app:kelir@127.0.0.1:{port}/kelir"),
        ..with_owner(&app, Some(owner_url(&app)))
    };
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");

    let mut preparing = Box::pin(pools.prepare(&config));
    tokio::select! {
        prepared = &mut preparing => {
            panic!("prepare returned while the serving pool could not connect: {prepared:?}")
        }
        () = owner_seen(&app, "idle once migrated", |connection| {
            connection.state == "idle" && connection.query.contains("pg_advisory_unlock")
        }) => {}
    }
    drop(preparing);

    no_owner_connection(&app, "after a stop while the owner was idle").await;
}

/// **A stop mid-migration leaves no owner connection once its statement
/// ends** (#991, #856): `prepare` is dropped while the migrator waits on the
/// migration gate. Once the gate is released the connection is gone, and its
/// advisory lock with it: the next start prepares.
#[tokio::test]
async fn a_stop_mid_migration_leaves_no_owner_connection_once_its_statement_ends() {
    let app = TestApp::spawn().await;
    let config = with_owner(&app, Some(owner_url(&app)));
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");
    let lock = lock_the_migrations_table(&app).await;

    let mut preparing = Box::pin(pools.prepare(&config));
    tokio::select! {
        prepared = &mut preparing => {
            panic!("prepare returned past a locked _sqlx_migrations: {prepared:?}")
        }
        () = owner_seen(&app, "migrating, waiting on the lock", |connection| {
            connection.wait_event_type == "Lock"
        }) => {}
    }
    drop(preparing);
    eprintln!(
        "stopped mid-migration; before the lock is released the owner holds: {:#?}",
        owner_connections(&app).await
    );

    lock.rollback().await.expect("the lock is released");
    no_owner_connection(&app, "once the stopped migration's statement ended").await;

    let next = StartupPools::open(&config).expect("the pool and the owner's options build");
    tokio::time::timeout(PATIENCE, next.prepare(&config))
        .await
        .expect("the next start is held, by the stopped one's advisory lock")
        .expect("the next start prepares");
    no_owner_connection(&app, "after the next start").await;
    next.serving.close().await;
}

/// **A stop mid-narrowing leaves no owner connection once its statement
/// ends** (#991, #856): `prepare` is dropped while the narrowing's `REVOKE`
/// waits on the narrowing gate. Once the gate rolls back, the connection is
/// gone.
#[tokio::test]
async fn a_stop_mid_narrowing_leaves_no_owner_connection_once_its_statement_ends() {
    let app = TestApp::spawn().await;
    let config = with_owner(&app, Some(owner_url(&app)));
    let pools = StartupPools::open(&config).expect("the pool and the owner's options build");
    let gate = hold_the_first_narrowed_grant(&app).await;

    let mut preparing = Box::pin(pools.prepare(&config));
    tokio::select! {
        prepared = &mut preparing => {
            panic!("prepare returned past a held REVOKE: {prepared:?}")
        }
        () = owner_seen(&app, "narrowing, waiting on the gate", revoking_on_a_lock) => {}
    }
    drop(preparing);
    eprintln!(
        "stopped mid-narrowing; before the gate rolls back the owner holds: {:#?}",
        owner_connections(&app).await
    );

    gate.rollback().await.expect("the gate rolls back");
    no_owner_connection(&app, "once the stopped narrowing's statement ended").await;
    pools.serving.close().await;
}

/// **A refused owner is retried until the server listens** (#991): the
/// owner's connection is pointed at a port nothing listens on, and once it
/// has been refused, a forwarder to the real server starts listening there,
/// inside [`db::ACQUIRE_TIMEOUT`]. The connection is made, through it, and is
/// named [`db::MIGRATION_APPLICATION_NAME`].
#[tokio::test]
async fn a_refused_owner_is_retried_until_the_server_listens() {
    let app = TestApp::spawn().await;
    let options = db::migration_connect_options(&owner_url(&app), DatabaseTlsSettings::default())
        .expect("the owner's options build");
    let server = format!("{}:{}", options.get_host(), options.get_port());
    let port = closed_port();
    let options = options.host("127.0.0.1").port(port);

    let started = Instant::now();
    let connecting = tokio::spawn(async move { db::connect_owner(&options).await });

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !connecting.is_finished(),
        "control: the owner's connection ended before anything listened, so it was not retried"
    );

    let forwarder = TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("the refused port is still free");
    tokio::spawn(async move {
        while let Ok((mut inbound, _)) = forwarder.accept().await {
            let server = server.clone();
            tokio::spawn(async move {
                if let Ok(mut outbound) = TcpStream::connect(server.as_str()).await {
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                }
            });
        }
    });

    let mut connection = connecting
        .await
        .expect("the connecting task joins")
        .expect("the owner connects once the port listens");
    let took = started.elapsed();
    assert!(
        took < db::ACQUIRE_TIMEOUT,
        "connected after {took:?}, past the {:?} window",
        db::ACQUIRE_TIMEOUT
    );

    let name: String = sqlx::query_scalar("SELECT current_setting('application_name')")
        .fetch_one(&mut connection)
        .await
        .expect("the connection names itself");
    assert_eq!(name, db::MIGRATION_APPLICATION_NAME);
    connection.close().await.expect("the connection closes");
}

/// **A refused owner is given up on after the acquire timeout** (#991): with
/// nothing ever listening, the retry ends near [`db::ACQUIRE_TIMEOUT`], after
/// its last backoff, not at once and not later, with the refusal.
#[tokio::test]
async fn a_refused_owner_is_given_up_on_after_the_acquire_timeout() {
    let options = db::migration_connect_options(
        &format!("postgres://kelir:kelir@127.0.0.1:{}/kelir", closed_port()),
        DatabaseTlsSettings::default(),
    )
    .expect("the owner's options build");

    let started = Instant::now();
    let error = db::connect_owner(&options)
        .await
        .expect_err("nothing listens");
    let took = started.elapsed();

    assert!(
        (Duration::from_secs(3)..=db::ACQUIRE_TIMEOUT + Duration::from_secs(1)).contains(&took),
        "gave up after {took:?}, not near the {:?} window",
        db::ACQUIRE_TIMEOUT
    );
    assert!(
        matches!(
            &error,
            sqlx::Error::Io(error) if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
            )
        ),
        "gave up, but not on the refusal: {error:?}"
    );
}

/// A server that answers PostgreSQL's TLS request with *no*, on `host`.
async fn server_refusing_tls(host: &str) -> u16 {
    let listener = TcpListener::bind((host, 0))
        .await
        .expect("a loopback address");
    let port = listener.local_addr().expect("the bound address").port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            // SSLRequest is eight bytes: a length and the request code.
            let mut request = [0u8; 8];
            if socket.read_exact(&mut request).await.is_ok() {
                let _ = socket.write_all(b"N").await;
            }
        }
    });

    port
}

/// **The owner's connection keeps the TLS rule** (#803, #991): the two
/// settings apply to `KELIR_DATABASE_MIGRATION_URL` as to the serving URL.
///
/// - A host that is not local gets `require`: a server without TLS is
///   refused, at once rather than retried, naming the setting.
///   `127.0.0.2` is loopback on Linux and not a local host by the rule.
/// - On a local host, `KELIR_DATABASE_SSL_MODE=require` reaches the owner's
///   connection and refuses the same server.
/// - A non-local URL asking for less than `require` is refused before
///   anything is dialled.
#[tokio::test]
async fn the_owners_connection_keeps_the_tls_rule() {
    let app = TestApp::spawn().await;

    let not_local = format!(
        "postgres://kelir:s3cret-pw@127.0.0.2:{}/kelir",
        server_refusing_tls("127.0.0.2").await
    );
    let local = format!(
        "postgres://kelir:s3cret-pw@127.0.0.1:{}/kelir",
        server_refusing_tls("127.0.0.1").await
    );

    for (what, config) in [
        (
            "a host that is not local",
            with_owner(&app, Some(not_local)),
        ),
        (
            "KELIR_DATABASE_SSL_MODE=require",
            AppConfig {
                database_ssl_mode: Some("require".to_owned()),
                ..with_owner(&app, Some(local))
            },
        ),
    ] {
        let pools = StartupPools::open(&config).expect("the configuration alone is acceptable");
        let started = Instant::now();
        let refused = pools
            .prepare(&config)
            .await
            .expect_err("no TLS, no owner's connection");
        let took = started.elapsed();

        let DatabaseStartupError::OwnerConnection(error) = &refused else {
            panic!("{what}: refused, but not at the owner's connection: {refused:?}");
        };
        assert!(
            matches!(error, sqlx::Error::Tls(_)),
            "{what}: refused, but not for TLS: {error:?}"
        );
        let text = format!("{refused} {refused:?}");
        assert!(text.contains("KELIR_DATABASE_SSL_MODE"), "{what}: {text}");
        assert!(!text.contains("s3cret-pw"), "{what}: {text}");
        assert!(
            took < db::ACQUIRE_TIMEOUT / 2,
            "{what}: refused after {took:?}: a TLS refusal was retried"
        );
    }

    let weak = with_owner(
        &app,
        Some("postgres://kelir:s3cret-pw@db.example.test:5432/kelir?sslmode=disable".to_owned()),
    );
    match StartupPools::open(&weak) {
        Err(DatabaseStartupError::MigrationOptions(_)) => {}
        Err(other) => panic!("refused, but not for the owner's options: {other:?}"),
        Ok(_) => panic!("a non-local owner's URL with sslmode=disable was accepted"),
    }
}

/// The built binary, given both URLs, as a container is, in `test`, logging
/// at `info` to `stdout`.
#[cfg(unix)]
fn binary(
    serving_url: &str,
    owner_url: &str,
    stdout: std::process::Stdio,
) -> tokio::process::Child {
    let bind_address = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("the bound address");

    tokio::process::Command::new(env!("CARGO_BIN_EXE_kelir-backend"))
        .env("KELIR_APP_ENV", "test")
        .env("KELIR_JWT_SECRET", common::JWT_SECRET)
        .env("KELIR_DATABASE_URL", serving_url)
        .env("KELIR_DATABASE_MIGRATION_URL", owner_url)
        .env("KELIR_BIND_ADDRESS", bind_address.to_string())
        .env("KELIR_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("KELIR_DATABASE_SSL_MODE")
        .env_remove("KELIR_DATABASE_SSL_ROOT_CERT")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_USERNAME")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_PASSWORD")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_EMAIL")
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .kill_on_drop(true)
        .spawn()
        .expect("the kelir-backend binary starts")
}

/// **The binary stopped mid-migration exits zero and leaves no owner
/// connection** (#991, #856): the binary, given both URLs, is held by the
/// migration gate, and sent SIGTERM while its owner's connection waits there.
/// It exits 0 at once; once the gate is released, the connection is gone.
#[cfg(unix)]
#[tokio::test]
async fn the_binary_stopped_mid_migration_exits_zero_and_leaves_no_owner_connection() {
    let app = TestApp::spawn().await;
    let lock = lock_the_migrations_table(&app).await;
    let mut child = binary(
        &common::as_the_serving_role(&owner_url(&app)),
        &owner_url(&app),
        std::process::Stdio::null(),
    );

    owner_seen(&app, "the binary's, waiting on the lock", |connection| {
        connection.wait_event_type == "Lock"
    })
    .await;

    let pid = child.id().expect("the binary is running");
    let killed = tokio::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await
        .expect("kill(1) runs");
    assert!(killed.success(), "kill -TERM {pid} failed: {killed}");

    let signalled = Instant::now();
    let status = tokio::time::timeout(PATIENCE, child.wait())
        .await
        .expect("the binary was still migrating long after SIGTERM")
        .expect("the binary's exit status");
    let took = signalled.elapsed();
    assert_eq!(
        status.code(),
        Some(0),
        "SIGTERM mid-migration did not end startup cleanly: {status}"
    );
    assert!(
        took < Duration::from_secs(2),
        "SIGTERM mid-migration took {took:?}: the migration was waited for"
    );

    lock.rollback().await.expect("the lock is released");
    no_owner_connection(&app, "once the stopped binary's statement ended").await;
}

/// **The binary refused as the owner exits one at once** (#991): wrong
/// credentials on `KELIR_DATABASE_MIGRATION_URL` end startup with exit 1, as
/// before the owner's pool became one connection, without waiting out the
/// retry, and the log names the setting and not the password. The role does
/// not exist, so the server refuses it whether it checks passwords (`28P01`)
/// or trusts (`28000`): either is an error the retry does not take.
#[cfg(unix)]
#[tokio::test]
async fn the_binary_refused_as_the_owner_exits_one_at_once() {
    let app = TestApp::spawn().await;
    let wrong = signed_in_as(&owner_url(&app), "kelir_no_such_owner", WRONG_PASSWORD);
    let child = binary(
        &common::as_the_serving_role(&owner_url(&app)),
        &wrong,
        std::process::Stdio::piped(),
    );

    let started = Instant::now();
    let output = tokio::time::timeout(PATIENCE, child.wait_with_output())
        .await
        .expect("the binary was still starting, refused as the owner")
        .expect("the binary's output");
    let took = started.elapsed();
    let log = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(1),
        "a refused owner did not fail startup: {}\n{log}",
        output.status
    );
    assert!(
        took < db::ACQUIRE_TIMEOUT,
        "the refused owner took {took:?}: the refusal was retried\n{log}"
    );
    assert!(log.contains("KELIR_DATABASE_MIGRATION_URL"), "{log}");
    assert!(
        log.contains("authentication failed") || log.contains("does not exist"),
        "exit 1, but not for the owner's credentials\n{log}"
    );
    assert!(!log.contains(WRONG_PASSWORD), "{log}");
}
