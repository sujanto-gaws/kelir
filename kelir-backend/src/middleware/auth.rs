//! Authentication and authorisation for request handlers (SRS FR-IDM-004/005,
//! FR-API-008).
//!
//! Handlers take [`Authenticated`] as an argument; a request without a valid
//! access token never reaches them. Authorisation is then an explicit call to
//! [`Authenticated::require`] with a `module:resource:action` string.
//!
//! Permission checks belong in the service layer (coding standard §2.6). The
//! guard here is what the service calls, not a substitute for it.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::AppError;
use crate::modules::auth::token::{verify_access_token, AccessClaims};
use crate::state::AppState;

/// The authenticated caller, extracted from the `Authorization: Bearer` header.
///
/// Its presence in a handler signature is what makes a route protected: there is
/// no way to obtain one without a valid, unexpired, correctly signed token.
#[derive(Debug, Clone)]
pub struct Authenticated {
    pub claims: AccessClaims,
}

impl Authenticated {
    pub fn user_id(&self) -> uuid::Uuid {
        self.claims.sub
    }

    pub fn tenant_id(&self) -> uuid::Uuid {
        self.claims.tenant_id
    }

    /// The caller's name **as the token carries it**.
    ///
    /// Denormalized into `activity_events.actor_name` at the moment something
    /// happens (#247 AC5), so a rename does not rewrite the past. It comes off
    /// the token rather than a query because an activity event is written in
    /// the action's own transaction and must not add a lookup that could fail
    /// after the action is decided.
    pub fn username(&self) -> &str {
        &self.claims.username
    }

    /// Requires a permission, or fails with `Forbidden`.
    ///
    /// `Forbidden`, not `NotFound`: the caller is authenticated, so hiding the
    /// resource's existence buys nothing and makes the refusal hard to diagnose.
    pub fn require(&self, permission: &str) -> Result<(), AppError> {
        if self.claims.has_permission(permission) {
            return Ok(());
        }

        tracing::info!(
            user_id = %self.claims.sub,
            permission,
            "permission denied"
        );

        Err(AppError::Forbidden)
    }
}

impl FromRequestParts<AppState> for Authenticated {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = bearer_token(header).ok_or(AppError::Unauthorized)?;
        let claims = verify_access_token(&state.config.jwt_secret, token)?;

        Ok(Self { claims })
    }
}

/// Extracts the credential from an `Authorization` header value.
///
/// The scheme is matched case-insensitively because RFC 7235 defines it that
/// way, and clients do send `bearer`.
fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, credential) = header.split_once(' ')?;

    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }

    let credential = credential.trim();

    if credential.is_empty() {
        None
    } else {
        Some(credential)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::auth::token::issue_access_token;
    use uuid::Uuid;

    fn claims_with(permissions: &[&str]) -> AccessClaims {
        AccessClaims {
            sub: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            username: "user.john".to_owned(),
            roles: vec![],
            permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
            exp: 0,
            iat: 0,
        }
    }

    #[test]
    fn grants_a_held_permission() {
        let caller = Authenticated {
            claims: claims_with(&["identity:user:read", "identity:user:update"]),
        };

        assert!(caller.require("identity:user:read").is_ok());
    }

    #[test]
    fn refuses_a_permission_that_is_not_held() {
        let caller = Authenticated {
            claims: claims_with(&["identity:user:read"]),
        };

        let error = caller.require("identity:user:delete").expect_err("denied");
        assert_eq!(error.code(), "FORBIDDEN");
    }

    #[test]
    fn permission_matching_is_exact() {
        // A prefix must not grant a longer permission, or holding
        // `identity:user:read` would imply `identity:user:read-secrets`.
        let caller = Authenticated {
            claims: claims_with(&["identity:user"]),
        };

        assert!(caller.require("identity:user:read").is_err());

        let broader = Authenticated {
            claims: claims_with(&["identity:user:read"]),
        };
        assert!(broader.require("identity:user").is_err());
    }

    #[test]
    fn parses_a_bearer_header() {
        assert_eq!(bearer_token("Bearer abc.def.ghi"), Some("abc.def.ghi"));
        assert_eq!(bearer_token("bearer abc.def.ghi"), Some("abc.def.ghi"));
    }

    #[test]
    fn rejects_other_schemes_and_empty_credentials() {
        assert_eq!(bearer_token("Basic dXNlcjpwYXNz"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(bearer_token("Bearer"), None);
        assert_eq!(bearer_token(""), None);
    }

    #[test]
    fn a_valid_token_yields_the_permissions_it_carries() {
        let secret = "test-secret";
        let user_id = Uuid::now_v7();
        let (token, _) = issue_access_token(
            secret,
            user_id,
            Uuid::now_v7(),
            "user.john",
            vec!["ROLE-ADMIN".to_owned()],
            vec!["identity:user:read".to_owned()],
        )
        .expect("issues");

        let claims = verify_access_token(secret, &token).expect("verifies");
        let caller = Authenticated { claims };

        assert_eq!(caller.user_id(), user_id);
        assert!(caller.require("identity:user:read").is_ok());
        assert!(caller.require("identity:role:delete").is_err());
    }
}
