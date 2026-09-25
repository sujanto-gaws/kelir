//! An external system — a registry entry for something Kelir integrates with
//! (Database Schema §12.1, architectures/03 §4.1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{code, required_text, Problems, MAX_NAME_LENGTH, MAX_URL_LENGTH};
use crate::error::AppError;
use crate::response::Pagination;
use crate::utils::serde::present_or_absent;

/// The shortest and longest request timeout a system may be configured with.
///
/// `timeout_seconds` has a default of 30 and no `CHECK` in §12.1. Zero or a
/// negative number would be a system nothing could ever reach, and an hour is
/// a worker held for an hour; five minutes is the ceiling a synchronous call
/// can plausibly want.
pub const MIN_TIMEOUT_SECONDS: i32 = 1;
pub const MAX_TIMEOUT_SECONDS: i32 = 300;

/// Retry bounds for [`RetryPolicy`]. Nothing retries yet (FR-INT-007), so these
/// bound what can be *configured*, and are wide enough not to pre-empt that
/// row's decisions.
pub const MAX_RETRIES: u32 = 20;
pub const MAX_INITIAL_DELAY_SECONDS: u32 = 86_400;
pub const MAX_BACKOFF_MULTIPLIER: f64 = 10.0;
pub const MAX_DEAD_LETTER_AFTER_ATTEMPTS: u32 = 50;

/// Why an edit may not move a system into or out of `INACTIVE`.
///
/// **Turning a system on or off is one permission** — the product owner's
/// decision on #520, 2026-09-25 — and it is `integration:external-system:deactivate`,
/// read as *change whether it is active*. An edit is `:update`'s, so letting a
/// `PUT` set `INACTIVE`, or set any status on a system that is `INACTIVE`,
/// would be that permission reached through the other one. `ACTIVE` ↔
/// `MAINTENANCE` on a system that is not `INACTIVE` stays an edit: it says
/// whether the system is being worked on, not whether it is in service.
pub const ACTIVATION_IS_NOT_AN_EDIT: &str = "Whether a system is active is changed through \
     POST /external-systems/{id}/deactivate and POST /external-systems/{id}/activate, \
     not by an edit";

/// The refusal an edit gets for touching `status` on an `INACTIVE` system.
pub fn activation_is_not_an_edit() -> AppError {
    AppError::validation(vec![crate::error::ValidationDetail::new(
        "status",
        "enum",
        "NOT_ALLOWED",
        ACTIVATION_IS_NOT_AN_EDIT,
    )])
}

/// `external_systems.status` (§12.1's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalSystemStatus {
    Active,
    Inactive,
    Maintenance,
}

impl ExternalSystemStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Maintenance => "MAINTENANCE",
        }
    }

    /// An unknown stored value reads as `Inactive` — a system nobody recognises
    /// is not offered as usable.
    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "MAINTENANCE" => Self::Maintenance,
            _ => Self::Inactive,
        }
    }
}

/// `external_systems.system_type` — the vocabulary §12.1 gives in a comment.
///
/// **The column has no `CHECK`**, so this list is held by the API alone; a
/// value outside it is refused at the boundary, and a stored value outside it
/// (written by something other than this module) reads back as `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalSystemType {
    Erp,
    Crm,
    Hris,
    ESignature,
    EmailProvider,
    SsoProvider,
    PaymentGateway,
    TaxSystem,
    BankSystem,
    BiSystem,
    DocumentArchive,
}

impl ExternalSystemType {
    pub const ALL: &'static [Self] = &[
        Self::Erp,
        Self::Crm,
        Self::Hris,
        Self::ESignature,
        Self::EmailProvider,
        Self::SsoProvider,
        Self::PaymentGateway,
        Self::TaxSystem,
        Self::BankSystem,
        Self::BiSystem,
        Self::DocumentArchive,
    ];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Erp => "ERP",
            Self::Crm => "CRM",
            Self::Hris => "HRIS",
            Self::ESignature => "E_SIGNATURE",
            Self::EmailProvider => "EMAIL_PROVIDER",
            Self::SsoProvider => "SSO_PROVIDER",
            Self::PaymentGateway => "PAYMENT_GATEWAY",
            Self::TaxSystem => "TAX_SYSTEM",
            Self::BankSystem => "BANK_SYSTEM",
            Self::BiSystem => "BI_SYSTEM",
            Self::DocumentArchive => "DOCUMENT_ARCHIVE",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_db() == value)
    }
}

/// How a system authenticates — `external_systems.auth_type` and, with the
/// same `CHECK`, `integration_credentials.credential_type` (§12.1, §12.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum AuthType {
    #[serde(rename = "API_KEY")]
    ApiKey,
    #[serde(rename = "BASIC_AUTH")]
    BasicAuth,
    #[serde(rename = "BEARER_TOKEN")]
    BearerToken,
    #[serde(rename = "OAUTH2_CLIENT_CREDENTIALS")]
    Oauth2ClientCredentials,
    #[serde(rename = "JWT")]
    Jwt,
    #[serde(rename = "HMAC_SECRET")]
    HmacSecret,
    #[serde(rename = "CERTIFICATE")]
    Certificate,
    #[serde(rename = "SFTP_PASSWORD")]
    SftpPassword,
}

impl AuthType {
    pub const ALL: &'static [Self] = &[
        Self::ApiKey,
        Self::BasicAuth,
        Self::BearerToken,
        Self::Oauth2ClientCredentials,
        Self::Jwt,
        Self::HmacSecret,
        Self::Certificate,
        Self::SftpPassword,
    ];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::ApiKey => "API_KEY",
            Self::BasicAuth => "BASIC_AUTH",
            Self::BearerToken => "BEARER_TOKEN",
            Self::Oauth2ClientCredentials => "OAUTH2_CLIENT_CREDENTIALS",
            Self::Jwt => "JWT",
            Self::HmacSecret => "HMAC_SECRET",
            Self::Certificate => "CERTIFICATE",
            Self::SftpPassword => "SFTP_PASSWORD",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_db() == value)
    }
}

/// `retry_policy_json` — the four keys §12.1 names, each optional.
///
/// **Typed rather than free JSON**, so a caller misspelling `maxRetries` is
/// told so rather than storing a policy nothing will ever read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetryPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dead_letter_after_attempts: Option<u32>,
}

impl RetryPolicy {
    /// Reads the stored column. A value this build cannot read — written by
    /// something else, or by a later release with a fifth key — reads as the
    /// empty policy rather than failing the whole system's read.
    pub fn from_db(value: serde_json::Value) -> Self {
        serde_json::from_value(value).unwrap_or_default()
    }

    pub fn to_db(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// An external system as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSystem {
    pub id: Uuid,
    /// The business code — `SAP_ERP`. Unique per tenant and **not updatable**.
    pub system_code: String,
    pub system_name: String,
    pub system_type: Option<ExternalSystemType>,
    pub base_url: Option<String>,
    pub auth_type: Option<AuthType>,
    pub timeout_seconds: i32,
    pub retry_policy: RetryPolicy,
    pub description: Option<String>,
    pub status: ExternalSystemStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Registering a system. It is always registered `ACTIVE`.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterExternalSystemRequest {
    pub system_code: String,
    pub system_name: String,
    pub system_type: Option<ExternalSystemType>,
    /// `http` or `https`, with a host, and **no user name, password or query
    /// string in it** — a credential belongs in a credential reference, not in
    /// a URL anyone with `:read` can see.
    pub base_url: Option<String>,
    pub auth_type: Option<AuthType>,
    /// Defaults to 30.
    pub timeout_seconds: Option<i32>,
    pub retry_policy: Option<RetryPolicy>,
    pub description: Option<String>,
}

/// Editing a system. `None` leaves a field alone; for the nullable fields,
/// `null` clears it.
///
/// `systemCode` is absent because it may not change: it is what configuration
/// names a system by. `status` accepts `ACTIVE` and `MAINTENANCE` only, and
/// only on a system that is not `INACTIVE` — **into and out of `INACTIVE` is
/// `POST {id}/deactivate` and `POST {id}/activate`**, under their own
/// permission ([`ACTIVATION_IS_NOT_AN_EDIT`]).
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateExternalSystemRequest {
    pub system_name: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<ExternalSystemType>)]
    pub system_type: Option<Option<ExternalSystemType>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<String>)]
    pub base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<AuthType>)]
    pub auth_type: Option<Option<AuthType>>,
    pub timeout_seconds: Option<i32>,
    /// Replaces the stored policy whole.
    pub retry_policy: Option<RetryPolicy>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    pub status: Option<ExternalSystemStatus>,
}

/// The list's query string: paging, a search and two filters.
///
/// Unknown parameters are ignored, as [`Pagination`] ignores them everywhere.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ExternalSystemQuery {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the system code or name. `%` and `_` in
    /// it match themselves.
    pub search: Option<String>,
    /// `ACTIVE`, `INACTIVE` or `MAINTENANCE`.
    pub status: Option<ExternalSystemStatus>,
    /// One of the system types.
    pub system_type: Option<ExternalSystemType>,
}

impl ExternalSystemQuery {
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }
}

pub fn validate_register(request: &RegisterExternalSystemRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    code(&request.system_code, "systemCode", &mut problems);
    required_text(
        &request.system_name,
        "systemName",
        MAX_NAME_LENGTH,
        &mut problems,
    );

    if let Some(url) = request.base_url.as_deref() {
        base_url(url, &mut problems);
    }
    if let Some(timeout) = request.timeout_seconds {
        timeout_seconds(timeout, &mut problems);
    }
    if let Some(policy) = &request.retry_policy {
        retry_policy(policy, &mut problems);
    }

    problems.finish()
}

pub fn validate_update(request: &UpdateExternalSystemRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    if let Some(name) = &request.system_name {
        required_text(name, "systemName", MAX_NAME_LENGTH, &mut problems);
    }
    if let Some(Some(url)) = request.base_url.as_ref() {
        base_url(url, &mut problems);
    }
    if let Some(timeout) = request.timeout_seconds {
        timeout_seconds(timeout, &mut problems);
    }
    if let Some(policy) = &request.retry_policy {
        retry_policy(policy, &mut problems);
    }
    if request.status == Some(ExternalSystemStatus::Inactive) {
        problems.push("status", "enum", "NOT_ALLOWED", ACTIVATION_IS_NOT_AN_EDIT);
    }

    problems.finish()
}

/// A base URL: absolute `http`/`https` with a host, at most
/// [`MAX_URL_LENGTH`], no fragment, **no user information and no query
/// string** — a scheme, host, port and path, and nothing that could carry a
/// key.
///
/// A blank value is not checked here — the service stores it as absent.
fn base_url(value: &str, problems: &mut Problems) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return;
    }
    if trimmed.chars().count() > MAX_URL_LENGTH {
        problems.push(
            "baseUrl",
            "maxLength",
            "TOO_LONG",
            format!("baseUrl must be at most {MAX_URL_LENGTH} characters"),
        );
        return;
    }

    let Ok(parsed) = url::Url::parse(trimmed) else {
        problems.push(
            "baseUrl",
            "format",
            "INVALID_FORMAT",
            "baseUrl must be an absolute http or https URL",
        );
        return;
    };

    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        problems.push(
            "baseUrl",
            "format",
            "INVALID_FORMAT",
            "baseUrl must be an absolute http or https URL with a host",
        );
    } else if !parsed.username().is_empty() || parsed.password().is_some() {
        // The one check here about secrets rather than shape: a URL of the form
        // `https://user:secret@host` is a plaintext credential stored in a
        // column every `:read` holder is shown.
        problems.push(
            "baseUrl",
            "format",
            "CREDENTIALS_IN_URL",
            "baseUrl must not carry a user name or password; register a credential reference instead",
        );
    } else if parsed.query().is_some() {
        // The second check about secrets: `?api_key=sk_live_…` is where an API
        // key goes when someone pastes a working call as a base URL, and the
        // column is shown to every `:read` holder and written to the audit
        // trail (#520, finding F1). Any query is refused, an empty `?` too:
        // no query a base URL could carry is configuration rather than a call.
        problems.push(
            "baseUrl",
            "format",
            "QUERY_IN_BASE_URL",
            "baseUrl is a scheme, host, port and path only; a query string belongs to a call, \
             not to the system, and a key in one belongs in a credential reference",
        );
    } else if parsed.fragment().is_some() {
        problems.push(
            "baseUrl",
            "format",
            "INVALID_FORMAT",
            "baseUrl must not carry a fragment",
        );
    }
}

fn timeout_seconds(value: i32, problems: &mut Problems) {
    if !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&value) {
        problems.push(
            "timeoutSeconds",
            "range",
            "OUT_OF_RANGE",
            format!(
                "timeoutSeconds must be between {MIN_TIMEOUT_SECONDS} and {MAX_TIMEOUT_SECONDS}"
            ),
        );
    }
}

fn retry_policy(policy: &RetryPolicy, problems: &mut Problems) {
    if policy.max_retries.is_some_and(|value| value > MAX_RETRIES) {
        problems.push(
            "retryPolicy.maxRetries",
            "range",
            "OUT_OF_RANGE",
            format!("maxRetries must be at most {MAX_RETRIES}"),
        );
    }
    if policy
        .initial_delay_seconds
        .is_some_and(|value| value > MAX_INITIAL_DELAY_SECONDS)
    {
        problems.push(
            "retryPolicy.initialDelaySeconds",
            "range",
            "OUT_OF_RANGE",
            format!("initialDelaySeconds must be at most {MAX_INITIAL_DELAY_SECONDS}"),
        );
    }
    if policy
        .backoff_multiplier
        .is_some_and(|value| !(1.0..=MAX_BACKOFF_MULTIPLIER).contains(&value))
    {
        problems.push(
            "retryPolicy.backoffMultiplier",
            "range",
            "OUT_OF_RANGE",
            format!("backoffMultiplier must be between 1 and {MAX_BACKOFF_MULTIPLIER}"),
        );
    }
    if policy
        .dead_letter_after_attempts
        .is_some_and(|value| !(1..=MAX_DEAD_LETTER_AFTER_ATTEMPTS).contains(&value))
    {
        problems.push(
            "retryPolicy.deadLetterAfterAttempts",
            "range",
            "OUT_OF_RANGE",
            format!(
                "deadLetterAfterAttempts must be between 1 and {MAX_DEAD_LETTER_AFTER_ATTEMPTS}"
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::details;
    use super::*;

    fn register() -> RegisterExternalSystemRequest {
        RegisterExternalSystemRequest {
            system_code: "SAP_ERP".to_owned(),
            system_name: "Corporate ERP".to_owned(),
            system_type: Some(ExternalSystemType::Erp),
            base_url: Some("https://erp.example.com/api".to_owned()),
            auth_type: Some(AuthType::Oauth2ClientCredentials),
            timeout_seconds: Some(30),
            retry_policy: None,
            description: None,
        }
    }

    fn codes(request: &RegisterExternalSystemRequest) -> Vec<(String, String)> {
        details(validate_register(request).expect_err("refused"))
            .into_iter()
            .map(|detail| (detail.path, detail.code))
            .collect()
    }

    #[test]
    fn a_complete_registration_is_valid() {
        assert!(validate_register(&register()).is_ok());
    }

    #[test]
    fn a_base_url_must_be_http_or_https_with_a_host() {
        for bad in [
            "ftp://erp.example.com",
            "erp.example.com/api",
            "https://",
            "not a url",
        ] {
            let mut request = register();
            request.base_url = Some(bad.to_owned());

            assert_eq!(
                codes(&request),
                vec![("baseUrl".to_owned(), "INVALID_FORMAT".to_owned())],
                "{bad}"
            );
        }
    }

    #[test]
    fn a_base_url_carrying_a_password_is_refused_as_a_credential() {
        for bad in [
            "https://svc:hunter2@erp.example.com/api",
            "https://svc@erp.example.com/api",
        ] {
            let mut request = register();
            request.base_url = Some(bad.to_owned());

            assert_eq!(
                codes(&request),
                vec![("baseUrl".to_owned(), "CREDENTIALS_IN_URL".to_owned())],
                "{bad}"
            );
        }
    }

    #[test]
    fn a_base_url_with_any_query_string_is_refused() {
        for bad in [
            concat!(
                "https://erp.example.com/api?api_key=sk_live",
                "_4eC39HqLyjWDarjtT1zdp7dc"
            ),
            "https://erp.example.com/api?",
            "https://erp.example.com:8443/api?version=2",
        ] {
            let mut request = register();
            request.base_url = Some(bad.to_owned());

            assert_eq!(
                codes(&request),
                vec![("baseUrl".to_owned(), "QUERY_IN_BASE_URL".to_owned())],
                "{bad}"
            );
        }

        let mut request = register();
        request.base_url = Some("https://erp.example.com:8443/api/v2".to_owned());
        assert!(
            validate_register(&request).is_ok(),
            "a port and a path are fine"
        );
    }

    #[test]
    fn a_timeout_outside_its_bounds_is_refused() {
        for bad in [0, -1, MAX_TIMEOUT_SECONDS + 1] {
            let mut request = register();
            request.timeout_seconds = Some(bad);

            assert_eq!(
                codes(&request),
                vec![("timeoutSeconds".to_owned(), "OUT_OF_RANGE".to_owned())]
            );
        }
    }

    #[test]
    fn a_retry_policy_is_bounded_key_by_key() {
        let mut request = register();
        request.retry_policy = Some(RetryPolicy {
            max_retries: Some(MAX_RETRIES + 1),
            initial_delay_seconds: Some(5),
            backoff_multiplier: Some(0.5),
            dead_letter_after_attempts: Some(0),
        });

        let paths: Vec<String> = codes(&request).into_iter().map(|(path, _)| path).collect();

        assert_eq!(
            paths,
            vec![
                "retryPolicy.maxRetries",
                "retryPolicy.backoffMultiplier",
                "retryPolicy.deadLetterAfterAttempts"
            ]
        );
    }

    #[test]
    fn an_edit_cannot_deactivate() {
        let request = UpdateExternalSystemRequest {
            status: Some(ExternalSystemStatus::Inactive),
            ..Default::default()
        };

        let details = details(validate_update(&request).expect_err("refused"));

        assert_eq!(details[0].path, "status");
        assert_eq!(details[0].code, "NOT_ALLOWED");
    }

    #[test]
    fn an_edit_may_put_a_system_into_maintenance_and_back() {
        for status in [
            ExternalSystemStatus::Maintenance,
            ExternalSystemStatus::Active,
        ] {
            let request = UpdateExternalSystemRequest {
                status: Some(status),
                ..Default::default()
            };

            assert!(validate_update(&request).is_ok());
        }
    }

    #[test]
    fn every_vocabulary_round_trips_through_its_column_and_its_wire_form() {
        for kind in ExternalSystemType::ALL {
            assert_eq!(ExternalSystemType::from_db(kind.as_db()), Some(*kind));
            assert_eq!(
                serde_json::to_value(kind).expect("serializes"),
                serde_json::json!(kind.as_db())
            );
        }
        for kind in AuthType::ALL {
            assert_eq!(AuthType::from_db(kind.as_db()), Some(*kind));
            assert_eq!(
                serde_json::to_value(kind).expect("serializes"),
                serde_json::json!(kind.as_db())
            );
        }
        for status in [
            ExternalSystemStatus::Active,
            ExternalSystemStatus::Inactive,
            ExternalSystemStatus::Maintenance,
        ] {
            assert_eq!(ExternalSystemStatus::from_db(status.as_db()), status);
            assert_eq!(
                serde_json::to_value(status).expect("serializes"),
                serde_json::json!(status.as_db())
            );
        }
    }

    #[test]
    fn a_retry_policy_nobody_can_read_is_the_empty_policy() {
        assert_eq!(
            RetryPolicy::from_db(serde_json::json!({ "maxRetries": "three" })),
            RetryPolicy::default()
        );
        assert_eq!(
            RetryPolicy::from_db(serde_json::json!({ "maxRetries": 3 })).max_retries,
            Some(3)
        );
    }
}
