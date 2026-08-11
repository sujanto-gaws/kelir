use std::sync::Arc;

use sqlx::PgPool;

use crate::config::AppConfig;

/// Shared application state handed to every handler.
///
/// Cloning is cheap: `PgPool` is internally reference-counted and the config
/// sits behind an `Arc`, so Axum can clone this per request.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<AppConfig>,
}

impl AppState {
    pub fn new(pool: PgPool, config: AppConfig) -> Self {
        Self {
            pool,
            config: Arc::new(config),
        }
    }
}
