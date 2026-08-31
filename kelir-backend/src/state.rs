use std::sync::Arc;

use sqlx::PgPool;

use crate::config::AppConfig;
use crate::mail::Mailer;
use crate::middleware::rate_limit::RateLimiter;
use crate::modules::attachment::storage::ObjectStorage;

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
    /// Where an attachment's bytes go (FR-ATT-001, #244).
    ///
    /// On the state for the mailer's reason one field down: built once at
    /// startup so a misconfiguration is loud immediately, and constructible
    /// directly so a test can hand the router the store it will then read from.
    pub storage: ObjectStorage,
    /// How the one transactional email leaves the process (FR-AUTH-006).
    ///
    /// On the state rather than built per request so a test can hold the same
    /// instance the handler sends through, and so the SMTP connection pool is
    /// shared rather than reopened per reset.
    pub mailer: Mailer,
}

impl AppState {
    pub fn new(pool: PgPool, config: AppConfig) -> Self {
        // A mailer that cannot be built is a configuration error, and the one
        // way it can fail is an SMTP host whose TLS parameters do not resolve.
        // Falling back to `Logged` rather than panicking keeps a bad mail
        // setting from stopping a deployment that mostly does not send mail —
        // and it is loud, because `Logged` warns on every send.
        let mailer = Mailer::from_config(&config).unwrap_or_else(|error| {
            tracing::error!(%error, "could not build the SMTP mailer; mail will be logged instead");

            Mailer::Logged {
                from: config.mail_from.clone(),
            }
        });

        Self::with_mailer(pool, config, mailer)
    }

    /// The same, with the mailer supplied — how the test harness captures mail.
    pub fn with_mailer(pool: PgPool, config: AppConfig, mailer: Mailer) -> Self {
        let storage = ObjectStorage::from_config(&config);

        Self {
            pool,
            config: Arc::new(config),
            rate_limiter: Arc::new(RateLimiter::new()),
            mailer,
            storage,
        }
    }
}
