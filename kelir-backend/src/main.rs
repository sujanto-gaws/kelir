//! Binary entry point.
//!
//! The application itself lives in the library crate (`src/lib.rs`); this file
//! only loads configuration, migrates, opens the serving pool, bootstraps and
//! serves.
//! Keeping it thin is what lets `tests/` drive exactly the router the binary
//! serves rather than a reconstruction of it.

use std::process::ExitCode;

use kelir_backend::config::AppConfig;
use kelir_backend::state::AppState;
use kelir_backend::{db, health, modules, server};
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

    // **Two roles** (ADR-0051 §2). The owner migrates, through
    // KELIR_DATABASE_MIGRATION_URL and a pool of its own, narrows the serving
    // role, and its pool is closed before this returns, so it is gone before
    // the server binds. Everything below runs on the serving pool, as a role
    // that owns no relation, function, schema or type and may not change
    // `audit_events`; staging and production check that posture before going
    // on.
    //
    // Migrations run at startup so a fresh compose stack is usable immediately.
    // This is safe while deployments are single-instance; once several replicas
    // start together, migration moves to a release step (release process §5,
    // ADR-0051 §6).
    let pool = db::prepare_serving_pool(&config).await?;

    // After migrations, so the seeded role exists; before serving, so the
    // instance is never reachable without a way to sign in.
    modules::auth::bootstrap::ensure_administrator(&pool, &config).await?;

    let bind_address = config.bind_address.clone();
    let state = AppState::new(pool, config);

    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!(address = %bind_address, "listening");

    server::serve(listener, state, std::future::pending()).await?;

    Ok(())
}
