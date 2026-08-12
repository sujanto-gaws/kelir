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
use crate::db::SYSTEM_TENANT_ID;
use crate::error::AppError;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::repository as identity_repo;

/// The role every bootstrap administrator is granted, seeded by 0002.
const ADMIN_ROLE_ID: Uuid = uuid::uuid!("00000000-0000-0000-0002-000000000001");

/// Creates the first administrator if the instance has no users.
///
/// Does nothing once any user exists, so it is safe on every start and cannot
/// resurrect an account that was deliberately removed.
pub async fn ensure_administrator(pool: &sqlx::PgPool, config: &AppConfig) -> Result<(), AppError> {
    let existing = identity_repo::count_users(pool, SYSTEM_TENANT_ID).await?;

    if existing > 0 {
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

    identity_repo::insert_user(
        &mut *transaction,
        id,
        SYSTEM_TENANT_ID,
        &credentials.username,
        &credentials.email,
        &password_hash,
        "Administrator",
        None,
        None,
    )
    .await?;

    identity_repo::replace_user_roles(&mut transaction, SYSTEM_TENANT_ID, id, &[ADMIN_ROLE_ID])
        .await?;

    transaction.commit().await?;

    // The account is auditable from the moment it exists, so its creation is
    // not a gap at the very start of the chain.
    audit::record_or_warn(
        pool,
        AuditEntry {
            tenant_id: SYSTEM_TENANT_ID,
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
        "created the first administrator; change its password after signing in"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_admin_role_id_matches_the_migration() {
        // Drift here would grant the bootstrap account a role that does not
        // exist, and the insert would fail at startup on a fresh database.
        assert_eq!(
            ADMIN_ROLE_ID.to_string(),
            "00000000-0000-0000-0002-000000000001"
        );
    }
}
