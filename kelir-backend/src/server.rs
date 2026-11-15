//! Serving: the background workers and the HTTP server, from start to stop
//! ([#856]).
//!
//! **Here rather than in `main.rs`** so a test drives the same function the
//! binary runs: `main.rs` binds the listener and passes the process's stop
//! signal, and `tests/graceful_shutdown.rs` passes a future it completes
//! itself.
//!
//! [#856]: https://github.com/sujanto-gaws/kelir/issues/856

use std::future::Future;
use std::net::SocketAddr;

use tokio::net::TcpListener;

use crate::modules;
use crate::router;
use crate::state::AppState;

/// Starts the workers and serves `listener` until the process ends.
///
/// `_shutdown` is not honoured yet ([#856]): the process is killed rather than
/// stopped, and an in-flight request is lost with it.
///
/// [#856]: https://github.com/sujanto-gaws/kelir/issues/856
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    _shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    // **The scanner runs beside the server, not inside a request** (FR-ATT-001,
    // coding standard §2.4). An upload returns as soon as the bytes are stored,
    // with `virus_scan_status = PENDING`; this loop answers afterwards and the
    // download refuses until it has. It is spawned rather than awaited for the
    // obvious reason and detached rather than tracked for a less obvious one:
    // there is nothing to shut it down cleanly *for*. Its only durable state is
    // a row it did not move, and a scan interrupted by a restart is picked up
    // by the next process because the row is still `PENDING`.
    tokio::spawn(modules::attachment::worker::run(state.clone()));
    // The second worker, and the reason there are two rather than one loop with
    // a match in it: they poll different tables at different rates and one is
    // bound by a virus scanner while the other is bound by a mail relay.
    tokio::spawn(modules::notification::worker::run(state.clone()));
    // The third, over `outbox_events` (ADR-0041): what a committed transition
    // left for its after-hooks. Detached for the scanner's reason — a delivery
    // interrupted by a restart is a `PROCESSING` row whose lease expires, and
    // the next process claims it again.
    tokio::spawn(modules::outbox::worker::run(state.clone()));

    let app = router::create_router(state);

    // With connect info, so the socket peer address reaches the handlers. It is
    // the only address a caller cannot forge, and both the rate limiter and the
    // audit trail are keyed on it (see `middleware::client_address`).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}
