use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

// Minimal error shape carried over from the scaffold. Phase 1 replaces it with
// the central `AppError` implementing `IntoResponse` over the standard envelope
// `{success: false, error: {code, message, details}}` (coding standard 2.3).
#[allow(dead_code, reason = "scaffold replaced by AppError in Phase 1")]
#[derive(Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

#[allow(dead_code, reason = "scaffold replaced by AppError in Phase 1")]
pub fn error_response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
) -> impl IntoResponse {
    let body = ErrorBody {
        code,
        message: message.into(),
    };

    (status, Json(body))
}
