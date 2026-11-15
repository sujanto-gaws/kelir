//! Binary entry point.
//!
//! The application itself lives in the library crate (`src/lib.rs`); this file
//! only loads configuration, migrates, opens the serving pool, bootstraps and
//! serves.
//! Keeping it thin is what lets `tests/` drive exactly the router the binary
//! serves rather than a reconstruction of it.

use std::future::Future;
use std::process::ExitCode;
use std::time::Duration;

use kelir_backend::config::AppConfig;
use kelir_backend::state::AppState;
use kelir_backend::{db, health, modules, server};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Startup failures are reported and exit non-zero so the container
            // orchestrator restarts or reports, rather than a silent no-op.
            tracing::error!(error = %error, "kelir backend failed to start");
            ExitCode::FAILURE
        }
    }
}

/// Tracing is configured before anything else so startup failures are visible.
/// `KELIR_LOG` overrides the default filter; `info` keeps business-significant
/// events without SQL noise (coding standard §2.7).
fn init_tracing() {
    let filter = EnvFilter::try_from_env("KELIR_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn run() -> anyhow::Result<()> {
    // First, so a stop signal is never met by the default disposition: before
    // serving it ends startup, and after it drives the drain (#856).
    let mut shutdown = Box::pin(shutdown_signal()?);
    let config = AppConfig::from_env()?;

    tracing::info!(
        app_name = %config.app_name,
        environment = %config.app_env,
        version = health::VERSION,
        "starting kelir backend"
    );

    // **Two roles** (ADR-0051 §2): the serving pool, and the owner's, from
    // KELIR_DATABASE_MIGRATION_URL when it is set. Opened here, connecting
    // neither, so a stop during startup can close both.
    let pools = db::StartupPools::open(&config)?;

    // **A stop during startup ends startup** (#856). Migrating, bootstrapping
    // and binding can each wait, on a database that does not answer above all,
    // and a stop that waited for them would be Docker's kill ten seconds later.
    // Dropping the startup is safe: a migration interrupted mid-transaction is
    // rolled back by PostgreSQL, and nothing has been served.
    let started = tokio::select! {
        started = start(&pools, &config) => Some(started?),
        () = &mut shutdown => None,
    };

    let Some(listener) = started else {
        tracing::info!("stopped during startup");

        if tokio::time::timeout(POOL_CLOSE, pools.close())
            .await
            .is_err()
        {
            tracing::warn!("the database pools had not closed {POOL_CLOSE:?} into the stop");
        }

        return Ok(());
    };

    let state = AppState::new(pools.serving, config);

    server::serve(listener, state, shutdown).await?;

    Ok(())
}

/// How long a stop during startup waits for the pools to close.
const POOL_CLOSE: Duration = Duration::from_secs(1);

/// The startup that waits on something else: preparing the database, the
/// bootstrap and the bind. Returns the bound listener.
async fn start(pools: &db::StartupPools, config: &AppConfig) -> anyhow::Result<TcpListener> {
    // The owner migrates and narrows the serving role, and its pool is closed
    // before this goes on, so it is gone before the server binds. Everything
    // below runs on the serving pool, as a role that owns no relation,
    // function, schema or type and may not change `audit_events`; staging and
    // production check that posture before going on.
    //
    // Migrations run at startup so a fresh compose stack is usable immediately.
    // This is safe while deployments are single-instance; once several replicas
    // start together, migration moves to a release step (release process §5,
    // ADR-0051 §6).
    pools.prepare(config).await?;

    // After migrations, so the seeded role exists; before serving, so the
    // instance is never reachable without a way to sign in.
    modules::auth::bootstrap::ensure_administrator(&pools.serving, config).await?;

    let listener = TcpListener::bind(&config.bind_address).await?;
    tracing::info!(address = %config.bind_address, "listening");

    Ok(listener)
}

/// Completes on SIGTERM, which `docker stop` sends, or on SIGINT, which Ctrl-C
/// sends (#856).
///
/// **Both are listened for from when this is called**, first thing in `run`,
/// not from when the future is first polled: the binary is the container's
/// PID 1, and PID 1 ignores a signal it has no handler for, which is why
/// `docker stop` used to wait out its ten seconds and kill. `run` races its
/// startup against this future, so a signal before serving ends the startup,
/// and one after drives the drain. A handler that cannot be installed fails
/// startup rather than leaving a process that can only be killed.
///
/// The handlers stay installed after the first signal, so a second one during
/// the stop is ignored rather than killing the drain.
#[cfg(unix)]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()> + Send + 'static> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => tracing::info!("SIGTERM received"),
            _ = interrupt.recv() => tracing::info!("SIGINT received"),
        }
    })
}

/// Completes on Ctrl-C. There is no SIGTERM off Unix, and a developer's
/// machine stops on Ctrl-C.
#[cfg(not(unix))]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()> + Send + 'static> {
    Ok(async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => tracing::info!("Ctrl-C received"),
            // Not a stop: stopping on the error would stop a healthy process at
            // once.
            Err(error) => {
                tracing::error!(%error, "could not listen for Ctrl-C");
                std::future::pending::<()>().await;
            }
        }
    })
}
