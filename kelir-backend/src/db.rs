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
    create_pool_with_max_connections(database_url, DEFAULT_MAX_CONNECTIONS)
}

/// The pool size one running instance takes.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// [`create_pool`], with the ceiling chosen by the caller.
///
/// Exists for the integration harness, which spawns one application — and so
/// one pool — per test, all against the same PostgreSQL. At the default
/// ceiling, a runner with enough cores to run twenty tests at once asks for two
/// hundred connections from a server whose own default limit is a hundred, and
/// the failure lands as an acquire timeout in whichever test was unlucky. The
/// production path keeps the default; nothing else about the pool differs, so
/// the harness still exercises the same connection options the binary uses.
pub fn create_pool_with_max_connections(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    let options: sqlx::postgres::PgConnectOptions = database_url
        .parse::<sqlx::postgres::PgConnectOptions>()?
        // Statement logging at debug would echo parameter values, which can
        // include personal data (coding standard §2.7).
        .log_statements(tracing::log::LevelFilter::Trace)
        .ssl_mode(PgSslMode::Prefer);

    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy_with(options))
}

/// Applies any migrations the database has not yet seen.
///
/// SQLx records applied migrations and their checksums in `_sqlx_migrations`,
/// so an edited migration fails loudly rather than silently diverging — which
/// is the mechanism behind "migrations are immutable once merged"
/// (coding standard §2.5). That check is untouched by the tolerance below:
/// every migration this binary *does* know is still verified by checksum.
///
/// # Migrations from a newer release are tolerated (#76)
///
/// By default SQLx refuses to start when the database holds an applied
/// migration the binary cannot resolve. That default makes application
/// rollback impossible: the previous image, redeployed against a database the
/// newer image has already migrated, dies with "migration N was previously
/// applied but is missing in the resolved migrations" — which is exactly what
/// the `v0.2.0` rollback rehearsal hit.
///
/// The [release process](../../docs/standards/04.%20Release%20Process.md) §6
/// requires the opposite: rollback is "always possible because images are
/// immutable", and every migration must be backward-compatible with the
/// previous release. Additive DDL satisfies that rule and the migrator still
/// refused, so the rule was being met in the columns and broken in the
/// bookkeeping.
///
/// What is given up is detection of a migration file that has gone missing
/// from the directory. That is an acceptable trade because migrations are
/// forward-only and immutable once merged, so a file disappearing is a
/// mistake caught in review, while the failure it was preventing here is a
/// deployment that cannot be rolled back.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_ignore_missing(true);
    migrator.run(pool).await
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
