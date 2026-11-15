//! The delivery lease at its seams ([#853]; NFR-SCA-001, NFR-SCA-003): what
//! `notification_claim.rs` leaves unstaged.
//!
//! # Why a second file
//!
//! `notification_claim.rs` is the builder's: the criterion, its control, a
//! crash, a live lease, and the fence called directly. **This file is the
//! independent campaign over the same claim**, and each test names the
//! mutation of 342d9ed it would redden:
//!
//! | Mutation | Test |
//! |---|---|
//! | `mark_delivered` not matching on the lease, reached through two real passes | *an overtaken sender mid-send cannot settle the row the newer claim is sending* |
//! | `SKIP LOCKED` or the lease test dropped, seen by more than two senders at once | *many senders at once send each notification once* |
//! | The claim's `IS NULL` arm dropped, or `deleted_at` not filtered | *a never-claimed row is due, a soft-deleted one never is* |
//! | `mark_delivered` not scoped by tenant | *a settle names its tenant* |
//! | A failed send handing its row back to the queue | *a failed delivery settles under its claim and is not claimed again* |
//! | A pass that errors losing what it claimed and did not reach | *a pass that errors mid-batch leaves its unreached rows to the lease* |
//! | The partial index's predicate drifting from the claim's | *the claim reads the partial index its predicate names* |
//! | `pass`'s `may_begin_send` `break` removed | *a pass stops starting sends when its lease runs short* |
//!
//! **The last one takes three minutes of wall clock.** `pass` reads the lease
//! from the constant `LEASE` and the elapsed time from `std::time::Instant`,
//! neither of which a test can inject, so the only way to put a pass past
//! `LEASE - SEND_ALLOWANCE` is a send that really takes that long
//! (`Mailer::captured_taking`).
//!
//! [#853]: https://github.com/sujanto-gaws/kelir/issues/853

mod common;

use std::time::Duration;

use kelir_backend::mail::Mailer;
use kelir_backend::modules::notification::domain::{LEASE, SEND_ALLOWANCE};
use kelir_backend::modules::notification::{repository, worker};
use kelir_backend::state::AppState;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;
use uuid::Uuid;

use common::{fixtures, TestApp};

/// How long a staged step may take before the test says which one did not
/// happen.
const PATIENCE: Duration = Duration::from_secs(30);

/// The first sender's `application_name`, and its gate's advisory key.
const FIRST: &str = "kelir-lease-first";
const FIRST_GATE: i64 = 8531;

/// The second sender's, and its gate's key.
const SECOND: &str = "kelir-lease-second";
const SECOND_GATE: i64 = 8532;

/// One batch, told apart in a mailbox by subject: the seeded `TASK_ASSIGNED`
/// email template's subject is the notification's title.
const TITLES: [&str; 3] = ["Lease one", "Lease two", "Lease three"];

/// A pass running in the background.
type Pass = JoinHandle<Result<(), sqlx::Error>>;

/// A person with an address, and the notification centre's permission.
async fn recipient(app: &TestApp, username: &str) -> Uuid {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        &["notification:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await
}

/// A `PENDING` notification of the shape `service::notify` writes, with no
/// lease: what every row written before 0053, or by the previous binary, is.
async fn pending_notification(app: &TestApp, recipient: Uuid, title: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO notifications
            (id, tenant_id, recipient_user_id, notification_type, title, body, channel, status)
         VALUES ($1, $2, $3, 'TASK_ASSIGNED', $4, 'Approve the request', 'IN_APP', 'PENDING')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(recipient)
    .bind(title)
    .execute(&app.pool)
    .await
    .expect("the notification");

    id
}

/// One notification per title, all to one person, oldest first.
async fn batch(app: &TestApp, username: &str, titles: &[String]) -> Vec<Uuid> {
    let person = recipient(app, username).await;
    let mut ids = Vec::new();

    for title in titles {
        ids.push(pending_notification(app, person, title).await);
    }

    ids
}

fn titles() -> Vec<String> {
    TITLES.iter().map(|title| (*title).to_owned()).collect()
}

/// One sender: a pool and a mailer of its own over the test's database.
async fn sender(app: &TestApp, name: &str) -> AppState {
    let options = (*app.pool.connect_options()).clone().application_name(name);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("the sender's pool");

    AppState::with_mailer(pool, (*app.state.config).clone(), Mailer::captured())
}

fn spawn_pass(state: &AppState) -> Pass {
    let state = state.clone();

    tokio::spawn(async move { worker::pass(&state).await })
}

async fn finished(running: Pass, who: &str) {
    tokio::time::timeout(PATIENCE, running)
        .await
        .unwrap_or_else(|_| panic!("{who} did not finish"))
        .unwrap_or_else(|error| panic!("{who}'s task: {error}"))
        .unwrap_or_else(|error| panic!("{who}'s pass: {error}"));
}

/// How many emails each title was sent as, across every sender's mailer.
fn emails_per_title(senders: &[&AppState], titles: &[String]) -> Vec<usize> {
    let sent: Vec<_> = senders
        .iter()
        .flat_map(|state| state.mailer.captured_messages())
        .collect();

    titles
        .iter()
        .map(|title| sent.iter().filter(|mail| mail.subject == *title).count())
        .collect()
}

async fn status_of(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM notifications WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the notification")
}

async fn lease_of(app: &TestApp, id: Uuid) -> Option<chrono::DateTime<chrono::Utc>> {
    sqlx::query_scalar("SELECT lease_expires_at FROM notifications WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the notification's lease")
}

async fn attempts_for(app: &TestApp, id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_logs WHERE notification_id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the attempts")
}

async fn execute(app: &TestApp, statement: &str) {
    sqlx::query(statement)
        .execute(&app.pool)
        .await
        .expect(statement);
}

/// Every `PENDING` lease set to a second ago: what the clock does to a lease
/// five minutes on, without the five minutes.
async fn expire_every_lease(app: &TestApp) {
    execute(
        app,
        "UPDATE notifications SET lease_expires_at = now() - interval '1 second'
         WHERE status = 'PENDING' AND lease_expires_at IS NOT NULL",
    )
    .await;
}

/// Two `BEFORE INSERT` gates on `notification_logs`, one per sender, each
/// waiting on an advisory lock the test holds. A delivery's log row is written
/// after its mail is sent and before its row is settled, so a held sender has
/// an email out and its row unsettled.
struct Gates {
    connection: PgConnection,
}

impl Gates {
    async fn close(app: &TestApp) -> Self {
        execute(
            app,
            &format!(
                "CREATE FUNCTION test_hold_lease_senders() RETURNS trigger
                 LANGUAGE plpgsql AS $$
                 BEGIN
                     IF current_setting('application_name') = '{FIRST}' THEN
                         PERFORM pg_advisory_xact_lock_shared({FIRST_GATE});
                     ELSIF current_setting('application_name') = '{SECOND}' THEN
                         PERFORM pg_advisory_xact_lock_shared({SECOND_GATE});
                     END IF;
                     RETURN NEW;
                 END
                 $$"
            ),
        )
        .await;
        execute(
            app,
            "CREATE TRIGGER test_hold_lease_senders BEFORE INSERT ON notification_logs
             FOR EACH ROW EXECUTE FUNCTION test_hold_lease_senders()",
        )
        .await;

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the gates' connection");
        for key in [FIRST_GATE, SECOND_GATE] {
            sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(key)
                .execute(&mut connection)
                .await
                .expect("closes a gate");
        }

        Gates { connection }
    }

    /// Returns once the sender named waits at its gate.
    async fn holds(app: &TestApp, name: &str, running: &Pass) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let held: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND application_name = $1
                   AND wait_event_type = 'Lock'",
            )
            .bind(name)
            .fetch_one(&app.pool)
            .await
            .expect("reads pg_stat_activity");
            if held == 1 {
                return;
            }
            assert!(!running.is_finished(), "{name} was not held");
            assert!(
                tokio::time::Instant::now() < deadline,
                "{name} was not held"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn open(&mut self, key: i64) {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(key)
            .execute(&mut self.connection)
            .await
            .expect("opens a gate");
    }
}

/// **A sender whose lease passed mid-send cannot settle the row a newer claim
/// is sending**, through two real passes and not a direct call.
///
/// The first sender is held with row one's email out and the row unsettled.
/// Row one's lease is then made to pass, and only row one's. The second
/// sender claims it, sends it, and is held in the same place. The first is let
/// through: its settle of row one must move nothing — the row is still
/// `PENDING`, so only the lease tells the two claims apart — and it delivers
/// rows two and three, which it still holds. Then the second is let through
/// and settles row one.
///
/// Row one is emailed twice: that is the residual duplicate `SEND_ALLOWANCE`
/// documents, a send that outlived its lease. What is proved is that the row
/// is settled once, by the claim that holds it.
#[tokio::test]
async fn an_overtaken_sender_mid_send_cannot_settle_the_row_the_newer_claim_is_sending() {
    let app = TestApp::spawn().await;
    let titles = titles();
    let ids = batch(&app, "ntf-lease-overtaken", &titles).await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    let mut gates = Gates::close(&app).await;
    let first_running = spawn_pass(&first);
    Gates::holds(&app, FIRST, &first_running).await;
    assert_eq!(first.mailer.captured_messages().len(), 1);

    sqlx::query(
        "UPDATE notifications SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(ids[0])
    .execute(&app.pool)
    .await
    .expect("row one's lease passes");

    let second_running = spawn_pass(&second);
    Gates::holds(&app, SECOND, &second_running).await;
    let newer_lease = lease_of(&app, ids[0]).await;
    assert!(newer_lease.is_some(), "the second claim wrote no lease");

    gates.open(FIRST_GATE).await;
    finished(first_running, "the first sender").await;

    assert_eq!(
        status_of(&app, ids[0]).await,
        "PENDING",
        "the overtaken sender settled a row the newer claim was still sending"
    );
    assert_eq!(lease_of(&app, ids[0]).await, newer_lease);
    assert_eq!(status_of(&app, ids[1]).await, "SENT");
    assert_eq!(status_of(&app, ids[2]).await, "SENT");

    gates.open(SECOND_GATE).await;
    finished(second_running, "the second sender").await;

    assert_eq!(status_of(&app, ids[0]).await, "SENT");
    assert_eq!(
        emails_per_title(&[&first, &second], &titles),
        vec![2, 1, 1],
        "only the row whose lease passed mid-send may go out twice"
    );
    assert_eq!(
        second.mailer.captured_messages().len(),
        1,
        "the second sender claimed a row whose lease had not passed"
    );
    assert_eq!(attempts_for(&app, ids[0]).await, 2);
}

/// **Six senders at once over forty-eight notifications send each one once.**
///
/// The criterion's two senders are staged; this is the race left to the
/// scheduler, with more claimants than one `SKIP LOCKED` skip can explain and
/// more rows than one pass's batch of 32, so the claims overlap and none takes
/// everything. Three rounds, every sender every round.
#[tokio::test]
async fn many_senders_at_once_send_each_notification_once() {
    const SENDERS: usize = 6;
    const ROWS: usize = 48;

    let app = TestApp::spawn().await;
    let titles: Vec<String> = (0..ROWS).map(|n| format!("Crowd {n:02}")).collect();
    let ids = batch(&app, "ntf-lease-crowd", &titles).await;

    let mut senders = Vec::new();
    for n in 0..SENDERS {
        senders.push(sender(&app, &format!("kelir-lease-crowd-{n}")).await);
    }

    for _ in 0..3 {
        let running: Vec<Pass> = senders.iter().map(spawn_pass).collect();
        for (n, pass) in running.into_iter().enumerate() {
            finished(pass, &format!("sender {n}")).await;
        }
    }

    let everyone: Vec<&AppState> = senders.iter().collect();
    assert_eq!(
        emails_per_title(&everyone, &titles),
        vec![1; ROWS],
        "a notification was emailed more than once, or not at all"
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
        assert_eq!(attempts_for(&app, id).await, 1);
    }
}

/// **A row nobody has claimed — every row written before 0053 — is due, and a
/// soft-deleted one never is**, whatever its lease says.
///
/// The claim's due test is `lease_expires_at IS NULL OR <= now()`. Every row
/// the previous release wrote has a null lease, so dropping the `IS NULL` arm
/// would strand the whole backlog the upgrade inherits. A live lease is the
/// one row the claim must pass over.
#[tokio::test]
async fn a_never_claimed_row_is_due_and_a_soft_deleted_one_never_is() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-lease-due").await;

    let never = pending_notification(&app, person, "Never claimed").await;
    let lapsed = pending_notification(&app, person, "Lapsed").await;
    let live = pending_notification(&app, person, "Live").await;
    let deleted = pending_notification(&app, person, "Deleted").await;
    let deleted_lapsed = pending_notification(&app, person, "Deleted lapsed").await;

    sqlx::query(
        "UPDATE notifications SET lease_expires_at = now() - interval '1 minute'
         WHERE id = ANY($1)",
    )
    .bind(vec![lapsed, deleted_lapsed])
    .execute(&app.pool)
    .await
    .expect("lapsed leases");
    sqlx::query(
        "UPDATE notifications SET lease_expires_at = now() + interval '1 minute' WHERE id = $1",
    )
    .bind(live)
    .execute(&app.pool)
    .await
    .expect("a live lease");
    sqlx::query("UPDATE notifications SET deleted_at = now() WHERE id = ANY($1)")
        .bind(vec![deleted, deleted_lapsed])
        .execute(&app.pool)
        .await
        .expect("soft deletes");
    assert_eq!(lease_of(&app, never).await, None);

    let claimed = repository::claim_deliveries(&app.pool, 32, LEASE.as_secs_f64())
        .await
        .expect("the claim");
    let claimed: Vec<Uuid> = claimed.iter().map(|row| row.id).collect();

    assert_eq!(
        claimed,
        vec![never, lapsed],
        "the claim took the wrong rows, or in the wrong order"
    );
    assert!(lease_of(&app, never).await.is_some());
    assert_eq!(lease_of(&app, deleted).await, None);
}

/// **A settle names its tenant.** The claim reads across every tenant on
/// purpose, and the settle takes the row's tenant back as a predicate: the
/// right lease under another tenant's id moves nothing.
#[tokio::test]
async fn a_settle_names_its_tenant() {
    let app = TestApp::spawn().await;
    let other = fixtures::create_tenant(&app.pool, "TNT-853", "Lease Other Tenant").await;
    let ids = batch(&app, "ntf-lease-tenant", &titles()).await;

    let claimed = repository::claim_deliveries(&app.pool, 32, LEASE.as_secs_f64())
        .await
        .expect("the claim");
    assert_eq!(claimed[0].tenant_id, fixtures::SYSTEM_TENANT_ID);

    let moved =
        repository::mark_delivered(&app.pool, other, claimed[0].id, claimed[0].lease, "SENT")
            .await
            .expect("the cross-tenant settle");
    assert_eq!(moved, 0, "a settle under another tenant moved the row");
    assert_eq!(status_of(&app, ids[0]).await, "PENDING");
}

/// **A failed delivery settles under its own claim and is not handed back.**
///
/// The design (0053's column comment, `deliver_one`): a send that fails is
/// recorded as a `FAILED` attempt and the row settles `FAILED`, under the
/// lease it was claimed with, which is left as it was. It is not released for
/// another sender to retry: once that lease passes, no claim takes it again.
#[tokio::test]
async fn a_failed_delivery_settles_under_its_claim_and_is_not_claimed_again() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-lease-failed").await;
    sqlx::query(
        "INSERT INTO notification_channels
            (id, tenant_id, channel_code, channel_type, is_enabled)
         VALUES ($1, $2, 'SMS', 'SMS', true)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("a channel this build cannot send on");
    let id = pending_notification(&app, person, "Fails on SMS").await;

    app.deliver_notifications().await;

    assert_eq!(status_of(&app, id).await, "FAILED");
    let settled_lease = lease_of(&app, id).await;
    assert!(settled_lease.is_some(), "the delivery was not claimed");

    sqlx::query(
        "UPDATE notifications SET lease_expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(id)
    .execute(&app.pool)
    .await
    .expect("the lease passes");

    let again = repository::claim_deliveries(&app.pool, 32, LEASE.as_secs_f64())
        .await
        .expect("the next claim");
    assert!(again.is_empty(), "a failed delivery was claimed again");

    app.deliver_notifications().await;
    assert_eq!(app.sent_mail().len(), 1, "a failed delivery was sent again");
    assert_eq!(
        attempts_for(&app, id).await,
        2,
        "one EMAIL and one SMS attempt"
    );
}

/// **A pass that errors mid-batch loses nothing it claimed**: the rows it did
/// not reach stay `PENDING` under its lease, no sender takes them while the
/// lease lives, and the next claim after it passes delivers them.
///
/// The error is a write that fails after an email went out — the log insert
/// for row one, refused by a trigger — so row one is also the crash-between-
/// send-and-settle case: at-least-once, sent again after the lease. That is
/// the duplicate the claim's comment names, pinned so it cannot quietly grow.
#[tokio::test]
async fn a_pass_that_errors_mid_batch_leaves_its_unreached_rows_to_the_lease() {
    let app = TestApp::spawn().await;
    let titles = titles();
    let ids = batch(&app, "ntf-lease-error", &titles).await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    execute(
        &app,
        &format!(
            "CREATE FUNCTION test_refuse_first_log() RETURNS trigger
             LANGUAGE plpgsql AS $$
             BEGIN
                 IF NEW.notification_id = '{}' THEN
                     RAISE EXCEPTION 'test: the log write fails';
                 END IF;
                 RETURN NEW;
             END
             $$",
            ids[0]
        ),
    )
    .await;
    execute(
        &app,
        "CREATE TRIGGER test_refuse_first_log BEFORE INSERT ON notification_logs
         FOR EACH ROW EXECUTE FUNCTION test_refuse_first_log()",
    )
    .await;

    let failed = worker::pass(&first).await;
    assert!(
        failed.is_err(),
        "the refused log write did not fail the pass"
    );
    assert_eq!(first.mailer.captured_messages().len(), 1);

    worker::pass(&second).await.expect("the second pass");
    assert!(
        second.mailer.captured_messages().is_empty(),
        "a sender took rows another's live lease holds"
    );
    for id in &ids {
        assert_eq!(status_of(&app, *id).await, "PENDING");
    }

    execute(
        &app,
        "DROP TRIGGER test_refuse_first_log ON notification_logs",
    )
    .await;
    expire_every_lease(&app).await;
    worker::pass(&second)
        .await
        .expect("the pass after the lease");

    assert_eq!(
        emails_per_title(&[&first, &second], &titles),
        vec![2, 1, 1],
        "an unreached row was lost or doubled, or the sent-unsettled row was lost"
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
    }
}

/// **The claim reads the partial index 0053 built for it.**
///
/// The index's predicate must be implied by the claim's `WHERE`, or the
/// planner cannot use it and the claim reads every notification ever sent.
/// The claim's statement is restated here as `repository::claim_deliveries`
/// sends it; sequential scans are turned off so the plan shows whether the
/// index is usable at all, which is the property, rather than whether it is
/// cheaper on a near-empty table.
#[tokio::test]
async fn the_claim_reads_the_partial_index_its_predicate_names() {
    let app = TestApp::spawn().await;

    let (definition, predicate): (String, Option<String>) = sqlx::query_as(
        "SELECT pg_get_indexdef(i.indexrelid), pg_get_expr(i.indpred, i.indrelid)
         FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid
         WHERE c.relname = 'idx_notifications_created_at_id'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("0053's index");
    assert!(
        definition.contains("(created_at, id)"),
        "the index is not in the claim's order: {definition}"
    );
    let predicate = predicate.expect("the index is partial");
    assert!(
        predicate.contains("'PENDING'") && predicate.contains("deleted_at IS NULL"),
        "the index's predicate is not the claim's: {predicate}"
    );

    let mut tx = app.pool.begin().await.expect("a transaction");
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *tx)
        .await
        .expect("no sequential scans");
    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN
         WITH claimed AS (
             UPDATE notifications n
             SET lease_expires_at = now() + make_interval(secs => 300)
             FROM (
                 SELECT id
                 FROM notifications
                 WHERE status = 'PENDING' AND deleted_at IS NULL
                   AND (lease_expires_at IS NULL OR lease_expires_at <= now())
                 ORDER BY created_at, id
                 LIMIT 32
                 FOR UPDATE SKIP LOCKED
             ) due
             WHERE n.id = due.id
             RETURNING n.id
         )
         SELECT id FROM claimed",
    )
    .fetch_all(&mut *tx)
    .await
    .expect("the claim's plan");
    tx.rollback().await.expect("rolls back");

    let plan = plan.join("\n");
    assert!(
        plan.contains("idx_notifications_created_at_id"),
        "the claim cannot use 0053's index:\n{plan}"
    );
}

/// **A pass stops starting sends when its lease runs short**, and leaves the
/// rest of its batch to the next claim (`worker::pass`'s `may_begin_send`
/// `break`).
///
/// The first send takes one second longer than `LEASE - SEND_ALLOWANCE`, so
/// the pass reaches its second row with less than `SEND_ALLOWANCE` of its lease
/// left. It must send nothing more: rows two and three stay `PENDING` under
/// the lease it wrote, no other sender takes them while it lives, and once it
/// passes another sender delivers each once. Without the `break` the pass
/// would send all three, the last two each starting with the lease nearly or
/// wholly gone.
///
/// **Three minutes and a second of wall clock**, because neither the lease
/// nor the clock `pass` reads can be injected (the module comment).
#[tokio::test]
async fn a_pass_stops_starting_sends_when_its_lease_runs_short() {
    let slow_send = LEASE - SEND_ALLOWANCE + Duration::from_secs(1);
    let app = TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(slow_send)).await;
    let titles = titles();
    let ids = batch(&app, "ntf-lease-short", &titles).await;

    app.deliver_notifications().await;

    let sent: Vec<String> = app
        .sent_mail()
        .into_iter()
        .map(|mail| mail.subject)
        .collect();
    assert_eq!(
        sent,
        vec![titles[0].clone()],
        "the pass started a send with less than SEND_ALLOWANCE of its lease left"
    );
    assert_eq!(status_of(&app, ids[0]).await, "SENT");
    for id in &ids[1..] {
        assert_eq!(status_of(&app, *id).await, "PENDING");
        assert_eq!(attempts_for(&app, *id).await, 0);
    }

    let still_live: bool = sqlx::query_scalar(
        "SELECT bool_and(lease_expires_at > now()) FROM notifications WHERE id = ANY($1)",
    )
    .bind(&ids[1..])
    .fetch_one(&app.pool)
    .await
    .expect("the unreached rows' leases");
    assert!(still_live, "the pass gave its unreached rows back early");

    let next = sender(&app, SECOND).await;
    worker::pass(&next).await.expect("a pass inside the lease");
    assert!(
        next.mailer.captured_messages().is_empty(),
        "another sender took rows a live lease holds"
    );

    expire_every_lease(&app).await;
    worker::pass(&next).await.expect("a pass after the lease");

    assert_eq!(
        emails_per_title(&[&app.state, &next], &titles),
        vec![1, 1, 1]
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
    }
}
