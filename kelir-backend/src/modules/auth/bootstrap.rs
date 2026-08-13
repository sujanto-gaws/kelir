//! First-run administrator.
//!
//! `0002_identity.sql` seeds `ROLE-ADMIN` with every permission, but no user
//! holds it — so a freshly deployed instance has a complete permission model
//! and nobody who can use it.
//!
//! Seeding a user with a fixed password in the migration would solve that and
//! be worse: the credentials would be in the repository, identical across every
//! deployment, and unchangeable without a new migration. Instead the account is
//! created at startup from configuration, once, and only when no user exists.

use uuid::Uuid;

use super::password::hash_password;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::repository as identity_repo;
use crate::modules::organization::service as organization;

/// The role every bootstrap administrator is granted, seeded by 0002.
const ADMIN_ROLE_ID: Uuid = uuid::uuid!("00000000-0000-0000-0002-000000000001");

/// Advisory lock held for the creating transaction. The value is the ASCII of
/// `KELIRBT!` and carries no meaning beyond being unlikely to collide with
/// another advisory lock in the same database.
const BOOTSTRAP_LOCK_KEY: i64 = 0x4B45_4C49_5242_5421;

/// Creates the first administrator when this database holds no user at all.
///
/// The guard is deliberately not tenant-scoped, and deliberately counts
/// soft-deleted rows: it asks whether this database has *ever* had a user, not
/// whether some tenant has a live one. Either narrowing re-arms a switch that
/// creates an account holding every permission out of `KELIR_BOOTSTRAP_ADMIN_*`
/// — variables that stay set in a deployment's environment long after first
/// run:
///
/// * Scoped to a tenant, it fires again as soon as users stop sitting in the
///   tenant it happens to count. Which tenant that is is a deployment setting
///   (FR-IDM-009 resolves the sign-in tenant from configuration), so it can
///   change without anyone being removed.
/// * Restricted to live rows, it fires again as soon as the last account is
///   soft-deleted — and `uq_users_tenant_id_username` is partial on
///   `deleted_at IS NULL`, so a second administrator would be created beside
///   the removed one rather than colliding with it.
///
/// What the guard therefore guarantees: while any row remains in `users` — any
/// tenant, live or soft-deleted — this does nothing, so it can neither
/// resurrect a removed account nor mint a second administrator beside an
/// existing one. It can still fire on a database whose `users` table is empty,
/// which is a genuinely new deployment, or one whose users were hard-deleted by
/// maintenance, by an import, or by a restore taken from before the first
/// account existed. A deployment that leaves `KELIR_BOOTSTRAP_ADMIN_*` set
/// after first run is choosing that last behaviour; unsetting them turns the
/// switch off outright.
///
/// Which tenant the account is created *in* is a separate question from whether
/// to create one at all, and it is the deployment's default tenant (FR-IDM-009).
///
/// Every guarantee above holds with `KELIR_MULTI_TENANT` on, unchanged, because
/// none of them mentions a tenant. What that mode does *not* get is a per-tenant
/// bootstrap: a tenant created later, with no users of its own, does not receive
/// an administrator from configuration on the next restart. That is deliberate.
/// This is a first-run switch for the deployment, and a switch that fires
/// whenever some tenant happens to be empty would be a standing path from an
/// environment variable to an account holding every permission — with a role
/// that `0002_identity.sql` seeds in the system tenant. Giving a new tenant its
/// first administrator belongs to whoever provisions the tenant, through the API
/// and `organization:tenant:manage`, not to a process-wide variable.
///
/// Replicas starting together are serialised by an advisory lock and the re-read
/// it protects, so exactly one of them creates the account and the others stand
/// down rather than failing their startup.
pub async fn ensure_administrator(pool: &sqlx::PgPool, config: &AppConfig) -> Result<(), AppError> {
    // The first administrator belongs to the deployment's default tenant in
    // both modes: a multi-tenant deployment still needs one account before any
    // tenant can be created. Resolving it here rather than assuming the seeded
    // system tenant means `KELIR_DEFAULT_TENANT_CODE` is honoured at startup
    // too, and a code pointing at no tenant fails the boot instead of silently
    // creating the account somewhere else. It is resolved before the guard, and
    // not only when the account is about to be created, so that misconfiguration
    // is reported on the next restart rather than on the next sign-in.
    let tenant = organization::resolve_default(pool, config).await?;
    let tenant_id = tenant.id;

    if identity_repo::any_user_exists(pool).await? {
        return Ok(());
    }

    let Some(credentials) = config.bootstrap_admin.as_ref() else {
        // Not an error: a deployment may intend to create its first user by
        // another route. But it is worth saying loudly, because the symptom
        // otherwise is a working API that nobody can sign in to.
        tracing::warn!(
            "no users exist and KELIR_BOOTSTRAP_ADMIN_USERNAME/PASSWORD are unset — \
             nobody can sign in; set them and restart to create the first administrator"
        );
        return Ok(());
    };

    let username = credentials.username.clone();
    let password = credentials.password.clone();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password hashing task failed: {error}"),
        })??;

    let id = Uuid::now_v7();
    let mut transaction = pool.begin().await?;

    // Two replicas starting together would otherwise both read an empty table
    // and both insert, and the loser would fail its startup on the unique
    // index. The lock is tried rather than waited on: a holder means another
    // instance is already creating the account, which is the outcome this one
    // wants anyway. It is released when the transaction ends, so a crashed
    // instance cannot leave the bootstrap wedged.
    let acquired = sqlx::query_scalar!(
        r#"SELECT pg_try_advisory_xact_lock($1) AS "acquired!""#,
        BOOTSTRAP_LOCK_KEY
    )
    .fetch_one(&mut *transaction)
    .await?;

    if !acquired {
        transaction.rollback().await?;
        tracing::info!("another instance is creating the first administrator; standing down");
        return Ok(());
    }

    // Re-read under the lock: the first check ran before the transaction began,
    // so another instance may have committed in between.
    if identity_repo::any_user_exists(&mut *transaction).await? {
        transaction.rollback().await?;
        tracing::info!("another instance created the first administrator; standing down");
        return Ok(());
    }

    identity_repo::insert_user(
        &mut *transaction,
        id,
        tenant_id,
        &credentials.username,
        &credentials.email,
        &password_hash,
        "Administrator",
        None,
        None,
    )
    .await?;

    identity_repo::replace_user_roles(&mut transaction, tenant_id, id, &[ADMIN_ROLE_ID]).await?;

    transaction.commit().await?;

    // The account is auditable from the moment it exists, so its creation is
    // not a gap at the very start of the chain.
    audit::record_or_warn(
        pool,
        AuditEntry {
            tenant_id,
            event_type: "User.Created",
            action: "CREATE",
            object_type: "USER",
            object_id: id,
            actor_user_id: None,
            ip_address: None,
            reason: Some("first-run administrator created from configuration"),
            old_value: None,
            new_value: None,
        },
    )
    .await;

    // The username, never the password: the operator already knows it, and
    // logs are the last place a credential should appear (coding standard §2.7).
    tracing::info!(
        username = %username,
        "created the first administrator; it must change its password after signing in"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::config::BootstrapAdmin;
    // The tenant the tests read is the one `resolve_default` lands on for the
    // default configuration: the tenant seeded by 0001_core.sql.
    use crate::db::SYSTEM_TENANT_ID;
    use crate::modules::auth::password::verify_password;

    const BOOTSTRAP_PASSWORD: &str = "a-real-bootstrap-password";

    fn config_with_admin(password: &str) -> AppConfig {
        let mut config = AppConfig::test_default();
        config.bootstrap_admin = Some(BootstrapAdmin {
            username: "admin".to_owned(),
            email: "admin@example.com".to_owned(),
            password: password.to_owned(),
        });
        config
    }

    async fn seed_user(pool: &PgPool, username: &str) -> Uuid {
        let id = Uuid::now_v7();

        identity_repo::insert_user(
            pool,
            id,
            SYSTEM_TENANT_ID,
            username,
            &format!("{username}@example.com"),
            "$argon2id$not-a-real-hash",
            "Existing",
            None,
            None,
        )
        .await
        .expect("seeds a user");

        id
    }

    async fn live_users(pool: &PgPool) -> i64 {
        identity_repo::count_users(pool, SYSTEM_TENANT_ID)
            .await
            .expect("counts users")
    }

    async fn administrator(pool: &PgPool) -> identity_repo::UserCredentials {
        identity_repo::find_credentials_by_username(pool, SYSTEM_TENANT_ID, "admin")
            .await
            .expect("reads the administrator")
            .expect("the administrator exists")
    }

    #[sqlx::test]
    async fn creates_the_administrator_when_no_user_exists(pool: PgPool) {
        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("bootstraps");

        let users = identity_repo::list_users(&pool, SYSTEM_TENANT_ID, 10, 0)
            .await
            .expect("lists users");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "admin");
        assert!(verify_password(
            BOOTSTRAP_PASSWORD,
            &administrator(&pool).await.password_hash
        ));
    }

    #[sqlx::test]
    async fn creates_nothing_when_a_user_already_exists(pool: PgPool) {
        seed_user(&pool, "existing").await;

        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("stands down");

        let users = identity_repo::list_users(&pool, SYSTEM_TENANT_ID, 10, 0)
            .await
            .expect("lists users");

        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "existing");
    }

    #[sqlx::test]
    async fn creates_nothing_when_the_only_user_is_in_another_tenant(pool: PgPool) {
        // The reason the guard is not tenant-scoped: which tenant users sit in
        // is a deployment setting (FR-IDM-009), so a counter that reads one
        // tenant would report an empty instance and bootstrap a second
        // administrator on every restart of a deployment that moved.
        let tenant_id = Uuid::now_v7();
        sqlx::query!(
            "INSERT INTO tenants (id, tenant_code, name) VALUES ($1, 'TNT-002', 'Another')",
            tenant_id
        )
        .execute(&pool)
        .await
        .expect("seeds a tenant");

        identity_repo::insert_user(
            &pool,
            Uuid::now_v7(),
            tenant_id,
            "elsewhere",
            "elsewhere@example.com",
            "$argon2id$not-a-real-hash",
            "Elsewhere",
            None,
            None,
        )
        .await
        .expect("seeds a user in another tenant");

        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("stands down");

        assert_eq!(live_users(&pool).await, 0);
    }

    #[sqlx::test]
    async fn running_the_bootstrap_twice_creates_one_administrator(pool: PgPool) {
        let config = config_with_admin(BOOTSTRAP_PASSWORD);

        ensure_administrator(&pool, &config)
            .await
            .expect("first run");
        ensure_administrator(&pool, &config)
            .await
            .expect("second run");

        assert_eq!(live_users(&pool).await, 1);
    }

    #[sqlx::test]
    async fn does_not_run_when_a_soft_deleted_user_is_the_only_row(pool: PgPool) {
        // The soft-deleted row is invisible to a live-only count and to the
        // partial unique index, so a guard that looked only at live rows would
        // create a second administrator beside the removed one rather than
        // failing on the index.
        let id = seed_user(&pool, "removed").await;
        identity_repo::soft_delete_user(&pool, SYSTEM_TENANT_ID, id, None)
            .await
            .expect("soft deletes");

        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("stands down");

        assert_eq!(
            live_users(&pool).await,
            0,
            "the bootstrap recreated an administrator after the account was removed"
        );
    }

    #[sqlx::test]
    async fn does_not_reset_an_existing_administrators_password(pool: PgPool) {
        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("first run");
        let original = administrator(&pool).await.password_hash;

        // A deployment that edits the variable after first run must not have
        // its administrator's password reset from configuration on restart.
        ensure_administrator(&pool, &config_with_admin("a-different-bootstrap-password"))
            .await
            .expect("second run");

        assert_eq!(administrator(&pool).await.password_hash, original);
        assert!(verify_password(BOOTSTRAP_PASSWORD, &original));
    }

    #[sqlx::test]
    async fn grants_the_administrator_the_role_seeded_by_the_migration(pool: PgPool) {
        ensure_administrator(&pool, &config_with_admin(BOOTSTRAP_PASSWORD))
            .await
            .expect("bootstraps");

        let admin = administrator(&pool).await;

        // Asserted against the seeded row, not against ADMIN_ROLE_ID: a
        // migration that renamed or repointed the administrator role would
        // leave the constant granting a role nobody has.
        let seeded = sqlx::query!(
            "SELECT id, role_code FROM roles WHERE tenant_id = $1 AND is_system = true",
            SYSTEM_TENANT_ID
        )
        .fetch_one(&pool)
        .await
        .expect("the migration seeds a system role");

        let granted = identity_repo::roles_of_user(&pool, admin.id)
            .await
            .expect("reads the granted roles");

        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].id, seeded.id);
        assert_eq!(granted[0].role_code, seeded.role_code);

        // And the role is worth having: the account can administer everything
        // the catalogue defines, which is the reason the bootstrap exists.
        let mut held = identity_repo::permissions_for_user(&pool, admin.id)
            .await
            .expect("reads permissions");
        let mut defined: Vec<String> = identity_repo::list_permissions(&pool)
            .await
            .expect("reads the catalogue")
            .into_iter()
            .map(|permission| permission.permission_code)
            .collect();

        held.sort();
        defined.sort();
        assert!(!defined.is_empty());
        assert_eq!(held, defined);
    }

    #[sqlx::test]
    async fn creates_nothing_when_no_credentials_are_configured(pool: PgPool) {
        ensure_administrator(&pool, &AppConfig::test_default())
            .await
            .expect("warns rather than failing startup");

        assert_eq!(live_users(&pool).await, 0);
    }
}
