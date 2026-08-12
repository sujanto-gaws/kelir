use std::sync::Arc;

use sqlx::PgPool;

use crate::config::AppConfig;
use crate::middleware::rate_limit::RateLimiter;

/// Shared application state handed to every handler.
///
/// Cloning is cheap: `PgPool` is internally reference-counted and the config
/// sits behind an `Arc`, so Axum can clone this per request.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<AppConfig>,
    /// Shared so every request sees the same counters; in-memory, so per
    /// instance (see `middleware::rate_limit`).
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub fn new(pool: PgPool, config: AppConfig) -> Self {
        Self {
            pool,
            config: Arc::new(config),
            rate_limiter: Arc::new(RateLimiter::new()),
        }
    }
}
