use chrono::Utc;
use uuid::Uuid;

use super::password::verify_password;
use super::token::{
    generate_refresh_token, hash_refresh_token, issue_access_token, ACCESS_TOKEN_TTL_MINUTES,
};
use crate::db::SYSTEM_TENANT_ID;
use crate::error::AppError;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::domain::UserStatus;
use crate::modules::identity::repository as identity_repo;
use crate::state::AppState;

/// What a successful sign-in returns.
pub struct SignedIn {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: Uuid,
    pub username: String,
}

/// Failed attempts before the account is locked (NFR-SEC-008).
pub const MAX_FAILED_LOGINS: i32 = 5;

/// Authenticates a username-or-email and password (FR-AUTH-001..003).
///
/// Every failure returns the same `Unauthorized`. Distinguishing "no such user"
/// from "wrong password" would turn the login endpoint into a way to enumerate
/// accounts, so the difference is recorded in the audit trail instead, where
/// only an administrator sees it.
pub async fn sign_in(
    state: &AppState,
    username: &str,
    password: &str,
    ip_address: Option<&str>,
) -> Result<SignedIn, AppError> {
    let tenant_id = SYSTEM_TENANT_ID;

    let Some(credentials) =
        identity_repo::find_credentials_by_username(&state.pool, tenant_id, username).await?
    else {
        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id,
                event_type: "Security.SignInFailed",
                action: "LOGIN_FAILED",
                object_type: "USER",
                object_id: Uuid::nil(),
                actor_user_id: None,
                ip_address,
                reason: Some("unknown username"),
                old_value: None,
                new_value: None,
            },
        )
        .await;

        // Cost of verifying a hash is paid anyway, so an attacker cannot tell
        // an unknown user from a wrong password by timing the response.
        let _ = verify_password(password, DUMMY_HASH);
        return Err(AppError::Unauthorized);
    };

    let status = UserStatus::from_db(&credentials.status);

    if !status.can_sign_in() {
        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id,
                event_type: "Security.SignInFailed",
                action: "LOGIN_FAILED",
                object_type: "USER",
                object_id: credentials.id,
                actor_user_id: Some(credentials.id),
                ip_address,
                reason: Some("account is not active"),
                old_value: None,
                new_value: None,
            },
        )
        .await;

        return Err(AppError::Unauthorized);
    }

    // Hashing is CPU-bound and deliberately slow; running it on the async
    // runtime would stall other requests (coding standard §2.4).
    let stored_hash = credentials.password_hash.clone();
    let candidate = password.to_owned();
    let verified = tokio::task::spawn_blocking(move || verify_password(&candidate, &stored_hash))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password verification task failed: {error}"),
        })?;

    if !verified {
        let failures = identity_repo::record_failed_login(&state.pool, credentials.id).await?;

        if failures >= MAX_FAILED_LOGINS {
            identity_repo::update_user_fields(
                &state.pool,
                tenant_id,
                credentials.id,
                None,
                None,
                Some(UserStatus::Locked.as_db()),
                None,
                None,
            )
            .await?;

            tracing::warn!(user_id = %credentials.id, failures, "account locked after repeated failures");
        }

        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id,
                event_type: "Security.SignInFailed",
                action: "LOGIN_FAILED",
                object_type: "USER",
                object_id: credentials.id,
                actor_user_id: Some(credentials.id),
                ip_address,
                reason: Some("incorrect password"),
                old_value: None,
                new_value: None,
            },
        )
        .await;

        return Err(AppError::Unauthorized);
    }

    let issued = issue_session(
        state,
        credentials.id,
        tenant_id,
        &credentials.username,
        None,
    )
    .await?;

    identity_repo::record_successful_login(&state.pool, credentials.id).await?;

    // FR-AUTH-008: successful sign-in is an audited event.
    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Security.SignedIn",
            action: "LOGIN",
            object_type: "USER",
            object_id: credentials.id,
            actor_user_id: Some(credentials.id),
            ip_address,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(issued)
}

/// Exchanges a refresh token for a new pair, rotating it (FR-AUTH-003).
///
/// Rotation makes theft detectable: the old token is revoked on use, so if it is
/// ever presented again, either the client or an attacker is replaying it. The
/// safe response to that ambiguity is to revoke the user's whole session family
/// and make everyone sign in again.
pub async fn refresh(
    state: &AppState,
    presented_token: &str,
    ip_address: Option<&str>,
) -> Result<SignedIn, AppError> {
    let token_hash = hash_refresh_token(presented_token);

    let Some(stored) = identity_repo::find_refresh_token(&state.pool, &token_hash).await? else {
        return Err(AppError::Unauthorized);
    };

    if stored.revoked_at.is_some() {
        // A revoked token being presented means it was captured, or the client
        // retried after rotation. Either way the family is no longer trustworthy.
        let revoked =
            identity_repo::revoke_all_for_user(&state.pool, stored.user_id, "refresh token reuse")
                .await?;

        tracing::warn!(
            user_id = %stored.user_id,
            revoked,
            "revoked refresh token presented; ended every session for the user"
        );

        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id: stored.tenant_id,
                event_type: "Security.RefreshTokenReused",
                action: "TOKEN_REUSE",
                object_type: "USER",
                object_id: stored.user_id,
                actor_user_id: Some(stored.user_id),
                ip_address,
                reason: Some("revoked refresh token presented"),
                old_value: None,
                new_value: None,
            },
        )
        .await;

        return Err(AppError::Unauthorized);
    }

    if stored.expires_at <= Utc::now() {
        return Err(AppError::Unauthorized);
    }

    let Some(user) =
        identity_repo::find_user(&state.pool, stored.tenant_id, stored.user_id).await?
    else {
        return Err(AppError::Unauthorized);
    };

    if !user.status.can_sign_in() {
        // Deactivating an account must end its sessions, not merely stop new
        // sign-ins.
        identity_repo::revoke_all_for_user(&state.pool, stored.user_id, "account not active")
            .await?;
        return Err(AppError::Unauthorized);
    }

    identity_repo::revoke_refresh_token(&state.pool, stored.id, "rotated").await?;

    issue_session(
        state,
        stored.user_id,
        stored.tenant_id,
        &user.username,
        Some(stored.id),
    )
    .await
}

/// Ends the session behind a refresh token (FR-AUTH-004).
///
/// Idempotent: signing out twice, or with a token that was never valid, is a
/// success. The client's intent is to have no session, and it does.
pub async fn sign_out(
    state: &AppState,
    presented_token: Option<&str>,
    ip_address: Option<&str>,
) -> Result<(), AppError> {
    let mut actor_user_id = None;

    if let Some(token) = presented_token {
        let hash = hash_refresh_token(token);

        if let Some(stored) = identity_repo::find_refresh_token(&state.pool, &hash).await? {
            identity_repo::revoke_refresh_token(&state.pool, stored.id, "signed out").await?;
            actor_user_id = Some(stored.user_id);
        }
    }

    if let Some(user_id) = actor_user_id {
        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id: SYSTEM_TENANT_ID,
                event_type: "Security.SignedOut",
                action: "LOGOUT",
                object_type: "USER",
                object_id: user_id,
                actor_user_id: Some(user_id),
                ip_address,
                reason: None,
                old_value: None,
                new_value: None,
            },
        )
        .await;
    }

    Ok(())
}

/// Issues an access/refresh pair and stores the refresh digest.
async fn issue_session(
    state: &AppState,
    user_id: Uuid,
    tenant_id: Uuid,
    username: &str,
    rotated_from_id: Option<Uuid>,
) -> Result<SignedIn, AppError> {
    let roles = identity_repo::role_codes_for_user(&state.pool, user_id).await?;
    let permissions = identity_repo::permissions_for_user(&state.pool, user_id).await?;

    let (access_token, _expires_at) = issue_access_token(
        &state.config.jwt_secret,
        user_id,
        tenant_id,
        username,
        roles,
        permissions,
    )?;

    let refresh = generate_refresh_token();

    identity_repo::insert_refresh_token(
        &state.pool,
        Uuid::now_v7(),
        tenant_id,
        user_id,
        &refresh.hash,
        refresh.expires_at,
        rotated_from_id,
    )
    .await?;

    Ok(SignedIn {
        access_token,
        refresh_token: refresh.token,
        user_id,
        username: username.to_owned(),
    })
}

/// Changes the signed-in user's own password (FR-AUTH-005).
///
/// Requires the current password even though the caller is authenticated: an
/// access token left open on a shared machine should not be enough to take the
/// account over permanently.
pub async fn change_own_password(
    state: &AppState,
    user_id: Uuid,
    current_password: &str,
    new_password: &str,
) -> Result<(), AppError> {
    crate::modules::identity::domain::validate_password_value(new_password)?;

    let Some(credentials) =
        identity_repo::find_credentials_by_id(&state.pool, SYSTEM_TENANT_ID, user_id).await?
    else {
        return Err(AppError::Unauthorized);
    };

    let stored = credentials.password_hash.clone();
    let candidate = current_password.to_owned();
    let verified = tokio::task::spawn_blocking(move || verify_password(&candidate, &stored))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password verification task failed: {error}"),
        })?;

    if !verified {
        return Err(AppError::validation(vec![
            crate::error::ValidationDetail::new(
                "currentPassword",
                "incorrect",
                "INCORRECT_PASSWORD",
                "That is not your current password",
            ),
        ]));
    }

    let new_password = new_password.to_owned();
    let hash = tokio::task::spawn_blocking(move || super::password::hash_password(&new_password))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password hashing task failed: {error}"),
        })??;

    identity_repo::set_password_hash(&state.pool, SYSTEM_TENANT_ID, user_id, &hash).await?;

    // Every other session ends: if the change was prompted by a suspected
    // compromise, leaving them alive defeats the point. The caller signs in
    // again like everyone else.
    let revoked =
        identity_repo::revoke_all_for_user(&state.pool, user_id, "password changed").await?;
    tracing::info!(user_id = %user_id, revoked, "password changed; sessions revoked");

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: SYSTEM_TENANT_ID,
            event_type: "User.PasswordChanged",
            action: "UPDATE",
            object_type: "USER",
            object_id: user_id,
            actor_user_id: Some(user_id),
            ip_address: None,
            reason: Some("changed by the account holder"),
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// A real Argon2id hash of a value nobody knows, verified against when the user
/// does not exist so that path costs the same as a wrong password.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHR2YWx1ZQ$Yw5DLJDMWvDBrKQwCFuoiIMBpVDGiVXKJPCPNVFtRhk";

/// Seconds until the access token expires, for the client to schedule a refresh.
pub fn access_token_ttl_seconds() -> i64 {
    ACCESS_TOKEN_TTL_MINUTES * 60
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dummy_hash_is_a_usable_argon2_hash() {
        // If this stopped parsing, the unknown-user path would return instantly
        // and reintroduce the timing difference it exists to remove.
        assert!(!verify_password("anything", DUMMY_HASH));
        assert!(DUMMY_HASH.starts_with("$argon2id$"));
    }

    #[test]
    fn lockout_threshold_matches_the_requirement() {
        // NFR-SEC-008 baselined 5 failed logins.
        assert_eq!(MAX_FAILED_LOGINS, 5);
    }

    #[test]
    fn access_ttl_is_reported_in_seconds() {
        assert_eq!(access_token_ttl_seconds(), ACCESS_TOKEN_TTL_MINUTES * 60);
    }
}
