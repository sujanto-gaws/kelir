//! The stop at its edges ([#856]; NFR-AVA-003, NFR-SCA-003): what
//! `graceful_shutdown.rs` leaves unstaged.
//!
//! # Why a second file
//!
//! `graceful_shutdown.rs` is the builder's: one request in flight, its reset
//! mail, and the workers quiet after `serve` returns. **This file is the
//! independent campaign over the same stop**, and each test names the criterion
//! it proves:
//!
//! | Criterion | Test |
//! |---|---|
//! | A worker in the middle of a pass at the stop finishes it, and takes no new work | *a notification pass in flight at the stop finishes before serve returns* |
//! | Work past the deadline is aborted, not waited on; leased rows are not marked delivered and come back | *a pass the deadline overtakes is abandoned and its leased rows come back* |
//! | A slow request does not hold `serve` past the deadline, and new connections stay refused | *a request that outlives the deadline is not waited for* |
//! | What the deadline overtakes is aborted, not left running | *a request the deadline abandoned is not served after serve returns*, *a detached send the deadline overtakes is aborted* |
//! | Detached mail is awaited, up to the deadline and no further | *serve waits for a detached send until the deadline and no longer* |
//! | Detached mail spawned during the drain is awaited | *a send handed off during the drain is awaited* |
//! | An idle connection does not hold the drain, and nothing connects after | *an idle keep alive connection is closed at the stop* |
//! | The binary answers SIGTERM: a request in flight completes, exit 0, inside Docker's ten seconds | *the binary finishes a request in flight at sigterm and exits zero* |
//! | A second stop signal does not kill the drain | *a second sigterm does not cut the drain short* |
//! | Ctrl-C stops the binary as SIGTERM does | *sigint stops the binary cleanly* |
//!
//! **The process tests run the built binary** (`CARGO_BIN_EXE_kelir-backend`)
//! over the harness's database and send it a real signal with `kill(1)`, because
//! #856's defect was the binary, PID 1 in its container, not answering SIGTERM,
//! and `serve` driven through a oneshot cannot see that.
//!
//! [#856]: https://github.com/sujanto-gaws/kelir/issues/856

mod common;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use kelir_backend::mail::{Mail, Mailer};
use kelir_backend::modules::notification::worker;
use kelir_backend::server;
use kelir_backend::state::AppState;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use common::{fixtures, TestApp};

/// How long a staged step may take before the test says which one did not
/// happen.
const PATIENCE: Duration = Duration::from_secs(10);

/// Docker's default stop timeout, after which it kills.
const DOCKER_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// How far past [`server::GRACE`] a stop the deadline ended may return: the
/// aborts, the tracker and the workers' joins, none of which waits.
const MARGIN: Duration = Duration::from_millis(750);

/// How far before [`server::GRACE`] a stop that waited for the deadline may
/// return. It cannot return early at all; this is clock slack.
const SLACK: Duration = Duration::from_millis(250);

/// A `serve` running over a loopback port.
struct Serving {
    stop: oneshot::Sender<()>,
    address: SocketAddr,
    task: JoinHandle<std::io::Result<()>>,
}

/// Starts [`server::serve`], the function `main.rs` runs, with a stop the
/// test completes.
async fn serve(app: &TestApp) -> Serving {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");

    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(server::serve(listener, app.state.clone(), async {
        let _ = stopped.await;
    }));

    Serving {
        stop,
        address,
        task,
    }
}

/// Waits for `serve` to return, at most Docker's ten seconds, and says how long
/// it took since `since`.
async fn returned(task: JoinHandle<std::io::Result<()>>, since: Instant) -> Duration {
    let outcome = tokio::time::timeout(DOCKER_STOP_TIMEOUT, task).await;
    let took = since.elapsed();

    outcome
        .unwrap_or_else(|_| {
            panic!(
                "serve had not returned {DOCKER_STOP_TIMEOUT:?} after the stop, when Docker kills"
            )
        })
        .expect("serve did not panic")
        .expect("serve stopped cleanly");

    took
}

/// Reads one response head, interim or final, up to its blank line.
async fn read_head(stream: &mut TcpStream, what: &str) -> String {
    let reading = async {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];

        while !head.ends_with(b"\r\n\r\n") {
            let read = stream.read(&mut byte).await.expect("the socket reads");

            assert_ne!(
                read,
                0,
                "the connection closed before {what}; read so far: {:?}",
                String::from_utf8_lossy(&head)
            );

            head.push(byte[0]);
        }

        String::from_utf8_lossy(&head).into_owned()
    };

    tokio::time::timeout(PATIENCE, reading)
        .await
        .unwrap_or_else(|_| panic!("no {what} within {PATIENCE:?}"))
}

/// Everything the peer sends until it closes, or `None` if it is still open
/// after `within`. A reset counts as closed.
async fn read_until_closed(stream: &mut TcpStream, within: Duration) -> Option<String> {
    let mut seen = Vec::new();
    let outcome = tokio::time::timeout(within, stream.read_to_end(&mut seen)).await;

    match outcome {
        Ok(_) => Some(String::from_utf8_lossy(&seen).into_owned()),
        Err(_) => None,
    }
}

/// Whether the server stops accepting connections within [`PATIENCE`].
async fn stops_accepting(address: SocketAddr) -> bool {
    let deadline = Instant::now() + PATIENCE;

    while Instant::now() < deadline {
        if TcpStream::connect(address).await.is_err() {
            return true;
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    false
}

/// Whether every connection attempt over `during` is refused.
async fn stays_refusing(address: SocketAddr, during: Duration) -> bool {
    let until = Instant::now() + during;

    while Instant::now() < until {
        if TcpStream::connect(address).await.is_ok() {
            return false;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    true
}

/// A reset request the server is inside of and has not been given the body of:
/// the head goes with `Expect: 100-continue`, and hyper answers `100 Continue`
/// only once the handler reads the body. Returns the connection and the body
/// still to send. The username names nobody, so no reset mail follows.
async fn held_request(address: SocketAddr) -> (TcpStream, String) {
    let body = json!({ "username": "nobody.at.all" }).to_string();
    let mut request = TcpStream::connect(address).await.expect("a connection");

    request
        .write_all(
            format!(
                "POST /api/v1/auth/forgot-password HTTP/1.1\r\n\
                 Host: kelir.test\r\n\
                 Content-Type: application/json\r\n\
                 Content-Length: {}\r\n\
                 Expect: 100-continue\r\n\
                 \r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .expect("the request's head");

    let interim = read_head(&mut request, "the 100 Continue").await;
    assert!(
        interim.starts_with("HTTP/1.1 100"),
        "control: the server did not take up the request before the stop: {interim:?}"
    );

    (request, body)
}

/// A person with an address, which the seeded `EMAIL` channel mails.
async fn recipient(app: &TestApp, username: &str) -> Uuid {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[],
    )
    .await
}

/// A `PENDING` notification of the shape `service::notify` writes. The seeded
/// `TASK_ASSIGNED` email template's subject is the title, so a mailbox tells
/// them apart.
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

/// `count` notifications to one person, oldest first.
async fn notifications(app: &TestApp, recipient: Uuid, stem: &str, count: usize) -> Vec<Uuid> {
    let mut ids = Vec::new();

    for n in 1..=count {
        ids.push(pending_notification(app, recipient, &format!("{stem} {n}")).await);
    }

    ids
}

/// What a sender has left of one notification.
#[derive(Debug, sqlx::FromRow)]
struct Held {
    title: String,
    status: String,
    /// Any lease at all, live or passed.
    leased: bool,
    /// A lease with more than four of its five minutes still to run.
    lease_live: bool,
}

async fn held(app: &TestApp, ids: &[Uuid]) -> Vec<Held> {
    sqlx::query_as::<_, Held>(
        "SELECT title, status,
                lease_expires_at IS NOT NULL AS leased,
                COALESCE(lease_expires_at > now() + interval '4 minutes', false) AS lease_live
         FROM notifications WHERE id = ANY($1) ORDER BY created_at, id",
    )
    .bind(ids.to_vec())
    .fetch_all(&app.pool)
    .await
    .expect("the notifications")
}

/// The subjects a mailer holds, sorted.
fn subjects(mailer: &Mailer) -> Vec<String> {
    let mut subjects: Vec<String> = mailer
        .captured_messages()
        .into_iter()
        .map(|mail| mail.subject)
        .collect();
    subjects.sort();
    subjects
}

/// Returns once the captured mailer holds a message.
async fn first_mail(app: &TestApp) {
    let deadline = Instant::now() + PATIENCE;

    while app.sent_mail().is_empty() {
        assert!(
            Instant::now() < deadline,
            "control: the notification sender serve started sent nothing within {PATIENCE:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A mail handed to the runtime the way the reset flow hands one.
fn detached_mail(app: &TestApp, subject: &str) {
    app.state.mailer.send_detached(
        &app.state.detached,
        Mail {
            to: "detached@example.test".to_owned(),
            subject: subject.to_owned(),
            body: "Handed off".to_owned(),
        },
    );
}

// ---------------------------------------------------------------------------
// The workers
// ---------------------------------------------------------------------------

/// **A pass under way at the stop is finished, not dropped, and no new work is
/// taken after the stop** (#856: each worker finishes the pass it is in).
///
/// Four notifications at 700 ms a send are one pass of about three seconds;
/// the stop lands after the first is mailed. A row written during the drain is
/// one no pass claims.
#[tokio::test]
async fn a_notification_pass_in_flight_at_the_stop_finishes_before_serve_returns() {
    let app =
        TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(Duration::from_millis(700)))
            .await;
    let person = recipient(&app, "user.midpass").await;
    let ids = notifications(&app, person, "Mid-pass", 4).await;

    let serving = serve(&app).await;

    // Control: the pass is under way, one row mailed and three to go.
    first_mail(&app).await;
    assert!(
        held(&app, &ids)
            .await
            .iter()
            .any(|row| row.status == "PENDING" && row.leased),
        "control: the pass had finished before the stop, so it was not in flight"
    );

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");

    let late = pending_notification(&app, person, "Written during the drain").await;

    let took = returned(serving.task, stopping).await;
    assert!(
        took < server::GRACE,
        "the stop took {took:?}, so the pass in flight was cut at the deadline rather than finished"
    );

    let rows = held(&app, &ids).await;
    assert!(
        rows.iter().all(|row| row.status == "SENT"),
        "the pass in flight at the stop did not finish what it had claimed: {rows:?}"
    );
    assert_eq!(
        subjects(&app.state.mailer),
        vec!["Mid-pass 1", "Mid-pass 2", "Mid-pass 3", "Mid-pass 4"],
        "each notification of the pass in flight is mailed once"
    );

    let late = held(&app, &[late]).await;
    assert!(
        late.len() == 1 && late[0].status == "PENDING" && !late[0].leased,
        "a worker took new work after the stop: {late:?}"
    );
}

/// **A pass the deadline overtakes is abandoned at the deadline, and what it
/// held is neither marked delivered nor lost** (#856; ADR-0034, migration
/// 0053's lease).
///
/// Eight notifications at two seconds a send are a sixteen-second pass; the
/// stop lands a second after the first is mailed, so the deadline falls in the
/// middle of a send. `serve` returns at the deadline, not when the pass would
/// have. The rows the pass did not settle are `PENDING` under a lease with most
/// of its five minutes to run, so another sender does not take them yet; once
/// the lease passes, the next sender does, and mails each one.
#[tokio::test]
async fn a_pass_the_deadline_overtakes_is_abandoned_and_its_leased_rows_come_back() {
    let app =
        TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(Duration::from_secs(2))).await;
    let person = recipient(&app, "user.overtaken").await;
    let ids = notifications(&app, person, "Overtaken", 8).await;

    let serving = serve(&app).await;

    first_mail(&app).await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");

    let took = returned(serving.task, stopping).await;
    assert!(
        took >= server::GRACE - SLACK,
        "control: the stop took {took:?}, under the {:?} deadline, so the pass did not outlive it",
        server::GRACE
    );
    assert!(
        took <= server::GRACE + MARGIN,
        "the stop took {took:?}: the pass the deadline overtook was waited on past {:?}",
        server::GRACE
    );

    let rows = held(&app, &ids).await;
    let mailed = subjects(&app.state.mailer);

    let unsettled: Vec<&Held> = rows.iter().filter(|row| row.status == "PENDING").collect();
    assert!(
        !unsettled.is_empty(),
        "control: the pass settled every row, so the deadline overtook nothing: {rows:?}"
    );
    assert!(
        rows.iter()
            .filter(|row| row.status == "SENT")
            .all(|row| mailed.contains(&row.title)),
        "a row was marked delivered that was never mailed: rows {rows:?}, mailed {mailed:?}"
    );
    assert!(
        rows.iter()
            .all(|row| row.status == "SENT" || row.status == "PENDING"),
        "the abandoned pass left a row neither delivered nor waiting: {rows:?}"
    );
    assert!(
        unsettled.iter().all(|row| row.lease_live),
        "a row the abandoned pass held is not under its live lease: {unsettled:?}"
    );

    let mut waiting: Vec<String> = unsettled.iter().map(|row| row.title.clone()).collect();
    waiting.sort();

    // The next process, while the lease runs: it takes nothing.
    let next = AppState::with_mailer(
        app.serving_pool.clone(),
        (*app.state.config).clone(),
        Mailer::captured(),
    );
    worker::pass(&next).await.expect("the next sender's pass");
    assert!(
        next.mailer.captured_messages().is_empty(),
        "another sender took rows the abandoned pass still held under its lease"
    );

    // Five minutes later: the lease has passed, and the rows come back.
    sqlx::query(
        "UPDATE notifications SET lease_expires_at = now() - interval '1 second'
         WHERE id = ANY($1) AND status = 'PENDING'",
    )
    .bind(ids.clone())
    .execute(&app.pool)
    .await
    .expect("the leases pass");

    worker::pass(&next).await.expect("the next sender's pass");

    let rows = held(&app, &ids).await;
    assert!(
        rows.iter().all(|row| row.status == "SENT"),
        "the rows the abandoned pass held did not come back once its lease passed: {rows:?}"
    );
    assert_eq!(
        subjects(&next.mailer),
        waiting,
        "the next sender mails exactly what the abandoned pass left"
    );
}

// ---------------------------------------------------------------------------
// The requests
// ---------------------------------------------------------------------------

/// **A request that outlives the deadline is not waited on**, and the listener
/// stays closed while it is held (#856: work past the deadline is aborted;
/// new connections are refused after the stop).
#[tokio::test]
async fn a_request_that_outlives_the_deadline_is_not_waited_for() {
    let app = TestApp::spawn().await;
    let serving = serve(&app).await;
    let address = serving.address;

    let (_request, _body) = held_request(address).await;

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");

    assert!(
        stops_accepting(address).await,
        "the server was told to stop and went on accepting connections for {PATIENCE:?}"
    );
    assert!(
        stays_refusing(address, Duration::from_secs(1)).await,
        "a connection was accepted during the drain, after the listener had refused one"
    );

    let took = returned(serving.task, stopping).await;
    assert!(
        took >= server::GRACE - SLACK,
        "control: the stop took {took:?}, under the {:?} deadline, so it did not wait for the \
         held request at all",
        server::GRACE
    );
    assert!(
        took <= server::GRACE + MARGIN,
        "the stop took {took:?}: a request that outlived the {:?} deadline was waited on",
        server::GRACE
    );

    assert!(
        TcpStream::connect(address).await.is_err(),
        "a connection was accepted after serve returned"
    );
}

/// **A request the deadline abandoned is dropped, not left running** (#856:
/// whatever is left at the deadline is aborted; `server.rs`: "A request still
/// unanswered is dropped").
///
/// `serve` aborts the task that runs axum's `serve`, but axum runs each
/// connection on a `tokio::spawn` of its own, which that abort does not reach.
/// The connection is then still open after `serve` has returned and logged
/// `stopped`, and a body sent on it now is answered.
#[tokio::test]
async fn a_request_the_deadline_abandoned_is_not_served_after_serve_returns() {
    let app = TestApp::spawn().await;
    let serving = serve(&app).await;

    let (mut request, body) = held_request(serving.address).await;

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");
    returned(serving.task, stopping).await;

    if read_until_closed(&mut request, Duration::from_secs(1))
        .await
        .is_none()
    {
        let _ = request.write_all(body.as_bytes()).await;
        let answered = read_until_closed(&mut request, Duration::from_secs(3)).await;

        panic!(
            "the request the deadline abandoned still had its connection open after serve \
             returned; given its body now, it answered: {answered:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// The detached sends
// ---------------------------------------------------------------------------

/// **A detached send is waited for until the deadline, and not past it**
/// (#856: detached mail is awaited; what the deadline overtakes is not waited
/// on).
#[tokio::test]
async fn serve_waits_for_a_detached_send_until_the_deadline_and_no_longer() {
    let app =
        TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(Duration::from_secs(12))).await;
    let serving = serve(&app).await;

    detached_mail(&app, "Slower than the stop");

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");

    let took = returned(serving.task, stopping).await;
    assert!(
        took >= server::GRACE - SLACK,
        "the stop took {took:?}: it did not wait for the detached send in hand",
    );
    assert!(
        took <= server::GRACE + MARGIN,
        "the stop took {took:?}: a detached send slower than the {:?} deadline was waited on",
        server::GRACE
    );
    assert!(
        app.sent_mail().is_empty(),
        "control: the twelve-second send finished inside the stop"
    );
}

/// **A detached send the deadline overtakes is aborted, not left running**
/// (#856: whatever is left at the deadline is aborted and logged).
///
/// `serve` stops waiting for `AppState::detached` at the deadline and logs
/// `abandoned`, but a `TaskTracker` does not cancel what it tracks: the send is
/// still running after `serve` has returned, and only the runtime's teardown
/// would end it.
#[tokio::test]
async fn a_detached_send_the_deadline_overtakes_is_aborted() {
    let app =
        TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(Duration::from_secs(12))).await;
    let serving = serve(&app).await;

    detached_mail(&app, "Slower than the stop");

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");
    returned(serving.task, stopping).await;

    assert!(
        app.state.detached.is_empty(),
        "serve returned with {} detached send(s) still running; the deadline abandoned them \
         without aborting them",
        app.state.detached.len()
    );
}

/// **A send handed off during the drain is awaited** (#856: the tracker is
/// closed only after the last request is answered, because a request still
/// being served can hand off one more).
///
/// A request is held open so the drain is under way; a send is handed off then,
/// and the request is let go. `serve` returns with the send delivered.
#[tokio::test]
async fn a_send_handed_off_during_the_drain_is_awaited() {
    let app =
        TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(Duration::from_millis(1500)))
            .await;
    let serving = serve(&app).await;
    let address = serving.address;

    let (mut request, body) = held_request(address).await;

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");
    assert!(
        stops_accepting(address).await,
        "control: the server was told to stop and went on accepting connections"
    );

    detached_mail(&app, "Handed off during the drain");

    request
        .write_all(body.as_bytes())
        .await
        .expect("the request's body, during the drain");
    let answer = read_head(&mut request, "the answer to the held request").await;
    assert!(
        answer.starts_with("HTTP/1.1 202"),
        "control: the held request was not answered during the drain: {answer:?}"
    );

    let took = returned(serving.task, stopping).await;
    assert!(
        took < server::GRACE,
        "the stop took {took:?}: a 1.5-second send was cut at the deadline"
    );
    assert_eq!(
        subjects(&app.state.mailer),
        vec!["Handed off during the drain"],
        "the send handed off during the drain was dropped by the stop"
    );
}

// ---------------------------------------------------------------------------
// The connections
// ---------------------------------------------------------------------------

/// **An idle keep-alive connection is closed at the stop and does not hold the
/// drain**; nothing connects after `serve` returns (#856: new connections are
/// refused after the stop).
#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed_at_the_stop() {
    let app = TestApp::spawn().await;
    let serving = serve(&app).await;
    let address = serving.address;

    let mut idle = TcpStream::connect(address).await.expect("a connection");
    idle.write_all(b"GET /health/live HTTP/1.1\r\nHost: kelir.test\r\n\r\n")
        .await
        .expect("the probe");
    let head = read_head(&mut idle, "the liveness answer").await;
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "control: the liveness probe was not answered: {head:?}"
    );

    let stopping = Instant::now();
    serving
        .stop
        .send(())
        .expect("serve is listening for its stop");

    assert!(
        read_until_closed(&mut idle, Duration::from_secs(3))
            .await
            .is_some(),
        "an idle keep-alive connection was still open three seconds after the stop"
    );

    let took = returned(serving.task, stopping).await;
    assert!(
        took < Duration::from_secs(3),
        "the stop took {took:?} with nothing in flight but an idle connection"
    );

    assert!(
        TcpStream::connect(address).await.is_err(),
        "a connection was accepted after serve returned"
    );
}

// ---------------------------------------------------------------------------
// The binary, and a real signal
// ---------------------------------------------------------------------------

/// The built `kelir-backend` over the harness's database.
#[cfg(unix)]
struct Binary {
    child: tokio::process::Child,
    address: SocketAddr,
}

/// Starts the binary `main.rs` builds, configured through its environment as a
/// container is, and returns once it accepts connections.
///
/// The database is the test's own, already migrated, narrowed and
/// bootstrapped, so the binary's migrations, narrowing and bootstrap find
/// nothing to do. The bootstrap and TLS
/// variables a developer's shell might carry are removed. Its log goes to the
/// test's output at `warn`, so a failure here shows why.
#[cfg(unix)]
async fn start_binary(app: &TestApp) -> Binary {
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("the bound address");
    // The two URLs a deployment gives it (ADR-0051 §2): the owner migrates and
    // narrows, and the binary serves as the harness's serving role.
    let owner_url = common::with_database(&common::server_url(), &app.database_name);
    let serving_url = common::as_the_serving_role(&owner_url);

    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kelir-backend"))
        .env("KELIR_APP_ENV", "test")
        .env("KELIR_JWT_SECRET", common::JWT_SECRET)
        .env("KELIR_DATABASE_URL", &serving_url)
        .env("KELIR_DATABASE_MIGRATION_URL", &owner_url)
        .env("KELIR_BIND_ADDRESS", address.to_string())
        .env("KELIR_NOTIFICATION_POLL_SECONDS", "1")
        .env("KELIR_CLAMAV_POLL_SECONDS", "1")
        .env("KELIR_LOG", "warn")
        .env_remove("KELIR_DATABASE_SSL_MODE")
        .env_remove("KELIR_DATABASE_SSL_ROOT_CERT")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_USERNAME")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_PASSWORD")
        .env_remove("KELIR_BOOTSTRAP_ADMIN_EMAIL")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the kelir-backend binary starts");

    let mut binary = Binary { child, address };
    let deadline = Instant::now() + PATIENCE;

    loop {
        if let Some(status) = binary.child.try_wait().expect("the binary's status") {
            panic!("the binary exited before it was listening: {status}");
        }

        if answers_liveness(address).await {
            return binary;
        }

        assert!(
            Instant::now() < deadline,
            "the binary was not listening on {address} within {PATIENCE:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Whether `GET /health/live` is answered `200`.
///
/// **An answer, not a connection**: the kernel completes a connection to a
/// bound socket before `serve` has started, and the Ctrl-C listener is
/// installed only once `serve`'s stop future is first polled. A request
/// answered means both have happened.
#[cfg(unix)]
async fn answers_liveness(address: SocketAddr) -> bool {
    let Ok(mut probe) = TcpStream::connect(address).await else {
        return false;
    };

    if probe
        .write_all(b"GET /health/live HTTP/1.1\r\nHost: kelir.test\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }

    read_until_closed(&mut probe, Duration::from_secs(2))
        .await
        .is_some_and(|answer| answer.starts_with("HTTP/1.1 200"))
}

/// Sends `signal` (`TERM`, `INT`) to the binary with `kill(1)`.
#[cfg(unix)]
async fn signal(binary: &Binary, signal: &str) {
    let pid = binary.child.id().expect("the binary is still running");
    let status = tokio::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .await
        .expect("kill(1) runs");

    assert!(status.success(), "kill -{signal} {pid} failed: {status}");
}

/// Waits for the binary to exit, at most Docker's ten seconds since `since`,
/// and asserts it exited 0.
#[cfg(unix)]
async fn exits_zero(binary: &mut Binary, since: Instant) {
    let outcome = tokio::time::timeout(DOCKER_STOP_TIMEOUT, binary.child.wait()).await;
    let took = since.elapsed();

    let status = outcome
        .unwrap_or_else(|_| {
            panic!("the binary was still running {DOCKER_STOP_TIMEOUT:?} after the signal, when Docker kills")
        })
        .expect("the binary's exit status");

    assert_eq!(
        status.code(),
        Some(0),
        "the binary did not exit cleanly on its stop signal: {status}"
    );
    assert!(
        took < DOCKER_STOP_TIMEOUT,
        "the binary took {took:?} to stop, past Docker's {DOCKER_STOP_TIMEOUT:?}"
    );
}

/// **Plan 22 row 5's wording: a request in flight at SIGTERM completes**, the
/// binary exits 0, and all of it inside Docker's ten seconds. The signal is a
/// real one, sent to the process `main.rs` runs.
#[cfg(unix)]
#[tokio::test]
async fn the_binary_finishes_a_request_in_flight_at_sigterm_and_exits_zero() {
    let app = TestApp::spawn().await;
    let mut binary = start_binary(&app).await;
    let address = binary.address;

    let (mut request, body) = held_request(address).await;

    let signalled = Instant::now();
    signal(&binary, "TERM").await;

    assert!(
        stops_accepting(address).await,
        "the binary was sent SIGTERM and went on accepting connections for {PATIENCE:?}"
    );

    request
        .write_all(body.as_bytes())
        .await
        .expect("the request's body, after SIGTERM");
    let answer = read_head(&mut request, "the answer to the request in flight").await;
    assert!(
        answer.starts_with("HTTP/1.1 202"),
        "the request in flight at SIGTERM was not answered: {answer:?}"
    );

    exits_zero(&mut binary, signalled).await;
}

/// **A second SIGTERM during the drain does not cut it short.** The first
/// signal's listener is gone by then; the second must still not reach the
/// default disposition, which would kill the process mid-request.
#[cfg(unix)]
#[tokio::test]
async fn a_second_sigterm_does_not_cut_the_drain_short() {
    let app = TestApp::spawn().await;
    let mut binary = start_binary(&app).await;
    let address = binary.address;

    let (mut request, body) = held_request(address).await;

    let signalled = Instant::now();
    signal(&binary, "TERM").await;
    assert!(
        stops_accepting(address).await,
        "control: the binary was sent SIGTERM and went on accepting connections"
    );

    signal(&binary, "TERM").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let early = binary.child.try_wait().expect("the binary's status");
    assert!(
        early.is_none(),
        "a second SIGTERM ended the binary mid-drain: {early:?}"
    );

    request
        .write_all(body.as_bytes())
        .await
        .expect("the request's body, after the second SIGTERM");
    let answer = read_head(&mut request, "the answer to the request in flight").await;
    assert!(
        answer.starts_with("HTTP/1.1 202"),
        "the request in flight was not answered after a second SIGTERM: {answer:?}"
    );

    exits_zero(&mut binary, signalled).await;
}

/// **Ctrl-C stops the binary as SIGTERM does** (`main.rs`: SIGTERM or Ctrl-C).
#[cfg(unix)]
#[tokio::test]
async fn sigint_stops_the_binary_cleanly() {
    let app = TestApp::spawn().await;
    let mut binary = start_binary(&app).await;

    let signalled = Instant::now();
    signal(&binary, "INT").await;

    exits_zero(&mut binary, signalled).await;
}
