//! Two senders over one batch send each notification once ([#853]; NFR-SCA-001,
//! NFR-SCA-003).
//!
//! # Two processes, as a test can have them
//!
//! **A sender here is an `AppState` of its own**: a pool of its own, opened
//! with an `application_name` of its own, and a captured mailer of its own.
//! Nothing in memory is shared between the two, so the only thing they agree
//! on is what the database tells them, which is all two backends agree on.
//! Each runs `notification::worker::pass`, the loop's work with the sleep
//! taken out.
//!
//! # Staged, not hoped for
//!
//! **A `BEFORE INSERT` trigger on `notification_logs` holds the first sender**
//! on an advisory lock the test holds, and only the first: it reads the
//! connection's `application_name`. A delivery's log row is written after its
//! mail is sent and before the notification is settled, so the first sender is
//! held at the widest point of its window, with an email out and the row not
//! yet moved. The second sender's pass runs to its end inside that window, and
//! the gate then opens. It is `auth_session.rs`'s insert gate (ADR-0048),
//! narrowed to one connection's name.
//!
//! # The control, and red first
//!
//! **The first test was committed before the claim**, where the unclaimed read
//! gives 2 emails for each of the three. The control keeps that reading in the
//! suite:
//! the same staging with the leases cleared mid-send, which puts each row back
//! as the old unclaimed read left it, reads 2 each, so the 1s are the claim's
//! and not the staging's.
//!
//! [#853]: https://github.com/sujanto-gaws/kelir/issues/853

mod common;

use std::time::Duration;

use kelir_backend::mail::Mailer;
use kelir_backend::modules::notification::domain::LEASE;
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

/// The advisory key the log gate waits on: the issue's number.
const LOG_GATE: i64 = 853;

/// The first sender's `application_name`, which is the one the gate holds.
const FIRST: &str = "kelir-sender-first";

/// The second sender's, which the gate lets through.
const SECOND: &str = "kelir-sender-second";

/// One batch, told apart in a mailbox by subject: the seeded `TASK_ASSIGNED`
/// email template's subject is the notification's title.
const TITLES: [&str; 3] = ["Notice one", "Notice two", "Notice three"];

/// A first sender's pass, running in the background.
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

/// A `PENDING` notification of the shape `service::notify` writes.
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

/// [`TITLES`], one notification each, all to one person.
async fn batch(app: &TestApp, username: &str) -> Vec<Uuid> {
    let person = recipient(app, username).await;
    let mut ids = Vec::new();

    for title in TITLES {
        ids.push(pending_notification(app, person, title).await);
    }

    ids
}

/// One sender: a pool and a mailer of its own over the test's database.
async fn sender(app: &TestApp, name: &str) -> AppState {
    let options = app.pool.connect_options();
    let options = (*options).clone().application_name(name);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("the sender's pool");

    AppState::with_mailer(pool, (*app.state.config).clone(), Mailer::captured())
}

/// The first sender's pass, in the background, where the gate can hold it.
fn first_pass(first: &AppState) -> Pass {
    let first = first.clone();

    tokio::spawn(async move { worker::pass(&first).await })
}

/// Waits for a pass the gate held, now let through.
async fn finished(running: Pass) {
    tokio::time::timeout(PATIENCE, running)
        .await
        .expect("the first sender finished")
        .expect("the first sender's task")
        .expect("the first sender's pass");
}

/// How many emails each of [`TITLES`] was sent as, across both mailers.
fn emails_per_notification(first: &AppState, second: &AppState) -> Vec<usize> {
    let mut sent = first.mailer.captured_messages();
    sent.extend(second.mailer.captured_messages());

    TITLES
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

async fn execute(app: &TestApp, statement: &str) {
    sqlx::query(statement)
        .execute(&app.pool)
        .await
        .expect(statement);
}

/// A `BEFORE INSERT` trigger on `notification_logs` that waits on
/// [`LOG_GATE`] for the [`FIRST`] sender's connections only, held by a
/// connection of the test's own until [`LogGate::open`].
struct LogGate {
    connection: PgConnection,
}

impl LogGate {
    async fn close(app: &TestApp) -> Self {
        execute(
            app,
            &format!(
                "CREATE FUNCTION test_hold_first_sender() RETURNS trigger
                 LANGUAGE plpgsql AS $$
                 BEGIN
                     IF current_setting('application_name') = '{FIRST}' THEN
                         PERFORM pg_advisory_xact_lock_shared({LOG_GATE});
                     END IF;
                     RETURN NEW;
                 END
                 $$"
            ),
        )
        .await;
        execute(
            app,
            "CREATE TRIGGER test_hold_first_sender BEFORE INSERT ON notification_logs
             FOR EACH ROW EXECUTE FUNCTION test_hold_first_sender()",
        )
        .await;

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the gate's connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(LOG_GATE)
            .execute(&mut connection)
            .await
            .expect("closes the gate");

        LogGate { connection }
    }

    /// Returns once the first sender waits at the gate.
    async fn holds(&self, app: &TestApp, running: &Pass) {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            let held: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = current_database() AND application_name = $1
                   AND wait_event_type = 'Lock'",
            )
            .bind(FIRST)
            .fetch_one(&app.pool)
            .await
            .expect("reads pg_stat_activity");
            if held == 1 {
                return;
            }
            assert!(!running.is_finished(), "the first sender was not held");
            assert!(
                tokio::time::Instant::now() < deadline,
                "the first sender was not held"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn open(mut self) {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(LOG_GATE)
            .execute(&mut self.connection)
            .await
            .expect("opens the gate");
        self.connection
            .close()
            .await
            .expect("closes the gate's connection");
    }
}

/// **The trace's criterion: two senders in two processes, against one batch,
/// send each notification exactly once.**
///
/// The first sender has sent the first email and is held before it settles
/// the row. The second sender's whole pass runs then. Before #853 the batch
/// was read with no claim, so the second read all three as `PENDING` and sent
/// them, and the first, let through, sent the two it had read and not yet
/// reached: two emails each.
#[tokio::test]
async fn two_senders_over_one_batch_send_each_notification_once() {
    let app = TestApp::spawn().await;
    let ids = batch(&app, "ntf-claim-once").await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    let gate = LogGate::close(&app).await;
    let running = first_pass(&first);
    gate.holds(&app, &running).await;
    assert_eq!(
        first.mailer.captured_messages().len(),
        1,
        "the first sender was held before its first email, not after it"
    );

    worker::pass(&second).await.expect("the second pass");

    gate.open().await;
    finished(running).await;

    assert_eq!(
        emails_per_notification(&first, &second),
        vec![1, 1, 1],
        "a notification was emailed more than once, or not at all"
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
    }
}

/// **The control: the same staging, with the claim taken back mid-send, reads
/// 2.**
///
/// While the first sender is held, the batch's leases are cleared, which puts
/// each row back as the unclaimed read before #853 left it. The second sender
/// then claims and sends all three, and the first, let through, sends the two
/// it had not reached. Without this, the test above could read 1 because the
/// staging never let two sends happen at all.
#[tokio::test]
async fn the_control_a_row_put_back_mid_send_is_emailed_twice() {
    let app = TestApp::spawn().await;
    let ids = batch(&app, "ntf-claim-control").await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    let gate = LogGate::close(&app).await;
    let running = first_pass(&first);
    gate.holds(&app, &running).await;

    execute(&app, "UPDATE notifications SET lease_expires_at = NULL").await;
    worker::pass(&second).await.expect("the second pass");

    gate.open().await;
    finished(running).await;

    assert_eq!(
        emails_per_notification(&first, &second),
        vec![2, 2, 2],
        "the staging did not let two senders send, so its 1s prove nothing"
    );
    // The second claim settled each row, and the first, overtaken, settled none.
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
    }
}

/// **A sender that crashed holding a batch holds it only until its lease
/// passes**, and the next claim takes it again.
///
/// The crash is a claim that is never settled: the first sender claims the
/// batch and does nothing more, which is all a process killed after its claim
/// leaves in the database. Its lease is zero seconds long, so it has passed by
/// the time the second sender claims.
#[tokio::test]
async fn a_lease_left_by_a_crashed_sender_is_claimed_again() {
    let app = TestApp::spawn().await;
    let ids = batch(&app, "ntf-claim-crash").await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    let held = repository::claim_deliveries(&first.pool, 32, 0.0)
        .await
        .expect("the crashed sender's claim");
    assert_eq!(held.len(), 3, "the crashed sender claimed the batch");

    worker::pass(&second).await.expect("the second pass");

    assert_eq!(
        emails_per_notification(&first, &second),
        vec![1, 1, 1],
        "a row a crashed sender held was lost, or sent twice"
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "SENT");
    }
}

/// **A lease that has not passed is not claimed**, even from a sender that
/// will never come back: it is only the lease's end that hands the row on.
#[tokio::test]
async fn a_live_lease_is_not_claimed_by_a_second_sender() {
    let app = TestApp::spawn().await;
    let ids = batch(&app, "ntf-claim-live").await;
    let first = sender(&app, FIRST).await;
    let second = sender(&app, SECOND).await;

    let held = repository::claim_deliveries(&first.pool, 32, LEASE.as_secs_f64())
        .await
        .expect("the first claim");
    assert_eq!(held.len(), 3);

    worker::pass(&second).await.expect("the second pass");

    assert!(
        second.mailer.captured_messages().is_empty(),
        "the second sender sent a notification the first one holds"
    );
    for id in ids {
        assert_eq!(status_of(&app, id).await, "PENDING");
    }
}

/// **A sender that outlived its lease cannot settle the row a newer claim
/// holds** — `mark_delivered` matches on the lease, as the outbox's settle
/// does. The row is `PENDING` throughout, so the status predicate alone would
/// have let the late sender through.
#[tokio::test]
async fn a_sender_overtaken_after_its_lease_cannot_settle_the_row() {
    let app = TestApp::spawn().await;
    let ids = batch(&app, "ntf-claim-fence").await;

    let late = repository::claim_deliveries(&app.pool, 32, 0.0)
        .await
        .expect("the late sender's claim");
    let current = repository::claim_deliveries(&app.pool, 32, LEASE.as_secs_f64())
        .await
        .expect("the newer claim");
    assert_eq!(current.len(), 3, "a passed lease was not claimed again");

    let moved = repository::mark_delivered(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        late[0].id,
        late[0].lease,
        "FAILED",
    )
    .await
    .expect("the late settle");
    assert_eq!(moved, 0, "a sender settled a row a newer claim holds");
    assert_eq!(status_of(&app, ids[0]).await, "PENDING");

    let moved = repository::mark_delivered(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        current[0].id,
        current[0].lease,
        "SENT",
    )
    .await
    .expect("the current settle");
    assert_eq!(moved, 1, "the claim that holds the row could not settle it");
    assert_eq!(status_of(&app, ids[0]).await, "SENT");
}

/// **The claim locks `notifications` and nothing else.** A recipient whose
/// `users` row another transaction holds `FOR UPDATE` — a user edit in flight
/// — is still claimed and emailed: the address is joined in a plain read after
/// the claim, so a sign-in or an edit never waits on a sender, nor a sender on
/// them.
#[tokio::test]
async fn a_claim_takes_no_lock_on_the_recipients_row() {
    let app = TestApp::spawn().await;
    batch(&app, "ntf-claim-lock").await;

    let mut holder = app.pool.begin().await.expect("the holder's transaction");
    sqlx::query("SELECT id FROM users WHERE email = $1 FOR UPDATE")
        .bind("ntf-claim-lock@example.test")
        .fetch_one(&mut *holder)
        .await
        .expect("holds the recipient's row");

    tokio::time::timeout(Duration::from_secs(10), worker::pass(&app.state))
        .await
        .expect("the pass waited on the recipient's row")
        .expect("the pass");
    holder.rollback().await.expect("releases the row");

    assert_eq!(app.sent_mail().len(), 3);
}
