use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::service;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::middleware::client_address::ClientAddress;
use crate::middleware::rate_limit::Decision;
use crate::modules::identity::repository as identity_repo;
use crate::modules::organization::service as organization;
use crate::response::ItemEnvelope;
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignInRequest {
    /// Username or email address (FR-AUTH-001).
    pub username: String,
    pub password: String,
    /// Which tenant to authenticate against (FR-IDM-009).
    ///
    /// Required only when the deployment runs in multi-tenant mode. Single-tenant
    /// deployments resolve their configured default tenant and ignore this
    /// field, so existing clients need not send it.
    #[serde(default)]
    pub tenant_code: Option<String>,
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

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
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
        .route("/change-password", post(change_password))
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
    client: ClientAddress,
    Json(request): Json<SignInRequest>,
) -> Result<Json<ItemEnvelope<SessionResponse>>, AppError> {
    // Keyed on the source address, not the username: an attacker chooses the
    // username freely, so keying on it would let them sidestep the limit by
    // varying it — which is exactly the credential-stuffing shape that account
    // lockout already misses.
    //
    // Scoped by tenant as well, so one tenant's traffic cannot exhaust another
    // tenant's budget for the same source address. The scope is computed the way
    // resolution computes it, which in single-tenant mode is a constant — the
    // tenant code is a caller-supplied field, and honouring it here while
    // resolution ignores it would hand an attacker a fresh bucket per request.
    let limiter_key = format!(
        "{}|{}",
        organization::sign_in_scope(&state.config, request.tenant_code.as_deref()),
        client.rate_limit_key()
    );

    if let Decision::Block {
        retry_after_seconds,
    } = state.rate_limiter.check(&limiter_key)
    {
        tracing::warn!(client = %client, "sign-in rate limit exceeded");

        return Err(AppError::TooManyRequests {
            retry_after_seconds,
        });
    }

    let ip = client.to_string();
    let signed_in = service::sign_in(
        &state,
        request.tenant_code.as_deref(),
        &request.username,
        &request.password,
        Some(&ip),
    )
    .await?;

    // Succeeded, so this source is not the problem: forget its count rather
    // than letting an earlier typo streak count against it.
    state.rate_limiter.reset(&limiter_key);

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
    client: ClientAddress,
    Json(request): Json<RefreshRequest>,
) -> Result<Json<ItemEnvelope<SessionResponse>>, AppError> {
    let ip = client.to_string();
    let signed_in = service::refresh(&state, &request.refresh_token, Some(&ip)).await?;

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
    client: ClientAddress,
    Json(request): Json<SignOutRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    let ip = client.to_string();

    // The caller is identified from the token being revoked rather than from an
    // access token: signing out must work when the access token has already
    // expired, which is exactly when a client is most likely to try.
    service::sign_out(&state, request.refresh_token.as_deref(), Some(&ip)).await?;

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

/// Change your own password (FR-AUTH-005).
#[utoipa::path(
    post,
    path = "/api/v1/auth/change-password",
    tag = "auth",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Changed; every session for the account ends"),
        (status = 422, description = "The current password is wrong, or the new one is too short")
    ),
    security(("bearer" = []))
)]
async fn change_password(
    State(state): State<AppState>,
    caller: Authenticated,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    service::change_own_password(
        &state,
        caller.tenant_id(),
        caller.user_id(),
        &request.current_password,
        &request.new_password,
    )
    .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
