//! An endpoint one external system exposes (Database Schema §12.2,
//! architectures/03 §4.2).
//!
//! **Governed by the system's own permissions** (#520, the product owner's
//! answer 1): reading one needs `integration:external-system:read` and
//! creating or editing one needs `:update`. Retiring one is an edit to
//! `status` — `INACTIVE` — for the reason the system itself has no delete.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{code, required_text, Problems, MAX_NAME_LENGTH, MAX_URL_LENGTH};
use crate::error::AppError;
use crate::utils::serde::present_or_absent;

/// `integration_endpoints.method` (§12.2's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const ALL: &'static [Self] = &[Self::Get, Self::Post, Self::Put, Self::Patch, Self::Delete];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    /// An unknown stored value reads as `GET`; the `CHECK` means none exists.
    pub fn from_db(value: &str) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|method| method.as_db() == value)
            .unwrap_or(Self::Get)
    }
}

/// `integration_endpoints.status` (§12.2's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EndpointStatus {
    Active,
    Inactive,
}

impl EndpointStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            _ => Self::Inactive,
        }
    }
}

/// An endpoint as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationEndpoint {
    pub id: Uuid,
    pub external_system_id: Uuid,
    /// Unique within its system and **not updatable** — `CREATE_PURCHASE_ORDER`.
    pub endpoint_code: String,
    pub name: String,
    pub method: HttpMethod,
    /// Relative to the system's `baseUrl` — `/purchase-orders`.
    pub path: String,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Adding an endpoint to a system. It is always created `ACTIVE`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateIntegrationEndpointRequest {
    pub endpoint_code: String,
    pub name: String,
    pub method: HttpMethod,
    /// Starts with `/`; no scheme, host, fragment or whitespace.
    pub path: String,
    pub description: Option<String>,
}

/// Editing an endpoint. `status: "INACTIVE"` retires it, `"ACTIVE"` brings it
/// back. `endpointCode` may not change.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateIntegrationEndpointRequest {
    pub name: Option<String>,
    pub method: Option<HttpMethod>,
    pub path: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    pub status: Option<EndpointStatus>,
}

pub fn validate_create(request: &CreateIntegrationEndpointRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    code(&request.endpoint_code, "endpointCode", &mut problems);
    required_text(&request.name, "name", MAX_NAME_LENGTH, &mut problems);
    path(&request.path, &mut problems);

    problems.finish()
}

pub fn validate_update(request: &UpdateIntegrationEndpointRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    if let Some(name) = &request.name {
        required_text(name, "name", MAX_NAME_LENGTH, &mut problems);
    }
    if let Some(value) = &request.path {
        path(value, &mut problems);
    }

    problems.finish()
}

/// A path relative to the system's base URL.
///
/// Absolute URLs are refused so an endpoint cannot quietly point at a host
/// other than its system's — the base URL is where that is decided, and it is
/// validated there.
fn path(value: &str, problems: &mut Problems) {
    let before = problems.len();
    required_text(value, "path", MAX_URL_LENGTH, problems);

    if problems.len() > before {
        return;
    }

    let trimmed = value.trim();

    if !trimmed.starts_with('/')
        || trimmed.starts_with("//")
        || trimmed.contains("://")
        || trimmed.contains('#')
        || trimmed
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        problems.push(
            "path",
            "format",
            "INVALID_FORMAT",
            "path must start with '/', and carry no scheme, host, fragment or whitespace",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::details;
    use super::*;

    fn create(path: &str) -> CreateIntegrationEndpointRequest {
        CreateIntegrationEndpointRequest {
            endpoint_code: "CREATE_PURCHASE_ORDER".to_owned(),
            name: "Create purchase order".to_owned(),
            method: HttpMethod::Post,
            path: path.to_owned(),
            description: None,
        }
    }

    #[test]
    fn a_relative_path_is_accepted() {
        for good in ["/purchase-orders", "/orders/{id}/lines?expand=true", "/"] {
            assert!(validate_create(&create(good)).is_ok(), "{good}");
        }
    }

    #[test]
    fn a_path_that_names_another_host_is_refused() {
        for bad in [
            "https://elsewhere.example.com/x",
            "//elsewhere.example.com/x",
            "purchase-orders",
            "/orders#top",
            "/orders with space",
        ] {
            let details = details(validate_create(&create(bad)).expect_err("refused"));

            assert_eq!(details[0].path, "path", "{bad}");
            assert_eq!(details[0].code, "INVALID_FORMAT", "{bad}");
        }
    }

    #[test]
    fn a_blank_path_is_required_rather_than_malformed() {
        let details = details(validate_create(&create("  ")).expect_err("refused"));

        assert_eq!(details.len(), 1);
        assert_eq!(details[0].code, "REQUIRED");
    }

    #[test]
    fn the_method_vocabulary_round_trips() {
        for method in HttpMethod::ALL {
            assert_eq!(HttpMethod::from_db(method.as_db()), *method);
            assert_eq!(
                serde_json::to_value(method).expect("serializes"),
                serde_json::json!(method.as_db())
            );
        }
    }
}
