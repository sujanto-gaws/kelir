//! The application error type and its HTTP rendering.
//!
//! Several variants and constructors have no caller in Phase 1 — the endpoints
//! that raise Unauthorized, Forbidden and Conflict arrive with authentication
//! in Phase 2. They are defined now so the error vocabulary and its status-code
//! mapping are settled before modules start depending on them.
#![allow(
    dead_code,
    reason = "error variants are raised by the module services from Phase 2"
)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::response::ErrorEnvelope;

/// A single field-level validation failure.
///
/// The shape is fixed by JSON Form Schema S10.3 so that JFSS-driven forms and
/// hand-built forms report failures identically (naming convention §5).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationDetail {
    pub path: String,
    pub rule: String,
    pub code: String,
    pub message: String,
}

impl ValidationDetail {
    pub fn new(
        path: impl Into<String>,
        rule: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            rule: rule.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// The single error type crossing the HTTP boundary (coding standard §2.3).
///
/// Module errors convert into this with `From`; handlers return it directly.
/// Codes are stable and machine-readable — callers branch on `code`, never on
/// the human-readable message.
#[derive(Debug)]
pub enum AppError {
    NotFound { resource: &'static str },
    Validation { details: Vec<ValidationDetail> },
    BadRequest { message: String },
    Unauthorized,
    Forbidden,
    Conflict { message: String },
    Internal { source: anyhow::Error },
}

impl AppError {
    pub fn not_found(resource: &'static str) -> Self {
        Self::NotFound { resource }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict {
            message: message.into(),
        }
    }

    pub fn validation(details: Vec<ValidationDetail>) -> Self {
        Self::Validation { details }
    }

    /// Stable machine-readable code (naming convention §5).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Validation { .. } => "VALIDATION_ERROR",
            Self::BadRequest { .. } => "BAD_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::Conflict { .. } => "CONFLICT",
            Self::Internal { .. } => "INTERNAL_ERROR",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Validation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The client-facing message. Internal errors deliberately reveal nothing:
    /// the cause is logged server-side instead.
    fn message(&self) -> String {
        match self {
            Self::NotFound { resource } => format!("{resource} not found"),
            Self::Validation { .. } => "Validation failed".to_owned(),
            Self::BadRequest { message } | Self::Conflict { message } => message.clone(),
            Self::Unauthorized => "Authentication required".to_owned(),
            Self::Forbidden => "You do not have permission to perform this action".to_owned(),
            Self::Internal { .. } => "An unexpected error occurred".to_owned(),
        }
    }

    fn details(&self) -> Vec<ValidationDetail> {
        match self {
            Self::Validation { details } => details.clone(),
            _ => Vec::new(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for AppError {}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => Self::NotFound { resource: "Record" },
            other => Self::Internal {
                source: other.into(),
            },
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(source: anyhow::Error) -> Self {
        Self::Internal { source }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Log the cause before it is discarded; the client never sees it.
        if let Self::Internal { source } = &self {
            tracing::error!(error = ?source, "request failed with an internal error");
        }

        let status = self.status();
        let body = ErrorEnvelope::new(self.code(), self.message(), self.details());

        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_of(error: AppError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body reads");

        (
            status,
            serde_json::from_slice(&bytes).expect("body is json"),
        )
    }

    #[tokio::test]
    async fn renders_the_standard_error_envelope() {
        let (status, body) = body_of(AppError::not_found("Document")).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["code"], "NOT_FOUND");
        assert_eq!(body["error"]["message"], "Document not found");
        assert!(body["error"]["details"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[tokio::test]
    async fn validation_failures_carry_the_jfss_detail_shape() {
        let error = AppError::validation(vec![ValidationDetail::new(
            "amount",
            "max",
            "MAX_EXCEEDED",
            "Amount exceeds the limit",
        )]);

        let (status, body) = body_of(error).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "VALIDATION_ERROR");

        let detail = &body["error"]["details"][0];
        assert_eq!(detail["path"], "amount");
        assert_eq!(detail["rule"], "max");
        assert_eq!(detail["code"], "MAX_EXCEEDED");
        assert_eq!(detail["message"], "Amount exceeds the limit");
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_their_cause() {
        let error = AppError::Internal {
            source: anyhow::anyhow!("connection string password=hunter2 refused"),
        };

        let (status, body) = body_of(error).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["message"], "An unexpected error occurred");
        assert!(
            !body.to_string().contains("hunter2"),
            "the cause must not reach the client"
        );
    }

    #[test]
    fn missing_rows_become_not_found_rather_than_internal() {
        let error = AppError::from(sqlx::Error::RowNotFound);

        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(error.code(), "NOT_FOUND");
    }
}
