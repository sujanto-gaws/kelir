//! A credential reference — **where** one of a system's secrets lives
//! (Database Schema §12.3).
//!
//! # There is no secret here, and that is structural
//!
//! `secret_reference` holds `vault://kelir/erp/api-key` or
//! `env://KELIR_ERP_API_KEY`: a pointer into the deployment's secret store. No
//! type in this module has a field for the secret itself, no route resolves a
//! reference, and so no response, OpenAPI schema or log line can carry a
//! resolved secret (#520 AC-5). The runtime that will one day resolve a
//! reference to make a call is FR-INT-002's, and it will do so in process.
//!
//! # What [`validate_secret_reference`] checks, and what it cannot
//!
//! **Whether a string is a secret is not decidable from the string.** A random
//! API key and a random vault path are both strings of letters and digits. So
//! the check is on **shape**, and it is exactly this:
//!
//! * the value is at most [`MAX_SECRET_REFERENCE_LENGTH`] characters, with no
//!   surrounding whitespace kept;
//! * it begins with a scheme from a closed list — `vault://` or `env://`
//!   ([`SECRET_REFERENCE_SCHEMES`]) — so a bare token, a PEM block, a
//!   `user:password` pair or a URL to anywhere else is refused;
//! * **`env://NAME`**: `NAME` is an environment-variable name — an upper-case
//!   letter or `_`, then upper-case letters, digits and `_`;
//! * **`vault://path[#field]`**: `path` is one or more `/`-separated segments
//!   of letters, digits, `_`, `-` and `.`, none empty and none `.` or `..`, and
//!   the optional `#field` names one key of the secret in the same alphabet.
//!
//! That refuses anything with a space, a `+`, a `=` or a `/` where a segment
//! should be — which is most base64, every PEM block and every password with
//! punctuation — and it refuses anything without a scheme. **It does not
//! refuse a secret someone typed as a path segment** (`vault://sk_live_abc123`
//! has the right shape), and nothing that inspects only the string could.
//! What it guarantees is narrower and still worth having: every stored value
//! parses as a reference a resolver can follow.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::external_system::AuthType;
use super::Problems;
use crate::error::AppError;
use crate::utils::serde::present_or_absent;

/// The schemes a reference may use: the two secret stores architectures/03
/// names (environment and vault).
pub const SECRET_REFERENCE_SCHEMES: &[&str] = &["vault", "env"];

/// §1.3.1's width for opaque external references. `secret_reference` is `TEXT`
/// in §12.3; the bound is the API's.
pub const MAX_SECRET_REFERENCE_LENGTH: usize = 255;

/// A credential reference as the API returns it — under
/// `integration:credential:read` only.
///
/// **These nine fields are the whole of it** (#520 AC-5): the reference, the
/// type, the validity window and the flag, plus the ids and timestamps every
/// record carries. `tests/integration_external_systems.rs` asserts the key set,
/// so a field added here is a decision a test makes somebody take.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationCredential {
    pub id: Uuid,
    pub external_system_id: Uuid,
    pub credential_type: AuthType,
    /// Where the secret lives — `vault://kelir/erp/api-key`. Never the secret.
    pub secret_reference: String,
    pub valid_from: Option<NaiveDate>,
    pub valid_to: Option<NaiveDate>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateIntegrationCredentialRequest {
    pub credential_type: AuthType,
    /// `vault://path[#field]` or `env://NAME` — see the module documentation
    /// for exactly what is checked.
    pub secret_reference: String,
    pub valid_from: Option<NaiveDate>,
    pub valid_to: Option<NaiveDate>,
    /// Defaults to `true`.
    pub is_active: Option<bool>,
}

/// Editing a credential reference. `null` clears a validity bound.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateIntegrationCredentialRequest {
    pub credential_type: Option<AuthType>,
    pub secret_reference: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<NaiveDate>)]
    pub valid_from: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<NaiveDate>)]
    pub valid_to: Option<Option<NaiveDate>>,
    pub is_active: Option<bool>,
}

pub fn validate_create(request: &CreateIntegrationCredentialRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    validate_secret_reference(&request.secret_reference, &mut problems);
    validity_window(request.valid_from, request.valid_to, &mut problems);

    problems.finish()
}

/// Checks what can be checked of the request alone. The window is checked
/// again by the service against the stored bounds the request leaves alone.
pub fn validate_update(request: &UpdateIntegrationCredentialRequest) -> Result<(), AppError> {
    let mut problems = Problems::new();

    if let Some(reference) = &request.secret_reference {
        validate_secret_reference(reference, &mut problems);
    }

    problems.finish()
}

/// `validTo` before `validFrom` is a window nothing is ever valid in.
pub fn validate_window(from: Option<NaiveDate>, to: Option<NaiveDate>) -> Result<(), AppError> {
    let mut problems = Problems::new();
    validity_window(from, to, &mut problems);
    problems.finish()
}

fn validity_window(from: Option<NaiveDate>, to: Option<NaiveDate>, problems: &mut Problems) {
    if let (Some(from), Some(to)) = (from, to) {
        if to < from {
            problems.push(
                "validTo",
                "consistency",
                "WINDOW_INVERTED",
                "validTo must not be before validFrom",
            );
        }
    }
}

/// Holds a reference to the shape the module documentation states.
pub fn validate_secret_reference(value: &str, problems: &mut Problems) {
    const PATH: &str = "secretReference";
    let trimmed = value.trim();

    if trimmed.is_empty() {
        problems.push(PATH, "required", "REQUIRED", "secretReference is required");
        return;
    }
    if trimmed.chars().count() > MAX_SECRET_REFERENCE_LENGTH {
        problems.push(
            PATH,
            "maxLength",
            "TOO_LONG",
            format!("secretReference must be at most {MAX_SECRET_REFERENCE_LENGTH} characters"),
        );
        return;
    }

    if !is_secret_reference(trimmed) {
        problems.push(
            PATH,
            "format",
            "NOT_A_SECRET_REFERENCE",
            "secretReference must be a reference into the secret store — vault://path[#field] \
             or env://NAME — and never the secret itself",
        );
    }
}

/// Whether `value` has the shape of a reference. Pure, so the rule can be
/// tested value by value.
pub fn is_secret_reference(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };

    match scheme {
        "env" => is_environment_name(rest),
        "vault" => is_vault_path(rest),
        _ => false,
    }
}

fn is_environment_name(name: &str) -> bool {
    let mut characters = name.chars();

    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

fn is_vault_path(value: &str) -> bool {
    let (path, field) = match value.split_once('#') {
        Some((path, field)) => (path, Some(field)),
        None => (value, None),
    };

    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "_-.".contains(character))
    };

    path.split('/').all(segment_ok) && field.is_none_or(segment_ok)
}

#[cfg(test)]
mod tests {
    use super::super::details;
    use super::*;

    #[test]
    fn a_vault_path_and_an_environment_name_are_references() {
        for good in [
            "vault://kelir/erp/api-key",
            "vault://kelir/erp/oauth#client_secret",
            "vault://erp",
            "env://KELIR_ERP_API_KEY",
            "env://_PRIVATE_1",
        ] {
            assert!(is_secret_reference(good), "{good}");
        }
    }

    #[test]
    fn a_raw_secret_in_any_of_its_usual_shapes_is_refused() {
        for bad in [
            // A bare token, as an API key usually arrives.
            concat!("sk_live", "_51HxQ2eKZ8r"),
            // Base64, with its padding and its slashes.
            "dXNlcjpwYXNzd29yZA==",
            // A user:password pair.
            "svc:hunter2",
            // A PEM block.
            "-----BEGIN PRIVATE KEY-----",
            // A URL somewhere else, with a password in it.
            "https://svc:hunter2@vault.example.com/x",
            // The right scheme with a value that is not a name.
            "env://kelir_erp_api_key",
            "env://KELIR ERP",
            "env://",
            // The right scheme with a value that is not a path.
            "vault://",
            "vault://kelir//erp",
            "vault://kelir/../root",
            "vault://kelir/erp/p@ss word",
            "vault://kelir/erp#",
            // A scheme outside the list.
            "file:///etc/secrets/erp",
            "aws-sm://erp",
        ] {
            assert!(!is_secret_reference(bad), "{bad}");
        }
    }

    #[test]
    fn a_refused_reference_names_the_field_and_the_rule() {
        let mut problems = Problems::new();
        validate_secret_reference("hunter2", &mut problems);

        let details = details(problems.finish().expect_err("refused"));

        assert_eq!(details[0].path, "secretReference");
        assert_eq!(details[0].code, "NOT_A_SECRET_REFERENCE");
    }

    #[test]
    fn a_reference_longer_than_its_bound_is_refused_for_length() {
        let mut problems = Problems::new();
        let long = format!("vault://{}", "a".repeat(MAX_SECRET_REFERENCE_LENGTH));
        validate_secret_reference(&long, &mut problems);

        let details = details(problems.finish().expect_err("refused"));

        assert_eq!(details[0].code, "TOO_LONG");
    }

    #[test]
    fn an_inverted_window_is_refused() {
        let from = NaiveDate::from_ymd_opt(2026, 9, 24);
        let to = NaiveDate::from_ymd_opt(2026, 9, 23);

        let details = details(validate_window(from, to).expect_err("refused"));

        assert_eq!(details[0].code, "WINDOW_INVERTED");
        assert!(validate_window(to, from).is_ok());
        assert!(validate_window(from, from).is_ok(), "a one-day window");
    }
}
