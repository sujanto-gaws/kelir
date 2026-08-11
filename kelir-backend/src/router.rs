use axum::{routing::get, Router};

use crate::health;

/// Builds the application router.
///
/// Sprint 0 serves only the liveness probe, so that the compose stack has a
/// backend that answers. Phase 1 adds the full health set (`/health/ready`,
/// `/health/live`), `/version`, the response envelope, and the versioned
/// `/api/v1` module routes.
pub fn create_router() -> Router {
    Router::new().route("/health", get(health::healthcheck))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_health_endpoint() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_route() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .uri("/does-not-exist")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
