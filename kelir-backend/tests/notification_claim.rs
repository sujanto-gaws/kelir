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
//! [#853]: https://github.com/sujanto-gaws/kelir/issues/853

mod common;

use std::time::Duration;

use kelir_backend::mail::Mailer;
use kelir_backend::modules::notification::worker;
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
