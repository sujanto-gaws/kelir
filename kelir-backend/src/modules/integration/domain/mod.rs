//! Domain types and the rules requests are held to.
//!
//! Three entities — [`external_system`], [`endpoint`] and [`credential`] — and
//! the few checks they share, which live here so that a code, a name and a
//! length are refused identically whichever of the three carried them.

pub mod credential;
pub mod endpoint;
pub mod external_system;

pub use credential::{
    CreateIntegrationCredentialRequest, IntegrationCredential, UpdateIntegrationCredentialRequest,
};
pub use endpoint::{
    CreateIntegrationEndpointRequest, EndpointStatus, HttpMethod, IntegrationEndpoint,
    UpdateIntegrationEndpointRequest,
};
pub use external_system::{
    AuthType, ExternalSystem, ExternalSystemQuery, ExternalSystemStatus, ExternalSystemType,
    RegisterExternalSystemRequest, RetryPolicy, UpdateExternalSystemRequest,
};

use crate::error::{AppError, ValidationDetail};

/// `VARCHAR(64)` — `system_code`, `endpoint_code` (Database Schema §1.3.1).
pub const MAX_CODE_LENGTH: usize = 64;
/// `VARCHAR(200)` — `integration_endpoints.name`, and the bound the API puts on
/// `external_systems.system_name`, which §12.1 declares `TEXT`: a name is a
/// name wherever it is stored.
pub const MAX_NAME_LENGTH: usize = 200;
/// The URL width §1.3.1 sets, applied to `base_url` and `path`, both `TEXT` in
/// §12.
pub const MAX_URL_LENGTH: usize = 2048;

/// Collected field failures, reported together so a caller who got three
/// fields wrong learns all three from one response.
#[derive(Debug, Default)]
pub struct Problems(Vec<ValidationDetail>);

impl Problems {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, path: &str, rule: &str, code: &str, message: impl Into<String>) {
        self.0
            .push(ValidationDetail::new(path, rule, code, message.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn finish(self) -> Result<(), AppError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(AppError::validation(self.0))
        }
    }
}

/// A required, bounded, trimmed text value.
pub fn required_text(value: &str, path: &str, max: usize, problems: &mut Problems) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        problems.push(path, "required", "REQUIRED", format!("{path} is required"));
    } else if trimmed.chars().count() > max {
        problems.push(
            path,
            "maxLength",
            "TOO_LONG",
            format!("{path} must be at most {max} characters"),
        );
    }
}

/// A business code: required, at most [`MAX_CODE_LENGTH`], and made of
/// letters, digits, `_`, `-` and `.` only.
///
/// **The character set is this API's choice, not the schema's.** A code is
/// what configuration and a person name a system or an endpoint by — `SAP_ERP`,
/// `CREATE_PURCHASE_ORDER` — and a space or a slash in one is a code that will
/// break the first place it is interpolated.
pub fn code(value: &str, path: &str, problems: &mut Problems) {
    let trimmed = value.trim();
    let before = problems.len();

    required_text(value, path, MAX_CODE_LENGTH, problems);

    if problems.len() == before
        && !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.".contains(character))
    {
        problems.push(
            path,
            "format",
            "INVALID_FORMAT",
            format!("{path} may contain only letters, digits, '_', '-' and '.'"),
        );
    }
}

/// An optional value that, when present and not blank, is trimmed; blank reads
/// as absent.
pub fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
pub(crate) fn details(error: AppError) -> Vec<ValidationDetail> {
    match error {
        AppError::Validation { details } => details,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_code(value: &str) -> Result<(), AppError> {
        let mut problems = Problems::new();
        code(value, "systemCode", &mut problems);
        problems.finish()
    }

    #[test]
    fn a_plain_code_is_accepted() {
        assert!(check_code("SAP_ERP").is_ok());
        assert!(check_code("erp-1.v2").is_ok());
    }

    #[test]
    fn a_blank_code_is_required_not_malformed() {
        let details = details(check_code("   ").expect_err("refused"));

        assert_eq!(details.len(), 1, "one failure, not two: {details:?}");
        assert_eq!(details[0].code, "REQUIRED");
    }

    #[test]
    fn a_code_with_a_space_or_a_slash_is_refused() {
        for bad in ["SAP ERP", "SAP/ERP", "SAP:ERP"] {
            let details = details(check_code(bad).expect_err("refused"));
            assert_eq!(details[0].code, "INVALID_FORMAT", "{bad}");
        }
    }

    #[test]
    fn a_code_longer_than_the_column_is_refused() {
        let details = details(check_code(&"C".repeat(MAX_CODE_LENGTH + 1)).expect_err("refused"));

        assert_eq!(details[0].code, "TOO_LONG");
    }
}
