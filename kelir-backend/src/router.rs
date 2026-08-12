use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::error::ValidationDetail;
use crate::health;
use crate::middleware::cors::cors_layer;
use crate::modules::{auth, identity};
use crate::response::{ErrorBody, ErrorEnvelope, PageMeta};
use crate::state::AppState;

/// The generated OpenAPI document (FR-API-004).
///
/// Never hand-edited (coding standard §2.6): endpoints are added by annotating
/// their handler with `#[utoipa::path]` and listing it here.
#[derive(OpenApi)]
#[openapi(
    paths(
        health::healthcheck,
        health::liveness,
        health::readiness,
        health::version,
        auth::handlers::sign_in,
        auth::handlers::refresh,
        auth::handlers::sign_out,
        auth::handlers::me,
        auth::handlers::change_password,
        identity::handlers::list_users,
        identity::handlers::get_user,
        identity::handlers::create_user,
        identity::handlers::update_user,
        identity::handlers::deactivate_user,
        identity::handlers::set_password,
        identity::handlers::list_roles,
        identity::handlers::get_role,
        identity::handlers::create_role,
        identity::handlers::update_role,
        identity::handlers::delete_role,
        identity::handlers::list_permissions,
    ),
    components(schemas(
        health::HealthBody,
        health::ReadyBody,
        health::VersionBody,
        auth::handlers::SignInRequest,
        auth::handlers::RefreshRequest,
        auth::handlers::SignOutRequest,
        auth::handlers::SessionResponse,
        auth::handlers::CurrentUser,
        auth::handlers::ChangePasswordRequest,
        identity::domain::User,
        identity::domain::UserStatus,
        identity::domain::RoleSummary,
        identity::domain::Role,
        identity::domain::Permission,
        identity::domain::CreateUserRequest,
        identity::domain::UpdateUserRequest,
        identity::domain::CreateRoleRequest,
        identity::domain::UpdateRoleRequest,
        identity::handlers::SetPasswordRequest,
        ErrorEnvelope,
        ErrorBody,
        ValidationDetail,
        PageMeta,
    )),
    tags(
        (name = "operations", description = "Health, readiness and build information"),
        (name = "auth", description = "Sign in, sign out, session refresh"),
        (name = "identity", description = "Users, roles and permissions")
    ),
    info(
        title = "Kelir API",
        description = "Metadata-driven, document-centric, workflow-enabled platform API.",
        version = env!("CARGO_PKG_VERSION"),
    )
)]
pub struct ApiDoc;

/// Builds the application router.
///
/// Operational endpoints stay at the root; everything else is versioned under
/// `/api/v1` (naming convention §5). Phase 2 mounts the module routers onto
/// `api_v1_router`.
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::healthcheck))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .route("/version", get(health::version))
        .route("/api/docs/openapi.json", get(openapi_document))
        .nest("/api/v1", api_v1_router())
        // Applied last so it wraps every route, including the 404 fallback and
        // the preflight requests the browser sends before anything else.
        .layer(cors_layer(&state.config.frontend_url))
        .with_state(state)
}

/// The versioned API surface. Module routers mount here as each is built.
///
/// Authentication is per-route rather than a blanket layer: `/auth/login` and
/// `/auth/refresh` must stay reachable without a token, and a handler that takes
/// `Authenticated` cannot be reached without one (FR-API-008). Making the rule
/// visible in each handler's signature beats a layer whose exceptions live
/// somewhere else.
fn api_v1_router() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/identity", identity::handlers::routes())
}

async fn openapi_document() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let pool = crate::db::create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("lazy pool builds without a server");

        AppState::new(pool, AppConfig::test_default())
    }

    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let response = create_router(test_state())
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

        (status, json)
    }

    #[tokio::test]
    async fn serves_health() {
        let (status, body) = get("/health").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn serves_liveness_without_a_database() {
        // Liveness must not depend on PostgreSQL; no server is running here.
        let (status, body) = get("/health/live").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn serves_version() {
        let (status, body) = get("/version").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], health::VERSION);
        assert_eq!(body["environment"], "test");
    }

    #[tokio::test]
    async fn publishes_the_openapi_document() {
        let (status, body) = get("/api/docs/openapi.json").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["info"]["title"], "Kelir API");
        assert!(
            body["paths"]["/health"].is_object(),
            "health is documented in the generated spec"
        );
    }

    #[tokio::test]
    async fn the_tenant_code_is_visible_in_the_published_contract() {
        // The reason tenancy travels in the request body rather than a header
        // (FR-IDM-009): a header carries the same trust as a body field while
        // being invisible to every client generated from this document. If the
        // field ever stops appearing here, that rationale has quietly lapsed.
        let (_, body) = get("/api/docs/openapi.json").await;

        let properties = &body["components"]["schemas"]["SignInRequest"]["properties"];

        assert!(
            properties["tenantCode"].is_object(),
            "tenantCode is missing from the published SignInRequest: {properties}"
        );

        // Optional in the contract, so single-tenant clients that never send it
        // remain conformant.
        let required = &body["components"]["schemas"]["SignInRequest"]["required"];
        assert!(
            !required
                .as_array()
                .is_some_and(|names| names.iter().any(|name| name == "tenantCode")),
            "tenantCode must stay optional; existing clients do not send it"
        );
    }

    #[tokio::test]
    async fn unknown_routes_are_not_found() {
        let (status, _) = get("/api/v1/does-not-exist").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
