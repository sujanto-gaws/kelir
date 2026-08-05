use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

pub fn error_response(status: StatusCode, code: &'static str, message: impl Into<String>) -> impl IntoResponse {
    let body = ErrorBody {
        code,
        message: message.into(),
    };

    (status, Json(body))
}
