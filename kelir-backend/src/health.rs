use axum::Json;
use serde_json::json;

/// Liveness probe: reports that the process is up and serving.
///
/// Phase 1 splits this into `/health`, `/health/ready` (dependency checks) and
/// `/health/live`, and moves the payload onto the standard response envelope.
pub async fn healthcheck() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_status_ok() {
        let Json(body) = healthcheck().await;

        assert_eq!(body["status"], "ok");
    }
}
