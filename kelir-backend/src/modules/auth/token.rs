use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;

/// How long after issue an access token's `exp` falls.
///
/// Short by design: an access token cannot be revoked, so the time it is
/// accepted for is the window in which a stolen one is useful. That is this
/// and no longer: [`verify_access_token`] allows no leeway on `exp`
/// ([`ACCESS_TOKEN_LEEWAY_SECONDS`]). Continuity comes from the refresh
/// token, which can be revoked.
///
/// One thing ends a token sooner: its tenant or its user being deleted, which
/// `middleware::auth::Authenticated` reads on every request (#650, D-105).
pub const ACCESS_TOKEN_TTL_MINUTES: i64 = 15;

/// How long past its `exp` an access token is still accepted: not at all.
///
/// **Set explicitly because the library's default is not zero.**
/// `jsonwebtoken::Validation` allows 60 seconds unless told otherwise, and
/// until #650 that default stood unnoticed: every bound written as "15
/// minutes" was 16. A named zero is what stops the default coming back with
/// a change that builds its own `Validation`.
///
/// The cost is clock skew. A token issued by one instance and verified by
/// another whose clock is ahead is refused that much early, so replicas must
/// keep their clocks within well under a second of each other
/// (the installation guide says so; the shipped compose files run one backend).
pub const ACCESS_TOKEN_LEEWAY_SECONDS: u64 = 0;

/// How long a refresh token stays valid. Rotated on every use.
pub const REFRESH_TOKEN_TTL_DAYS: i64 = 30;

/// Access token payload (architecture 01 §18.1).
///
/// Permissions are embedded so authorisation does not query the database on
/// every request. The cost is staleness: a permission revoked mid-session takes
/// effect when the access token stops being accepted, within
/// `ACCESS_TOKEN_TTL_MINUTES`. Deletion is the exception and is not read from
/// here: a deleted tenant's token, and a deleted user's, is refused on its
/// next request (#650, D-105).
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
    validation.leeway = ACCESS_TOKEN_LEEWAY_SECONDS;

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

    /// A token whose `exp` is `seconds` from now, correct in every other way.
    fn expiring_in(seconds: i64) -> String {
        let exp = Utc::now().timestamp() + seconds;
        let claims = AccessClaims {
            sub: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            username: "user.john".to_owned(),
            roles: vec![],
            permissions: vec![],
            exp,
            iat: exp - ACCESS_TOKEN_TTL_MINUTES * 60,
        };

        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .expect("signs")
    }

    #[test]
    fn the_leeway_is_a_named_zero() {
        // At compile time, so it cannot be changed without this line.
        const _: () = assert!(ACCESS_TOKEN_LEEWAY_SECONDS == 0);
    }

    #[test]
    fn a_token_one_second_past_its_expiry_is_refused() {
        // #650 AC16. `jsonwebtoken` accepts a token for 60 seconds past its
        // `exp` unless the leeway is set; with it left at the default this
        // token verifies.
        assert!(verify_access_token(SECRET, &expiring_in(-1)).is_err());
    }

    #[test]
    fn a_token_two_seconds_before_its_expiry_is_accepted() {
        // The control: no leeway does not mean an early refusal.
        assert!(verify_access_token(SECRET, &expiring_in(2)).is_ok());
    }

    #[test]
    fn a_fresh_token_expires_900_seconds_after_it_was_issued() {
        let (token, _, _) = issue();
        let claims = verify_access_token(SECRET, &token).expect("verifies");

        assert_eq!(claims.exp - claims.iat, 900);
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
