mod config;
mod db;
mod error;
mod health;
mod middleware;
mod modules;
mod router;
mod utils;

use std::net::SocketAddr;

use config::AppConfig;

/// Address the API binds to. Phase 1 replaces this with `KELIR_*` configuration
/// loading; until then the port matches the compose port mapping.
const BIND_ADDR: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 8080);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = AppConfig::default();
    let app = router::create_router();

    // Startup failures are unrecoverable and must stop the process with a clear
    // message; the coding standard (2.3) permits expect() here but not in handlers.
    let listener = tokio::net::TcpListener::bind(BIND_ADDR)
        .await
        .expect("failed to bind the API listen address");

    tracing::info!(app_name = %config.app_name, address = %BIND_ADDR, "kelir backend listening");

    axum::serve(listener, app)
        .await
        .expect("the API server stopped unexpectedly");
}
