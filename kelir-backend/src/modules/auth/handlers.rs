use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::service;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::identity::repository as identity_repo;
use crate::response::ItemEnvelope;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignInRequest {
    /// Username or email address (FR-AUTH-001).
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    /// Seconds until the access token expires.
    pub expires_in: i64,
    pub user_id: Uuid,
    pub username: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignOutRequest {
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUser {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(sign_in))
        .route("/logout", post(sign_out))
        .route("/refresh", post(refresh))
        .route("/me", get(me))
}

/// Sign in with a username or email and a password.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = SignInRequest,
    responses(
        (status = 200, description = "Signed in", body = SessionResponse),
        (status = 401, description = "Invalid credentials or inactive account")
    )
)]
async fn sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SignInRequest>,
) -> Result<Json<ItemEnvelope<SessionResponse>>, AppError> {
    let ip = client_ip(&headers);

    let signed_in =
        service::sign_in(&state, &request.username, &request.password, ip.as_deref()).await?;

    Ok(Json(ItemEnvelope::new(SessionResponse {
        access_token: signed_in.access_token,
        refresh_token: signed_in.refresh_token,
        token_type: "Bearer",
        expires_in: service::access_token_ttl_seconds(),
        user_id: signed_in.user_id,
        username: signed_in.username,
    })))
}

/// Exchange a refresh token for a new pair. The presented token is revoked.
#[utoipa::path(
    post,
    path = "/api/v1/auth/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Rotated", body = SessionResponse),
        (status = 401, description = "Unknown, expired or already-used token")
    )
)]
async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RefreshRequest>,
) -> Result<Json<ItemEnvelope<SessionResponse>>, AppError> {
    let ip = client_ip(&headers);
    let signed_in = service::refresh(&state, &request.refresh_token, ip.as_deref()).await?;

    Ok(Json(ItemEnvelope::new(SessionResponse {
        access_token: signed_in.access_token,
        refresh_token: signed_in.refresh_token,
        token_type: "Bearer",
        expires_in: service::access_token_ttl_seconds(),
        user_id: signed_in.user_id,
        username: signed_in.username,
    })))
}

/// Sign out, revoking the presented refresh token. Idempotent.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    request_body = SignOutRequest,
    responses((status = 204, description = "Signed out"))
)]
async fn sign_out(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SignOutRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    let ip = client_ip(&headers);

    // The caller is identified from the token being revoked rather than from an
    // access token: signing out must work when the access token has already
    // expired, which is exactly when a client is most likely to try.
    service::sign_out(&state, request.refresh_token.as_deref(), ip.as_deref()).await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// The signed-in user, their roles and their effective permissions.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "The current user", body = CurrentUser),
        (status = 401, description = "Not signed in")
    ),
    security(("bearer" = []))
)]
async fn me(
    State(state): State<AppState>,
    caller: Authenticated,
) -> Result<Json<ItemEnvelope<CurrentUser>>, AppError> {
    let user = identity_repo::find_user(&state.pool, caller.tenant_id(), caller.user_id())
        .await?
        .ok_or_else(|| AppError::not_found("User"))?;

    Ok(Json(ItemEnvelope::new(CurrentUser {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        email: user.email,
        // Read from the token, not the database: these are the permissions this
        // session actually carries, which is what the client should render
        // against. They refresh when the access token does.
        roles: caller.claims.roles.clone(),
        permissions: caller.claims.permissions.clone(),
    })))
}

/// Best-effort client address for the audit trail (FR-AUD-005).
///
/// `X-Forwarded-For` is trusted only because the deployed topology puts Caddy in
/// front and nothing else can reach the backend (deployment §4.1). Exposed
/// directly, this header is caller-controlled and must not be trusted.
///
/// Absent behind no proxy — a local run records no address rather than the
/// socket's, because `ConnectInfo` would require the whole service to be built
/// with connect info for a value that is meaningless behind a reverse proxy.
fn client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_the_forwarded_address() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));

        assert_eq!(client_ip(&headers), Some("203.0.113.7".to_owned()));
    }

    #[test]
    fn takes_the_first_hop_from_a_forwarded_chain() {
        // The client is leftmost; the rest are proxies.
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 198.51.100.2"),
        );

        assert_eq!(client_ip(&headers), Some("203.0.113.7".to_owned()));
    }

    #[test]
    fn reports_nothing_when_no_proxy_supplied_an_address() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("   "));

        assert_eq!(client_ip(&headers), None);
        assert_eq!(client_ip(&HeaderMap::new()), None);
    }
}
