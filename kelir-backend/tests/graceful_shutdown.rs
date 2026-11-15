//! A stop is a stop, not a kill ([#856]; NFR-AVA-003, NFR-SCA-003).
//!
//! `docker stop` sends SIGTERM, waits ten seconds and kills. A backend that does
//! not answer the signal loses whatever request it was serving when the kill
//! lands, and the reset mail it had handed to the runtime with it. **This drives
//! [`server::serve`], the function `main.rs` runs**, over a real socket, and
//! completes the shutdown future `main.rs` completes on SIGTERM or Ctrl-C. No
//! signal is sent to the test process.
//!
//! # Why the request is a reset request with `Expect: 100-continue`
//!
//! Two things have to be true before the stop for the test to mean anything,
//! and each is seen rather than slept for:
//!
//! - **The server holds the request.** The headers go first, with
//!   `Expect: 100-continue`, and the body is held back. hyper answers
//!   `100 Continue` only when the handler starts reading the body, so that
//!   interim line is the server saying it has the request and is inside it.
//! - **The stop has begun while the request is unanswered.** The listener is
//!   closed once the server stops accepting, so a refused connection is the
//!   server saying it is shutting down. Only then is the body sent.
//!
//! `POST /auth/forgot-password` is the route because it also hands its mail to
//! the runtime rather than awaiting it (`Mailer::send_detached`), and that mail
//! is the other thing #856 says a stop loses. The mailer takes [`DELIVERY`]
//! per message, so a stop that does not wait for the send returns without it.
//!
//! [#856]: https://github.com/sujanto-gaws/kelir/issues/856

mod common;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use kelir_backend::mail::Mailer;
use kelir_backend::server;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use uuid::Uuid;

use common::{fixtures, TestApp};

/// How long one captured send takes: long enough that a stop which does not
/// wait for the reset mail returns visibly before it lands.
const DELIVERY: Duration = Duration::from_millis(1500);

/// How long a staged step may take before the test says which one did not
/// happen.
const PATIENCE: Duration = Duration::from_secs(10);

/// Docker's default stop timeout, after which it kills. The whole stop has to
/// fit inside it.
const DOCKER_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Longer than two of the outbox worker's two-second polls and four of the
/// sender's one-second polls (`notification_poll_seconds` in the harness).
const QUIET: Duration = Duration::from_millis(4500);

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

/// A `PENDING` outbox event of a type this build does not consume, which the
/// outbox worker settles `PROCESSED` on its next pass.
async fn pending_event(app: &TestApp) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO outbox_events
             (id, tenant_id, aggregate_type, aggregate_id, event_type, payload_json)
         VALUES ($1, $2, 'SYSTEM', $1, 'Report.Scheduled', '{}')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("the outbox event");

    id
}

/// What the workers have left of one notification and one event.
#[derive(Debug, PartialEq, Eq)]
struct Rows {
    notification: String,
    notification_leased: bool,
    event: String,
}

async fn rows(app: &TestApp, notification: Uuid, event: Uuid) -> Rows {
    let (notification, notification_leased): (String, bool) = sqlx::query_as(
        "SELECT status, lease_expires_at IS NOT NULL FROM notifications WHERE id = $1",
    )
    .bind(notification)
    .fetch_one(&app.pool)
    .await
    .expect("the notification's row");

    let event: String = sqlx::query_scalar("SELECT status FROM outbox_events WHERE id = $1")
        .bind(event)
        .fetch_one(&app.pool)
        .await
        .expect("the event's row");

    Rows {
        notification,
        notification_leased,
        event,
    }
}

/// Waits until both workers have settled their row: the control that they were
/// running, so their silence after the stop means stopped and not never
/// started.
async fn settled_by_the_workers(app: &TestApp, notification: Uuid, event: Uuid) -> Rows {
    let deadline = Instant::now() + PATIENCE;

    loop {
        let now = rows(app, notification, event).await;

        if now.notification != "PENDING" && now.event == "PROCESSED" {
            return now;
        }

        assert!(
            Instant::now() < deadline,
            "control: the workers serve started did not settle their rows within \
             {PATIENCE:?}: {now:?}"
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// #856's criterion: **a request in flight at the stop completes, the reset
/// mail it handed off is sent, the workers stop, and all of it inside Docker's
/// ten seconds.**
#[tokio::test]
async fn a_request_in_flight_at_the_stop_completes_and_the_workers_stop() {
    let app = TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(DELIVERY)).await;

    let email = "in.flight@kelir.test";
    let person = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "user.inflight",
        email,
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port");
    let address = listener.local_addr().expect("the bound address");

    let (stop, stopped) = oneshot::channel::<()>();
    let serving = tokio::spawn(server::serve(listener, app.state.clone(), async {
        let _ = stopped.await;
    }));

    // Control: the workers serve started are running.
    let notification = pending_notification(&app, person, "Before the stop").await;
    let event = pending_event(&app).await;
    settled_by_the_workers(&app, notification, event).await;

    let body = json!({ "username": "user.inflight" }).to_string();
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

    // Control: the server is inside the request.
    let interim = read_head(&mut request, "the 100 Continue").await;
    assert!(
        interim.starts_with("HTTP/1.1 100"),
        "control: the server did not take up the request before the stop: {interim:?}"
    );

    let stopping = Instant::now();
    stop.send(()).expect("serve is listening for its stop");

    // Control: the stop has begun, and the request is still unanswered — its
    // body has not been sent.
    assert!(
        stops_accepting(address).await,
        "the server was told to stop and went on accepting connections for {PATIENCE:?}"
    );

    request
        .write_all(body.as_bytes())
        .await
        .expect("the request's body, after the stop");

    let answer = read_head(&mut request, "the answer to the request in flight").await;
    assert!(
        answer.starts_with("HTTP/1.1 202"),
        "the request in flight at the stop was not answered: {answer:?}"
    );

    let returned = tokio::time::timeout(DOCKER_STOP_TIMEOUT, serving).await;
    let took = stopping.elapsed();
    assert!(
        returned.is_ok(),
        "serve had not returned {DOCKER_STOP_TIMEOUT:?} after the stop, when Docker kills"
    );
    returned
        .expect("checked above")
        .expect("serve did not panic")
        .expect("serve stopped cleanly");
    assert!(
        took < DOCKER_STOP_TIMEOUT,
        "the stop took {took:?}, past Docker's {DOCKER_STOP_TIMEOUT:?}"
    );

    // The reset mail the request handed off was sent before serve returned.
    let reset_mail = app
        .sent_mail()
        .into_iter()
        .filter(|mail| mail.to == email && mail.subject.starts_with("Reset your"))
        .count();
    assert_eq!(
        reset_mail, 1,
        "the reset mail handed off by the request in flight was dropped by the stop"
    );

    // The workers stopped: rows written after serve returned stay as written.
    let notification = pending_notification(&app, person, "After the stop").await;
    let event = pending_event(&app).await;
    tokio::time::sleep(QUIET).await;

    assert_eq!(
        rows(&app, notification, event).await,
        Rows {
            notification: "PENDING".to_owned(),
            notification_leased: false,
            event: "PENDING".to_owned(),
        },
        "a worker went on working after serve returned"
    );
}
