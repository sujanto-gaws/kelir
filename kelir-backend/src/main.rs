//! Binary entry point.
//!
//! The application itself lives in the library crate (`src/lib.rs`); this file
//! only loads configuration, opens the pool, migrates, bootstraps and serves.
//! Keeping it thin is what lets `tests/` drive exactly the router the binary
//! serves rather than a reconstruction of it.

use std::net::SocketAddr;
use std::process::ExitCode;

use kelir_backend::config::AppConfig;
use kelir_backend::state::AppState;
use kelir_backend::{db, health, modules, router};
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
    let config = AppConfig::from_env()?;

    tracing::info!(
        app_name = %config.app_name,
        environment = %config.app_env,
        version = health::VERSION,
        "starting kelir backend"
    );

    let pool = db::create_pool(&config.database_url)?;

    // Migrations run at startup so a fresh compose stack is usable immediately.
    // This is safe while deployments are single-instance; once several replicas
    // start together, migration moves to a release step (release process §5).
    db::run_migrations(&pool).await?;
    tracing::info!("database migrations applied");

    // After migrations, so the seeded role exists; before serving, so the
    // instance is never reachable without a way to sign in.
    modules::auth::bootstrap::ensure_administrator(&pool, &config).await?;

    let bind_address = config.bind_address.clone();
    let state = AppState::new(pool, config);

    // **The scanner runs beside the server, not inside a request** (FR-ATT-001,
    // coding standard §2.4). An upload returns as soon as the bytes are stored,
    // with `virus_scan_status = PENDING`; this loop answers afterwards and the
    // download refuses until it has. It is spawned rather than awaited for the
    // obvious reason and detached rather than tracked for a less obvious one:
    // there is nothing to shut it down cleanly *for*. Its only durable state is
    // a row it did not move, and a scan interrupted by a restart is picked up
    // by the next process because the row is still `PENDING`.
    tokio::spawn(modules::attachment::worker::run(state.clone()));

    let app = router::create_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!(address = %bind_address, "listening");

    // With connect info, so the socket peer address reaches the handlers. It is
    // the only address a caller cannot forge, and both the rate limiter and the
    // audit trail are keyed on it (see `middleware::client_address`).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
