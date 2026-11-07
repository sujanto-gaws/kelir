//! **A connection returning to the pool while it closes outlives `close()`**
//! ([#645], [#904]).
//!
//! sqlx-core 0.8.6's `PoolInner::close` (`src/pool/inner.rs`) marks the pool
//! closed, then for each permit count up to `max_connections` drains
//! `idle_conns` and waits for that many permits, and returns after the last
//! wait without draining again. Dropping a `PoolConnection` spawns
//! `return_to_pool` (`src/pool/connection.rs`), which checks `is_closed()`,
//! then pings the connection, then calls `release()`: that pushes it onto
//! `idle_conns` and only then returns its permit. A return that passed
//! `is_closed()` before the close and whose ping is answered after the last
//! drain is left behind: `close()` returns, the pool reports itself closed and
//! still counts the connection, and its backend sits idle in `ClientRead`
//! until the last `Pool` handle drops.
//!
//! That is the backend `common::deadlocks_once_settled` met in CI on #637, and
//! the reason it ends idle backends. launchbadge/sqlx#3217 reports it, and
//! launchbadge/sqlx#3952 fixes it in the 0.9 line; no 0.8 release has the fix.
//!
//! **Why five rounds are enough.** `#[tokio::test]` runs on a current-thread
//! runtime. `yield_now` lets the spawned return task run until its ping waits
//! on the socket. `close()` marks the pool closed when it is called and drains
//! in the same poll, so the ping's answer leaks the connection whenever it is
//! handled after the test resumes. When the server has answered before the
//! runtime resumes the test, the return task can be polled first and release
//! the connection before the close, which then drains it. Locally that took
//! 4 of 100 rounds (20 runs of five, each round counted), never two in one
//! run; with every core of the container kept busy, 0 of 100. A slower
//! server only widens the leak, so five rounds all coming out clean, about
//! 0.04^5 against sqlx 0.8.6, means the race is gone. Without the yield, the
//! return task first runs after the close began, sees the pool closed and
//! hangs up: 0 of 200 such rounds left a backend.
//!
//! [#645]: https://github.com/sujanto-gaws/kelir/issues/645
//! [#904]: https://github.com/sujanto-gaws/kelir/issues/904

mod common;

use std::time::Duration;

use common::{with_database, TestApp, TEST_POOL_MAX_CONNECTIONS};
use sqlx::{Connection, PgConnection};
use tokio::time::Instant;

/// The server the harness runs against, as `common` finds it.
fn server() -> String {
    std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("KELIR_DATABASE_URL"))
        .expect("the server's URL")
}

/// The state of backend `pid`, if it is still there.
async fn backend_state(server: &mut PgConnection, pid: i32) -> Option<String> {
    sqlx::query_scalar("SELECT COALESCE(state, '') FROM pg_stat_activity WHERE pid = $1")
        .bind(pid)
        .fetch_optional(server)
        .await
        .expect("read pg_stat_activity")
}

/// Waits for backend `pid` to leave `pg_stat_activity`, failing at a deadline.
async fn wait_for_exit(server: &mut PgConnection, pid: i32, why: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Some(state) = backend_state(server, pid).await {
        assert!(
            Instant::now() < deadline,
            "backend {pid} is still {state:?}: {why}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// One round: whether the returning connection was left behind a closed pool,
/// with `yield_first` deciding whether its return task runs before the close.
///
/// Whatever it finds, the round leaves no backend behind it.
async fn round(app: &TestApp, server: &mut PgConnection, yield_first: bool) -> bool {
    // The application's own pool options, over the test's database.
    let pool = kelir_backend::db::create_pool_with_max_connections(
        &with_database(&self::server(), &app.database_name),
        TEST_POOL_MAX_CONNECTIONS,
    )
    .expect("a pool");

    let mut connection = pool.acquire().await.expect("a connection");
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .expect("the connection's backend");

    // Spawns `return_to_pool`. Yielding lets it pass its `is_closed()` check
    // and send its ping before the close below begins.
    drop(connection);
    if yield_first {
        tokio::task::yield_now().await;
    }
    pool.close().await;
    assert!(pool.is_closed());

    // The pool's own count decides, not timing: a connection it still counts
    // is in `idle_conns`, so its backend is there and idle; one it does not
    // count was hung up, and its backend leaves on its own.
    let leaked = match pool.size() {
        0 => {
            wait_for_exit(server, pid, "the closed pool counts no connection").await;
            false
        }
        1 => {
            assert_eq!(
                backend_state(server, pid).await.as_deref(),
                Some("idle"),
                "the backend of the connection the closed pool still counts"
            );
            // Ended here so it cannot outlast the round.
            sqlx::query(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity
                 WHERE pid = $1 AND datname = $2 AND state = 'idle'",
            )
            .bind(pid)
            .bind(&app.database_name)
            .execute(&mut *server)
            .await
            .expect("end the backend the closed pool left");
            wait_for_exit(server, pid, "it was ended").await;
            true
        }
        size => panic!("the closed pool counts {size} connections; it had one"),
    };

    drop(pool);
    leaked
}

/// **A return whose ping straddles `close()` leaves its backend behind**: the
/// #645 shape, reproduced on demand, with `size() == 1` and the backend idle.
///
/// **When this goes red after a sqlx upgrade, `close()` drains after its last
/// permit, and the workaround in `common::deadlocks_once_settled`, ending
/// idle backends the closed pool left, is to be removed.** Up to five rounds,
/// and at least one must leak; the module's note gives the rates.
#[tokio::test]
async fn a_connection_returning_while_the_pool_closes_outlives_close() {
    let app = TestApp::spawn().await;
    let mut server = PgConnection::connect(&server())
        .await
        .expect("reach the server");

    let mut leaked = false;
    for _ in 0..5 {
        if round(&app, &mut server, true).await {
            leaked = true;
            break;
        }
    }
    server.close().await.expect("close the connection");
    assert!(
        leaked,
        "no round left a backend: sqlx's close() may now drain after its last \
         permit, and deadlocks_once_settled's ending of idle backends is to go (#904)"
    );
}

/// **A return that has not started when `close()` begins is closed by it**:
/// `return_to_pool` sees the pool closed and hangs up. This is why the
/// connection a test used last never leaks, only one returning beside it.
#[tokio::test]
async fn a_connection_returning_after_the_close_began_is_closed() {
    let app = TestApp::spawn().await;
    let mut server = PgConnection::connect(&server())
        .await
        .expect("reach the server");

    for _ in 0..5 {
        assert!(
            !round(&app, &mut server, false).await,
            "a return that began after the close left its backend"
        );
    }
    server.close().await.expect("close the connection");
}
