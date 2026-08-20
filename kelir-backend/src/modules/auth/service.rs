use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::password::verify_password;
use super::token::{
    generate_refresh_token, hash_refresh_token, issue_access_token, ACCESS_TOKEN_TTL_MINUTES,
};
use crate::error::AppError;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::domain::UserStatus;
use crate::modules::identity::repository as identity_repo;
use crate::modules::organization::service::{self as organization, TenantResolutionError};
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

/// How long that lock lasts (NFR-SEC-008).
///
/// The requirement baselines both numbers in one sentence — "5 failed logins
/// trigger a 15-minute lockout" — so the lockout expires rather than standing
/// until an administrator clears it. A permanent lock would be a stronger
/// control on paper and a denial-of-service lever in practice: five wrong
/// passwords fit inside the rate limiter's per-minute budget, so anyone with a
/// username list could disable every account in the deployment, and a
/// single-administrator deployment would have no way back in (#55).
pub const LOCKOUT_MINUTES: i32 = 15;

/// Whether a `locked_until` value means the account is locked out *now*.
///
/// `None` is an account that has never been locked; a timestamp already past is
/// one whose lockout has expired. Expiry is a comparison rather than a scheduled
/// job, so there is nothing to sweep and nothing to miss: a deployment that was
/// down while a lockout elapsed comes back with it already elapsed.
pub fn is_locked_out(locked_until: Option<DateTime<Utc>>) -> bool {
    locked_until.is_some_and(|until| until > Utc::now())
}

/// Authenticates a username-or-email and password (FR-AUTH-001..003).
///
/// Every failure returns the same `Unauthorized`. Distinguishing "no such user"
/// from "wrong password" would turn the login endpoint into a way to enumerate
/// accounts, so the difference is recorded in the audit trail instead, where
/// only an administrator sees it.
///
/// `tenant_code` names the tenant to authenticate against (FR-IDM-009). It is
/// ignored unless the deployment runs in multi-tenant mode, where it is
/// required; the resolved tenant scopes every query below and becomes the JWT
/// `tenant_id` claim that the rest of the system trusts.
pub async fn sign_in(
    state: &AppState,
    tenant_code: Option<&str>,
    username: &str,
    password: &str,
    ip_address: Option<&str>,
) -> Result<SignedIn, AppError> {
    let tenant =
        match organization::resolve_for_sign_in(&state.pool, &state.config, tenant_code).await {
            Ok(tenant) => tenant,
            Err(error @ (TenantResolutionError::Unknown | TenantResolutionError::NotActive)) => {
                // Same refusal, and the same cost, as a wrong password. An
                // unknown tenant that answered instantly would be as good an
                // enumeration oracle as a distinct error code.
                tracing::warn!(reason = %error, "sign-in refused: tenant did not resolve");
                let _ = verify_password(password, DUMMY_HASH);

                // No audit row: `audit_events.tenant_id` references `tenants`,
                // and there is no tenant here to reference. The warning above
                // is the operator-facing record.
                return Err(AppError::Unauthorized);
            }
            Err(error) => return Err(error.into()),
        };

    let tenant_id = tenant.id;

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

    // A lockout in force refuses before the password is even looked at, so the
    // attempts an attacker gets are bounded by the clock rather than by how fast
    // they can send them. Like the branch above it this returns early without
    // paying the hashing cost, and for the same reason: the caller learns
    // nothing from the timing that the refusal itself does not already tell
    // them, and paying Argon2 on a request that cannot succeed would make the
    // lockout a way to spend the server's CPU rather than a way to protect it.
    if is_locked_out(credentials.locked_until) {
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
                reason: Some("account is locked out after repeated failures"),
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
        // Counting the failure and locking on the fifth are one statement in the
        // repository, so concurrent attempts cannot each see a count below the
        // threshold and let all of them through.
        let outcome = identity_repo::record_failed_login(
            &state.pool,
            tenant_id,
            credentials.id,
            MAX_FAILED_LOGINS,
            LOCKOUT_MINUTES,
        )
        .await?;

        if let Some(locked_until) = outcome.locked_until.filter(|until| *until > Utc::now()) {
            tracing::warn!(
                user_id = %credentials.id,
                %locked_until,
                "account locked out after repeated failures"
            );
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

    identity_repo::record_successful_login(&state.pool, tenant_id, credentials.id).await?;

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
    // The tenant comes from the stored token, not from configuration: the
    // session already knows which tenant it belongs to, and that is the tenant
    // the audit row belongs in.
    let mut actor = None;

    if let Some(token) = presented_token {
        let hash = hash_refresh_token(token);

        if let Some(stored) = identity_repo::find_refresh_token(&state.pool, &hash).await? {
            identity_repo::revoke_refresh_token(&state.pool, stored.id, "signed out").await?;
            actor = Some((stored.tenant_id, stored.user_id));
        }
    }

    if let Some((tenant_id, user_id)) = actor {
        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id,
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
///
/// `tenant_id` comes from the caller's own access token, so the account being
/// changed is always the one the session was issued for — there is no tenant to
/// resolve here and nothing the caller can supply to point it elsewhere.
pub async fn change_own_password(
    state: &AppState,
    tenant_id: Uuid,
    user_id: Uuid,
    current_password: &str,
    new_password: &str,
) -> Result<(), AppError> {
    crate::modules::identity::domain::validate_password_value(new_password)?;

    let Some(credentials) =
        identity_repo::find_credentials_by_id(&state.pool, tenant_id, user_id).await?
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
        // Audited like a failed sign-in, and for the same reason: someone
        // holding a live session and guessing at the password is the shape a
        // hijacked session takes, and it is invisible in the login record
        // because no login is happening.
        audit::record_or_warn(
            &state.pool,
            AuditEntry {
                tenant_id,
                event_type: "Security.PasswordChangeFailed",
                action: "UPDATE_FAILED",
                object_type: "USER",
                object_id: user_id,
                actor_user_id: Some(user_id),
                ip_address: None,
                reason: Some("current password did not match"),
                old_value: None,
                new_value: None,
            },
        )
        .await;

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

    identity_repo::set_password_hash(&state.pool, tenant_id, user_id, &hash).await?;

    // Every refresh token is revoked: if the change was prompted by a suspected
    // compromise, leaving them alive defeats the point. The caller signs in
    // again like everyone else.
    //
    // **This does not end an access token already issued.** Access tokens are
    // stateless JWTs checked against no revocation list — that is the trade
    // architecture 01 §18.1 makes to keep authorization off the database — so
    // one issued a moment before this call stays valid until it expires, up to
    // `token::ACCESS_TOKEN_TTL_MINUTES`. What a password change guarantees is
    // that the session cannot be *extended*: the window is bounded and short,
    // not zero. The contract used to claim otherwise; #60 found it, and the
    // wording here and in the OpenAPI response now says what the code does.
    // Closing the window for real needs a per-request revocation check, which
    // is the design decision §18.1 declines.
    let revoked =
        identity_repo::revoke_all_for_user(&state.pool, user_id, "password changed").await?;
    tracing::info!(user_id = %user_id, revoked, "password changed; sessions revoked");

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
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
    use chrono::Duration;

    use super::*;

    #[test]
    fn the_dummy_hash_is_a_usable_argon2_hash() {
        // If this stopped parsing, the unknown-user path would return instantly
        // and reintroduce the timing difference it exists to remove.
        assert!(!verify_password("anything", DUMMY_HASH));
        assert!(DUMMY_HASH.starts_with("$argon2id$"));
    }

    #[test]
    fn a_lockout_expires_and_an_absent_one_never_started() {
        // The entire expiry mechanism is this comparison: nothing sweeps the
        // table, so a lockout ends because time passed rather than because
        // something ran. An `is_locked_out` that ignored the timestamp would
        // reinstate the permanent lock of #55, and this is what sees it.
        //
        // That the numbers are 5 and 15 is not asserted here. A constant
        // compared against itself passes whatever the login path does with it —
        // which is how the permanent lockout shipped under a green suite. The
        // threshold and the duration are proved in `tests/auth_lockout.rs`, by
        // driving real requests until the lock goes on and past the expiry until
        // it comes off.
        assert!(is_locked_out(Some(Utc::now() + Duration::minutes(1))));
        assert!(!is_locked_out(Some(Utc::now() - Duration::minutes(1))));
        assert!(!is_locked_out(None));
    }

    #[test]
    fn access_ttl_is_reported_in_seconds() {
        assert_eq!(access_token_ttl_seconds(), ACCESS_TOKEN_TTL_MINUTES * 60);
    }
}
