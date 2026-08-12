use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;

/// How long an access token stays valid.
///
/// Short by design: an access token cannot be revoked, so its lifetime is the
/// window in which a stolen one is useful. Continuity comes from the refresh
/// token, which can be revoked.
pub const ACCESS_TOKEN_TTL_MINUTES: i64 = 15;

/// How long a refresh token stays valid. Rotated on every use.
pub const REFRESH_TOKEN_TTL_DAYS: i64 = 30;

/// Access token payload (architecture 01 §18.1).
///
/// Permissions are embedded so authorisation does not query the database on
/// every request. The cost is staleness: a permission revoked mid-session takes
/// effect when the access token expires, within `ACCESS_TOKEN_TTL_MINUTES`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    /// Subject — the user id.
    pub sub: Uuid,
    pub tenant_id: Uuid,
    pub username: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    /// Expiry, seconds since the epoch.
    pub exp: i64,
    /// Issued at, seconds since the epoch.
    pub iat: i64,
}

impl AccessClaims {
    pub fn has_permission(&self, required: &str) -> bool {
        self.permissions.iter().any(|held| held == required)
    }
}

/// Issues a signed access token.
pub fn issue_access_token(
    secret: &str,
    user_id: Uuid,
    tenant_id: Uuid,
    username: &str,
    roles: Vec<String>,
    permissions: Vec<String>,
) -> Result<(String, DateTime<Utc>), AppError> {
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::minutes(ACCESS_TOKEN_TTL_MINUTES);

    let claims = AccessClaims {
        sub: user_id,
        tenant_id,
        username: username.to_owned(),
        roles,
        permissions,
        exp: expires_at.timestamp(),
        iat: issued_at.timestamp(),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|error| AppError::Internal {
        source: anyhow::anyhow!("failed to sign access token: {error}"),
    })?;

    Ok((token, expires_at))
}

/// Verifies a token's signature and expiry, returning its claims.
///
/// Every failure becomes `Unauthorized`: the reason a token is unacceptable is
/// not something a caller needs, and distinguishing "expired" from "forged"
/// tells an attacker which half of the problem to work on.
pub fn verify_access_token(secret: &str, token: &str) -> Result<AccessClaims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|error| {
        tracing::debug!(error = ?error, "access token rejected");
        AppError::Unauthorized
    })
}

/// An opaque refresh token and the digest stored against it.
pub struct RefreshToken {
    /// Given to the client. Never stored.
    pub token: String,
    /// Stored. A database leak therefore yields nothing usable.
    pub hash: String,
    pub expires_at: DateTime<Utc>,
}

/// Generates a refresh token.
///
/// Opaque and random rather than a JWT: it is checked against the database on
/// every use anyway, so signing buys nothing, and an opaque value carries no
/// readable claims if it leaks.
pub fn generate_refresh_token() -> RefreshToken {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);

    let token = hex_encode(&bytes);
    let hash = hash_refresh_token(&token);

    RefreshToken {
        token,
        hash,
        expires_at: Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS),
    }
}

/// SHA-256 of a refresh token, as stored in `refresh_tokens.token_hash`.
///
/// A plain digest, not a password hash: the token is 256 bits of entropy, so
/// there is nothing to brute-force, and lookup happens on every refresh.
pub fn hash_refresh_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("sha256:{}", hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "a-test-signing-secret";

    fn issue() -> (String, Uuid, Uuid) {
        let user_id = Uuid::now_v7();
        let tenant_id = Uuid::now_v7();
        let (token, _) = issue_access_token(
            SECRET,
            user_id,
            tenant_id,
            "user.john",
            vec!["ROLE-ADMIN".to_owned()],
            vec!["identity:user:read".to_owned()],
        )
        .expect("issues");

        (token, user_id, tenant_id)
    }

    #[test]
    fn round_trips_the_claims() {
        let (token, user_id, tenant_id) = issue();
        let claims = verify_access_token(SECRET, &token).expect("verifies");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.tenant_id, tenant_id);
        assert_eq!(claims.username, "user.john");
        assert!(claims.has_permission("identity:user:read"));
        assert!(!claims.has_permission("identity:user:delete"));
    }

    #[test]
    fn rejects_a_token_signed_with_another_secret() {
        let (token, _, _) = issue();

        assert!(verify_access_token("a-different-secret", &token).is_err());
    }

    #[test]
    fn rejects_a_tampered_token() {
        let (token, _, _) = issue();
        // Flip a character in the payload segment; the signature no longer matches.
        let mut parts: Vec<&str> = token.split('.').collect();
        let payload = parts[1].to_owned();
        let tampered = format!("{}X", &payload[..payload.len() - 1]);
        parts[1] = &tampered;

        assert!(verify_access_token(SECRET, &parts.join(".")).is_err());
    }

    #[test]
    fn rejects_an_expired_token() {
        let past = Utc::now() - Duration::hours(2);
        let claims = AccessClaims {
            sub: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            username: "user.john".to_owned(),
            roles: vec![],
            permissions: vec![],
            exp: past.timestamp(),
            iat: (past - Duration::minutes(15)).timestamp(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("signs");

        assert!(verify_access_token(SECRET, &token).is_err());
    }

    #[test]
    fn rejects_nonsense() {
        assert!(verify_access_token(SECRET, "").is_err());
        assert!(verify_access_token(SECRET, "not.a.token").is_err());
    }

    #[test]
    fn refresh_tokens_are_unique_and_stored_only_as_a_digest() {
        let first = generate_refresh_token();
        let second = generate_refresh_token();

        assert_ne!(first.token, second.token);
        assert_ne!(first.hash, second.hash);
        assert!(first.hash.starts_with("sha256:"));
        assert!(
            !first.hash.contains(&first.token),
            "the raw token must not appear in what is stored"
        );
        assert_eq!(first.hash, hash_refresh_token(&first.token));
    }

    #[test]
    fn refresh_tokens_outlive_access_tokens() {
        // Otherwise a session would end at the access token's expiry and the
        // refresh token would be pointless. Checked at compile time so it
        // cannot be broken by editing either constant.
        const _: () = assert!(REFRESH_TOKEN_TTL_DAYS * 24 * 60 > ACCESS_TOKEN_TTL_MINUTES);

        let refresh = generate_refresh_token();
        assert!(refresh.expires_at > Utc::now() + Duration::minutes(ACCESS_TOKEN_TTL_MINUTES));
    }
}
