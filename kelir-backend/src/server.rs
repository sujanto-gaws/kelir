//! Serving: the background workers and the HTTP server, from start to stop
//! ([#856]).
//!
//! **Here rather than in `main.rs`** so a test drives the same function the
//! binary runs: `main.rs` binds the listener and passes the process's stop
//! signal, and `tests/graceful_shutdown.rs` passes a future it completes
//! itself.
//!
//! # A stop, in order
//!
//! `docker stop` sends SIGTERM and kills ten seconds later. When `shutdown`
//! completes, everything below happens against one deadline, [`GRACE`] after
//! it:
//!
//! 1. **The workers are told to stop**, at once. Each finishes the pass it is
//!    in and leaves its loop.
//! 2. **The server stops accepting** and answers the requests it already holds
//!    (axum's graceful shutdown). An idle keep-alive connection is closed.
//! 3. **The detached sends are waited for**, after the last request is
//!    answered, because a request still being served can hand off one more.
//! 4. **The workers are waited for.**
//!
//! **What the deadline overtakes is abandoned, deliberately, and logged.** A
//! request still unanswered is dropped; a worker's pass is dropped, and its
//! claimed rows return when their lease passes, as after a crash; a reset mail
//! still sending is lost, and the person asks again. That is what Docker's kill
//! would do a moment later, done by the process instead, with a line in the log
//! naming what was left.
//!
//! [#856]: https://github.com/sujanto-gaws/kelir/issues/856

use std::future::{Future, IntoFuture};
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::modules;
use crate::router;
use crate::state::AppState;

/// How long a stop may take, from the signal to `serve` returning.
///
/// Two seconds under Docker's default of ten, so the process exits on its own
/// rather than being killed, with room for the runtime to wind down after.
pub const GRACE: Duration = Duration::from_secs(8);

/// Starts the workers and serves `listener` until `shutdown` completes, then
/// stops as the module comment describes and returns.
///
/// [`GRACE`] after `shutdown` completes, it abandons whatever is still running
/// and returns.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let stop = CancellationToken::new();
    let workers = spawn_workers(&state, &stop);
    let detached = state.detached.clone();

    let app = router::create_router(state);

    let stopping = stop.clone();
    // With connect info, so the socket peer address reaches the handlers. It is
    // the only address a caller cannot forge, and both the rate limiter and the
    // audit trail are keyed on it (see `middleware::client_address`).
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.await;
        tracing::info!(grace = ?GRACE, "stopping: no new connections, finishing the work in hand");
        stopping.cancel();
    })
    .into_future();

    // The server is its own task so its drain can be timed and, at the
    // deadline, dropped. The guard cancels as well, so a server that ended
    // without the signal, or panicked, still stops the workers rather than
    // leaving `serve` waiting for a signal nothing will act on.
    let ended = stop.clone().drop_guard();
    let serving = tokio::spawn(async move {
        let _stop_when_ended = ended;
        server.await
    });

    stop.cancelled().await;
    let deadline = Instant::now() + GRACE;

    let served = finish(deadline, serving, "the HTTP server").await;

    // Closed only now: until the last request was answered, one could still
    // hand off a send.
    detached.close();
    if tokio::time::timeout_at(deadline, detached.wait())
        .await
        .is_err()
    {
        tracing::warn!(
            abandoned = detached.len(),
            "the stop's deadline passed with detached sends still running; they are abandoned"
        );
    }

    for (name, worker) in workers {
        finish(deadline, worker, name).await;
    }

    tracing::info!("stopped");

    served.unwrap_or(Ok(()))
}

/// The three background workers, each told to stop through `stop`.
fn spawn_workers(
    state: &AppState,
    stop: &CancellationToken,
) -> [(&'static str, JoinHandle<()>); 3] {
    [
        // **The scanner runs beside the server, not inside a request**
        // (FR-ATT-001, coding standard §2.4). An upload returns as soon as the
        // bytes are stored, with `virus_scan_status = PENDING`; this loop
        // answers afterwards and the download refuses until it has. A scan
        // interrupted by a stop is picked up by the next process because the
        // row is still `PENDING`.
        (
            "the attachment scanner",
            tokio::spawn(modules::attachment::worker::run(
                state.clone(),
                stop.clone(),
            )),
        ),
        // The second worker, and the reason there are two rather than one loop
        // with a match in it: they poll different tables at different rates and
        // one is bound by a virus scanner while the other is bound by a mail
        // relay.
        (
            "the notification sender",
            tokio::spawn(modules::notification::worker::run(
                state.clone(),
                stop.clone(),
            )),
        ),
        // The third, over `outbox_events` (ADR-0041): what a committed
        // transition left for its after-hooks. A delivery interrupted by a stop
        // is a `PROCESSING` row whose lease expires, and the next process claims
        // it again.
        (
            "the outbox worker",
            tokio::spawn(modules::outbox::worker::run(state.clone(), stop.clone())),
        ),
    ]
}

/// Waits for `task` until `deadline`, and abandons it after.
///
/// `None` when the task did not finish: it panicked, or it was still running
/// at the deadline and has been aborted.
async fn finish<T>(deadline: Instant, mut task: JoinHandle<T>, what: &str) -> Option<T> {
    match tokio::time::timeout_at(deadline, &mut task).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            tracing::error!(%error, what, "a task ended abnormally during the stop");
            None
        }
        Err(_) => {
            task.abort();
            tracing::warn!(
                what,
                "the stop's deadline passed before this finished; it is abandoned"
            );
            None
        }
    }
}
