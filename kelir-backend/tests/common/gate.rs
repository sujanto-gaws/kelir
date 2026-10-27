//! A gate on the read a request runs after its commit, and the race it holds
//! open ([#785], [#787]).
//!
//! A write that committed and then read its row back on the pool, for its
//! answer and its audit row, met nothing there when a delete committed in
//! between. The change had committed, and the request answered 500 (or 404)
//! with no audit row. [`ReadGate`] stops that read, and only that read, until a
//! delete has answered; [`a_delete_follows`] runs the race. A request that
//! reads back inside its transaction, before its commit, is not stopped, and
//! answers what it wrote.
//!
//! [#785]: https://github.com/sujanto-gaws/kelir/issues/785
//! [#787]: https://github.com/sujanto-gaws/kelir/issues/787

use std::future::Future;
use std::time::Duration;

use sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;

use super::{TestApp, TestResponse};

/// The advisory key a stopped read waits on; its issue's number.
const READ_GATE: i64 = 787;

/// The advisory key the first stopped read holds, so that no other read is
/// stopped while it waits.
const READ_GATE_CLAIM: i64 = 7870;

/// How long a raced request is given to reach the gate or to answer.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// A gate on the plain reads of one table's rows that `predicate` matches.
///
/// **It stops the first read a request runs on the pool after its commit**,
/// and nothing else. That is a `SELECT` with no transaction id that takes no
/// row lock (`FOR UPDATE`, `FOR NO KEY UPDATE`, `FOR SHARE`, `FOR KEY SHARE`),
/// of a row `predicate` matches that is not deleted in the read's snapshot:
/// - a statement in a transaction that has written or locked a row has an id,
///   so a read back before the commit is not stopped;
/// - an `UPDATE`, such as a soft delete on the pool, is not a `SELECT`;
/// - a row lock is taken before its recheck reads the new row.
///
/// **Only the first such read is stopped.** It holds a claim while it waits,
/// and a read that finds the claim held passes, so a delete's own read of the
/// updated row is not stopped behind the update's.
///
/// **A stopped read then sees what a read run at that moment would.** It took
/// its snapshot before it was stopped, so the row it returns is the row as it
/// was then. Once the gate opens, the function asks again, with the fresh
/// snapshot each statement in it takes, whether the row has been deleted
/// since, and drops it if so.
///
/// A trigger cannot stop a `SELECT`, and row-level security does not apply to
/// a superuser, which CI's role is. So the table is renamed `<table>_stored`
/// and a view of its name put in its place, whose `WHERE` calls the gate's
/// function. The service's statements name the table, so they read, lock and
/// write through the view; the foreign keys and the triggers stay on the
/// table. Read a deleted row from `<table>_stored`, since the view hides it.
pub struct ReadGate {
    connection: Option<PgConnection>,
}

impl ReadGate {
    /// Closes the gate on `table`. `predicate` is SQL over `r`, the stored
    /// row, such as `r.name = 'Renamed'`. `deleted` is SQL over `r` that is
    /// true of a deleted row, such as `r.deleted_at IS NOT NULL`.
    pub async fn close(app: &TestApp, table: &str, predicate: &str, deleted: &str) -> Self {
        let stored = format!("{table}_stored");

        for statement in [
            format!("ALTER TABLE {table} RENAME TO {stored}"),
            format!(
                "CREATE FUNCTION test_row_deleted(r {stored})
                 RETURNS boolean LANGUAGE sql AS $$ SELECT {deleted} $$"
            ),
            // The claim is asked last, in an `IF` of its own, so that only a
            // read the gate would stop can take it.
            format!(
                "CREATE FUNCTION test_read_gate(r {stored})
                 RETURNS boolean LANGUAGE plpgsql AS $$
                 BEGIN
                     IF NOT test_row_deleted(r)
                        AND ({predicate})
                        AND txid_current_if_assigned() IS NULL
                        AND current_query() ~* '^\\s*SELECT\\M'
                        AND current_query()
                            !~* '\\mFOR\\s+(NO\\s+KEY\\s+UPDATE|UPDATE|KEY\\s+SHARE|SHARE)\\M' THEN
                         IF pg_try_advisory_xact_lock({READ_GATE_CLAIM}) THEN
                             PERFORM pg_advisory_xact_lock_shared({READ_GATE});
                             RETURN NOT EXISTS (
                                 SELECT 1 FROM {stored} s
                                 WHERE s.id = r.id AND test_row_deleted(s)
                             );
                         END IF;
                     END IF;
                     RETURN true;
                 END
                 $$"
            ),
            format!("CREATE VIEW {table} AS SELECT * FROM {stored} s WHERE test_read_gate(s)"),
        ] {
            sqlx::query(&statement)
                .execute(&app.pool)
                .await
                .unwrap_or_else(|error| panic!("the read gate: {error}\n{statement}"));
        }

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the read gate's connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(READ_GATE)
            .execute(&mut connection)
            .await
            .expect("close the read gate");

        ReadGate {
            connection: Some(connection),
        }
    }

    /// Whether a read is stopped at the gate now.
    pub async fn holds_a_waiter(&self, app: &TestApp) -> bool {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks
              WHERE locktype = 'advisory' AND objid::text::bigint = $1 AND objsubid = 1
                AND NOT granted
                AND database = (SELECT oid FROM pg_database WHERE datname = current_database()))",
        )
        .bind(READ_GATE)
        .fetch_one(&app.pool)
        .await
        .expect("read pg_locks")
    }

    /// Opens the gate, once; opening it again does nothing.
    pub async fn open(&mut self) {
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(READ_GATE)
            .execute(&mut connection)
            .await
            .expect("open the read gate");
        connection
            .close()
            .await
            .expect("close the read gate's connection");
    }
}

/// What [`a_delete_follows`] answered.
pub struct Raced {
    pub updated: TestResponse,
    pub deleted: TestResponse,
}

/// Sends `update`, then `delete` once the update has answered or been stopped
/// at `reads`, and opens `reads` once the delete has answered.
///
/// **The delete commits after the update's commit and before any read the
/// update runs after it.** A request that reads back on the pool is stopped at
/// the gate, and finds its row deleted once the gate opens. A request that
/// reads back inside its transaction answered before the delete was sent.
///
/// When the update has answered, the gate is opened before the delete is sent:
/// nothing waits at it, and the delete's own read of the updated row would
/// otherwise be the one it stops.
pub async fn a_delete_follows<U, D>(
    app: &TestApp,
    mut reads: ReadGate,
    update: U,
    delete: D,
) -> Raced
where
    U: Future<Output = TestResponse> + Send + 'static,
    D: Future<Output = TestResponse> + Send + 'static,
{
    let updating = tokio::spawn(update);

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !updating.is_finished() && !reads.holds_a_waiter(app).await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the update neither answered nor reached the read gate"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if updating.is_finished() {
        reads.open().await;
    }

    // Awaited with a deadline: a delete the gate stopped by mistake would
    // otherwise hang the test, since the gate opens after it.
    let deleted = answer(tokio::spawn(delete), "the delete").await;
    reads.open().await;
    let updated = answer(updating, "the update").await;

    Raced { updated, deleted }
}

/// `running`'s answer, failing rather than hanging when it never comes.
async fn answer(running: JoinHandle<TestResponse>, what: &str) -> TestResponse {
    tokio::time::timeout(PATIENCE, running)
        .await
        .unwrap_or_else(|_| panic!("{what} never answered"))
        .unwrap_or_else(|error| panic!("{what} panicked: {error}"))
}
