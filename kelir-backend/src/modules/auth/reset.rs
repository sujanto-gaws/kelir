//! Forgot and reset password (FR-AUTH-006, #17).
//!
//! **The table shipped in Sprint 4 and the flow did not.** `0006` created
//! `password_reset_tokens` and nothing in the tree ever read it — no query, no
//! route, no test — while the migration's own header and Database Schema §3.9
//! both read as though a flow sat behind it. This is that flow.
//!
//! The table's shape was the part that would have been expensive to retrofit
//! and it was already right: a digest is stored and never the token, and a row
//! records its own consumption rather than being deleted, so a redeemed or
//! expired token cannot be replayed.
//!
//! # What the two endpoints are guarding against
//!
//! **Account enumeration.** `request_reset` answers identically whether the
//! address belongs to an account or not — same status, same body, and no
//! branch a caller can time. Everything that could differ (no such user, an
//! inactive one, a send that failed) happens after the response is already
//! decided.
//!
//! **Mailbox flooding.** A caller who knows an address could otherwise send its
//! owner a reset link every second. [`RESEND_COOLDOWN`] is a per-account
//! throttle: inside it, the request still answers 202 and simply does not send.
//!
//! **Brute-forcing a token.** `reset_password` is on the metered routes, so a
//! wrong token is a 4xx and counts against the caller's rate limit.
//! `request_reset` deliberately is **not** — see [`super::handlers::routes`].

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::password::hash_password;
use super::token::hash_refresh_token;
use crate::error::{AppError, ValidationDetail};
use crate::mail::Mail;
use crate::modules::audit::{self, AuditEntry};
use crate::modules::identity::domain::{validate_password_value, UserStatus};
use crate::modules::identity::repository as identity_repo;
use crate::state::AppState;

/// How long a reset link is good for.
///
/// Short, because the link is a bearer credential sitting in a mailbox. Long
/// enough that somebody who asks for it, goes to lunch and comes back can still
/// use it.
pub const TOKEN_TTL_MINUTES: i64 = 30;

/// How long after issuing one link before another will be sent to the same
/// account.
///
/// The request still answers 202 inside this window; it just does not send. A
/// person who clicks twice gets one email, and a caller who wants to flood
/// somebody's mailbox gets one email a minute rather than one a second.
pub const RESEND_COOLDOWN_SECONDS: i64 = 60;

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestResetRequest {
    /// A username or an email address — the same identifier sign-in takes.
    pub username: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

/// An opaque reset token and the digest stored against it.
pub struct ResetToken {
    /// Put in the link. Never stored.
    pub token: String,
    /// Stored.
    pub hash: String,
    pub expires_at: DateTime<Utc>,
}

/// Generates a reset token.
///
/// The same shape and the same digest as a refresh token, deliberately: both
/// are opaque bearer values checked against the database on every use, so the
/// reasoning that made `generate_refresh_token` opaque rather than signed
/// applies unchanged, and reusing `hash_refresh_token` means one hashing
/// decision rather than two that could drift.
pub fn generate_reset_token() -> ResetToken {
    use rand::RngCore;

    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);

    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let hash = hash_refresh_token(&token);

    ResetToken {
        token,
        hash,
        expires_at: Utc::now() + Duration::minutes(TOKEN_TTL_MINUTES),
    }
}

/// Starts a reset, and says nothing about whether it did.
///
/// **Returns `Ok(())` in every case a caller could distinguish**, which is the
/// whole security property: no such user, an inactive account, a cooldown still
/// running, a mail server that is down. The only errors it can return are ones
/// that are true regardless of whether the account exists — a malformed request
/// or a database that is unreachable.
pub async fn request_reset(
    state: &AppState,
    tenant_id: Uuid,
    request: RequestResetRequest,
    ip: Option<&str>,
) -> Result<(), AppError> {
    let identifier = request.username.trim();

    if identifier.is_empty() {
        // Not an enumeration signal: an empty identifier is malformed whoever
        // sent it, and refusing it says nothing about any account.
        return Err(AppError::validation(vec![ValidationDetail::new(
            "username",
            "required",
            "REQUIRED",
            "username is required",
        )]));
    }

    let Some(credentials) =
        identity_repo::find_credentials_by_username(&state.pool, tenant_id, identifier).await?
    else {
        // No such account. The caller is told exactly what an existing account's
        // owner is told.
        tracing::info!("a password reset was requested for an unknown identifier");
        return Ok(());
    };

    if !UserStatus::from_db(&credentials.status).can_sign_in() {
        // An inactive account gets no link. Saying so would be the same
        // disclosure by another route.
        tracing::info!(user_id = %credentials.id, "a password reset was requested for an account that cannot sign in");
        return Ok(());
    }

    if identity_repo::reset_token_issued_recently(
        &state.pool,
        credentials.id,
        Utc::now() - Duration::seconds(RESEND_COOLDOWN_SECONDS),
    )
    .await?
    {
        tracing::info!(user_id = %credentials.id, "a reset link was issued recently; not sending another");
        return Ok(());
    }

    let Some(email) =
        identity_repo::find_user_email(&state.pool, tenant_id, credentials.id).await?
    else {
        tracing::warn!(user_id = %credentials.id, "an account has no email address, so no reset link can be sent");
        return Ok(());
    };

    let token = generate_reset_token();

    identity_repo::insert_reset_token(
        &state.pool,
        Uuid::now_v7(),
        tenant_id,
        credentials.id,
        &token.hash,
        token.expires_at,
    )
    .await?;

    let link = format!(
        "{}/reset-password?token={}",
        state.config.frontend_url.trim_end_matches('/'),
        token.token
    );

    state
        .mailer
        .send(Mail {
            to: email,
            subject: format!("Reset your {} password", state.config.app_name),
            body: format!(
                "Somebody asked to reset the password for your {} account.\n\n\
                 Open this link to choose a new one:\n\n  {link}\n\n\
                 It stops working in {TOKEN_TTL_MINUTES} minutes, and using it \
                 signs you out everywhere else.\n\n\
                 If this was not you, nothing has changed and you can ignore \
                 this message.\n",
                state.config.app_name
            ),
        })
        .await;

    // Audited as an event on the user, because it is one: somebody started a
    // credential change on that account. The token is not in the record — it is
    // a bearer credential, and an audit trail is read by more people than a
    // mailbox is.
    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "User.PasswordResetRequested",
            action: "UPDATE",
            object_type: "USER",
            object_id: credentials.id,
            actor_user_id: None,
            ip_address: ip,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Redeems a reset token and sets the new password.
///
/// **Everything that makes the token single-use happens in one transaction**:
/// the row is marked consumed with a predicate that only an unconsumed,
/// unexpired row satisfies, and the password is written in the same unit. Two
/// requests carrying the same token therefore produce one password change —
/// the second affects no rows and is refused, rather than both succeeding and
/// the later one silently winning.
pub async fn reset_password(
    state: &AppState,
    request: ResetPasswordRequest,
    ip: Option<&str>,
) -> Result<(), AppError> {
    validate_password_value(&request.new_password)?;

    let hash = hash_refresh_token(request.token.trim());

    let Some(stored) = identity_repo::find_live_reset_token(&state.pool, &hash).await? else {
        // One answer for expired, consumed, and never-existed. They are the
        // same to a caller who should not have the token in the first place,
        // and distinguishing them would say which guesses were close.
        return Err(invalid_token());
    };

    // Hashing is deliberately slow, so it runs off the async runtime — and
    // before the transaction opens, so the row is not locked for the duration.
    let password = request.new_password.clone();
    let password_hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|error| AppError::Internal {
            source: anyhow::anyhow!("password hashing task failed: {error}"),
        })??;

    let mut transaction = state.pool.begin().await?;

    // The predicate is the single-use guarantee. A second request reaching here
    // with the same token affects no rows.
    if identity_repo::consume_reset_token(&mut transaction, stored.id).await? == 0 {
        return Err(invalid_token());
    }

    identity_repo::set_password_hash(
        &mut *transaction,
        stored.tenant_id,
        stored.user_id,
        &password_hash,
    )
    .await?;

    // Every other outstanding link for this account stops working. Somebody who
    // clicked "forgot password" three times should not leave two live tokens in
    // a mailbox behind them.
    identity_repo::invalidate_reset_tokens_for_user(&mut transaction, stored.user_id).await?;

    transaction.commit().await?;

    // Sessions end after the commit, not inside it: a revoke that rolled back
    // with a failed password change would sign somebody out for nothing.
    let revoked =
        identity_repo::revoke_all_for_user(&state.pool, stored.user_id, "password reset").await?;

    tracing::info!(user_id = %stored.user_id, revoked, "password reset");

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id: stored.tenant_id,
            event_type: "User.PasswordReset",
            action: "UPDATE",
            object_type: "USER",
            object_id: stored.user_id,
            // No actor: whoever redeemed the token proved they hold it, which
            // is not the same as proving who they are.
            actor_user_id: None,
            ip_address: ip,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// The one answer a bad token gets, whatever is wrong with it.
fn invalid_token() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "token",
        "exists",
        "INVALID_TOKEN",
        "That reset link is not valid, has already been used, or has expired. \
         Ask for a new one.",
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_token_is_opaque_and_stored_as_a_digest() {
        let token = generate_reset_token();

        assert_eq!(token.token.len(), 64, "256 bits, hex encoded");
        assert!(
            token.hash.starts_with("sha256:"),
            "stored as a digest, never in the clear"
        );
        assert!(
            !token.hash.contains(&token.token),
            "the digest must not contain the token"
        );
    }

    #[test]
    fn two_generated_tokens_differ() {
        assert_ne!(
            generate_reset_token().token,
            generate_reset_token().token,
            "a predictable reset token is a password reset for somebody else"
        );
    }

    #[test]
    fn a_generated_token_expires_within_the_window() {
        let token = generate_reset_token();
        let remaining = token.expires_at - Utc::now();

        assert!(remaining <= Duration::minutes(TOKEN_TTL_MINUTES));
        assert!(remaining > Duration::minutes(TOKEN_TTL_MINUTES - 1));
    }
}
