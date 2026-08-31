//! Tenancy resolution (FR-IDM-009) and tenant administration (FR-ORG-001).
//!
//! **Resolution** decides *which* `tenant_id` a session gets. Every
//! tenant-scoped query in the system already filters that column; this is where
//! the value comes from. The answer is written into the JWT `tenant_id` claim
//! and everything downstream reads it from there via
//! `Authenticated::tenant_id()` — which is what per-request tenant resolution
//! amounts to once the session exists.
//!
//! **Why a deployment flag.** The SRS §2 glossary defines multi-tenant mode as
//! a deployment configuration flag, and that is what this implements. Two
//! alternatives were considered and rejected: resolving the tenant from a
//! subdomain needs wildcard DNS and TLS on a staging host that does not exist
//! yet, and a caller-supplied header carries exactly the same trust level as a
//! body field while being invisible in the OpenAPI contract.
//!
//! **Administration** is the second half, and it arrived three sprints later.
//! Decision **D-7** deferred it: with no way for a client to *learn* that a
//! deployment was multi-tenant, the flag was unusable and `config.rs` refused
//! to start with it on, so administering tenants would have meant creating rows
//! nobody could sign in to. **D-18** supersedes that. `GET /api/v1/deployment`
//! is the missing half — the login form asks it whether to show a tenant-code
//! field — and creating a tenant here creates its first administrator in the
//! same transaction, so the rows this makes are rows somebody can sign in to.
//!
//! The one boundary worth reading before changing anything below:
//! [`require_tenant_administrator`]. A tenant is not a row *inside* a tenant, so
//! `tenant_id` scoping cannot say who may touch it, and a permission alone
//! cannot either — the catalogue is global and a tenant administrator can grant
//! themselves any code in it.

use std::fmt;

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::{
    normalize_tenant_code, validate_create_tenant, validate_update_tenant, CreateTenantRequest,
    Tenant, TenantStatus, TenantView, UpdateTenantRequest, MAX_TENANT_CODE_LEN,
};
use super::repository::{self, TenantRecord};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::service as identity;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

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

// ---------------------------------------------------------------------------
// Tenant administration (FR-ORG-001, decision D-18)
// ---------------------------------------------------------------------------

/// The permission family a tenant's own administrator does not hold.
///
/// **The rule this expresses is the one that makes tenant-scoped roles safe.**
/// D-18 gives every tenant its own `ROLE-ADMIN`, and that role holds the whole
/// catalogue; without withholding this family, creating a tenant would hand its
/// administrator the ability to create more of them.
///
/// Withholding it is not the boundary, though — [`require_tenant_administrator`]
/// is. The permission catalogue is global and a tenant administrator holds
/// `identity:role:update`, so nothing stops them adding these codes back to
/// their own role. What stops the request is that it did not come from the
/// deployment's administering tenant.
pub const TENANT_ADMINISTRATION_PREFIX: &str = "organization:tenant:";

/// Confirms the caller may administer tenants, and answers with the tenant they
/// must be in.
///
/// Two conditions, and both are load-bearing:
///
/// 1. **The permission.** `organization:tenant:read` to look, `:manage` to
///    change — the codes `0005_delegation_tenant_permissions.sql` seeded for
///    this surface a sprint before it existed.
/// 2. **The tenant.** The caller must be signed in to the deployment's default
///    tenant (`KELIR_DEFAULT_TENANT_CODE`) — the one the first-run
///    administrator lives in. A tenant is not a row inside a tenant; it is the
///    partition itself, so `tenant_id` scoping cannot express who may touch it
///    and something else has to.
///
/// Resolved from configuration rather than compared against
/// [`crate::db::SYSTEM_TENANT_ID`], for the reason that constant's own doc
/// comment gives: a deployment that repoints its default tenant must not find
/// the answer hardcoded somewhere else.
async fn require_tenant_administrator(
    state: &AppState,
    caller: &Authenticated,
    permission: &str,
) -> Result<Tenant, AppError> {
    caller.require(permission)?;

    let administering = resolve_default(&state.pool, &state.config).await?;

    if caller.tenant_id() != administering.id {
        tracing::info!(
            user_id = %caller.user_id(),
            permission,
            "tenant administration refused: the caller is not in the administering tenant"
        );

        // `Forbidden`, matching `Authenticated::require`: the caller is
        // authenticated and holds the permission, they are simply not where it
        // may be used. Hiding the surface behind a 404 instead would make this
        // indistinguishable from a typo in the path.
        return Err(AppError::Forbidden);
    }

    Ok(administering)
}

/// Maps a repository row onto the published shape.
fn view(record: TenantRecord, administering_id: Uuid) -> TenantView {
    TenantView {
        is_default: record.id == administering_id,
        id: record.id,
        tenant_code: record.tenant_code,
        name: record.name,
        status: record.status,
        user_count: record.user_count,
        created_at: record.created_at,
    }
}

pub async fn list_tenants(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<TenantView>, PageMeta), AppError> {
    let administering =
        require_tenant_administrator(state, caller, "organization:tenant:read").await?;

    let total = repository::count(&state.pool).await?;
    let tenants = repository::list(&state.pool, pagination.limit(), pagination.offset()).await?;

    Ok((
        tenants
            .into_iter()
            .map(|record| view(record, administering.id))
            .collect(),
        pagination.meta(total.max(0) as u64),
    ))
}

pub async fn get_tenant(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<TenantView, AppError> {
    let administering =
        require_tenant_administrator(state, caller, "organization:tenant:read").await?;

    repository::find(&state.pool, id)
        .await?
        .map(|record| view(record, administering.id))
        .ok_or_else(|| AppError::not_found("Tenant"))
}

/// Creates a tenant and the administrator who can sign in to it, in one
/// transaction.
///
/// **The two halves are one transaction because a tenant without a user is the
/// thing D-13 refused to let this surface build** — a row nobody can sign in
/// to. Splitting them into two calls would put that state back within reach of
/// any client that made the first call and not the second.
pub async fn create_tenant(
    state: &AppState,
    caller: &Authenticated,
    request: CreateTenantRequest,
) -> Result<TenantView, AppError> {
    let administering =
        require_tenant_administrator(state, caller, "organization:tenant:manage").await?;

    validate_create_tenant(&request)?;

    let tenant_code = normalize_tenant_code(&request.tenant_code);
    let id = Uuid::now_v7();
    let mut transaction = state.pool.begin().await?;

    repository::insert(
        &mut *transaction,
        id,
        &tenant_code,
        request.name.trim(),
        Some(caller.user_id()),
    )
    .await
    .map_err(duplicate_tenant_to_conflict)?;

    let provisioned = identity::provision_tenant_identity(
        &mut transaction,
        id,
        identity::FirstAdministrator {
            username: &request.administrator.username,
            email: &request.administrator.email,
            display_name: &request.administrator.display_name,
            password: &request.administrator.password,
        },
        TENANT_ADMINISTRATION_PREFIX,
    )
    .await?;

    transaction.commit().await?;

    // Recorded against the administering tenant, not the new one: audit answers
    // "who did this", and the actor and the act both belong here. A record filed
    // under the new tenant would be invisible to the only people who may read
    // this surface.
    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: administering.id,
            event_type: "Tenant.Created",
            action: "CREATE",
            object_type: "TENANT",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(serde_json::json!({
                "tenantCode": tenant_code,
                "name": request.name.trim(),
                "administratorUserId": provisioned.user_id,
                "administratorRoleId": provisioned.role_id,
            })),
        },
    )
    .await;

    tracing::info!(
        tenant_code = %tenant_code,
        user_id = %provisioned.user_id,
        "created a tenant and its first administrator"
    );

    get_tenant_unchecked(state, id, administering.id).await
}

pub async fn update_tenant(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateTenantRequest,
) -> Result<TenantView, AppError> {
    let administering =
        require_tenant_administrator(state, caller, "organization:tenant:manage").await?;

    validate_update_tenant(&request)?;

    let before = repository::find(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("Tenant"))?;

    // Suspending the tenant the request came from would end the session making
    // the request, and there would then be nobody able to undo it — the same
    // refusal `deactivate_user` gives for your own account, for the same reason.
    if id == administering.id && matches!(request.status, Some(status) if !status.admits_sign_in())
    {
        return Err(AppError::bad_request(
            "You cannot suspend the tenant you administer from",
        ));
    }

    let updated = repository::update_fields(
        &state.pool,
        id,
        request.name.as_deref().map(str::trim),
        request.status.map(TenantStatus::as_db),
        Some(caller.user_id()),
    )
    .await?;

    if updated == 0 {
        return Err(AppError::not_found("Tenant"));
    }

    // Taking a tenant offline must end its users' sessions, not merely stop new
    // sign-ins. `resolve_for_sign_in` already refuses a suspended tenant, but a
    // refresh token issued a minute ago would otherwise keep a session alive
    // indefinitely — the mirror of what `update_user` does for an account.
    if matches!(request.status, Some(status) if !status.admits_sign_in()) {
        let revoked = repository::revoke_sessions(&state.pool, id, "tenant suspended").await?;
        tracing::info!(tenant_id = %id, revoked, "revoked sessions for a suspended tenant");
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: administering.id,
            event_type: "Tenant.Updated",
            action: "UPDATE",
            object_type: "TENANT",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(serde_json::json!({
                "name": before.name,
                "status": before.status,
            })),
            new_value: Some(serde_json::json!({
                "name": request.name,
                "status": request.status,
            })),
        },
    )
    .await;

    get_tenant_unchecked(state, id, administering.id).await
}

/// Soft-deletes a tenant and ends its sessions.
///
/// Its users, roles and data are left in place rather than cascaded: a
/// soft-deleted tenant resolves to nothing at sign-in, which is what makes the
/// data unreachable, and hard-deleting it would take the audit trail with it.
pub async fn delete_tenant(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    let administering =
        require_tenant_administrator(state, caller, "organization:tenant:manage").await?;

    if id == administering.id {
        // The deployment's default tenant. Deleting it would make every
        // subsequent sign-in fail to resolve — including the one that would be
        // needed to undo it.
        return Err(AppError::bad_request(
            "You cannot delete the tenant you administer from",
        ));
    }

    let removed = repository::soft_delete(&state.pool, id, Some(caller.user_id())).await?;

    if removed == 0 {
        return Err(AppError::not_found("Tenant"));
    }

    let revoked = repository::revoke_sessions(&state.pool, id, "tenant deleted").await?;
    tracing::info!(tenant_id = %id, revoked, "revoked sessions for a deleted tenant");

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: administering.id,
            event_type: "Tenant.Deleted",
            action: "DELETE",
            object_type: "TENANT",
            object_id: id,
            actor_user_id: Some(caller.user_id()),
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Re-reads a tenant after a write. The permission check has already happened;
/// a row that has just been written and cannot be read back is a fault, not a
/// 404.
async fn get_tenant_unchecked(
    state: &AppState,
    id: Uuid,
    administering_id: Uuid,
) -> Result<TenantView, AppError> {
    repository::find(&state.pool, id)
        .await?
        .map(|record| view(record, administering_id))
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("tenant {id} vanished between writing and reading it"),
        })
}

/// A duplicate `tenant_code` is a conflict, not a server error.
///
/// Detected from the constraint rather than pre-checked, so two concurrent
/// creators cannot both pass a check and then both insert.
fn duplicate_tenant_to_conflict(error: sqlx::Error) -> AppError {
    let is_duplicate = error
        .as_database_error()
        .is_some_and(|database_error| database_error.is_unique_violation());

    if is_duplicate {
        AppError::conflict("That tenant code is already in use")
    } else {
        AppError::from(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::organization::domain::TenantStatus;

    /// Built as a struct literal rather than through the environment, which is
    /// process-global and races across parallel tests.
    ///
    /// **These tests are why removing the boot guard was a one-line change.**
    /// Under **D-7** `AppConfig::from_env` refused `multi_tenant: true`
    /// outright, so nothing could reach the multi-tenant branch below through
    /// configuration — and the branch stayed fully exercised anyway, because
    /// the guard sat on the deployment path rather than on this one. **D-18**
    /// deleted the guard and changed nothing here.
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
