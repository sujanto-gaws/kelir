//! Tenancy resolution (FR-IDM-009).
//!
//! Every tenant-scoped query in the system already filters `tenant_id`; what
//! this decides is *which* `tenant_id` a session gets in the first place. The
//! answer is then written into the JWT `tenant_id` claim and everything
//! downstream reads it from there via `Authenticated::tenant_id()`.
//!
//! **Why a deployment flag.** The SRS §2 glossary defines multi-tenant mode as
//! a deployment configuration flag, and that is what this implements. Two
//! alternatives were considered and rejected: resolving the tenant from a
//! subdomain needs wildcard DNS and TLS on a staging host that does not exist
//! yet, and a caller-supplied header carries exactly the same trust level as a
//! body field while being invisible in the OpenAPI contract.

use std::fmt;

use sqlx::PgExecutor;

use super::domain::{normalize_tenant_code, Tenant};
use crate::config::AppConfig;
use crate::error::AppError;

/// The longest tenant code that can exist (`tenants.tenant_code VARCHAR(64)`).
///
/// Checked before the query so an over-long value is refused without a round
/// trip, and so nothing unbounded and caller-controlled reaches the logs.
const MAX_TENANT_CODE_LEN: usize = 64;

/// Why a sign-in could not be attributed to a tenant.
///
/// Deliberately more detailed than what the caller is told. The auth service
/// collapses [`Self::Unknown`] and [`Self::NotActive`] into one generic refusal
/// so the login endpoint cannot be used to enumerate tenants; the distinction
/// survives here for the operator-facing log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenantResolutionError {
    /// Multi-tenant mode is on and the request named no tenant.
    CodeRequired,
    /// No live tenant has that code.
    Unknown,
    /// The tenant exists but is SUSPENDED or INACTIVE.
    NotActive,
    /// Single-tenant mode, and the configured default tenant is not in the
    /// database. A deployment fault, not a credential one.
    DefaultMissing { code: String },
    /// The lookup itself failed. Kept separate from [`Self::Unknown`] so a
    /// database outage does not present to every caller as bad credentials —
    /// that would turn an incident into a silent, uniform login failure with
    /// nothing in the response to say so.
    LookupFailed { detail: String },
}

impl fmt::Display for TenantResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CodeRequired => f.write_str("multi-tenant mode requires a tenant code"),
            Self::Unknown => f.write_str("no such tenant"),
            Self::NotActive => f.write_str("the tenant is not active"),
            Self::DefaultMissing { code } => {
                write!(f, "the configured default tenant '{code}' does not exist")
            }
            Self::LookupFailed { detail } => write!(f, "tenant lookup failed: {detail}"),
        }
    }
}

impl std::error::Error for TenantResolutionError {}

/// A misconfigured default tenant is the only resolution failure that is the
/// deployment's fault rather than the caller's, so it is the only one that maps
/// to a 500 on its own. The rest are mapped deliberately by the auth service.
impl From<TenantResolutionError> for AppError {
    fn from(error: TenantResolutionError) -> Self {
        match error {
            TenantResolutionError::CodeRequired => {
                AppError::validation(vec![crate::error::ValidationDetail::new(
                    "tenantCode",
                    "required",
                    "REQUIRED",
                    "A tenant code is required on this deployment",
                )])
            }
            TenantResolutionError::Unknown | TenantResolutionError::NotActive => {
                AppError::Unauthorized
            }
            TenantResolutionError::DefaultMissing { .. }
            | TenantResolutionError::LookupFailed { .. } => AppError::Internal {
                source: anyhow::anyhow!("{error}"),
            },
        }
    }
}

/// Resolves the tenant a sign-in attempt belongs to.
///
/// Single-tenant deployments ignore `requested_code` entirely and resolve the
/// configured default. That is what makes the flag-off path byte-identical to
/// the behaviour before tenancy existed: a client that sends nothing, and a
/// client that sends a code, land in the same place.
pub async fn resolve_for_sign_in(
    executor: impl PgExecutor<'_>,
    config: &AppConfig,
    requested_code: Option<&str>,
) -> Result<Tenant, TenantResolutionError> {
    let tenant = if config.multi_tenant {
        let code = requested_code
            .map(normalize_tenant_code)
            .filter(|code| !code.is_empty())
            .ok_or(TenantResolutionError::CodeRequired)?;

        if code.len() > MAX_TENANT_CODE_LEN {
            // Longer than the column, so it matches nothing. Answering without
            // the query keeps an oversized, caller-controlled value out of the
            // log line below.
            return Err(TenantResolutionError::Unknown);
        }

        find_live(executor, &code).await?
    } else {
        resolve_default(executor, config).await?
    };

    // Applies in both modes: suspending or deactivating a tenant is how an
    // operator takes it offline, and that has to stop its users signing in even
    // when it is the only tenant there is.
    if !tenant.status.admits_sign_in() {
        tracing::warn!(
            tenant_code = %tenant.tenant_code,
            status = tenant.status.as_db(),
            "sign-in refused: tenant is not active"
        );
        return Err(TenantResolutionError::NotActive);
    }

    Ok(tenant)
}

/// Resolves the deployment's default tenant, whatever its status.
///
/// Deliberately says nothing about sign-in eligibility — that is
/// [`resolve_for_sign_in`]'s decision. Startup uses this to place the first-run
/// administrator, and a suspended tenant must not stop the process booting; it
/// should stop people logging in, which it does, one layer up.
pub async fn resolve_default(
    executor: impl PgExecutor<'_>,
    config: &AppConfig,
) -> Result<Tenant, TenantResolutionError> {
    let code = config.default_tenant_code.as_str();

    find_live(executor, code)
        .await
        .map_err(|error| match error {
            // The code came from configuration, not from a caller, so "not found"
            // here means the deployment points at a tenant nobody created — worth a
            // distinct, loud failure rather than a generic refusal.
            TenantResolutionError::Unknown => TenantResolutionError::DefaultMissing {
                code: code.to_owned(),
            },
            other => other,
        })
}

/// Shared lookup. Infrastructure errors are not resolution outcomes, so a
/// database failure is surfaced rather than folded into "unknown tenant" —
/// otherwise an outage would look to every caller like bad credentials.
async fn find_live(
    executor: impl PgExecutor<'_>,
    code: &str,
) -> Result<Tenant, TenantResolutionError> {
    match super::repository::find_by_code(executor, code).await {
        Ok(Some(tenant)) => Ok(tenant),
        Ok(None) => Err(TenantResolutionError::Unknown),
        Err(error) => {
            tracing::error!(error = %error, "tenant lookup failed");
            Err(TenantResolutionError::LookupFailed {
                detail: error.to_string(),
            })
        }
    }
}

/// The rate-limiter scope a sign-in attempt counts against.
///
/// Resolution happens after rate limiting — it touches the database, and the
/// limiter exists to keep unauthenticated volume away from it — so this derives
/// the scope from configuration and the request alone.
///
/// When the flag is off the answer is always the default code, never the
/// supplied one. Honouring a caller-supplied code in single-tenant mode would
/// let an attacker mint a fresh bucket per request by varying a field that
/// resolution then ignores, which would defeat the limiter entirely.
pub fn sign_in_scope(config: &AppConfig, requested_code: Option<&str>) -> String {
    if !config.multi_tenant {
        return config.default_tenant_code.clone();
    }

    requested_code
        .map(normalize_tenant_code)
        .filter(|code| !code.is_empty())
        // Truncated for the same reason resolution refuses over-long codes: the
        // key is a map key and must not be caller-sized.
        .map(|mut code| {
            code.truncate(MAX_TENANT_CODE_LEN);
            code
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::organization::domain::TenantStatus;

    fn config(multi_tenant: bool) -> AppConfig {
        AppConfig {
            multi_tenant,
            ..AppConfig::test_default()
        }
    }

    /// The database the SQLx macros were compiled against.
    ///
    /// Read at compile time rather than from the process environment so it
    /// cannot drift from the schema the queries above were verified against.
    /// The crate does not build without it, so a resolution test that needs a
    /// database is not adding a requirement the build did not already have —
    /// but `option_env!` keeps an offline build (`SQLX_OFFLINE`) compiling, and
    /// such a build skips these loudly rather than reporting a false pass.
    const TEST_DATABASE_URL: Option<&str> = option_env!("DATABASE_URL");

    /// Opens a transaction for a test, or `None` when no database is configured.
    ///
    /// Every tenant a test creates lives inside a transaction that is never
    /// committed, so tests neither see each other's rows nor leave any behind.
    async fn transaction() -> Option<sqlx::Transaction<'static, sqlx::Postgres>> {
        let url = TEST_DATABASE_URL?;
        let pool = crate::db::create_pool(url).expect("test database url parses");

        match pool.begin().await {
            Ok(transaction) => Some(transaction),
            Err(error) => {
                eprintln!("skipping: no database at {url}: {error}");
                None
            }
        }
    }

    /// Inserts a tenant visible only inside `transaction`.
    async fn given_tenant(
        transaction: &mut sqlx::PgConnection,
        code: &str,
        status: TenantStatus,
        deleted: bool,
    ) -> uuid::Uuid {
        let id = uuid::Uuid::now_v7();

        sqlx::query!(
            r#"
            INSERT INTO tenants (id, tenant_code, name, status, deleted_at)
            VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN now() END)
            "#,
            id,
            code,
            format!("Tenant {code}"),
            status.as_db(),
            deleted
        )
        .execute(&mut *transaction)
        .await
        .expect("tenant inserted");

        id
    }

    #[tokio::test]
    async fn single_tenant_resolves_the_configured_default() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        // The seeded SYSTEM tenant from 0001_core.sql.
        let tenant = resolve_for_sign_in(&mut *tx, &config(false), None)
            .await
            .expect("the default tenant resolves");

        assert_eq!(tenant.tenant_code, "SYSTEM");
        assert_eq!(tenant.status, TenantStatus::Active);
    }

    #[tokio::test]
    async fn single_tenant_ignores_a_supplied_code() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        let config = config(false);
        given_tenant(&mut tx, "OTHER-IGNORED", TenantStatus::Active, false).await;

        // Flag off means the login contract is unchanged: a client that sends a
        // code lands in exactly the same tenant as one that does not, so the
        // field cannot be used to reach another tenant's users.
        let with_code = resolve_for_sign_in(&mut *tx, &config, Some("OTHER-IGNORED"))
            .await
            .expect("resolves");
        let without = resolve_for_sign_in(&mut *tx, &config, None)
            .await
            .expect("resolves");

        assert_eq!(with_code.id, without.id);
        assert_eq!(with_code.tenant_code, "SYSTEM");
    }

    #[tokio::test]
    async fn multi_tenant_requires_a_code() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        for absent in [None, Some(""), Some("   ")] {
            let error = resolve_for_sign_in(&mut *tx, &config(true), absent)
                .await
                .expect_err("a tenant must be named");

            assert_eq!(error, TenantResolutionError::CodeRequired, "for {absent:?}");
        }
    }

    #[tokio::test]
    async fn multi_tenant_resolves_a_named_tenant_case_insensitively() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        let id = given_tenant(&mut tx, "ACME", TenantStatus::Active, false).await;

        for spelling in ["ACME", "acme", "  Acme  "] {
            let tenant = resolve_for_sign_in(&mut *tx, &config(true), Some(spelling))
                .await
                .unwrap_or_else(|_| panic!("{spelling} resolves"));

            assert_eq!(tenant.id, id, "for {spelling}");
        }
    }

    #[tokio::test]
    async fn an_unknown_tenant_does_not_resolve() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        let error = resolve_for_sign_in(&mut *tx, &config(true), Some("NO-SUCH-TENANT"))
            .await
            .expect_err("unknown");

        assert_eq!(error, TenantResolutionError::Unknown);
    }

    #[tokio::test]
    async fn a_suspended_or_inactive_tenant_refuses_sign_in() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        // FR-IDM-009: taking a tenant offline has to stop its users signing in,
        // or suspension would mean nothing.
        for (code, status) in [
            ("SUSPENDED-CO", TenantStatus::Suspended),
            ("INACTIVE-CO", TenantStatus::Inactive),
        ] {
            given_tenant(&mut tx, code, status, false).await;

            let error = resolve_for_sign_in(&mut *tx, &config(true), Some(code))
                .await
                .unwrap_err();

            assert_eq!(error, TenantResolutionError::NotActive, "for {code}");
        }
    }

    #[tokio::test]
    async fn a_suspended_default_tenant_refuses_sign_in_too() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        given_tenant(&mut tx, "LONE-CO", TenantStatus::Suspended, false).await;
        let config = AppConfig {
            multi_tenant: false,
            default_tenant_code: "LONE-CO".to_owned(),
            ..AppConfig::test_default()
        };

        let error = resolve_for_sign_in(&mut *tx, &config, None)
            .await
            .expect_err("a suspended tenant admits nobody, even alone");

        assert_eq!(error, TenantResolutionError::NotActive);
    }

    #[tokio::test]
    async fn a_soft_deleted_tenant_is_indistinguishable_from_an_unknown_one() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        given_tenant(&mut tx, "GONE-CO", TenantStatus::Active, true).await;

        let error = resolve_for_sign_in(&mut *tx, &config(true), Some("GONE-CO"))
            .await
            .expect_err("deleted tenants do not resolve");

        assert_eq!(error, TenantResolutionError::Unknown);
    }

    #[tokio::test]
    async fn a_default_tenant_that_does_not_exist_is_reported_as_such() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        let config = AppConfig {
            multi_tenant: false,
            default_tenant_code: "NEVER-CREATED".to_owned(),
            ..AppConfig::test_default()
        };

        let error = resolve_for_sign_in(&mut *tx, &config, None)
            .await
            .expect_err("misconfigured");

        // Distinct from Unknown so the operator sees a deployment fault rather
        // than every user appearing to have bad credentials.
        assert_eq!(
            error,
            TenantResolutionError::DefaultMissing {
                code: "NEVER-CREATED".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn an_over_long_code_is_refused_without_a_query() {
        let Some(mut tx) = transaction().await else {
            return;
        };

        // Longer than tenant_code's VARCHAR(64), so it can match nothing.
        let error = resolve_for_sign_in(&mut *tx, &config(true), Some(&"A".repeat(5_000)))
            .await
            .expect_err("cannot match");

        assert_eq!(error, TenantResolutionError::Unknown);
    }

    #[test]
    fn single_tenant_scopes_every_attempt_to_the_default() {
        // The supplied code is ignored, so it cannot be used to sidestep the
        // limiter by varying it.
        let config = config(false);

        assert_eq!(sign_in_scope(&config, None), "SYSTEM");
        assert_eq!(sign_in_scope(&config, Some("acme")), "SYSTEM");
        assert_eq!(sign_in_scope(&config, Some("anything-at-all")), "SYSTEM");
    }

    #[test]
    fn multi_tenant_scopes_per_tenant() {
        let config = config(true);

        assert_eq!(sign_in_scope(&config, Some("acme")), "ACME");
        assert_eq!(sign_in_scope(&config, Some("  Acme ")), "ACME");
        assert_ne!(
            sign_in_scope(&config, Some("acme")),
            sign_in_scope(&config, Some("globex")),
            "distinct tenants must not share a bucket"
        );
    }

    #[test]
    fn an_unnamed_tenant_still_gets_a_bucket() {
        // A request with no code is refused, but it must still be counted, or
        // omitting the field would be a free pass past the limiter.
        let config = config(true);

        assert_eq!(sign_in_scope(&config, None), "unknown");
        assert_eq!(sign_in_scope(&config, Some("   ")), "unknown");
    }

    #[test]
    fn the_scope_key_is_bounded() {
        let config = config(true);
        let key = sign_in_scope(&config, Some(&"A".repeat(10_000)));

        assert_eq!(key.len(), MAX_TENANT_CODE_LEN);
    }

    #[test]
    fn unknown_and_inactive_tenants_present_the_same_refusal() {
        // FR-IDM-009: the login endpoint must not distinguish them, or it
        // becomes a way to enumerate tenants.
        let unknown = AppError::from(TenantResolutionError::Unknown);
        let inactive = AppError::from(TenantResolutionError::NotActive);

        assert_eq!(unknown.code(), "UNAUTHORIZED");
        assert_eq!(inactive.code(), "UNAUTHORIZED");
        assert_eq!(unknown.status(), inactive.status());
    }

    #[test]
    fn a_missing_default_tenant_is_a_deployment_fault() {
        // Not a 401: nobody's credentials are wrong, the deployment points at a
        // tenant that was never created.
        let error = AppError::from(TenantResolutionError::DefaultMissing {
            code: "SYSTEM".to_owned(),
        });

        assert_eq!(error.code(), "INTERNAL_ERROR");
    }

    #[test]
    fn a_missing_tenant_code_is_a_validation_failure() {
        // Distinct from a wrong credential on purpose: it reveals only that the
        // deployment is multi-tenant, which the login form already shows, and
        // an honest client needs to be told what to send.
        let error = AppError::from(TenantResolutionError::CodeRequired);

        assert_eq!(error.code(), "VALIDATION_ERROR");
    }
}
