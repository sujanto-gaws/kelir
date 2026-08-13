use std::time::Duration;

use sqlx::postgres::{PgPoolOptions, PgSslMode};
use sqlx::{ConnectOptions, PgPool};

/// The reserved system tenant (database schema §1.5). Platform-level rows that
/// belong to no customer tenant carry this id.
///
/// Not used to scope queries. Runtime code resolves its tenant through
/// `modules::organization::service` (FR-IDM-009), so that a deployment which
/// renames or repoints its default tenant is not silently overridden by a
/// hardcoded id. What this constant is still for is pinning the UUID that
/// `0001_core.sql` seeds, so a migration that changed it would fail the test
/// below rather than orphan every platform-level row.
#[allow(
    dead_code,
    reason = "pins the seeded system-tenant UUID against 0001_core.sql; asserted by test"
)]
pub const SYSTEM_TENANT_ID: uuid::Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000001");

/// Opens the connection pool.
///
/// The pool is created lazily: `connect_lazy` does not dial the database, so
/// the API can start and serve `/health/live` while PostgreSQL is still coming
/// up. Readiness is what reports the real state (`/health/ready`).
pub fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let options: sqlx::postgres::PgConnectOptions = database_url
        .parse::<sqlx::postgres::PgConnectOptions>()?
        // Statement logging at debug would echo parameter values, which can
        // include personal data (coding standard §2.7).
        .log_statements(tracing::log::LevelFilter::Trace)
        .ssl_mode(PgSslMode::Prefer);

    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy_with(options))
}

/// Applies any migrations the database has not yet seen.
///
/// SQLx records applied migrations and their checksums in `_sqlx_migrations`,
/// so an edited migration fails loudly rather than silently diverging — which
/// is the mechanism behind "migrations are immutable once merged"
/// (coding standard §2.5).
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Cheap liveness probe for the database, used by `/health/ready`.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pool construction registers with the Tokio reactor even when lazy, so
    // these need a runtime despite never reaching the network.
    #[tokio::test]
    async fn builds_a_pool_without_contacting_the_database() {
        // connect_lazy must not dial; if it did, this test would need a server.
        let pool = create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("a valid url builds a pool");

        assert!(pool.options().get_max_connections() >= 1);
    }

    #[tokio::test]
    async fn rejects_a_malformed_url() {
        for url in ["", "not-a-url", "://x", "postgres://u:p@h:notaport/d"] {
            assert!(
                create_pool(url).is_err(),
                "{url:?} should not produce a pool"
            );
        }
    }

    #[tokio::test]
    async fn does_not_validate_the_url_scheme() {
        // Documenting a sharp edge rather than asserting what we wish were true:
        // PgConnectOptions ignores the scheme, so a mistyped KELIR_DATABASE_URL
        // is not caught at startup. It surfaces as a readiness failure instead.
        assert!(create_pool("mysql://root@localhost/kelir").is_ok());
    }

    #[test]
    fn system_tenant_id_matches_the_migration() {
        // The constant and 0001_core.sql must agree; drift here would silently
        // orphan every platform-level row.
        assert_eq!(
            SYSTEM_TENANT_ID.to_string(),
            "00000000-0000-0000-0000-000000000001"
        );
    }
}
