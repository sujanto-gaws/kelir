use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::error::ValidationDetail;
use crate::health;
use crate::middleware::cors::cors_layer;
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
    ),
    components(schemas(
        health::HealthBody,
        health::ReadyBody,
        health::VersionBody,
        ErrorEnvelope,
        ErrorBody,
        ValidationDetail,
        PageMeta,
    )),
    tags(
        (name = "operations", description = "Health, readiness and build information")
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

/// The versioned API surface. Empty in Phase 1 — module routers land here as
/// each is built.
fn api_v1_router() -> Router<AppState> {
    Router::new()
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
    async fn unknown_routes_are_not_found() {
        let (status, _) = get("/api/v1/does-not-exist").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
