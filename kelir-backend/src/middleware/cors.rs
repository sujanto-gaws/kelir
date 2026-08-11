use axum::http::{header, HeaderValue, Method};
use tower_http::cors::CorsLayer;

/// Cross-origin policy for the browser client.
///
/// The frontend and the API are separate origins in every environment — the
/// Vite dev server locally, and separate hosts behind the reverse proxy in
/// staging and production — so the browser blocks every API response without
/// this. The allowed origin comes from `KELIR_FRONTEND_URL` rather than a
/// wildcard: credentials are permitted, and the two are mutually exclusive by
/// specification, so a wildcard here would silently break authentication in
/// Phase 2.
pub fn cors_layer(frontend_url: &str) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
        .allow_credentials(true);

    match HeaderValue::from_str(frontend_url.trim_end_matches('/')) {
        Ok(origin) => layer.allow_origin(origin),
        Err(error) => {
            // A malformed KELIR_FRONTEND_URL must not become a wildcard, which
            // would be a silent downgrade. Deny instead, loudly.
            tracing::error!(
                error = ?error,
                frontend_url,
                "KELIR_FRONTEND_URL is not a valid origin; cross-origin requests will be refused"
            );
            layer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::create_router;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn state_with_frontend(url: &str) -> AppState {
        let pool = crate::db::create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("lazy pool builds");
        let mut config = crate::config::AppConfig::test_default();
        config.frontend_url = url.to_owned();

        AppState::new(pool, config)
    }

    #[tokio::test]
    async fn allows_the_configured_frontend_origin() {
        let response = create_router(state_with_frontend("http://localhost:5173"))
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .header("origin", "http://localhost:5173")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://localhost:5173"),
            "the browser needs this header to hand the body to the page"
        );
    }

    #[tokio::test]
    async fn answers_the_preflight_request() {
        // Without this the browser never sends the real request at all.
        let response = create_router(state_with_frontend("http://localhost:5173"))
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/v1/anything")
                    .header("origin", "http://localhost:5173")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert!(
            response.status().is_success(),
            "preflight returned {}",
            response.status()
        );
        assert!(response
            .headers()
            .contains_key("access-control-allow-methods"));
    }

    #[tokio::test]
    async fn does_not_allow_an_unknown_origin() {
        let response = create_router(state_with_frontend("http://localhost:5173"))
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .header("origin", "http://evil.example")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        // tower-http's exact-origin mode always echoes the *configured* origin
        // rather than omitting the header. That still denies the caller: the
        // browser compares this value against its own origin and blocks the
        // response when they differ. What matters is that the requesting origin
        // is never reflected back, which would grant access to anyone.
        let allowed = response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok());

        assert_ne!(
            allowed,
            Some("http://evil.example"),
            "the requesting origin must never be reflected back"
        );
        assert!(matches!(allowed, None | Some("http://localhost:5173")));
    }
}
