//! A tenant's audit chain is appended one writer at a time, and never before
//! its head ([#995], ADR-0052).
//!
//! `audit::record` read the tenant's head and inserted the row chained to it
//! as two statements on whatever connection the pool handed it, so two
//! writers for one tenant could read the same head and both chain to it. Each
//! row was still written whole and the append-only trigger still refused any
//! change to one; what forked was the chain, and `chain_breaks` reported the
//! later sibling in the words it uses for tampering.
//!
//! # What each test holds `record` to
//!
//! - **The staged race** stops a first writer at its head read, after it holds
//!   the lock, with [`ReadGate`]. A second writer for the same tenant then
//!   waits on the lock, a writer for another tenant does not, and once both
//!   are released the chain verifies. It is arranged, not raced for, so it
//!   goes red on the same interleaving every run when the lock is missing.
//! - **The clamp**: a head stamped in the future is followed by a row exactly
//!   one microsecond after it, and a head in the past by a row at the clock.
//! - **A burst** from two pools, standing in for two backends. It is not
//!   deterministic on its own: the staged race is what proves the lock, and
//!   the burst is what would notice a path that skips it.
//!
//! # Seen red, 2026-11-25
//!
//! - **Against `main`'s unlocked `record`**, before the fix: the staged race
//!   reported `FIRST` linking to the head `SECOND` had already chained to, the
//!   burst reported dozens of siblings, and the future-head test reported the
//!   new row sorting before the head it links to. The gate test passed, which
//!   is what showed the gate works on this table.
//! - **With the `pg_advisory_xact_lock` statement deleted**: the staged race
//!   and the burst went red on `chain_breaks`, the race in 1.6 seconds, on the
//!   fork and not on a timeout.
//! - **With the clamp removed** (`chain_time` returning the clock whatever the
//!   head says): the future-head test went red on `chain_breaks`, and the
//!   past-head test stayed green, as it should.
//!
//! Every `record` here runs on [`TestApp::serving_pool`], the role that holds
//! only `SELECT` and `INSERT` on `audit_events` (ADR-0051), so the lock is
//! shown to need no privilege that role lacks.
//!
//! [#995]: https://github.com/sujanto-gaws/kelir/issues/995

mod common;

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use common::audit_chain::{chain_breaks, stored_rows, StoredRow, GENESIS};
use common::gate::{ReadGate, PATIENCE};
use common::{fixtures, TestApp, TEST_POOL_MAX_CONNECTIONS};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use uuid::Uuid;

use kelir_backend::error::AppError;
use kelir_backend::modules::audit::domain::ObjectType;
use kelir_backend::modules::audit::{self, chain_hash, AuditEntry};

/// `audit::AUDIT_CHAIN_LOCK_CLASS`, the ASCII of `ADCH`, repeated so that the
/// staged race can see a writer waiting on it in `pg_locks`. A change there
/// that is not made here turns that test red.
const AUDIT_CHAIN_LOCK_CLASS: i32 = 0x4144_4348;

/// A probe row for `tenant_id`, told apart from its siblings by `action`.
fn entry(tenant_id: Uuid, action: &'static str) -> AuditEntry<'static> {
    AuditEntry {
        tenant_id,
        event_type: "AuditChain.Probed",
        action,
        object_type: ObjectType::Tenant,
        object_id: tenant_id,
        actor_user_id: None,
        ip_address: None,
        reason: None,
        old_value: None,
        new_value: None,
    }
}

/// `record` on `pool`, spawned, so a test can hold it at the gate.
fn spawn_record(
    pool: &PgPool,
    tenant_id: Uuid,
    action: &'static str,
) -> JoinHandle<Result<Uuid, AppError>> {
    let pool = pool.clone();
    tokio::spawn(async move { audit::record(&pool, entry(tenant_id, action)).await })
}

/// `running`'s id, failing rather than hanging when it never comes.
async fn written(running: JoinHandle<Result<Uuid, AppError>>, what: &str) -> Uuid {
    tokio::time::timeout(PATIENCE, running)
        .await
        .unwrap_or_else(|_| panic!("{what} never returned"))
        .unwrap_or_else(|error| panic!("{what} panicked: {error}"))
        .unwrap_or_else(|error| panic!("{what} failed: {error:?}"))
}

/// Whether a backend is waiting, not yet granted, on the chain lock of any
/// tenant. The two-argument form puts the class in `classid` and marks the
/// lock with `objsubid = 2`.
async fn a_writer_waits_on_the_chain_lock(app: &TestApp) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_locks
          WHERE locktype = 'advisory' AND classid::text::bigint = $1 AND objsubid = 2
            AND NOT granted
            AND database = (SELECT oid FROM pg_database WHERE datname = current_database()))",
    )
    .bind(i64::from(AUDIT_CHAIN_LOCK_CLASS))
    .fetch_one(&app.pool)
    .await
    .expect("read pg_locks")
}

/// Polls `condition` until it holds, failing with `what` after [`PATIENCE`].
async fn until<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !condition().await {
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// One tenant's rows, in the order a verifier walks them.
fn rows_of(rows: &[StoredRow], tenant_id: Uuid) -> Vec<&StoredRow> {
    rows.iter()
        .filter(|row| row.tenant_id == tenant_id)
        .collect()
}

/// The row `id`, read back.
fn the_row(rows: &[StoredRow], id: Uuid) -> &StoredRow {
    rows.iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("audit row {id} was not stored"))
}

#[tokio::test]
async fn record_writes_through_the_read_gate_and_the_trigger_still_refuses() {
    // The staged race depends on the gate passing `record`'s insert through its
    // view, and on the append-only trigger staying on the renamed table. Both
    // are checked here first, with a predicate that stops nothing.
    let app = TestApp::spawn().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    let mut reads = ReadGate::close(&app, "audit_events", "false", "false").await;

    let id = audit::record(&app.serving_pool, entry(tenant, "THROUGH_THE_VIEW"))
        .await
        .expect("record inserts through the gate's view");

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events_stored WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the stored table");
    assert_eq!(stored, 1, "the row reached the renamed table");

    let trigger: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_trigger
          WHERE tgrelid = 'audit_events_stored'::regclass AND tgname = $1)",
    )
    .bind(fixtures::AUDIT_APPEND_ONLY_TRIGGER)
    .fetch_one(&app.pool)
    .await
    .expect("read pg_trigger");
    assert!(trigger, "the append-only trigger moved with the table");

    // As the owner, which the trigger refuses whatever the role.
    let refused = sqlx::query("UPDATE audit_events_stored SET reason = 'rewritten' WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await;
    assert!(
        refused.is_err(),
        "the trigger still refuses an update to the renamed table"
    );

    reads.open().await;
}

#[tokio::test]
async fn a_second_writer_waits_for_the_first_and_another_tenant_does_not() {
    let app = TestApp::spawn().await;
    let tenant_a = fixtures::SYSTEM_TENANT_ID;
    let tenant_b = fixtures::create_tenant(&app.pool, "CHAIN-B", "Chain B").await;

    // The gate stops only a plain `SELECT` its predicate matches, so the head
    // the first writer reads has to exist before the gate closes.
    let head = audit::record(&app.serving_pool, entry(tenant_a, "HEAD"))
        .await
        .expect("the head is written");

    let mut reads =
        ReadGate::close(&app, "audit_events", &format!("r.id = '{head}'"), "false").await;

    // The first writer takes the lock and is stopped reading the head.
    let first = spawn_record(&app.serving_pool, tenant_a, "FIRST");
    until("the first writer never reached the read gate", || {
        reads.holds_a_waiter(&app)
    })
    .await;

    // The second writer either waits on the lock, or, with no lock to wait
    // on, reads the same head past the gate and finishes beside it.
    let second = spawn_record(&app.serving_pool, tenant_a, "SECOND");
    until(
        "the second writer neither finished nor waited on the chain lock",
        || async { second.is_finished() || a_writer_waits_on_the_chain_lock(&app).await },
    )
    .await;
    let second_waited = !second.is_finished() && reads.holds_a_waiter(&app).await;

    // Another tenant's key is another lock: its writer returns while the
    // first writer still holds tenant A's.
    let other = written(
        spawn_record(&app.serving_pool, tenant_b, "OTHER_TENANT"),
        "tenant B's writer, while tenant A's is stopped",
    )
    .await;
    assert!(
        reads.holds_a_waiter(&app).await,
        "the first writer was still stopped when tenant B's returned"
    );

    reads.open().await;
    let first = written(first, "the first writer").await;
    let second = written(second, "the second writer").await;

    let rows = stored_rows(&app).await;
    let breaks = chain_breaks(&rows);
    assert!(
        breaks.is_empty(),
        "the chain forked under two writers:\n{}",
        breaks.join("\n")
    );

    assert!(
        second_waited,
        "the second writer did not wait on the chain lock while the first held it"
    );
    assert_eq!(
        the_row(&rows, first).previous_hash,
        the_row(&rows, head).current_hash
    );
    assert_eq!(
        the_row(&rows, second).previous_hash,
        the_row(&rows, first).current_hash
    );
    assert_eq!(the_row(&rows, other).tenant_id, tenant_b);
}

/// A head row for `tenant_id` at `created_at`, chained to the tenant's
/// current head, written as the owner, as a backend whose clock is ahead or
/// behind would have written it.
async fn a_head_at(app: &TestApp, tenant_id: Uuid, created_at: DateTime<Utc>) -> Uuid {
    let previous_hash: String = sqlx::query_scalar(
        "SELECT current_hash FROM audit_events WHERE tenant_id = $1
          ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(&app.pool)
    .await
    .expect("read the head")
    .unwrap_or_else(|| GENESIS.to_owned());

    let id = Uuid::now_v7();
    let probe = entry(tenant_id, "SKEWED_HEAD");
    let current_hash = chain_hash(&previous_hash, &id, created_at, &probe);

    sqlx::query(
        "INSERT INTO audit_events (id, tenant_id, event_type, action, object_type, object_id,
                                   previous_hash, current_hash, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(probe.event_type)
    .bind(probe.action)
    .bind(probe.object_type.as_db())
    .bind(tenant_id)
    .bind(previous_hash)
    .bind(current_hash)
    .bind(created_at)
    .execute(&app.pool)
    .await
    .expect("the skewed head is written as the owner");

    id
}

/// `at`, at `TIMESTAMPTZ`'s resolution.
fn in_microseconds(at: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(at.timestamp_micros()).expect("a valid instant")
}

#[tokio::test]
async fn a_row_after_a_head_in_the_future_is_one_microsecond_after_it() {
    let app = TestApp::spawn().await;
    let tenant = fixtures::create_tenant(&app.pool, "CHAIN-AHEAD", "Chain ahead").await;

    let ahead = in_microseconds(Utc::now() + TimeDelta::hours(1));
    let head = a_head_at(&app, tenant, ahead).await;

    let next = audit::record(&app.serving_pool, entry(tenant, "AFTER_THE_FUTURE"))
        .await
        .expect("record writes after a head in the future");

    let rows = stored_rows(&app).await;
    let breaks = chain_breaks(&rows);
    assert!(
        breaks.is_empty(),
        "the chain does not walk past a head in the future:\n{}",
        breaks.join("\n")
    );

    let next = the_row(&rows, next);
    assert_eq!(next.created_at, ahead + TimeDelta::microseconds(1));
    assert_eq!(next.previous_hash, the_row(&rows, head).current_hash);
}

#[tokio::test]
async fn a_row_after_a_head_in_the_past_takes_the_clock() {
    let app = TestApp::spawn().await;
    let tenant = fixtures::create_tenant(&app.pool, "CHAIN-BEHIND", "Chain behind").await;

    a_head_at(
        &app,
        tenant,
        in_microseconds(Utc::now() - TimeDelta::hours(1)),
    )
    .await;

    let before = in_microseconds(Utc::now());
    let next = audit::record(&app.serving_pool, entry(tenant, "AFTER_THE_PAST"))
        .await
        .expect("record writes after a head in the past");
    let after = Utc::now();

    let rows = stored_rows(&app).await;
    assert!(
        chain_breaks(&rows).is_empty(),
        "{}",
        chain_breaks(&rows).join("\n")
    );

    let next = the_row(&rows, next).created_at;
    assert!(
        before <= next && next <= after,
        "the row took {next}, not the clock between {before} and {after}"
    );
}

#[tokio::test]
async fn the_first_row_of_an_empty_chain_takes_the_clock() {
    let app = TestApp::spawn().await;
    let tenant = fixtures::create_tenant(&app.pool, "CHAIN-EMPTY", "Chain empty").await;

    let before = in_microseconds(Utc::now());
    let first = audit::record(&app.serving_pool, entry(tenant, "FIRST_OF_ALL"))
        .await
        .expect("record writes the first row");
    let after = Utc::now();

    let rows = stored_rows(&app).await;
    let first = the_row(&rows, first);
    assert_eq!(first.previous_hash, GENESIS);
    assert!(before <= first.created_at && first.created_at <= after);
    assert!(
        chain_breaks(&rows).is_empty(),
        "{}",
        chain_breaks(&rows).join("\n")
    );
}

/// How many writers the burst starts at once, across both pools.
const BURST: usize = 40;

#[tokio::test]
async fn a_burst_from_two_pools_leaves_every_row_and_no_break() {
    let app = TestApp::spawn().await;
    let tenant = fixtures::SYSTEM_TENANT_ID;

    // A second pool as the serving role, standing in for a second backend.
    let second_backend = PgPoolOptions::new()
        .max_connections(TEST_POOL_MAX_CONNECTIONS)
        .connect_lazy_with((*app.serving_pool.connect_options()).clone());

    let before = rows_of(&stored_rows(&app).await, tenant).len();

    let writers: Vec<_> = (0..BURST)
        .map(|index| {
            let pool = if index % 2 == 0 {
                &app.serving_pool
            } else {
                &second_backend
            };
            spawn_record(pool, tenant, "BURST")
        })
        .collect();
    for writer in writers {
        written(writer, "a writer in the burst").await;
    }
    second_backend.close().await;

    let rows = stored_rows(&app).await;
    let breaks = chain_breaks(&rows);
    assert!(
        breaks.is_empty(),
        "the burst forked the chain:\n{}",
        breaks.join("\n")
    );

    let chain = rows_of(&rows, tenant);
    assert_eq!(chain.len(), before + BURST, "every writer left its row");

    let times: HashSet<DateTime<Utc>> = chain.iter().map(|row| row.created_at).collect();
    assert_eq!(times.len(), chain.len(), "no two rows share a created_at");
}
