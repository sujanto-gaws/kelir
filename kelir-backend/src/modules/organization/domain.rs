use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Lifecycle state of a tenant (`tenants.status`, database schema §1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TenantStatus {
    Active,
    Suspended,
    Inactive,
}

impl TenantStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Suspended => "SUSPENDED",
            Self::Inactive => "INACTIVE",
        }
    }

    /// Anything unrecognised is read as `Inactive`.
    ///
    /// The column is `CHECK`-constrained, so an unknown value means the schema
    /// moved ahead of this code. Defaulting a *permission-bearing* state to the
    /// closed one keeps that skew from admitting sign-ins it should refuse.
    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "SUSPENDED" => Self::Suspended,
            _ => Self::Inactive,
        }
    }

    /// Only an active tenant admits sign-in (FR-IDM-009).
    ///
    /// Suspension and deactivation are how an operator takes a tenant offline;
    /// if its users could still authenticate, neither would mean anything.
    pub fn admits_sign_in(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// A tenant, as the resolver needs it. Not an API type — nothing here is
/// serialised to a caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    pub id: Uuid,
    pub tenant_code: String,
    pub name: String,
    pub status: TenantStatus,
}

/// Canonical form of a tenant code.
///
/// Codes are uppercase by convention (`SYSTEM`, `TNT-001`), and a caller typing
/// `acme` means the same tenant as `ACME`. Normalising in one place is what lets
/// the lookup keep using the unique index on `tenant_code` — a case-insensitive
/// comparison in SQL would not — and is also what keeps the rate-limiter bucket
/// for a tenant from being split by capitalisation.
pub fn normalize_tenant_code(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

/// A tenant as the administration API publishes it (FR-ORG-001).
///
/// Separate from [`Tenant`], which the sign-in resolver reads, because the two
/// answer different questions and neither should grow the other's fields.
/// `settings_json` is deliberately absent: it is per-tenant configuration
/// (FR-CFG), not part of the tenant record an administrator maintains, and
/// publishing it here would make this the surface that edits it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantView {
    pub id: Uuid,
    pub tenant_code: String,
    pub name: String,
    pub status: TenantStatus,
    /// Whether this is the tenant administration is performed *from* — the
    /// deployment's default tenant (`KELIR_DEFAULT_TENANT_CODE`).
    ///
    /// It cannot be suspended or deleted, and the client uses this to disable
    /// those controls rather than let an administrator discover the refusal by
    /// being refused.
    pub is_default: bool,
    /// Live users in the tenant. Shown because suspending or deleting a tenant
    /// ends their sessions, and the number of people that affects is the one
    /// fact the confirmation needs.
    pub user_count: i64,
    pub created_at: DateTime<Utc>,
}

/// Creating a tenant creates its first administrator in the same transaction.
///
/// **Not two calls, by decision D-18.** A tenant with no user is exactly the
/// thing D-13 refused to let this surface build: a row nobody can sign in to.
/// The bootstrap switch cannot fill the gap either — `auth::bootstrap` is a
/// first-run switch for the deployment and deliberately does not fire per
/// tenant, and its own doc comment names this API as where a new tenant's first
/// administrator comes from instead.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTenantRequest {
    /// Business identifier, `TNT-001` shaped (naming convention §8). Normalised
    /// to upper case, and the handle a caller signs in with in multi-tenant
    /// mode.
    pub tenant_code: String,
    pub name: String,
    pub administrator: TenantAdministratorInput,
}

/// The first administrator of a new tenant.
///
/// Mirrors `identity::domain::CreateUserRequest` minus `roleIds`: the role is
/// not the caller's to choose. A new tenant has exactly one role at the moment
/// it is created — its own `ROLE-ADMIN`, provisioned beside it — so offering a
/// role list would offer an empty one.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantAdministratorInput {
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub password: String,
}

/// `tenantCode` is absent by design, matching `UpdateRoleRequest`.
///
/// The code is the handle users sign in with. Changing it would strand every
/// user of the tenant at a login form asking for a code that no longer resolves,
/// with no way for them to learn the new one — and no session carries the code
/// (the JWT carries `tenant_id`), so nothing would break loudly enough to be
/// noticed from the inside.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateTenantRequest {
    pub name: Option<String>,
    pub status: Option<TenantStatus>,
}

/// Longest tenant code the column holds (`tenants.tenant_code VARCHAR(64)`).
///
/// Read by the sign-in resolver too, which checks a requested code against it
/// before querying: an over-long value can match nothing, so refusing it early
/// saves a round trip and keeps an unbounded caller-controlled string out of
/// the log line that follows.
pub const MAX_TENANT_CODE_LEN: usize = 64;

/// Shortest one worth having. Two characters is arbitrary; nothing shorter is a
/// business identifier, and a one-character code invites a typo that resolves.
pub const MIN_TENANT_CODE_LEN: usize = 2;

/// Longest name the column holds (`tenants.name VARCHAR(200)`).
pub const MAX_TENANT_NAME_LEN: usize = 200;

/// Validates a create-tenant payload, collecting every problem at once (JSON
/// Form Schema S10.3) so one round trip is enough to fix the form.
///
/// The administrator's own fields are *not* validated here — they are validated
/// by `identity::domain`, which owns that vocabulary, so the two surfaces cannot
/// drift on what a username or a password is.
pub fn validate_create_tenant(request: &CreateTenantRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    validate_tenant_code(&request.tenant_code, &mut details);
    validate_tenant_name(&request.name, "name", &mut details);

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

/// Validates an update payload. An absent field is not a problem — it means
/// "leave this alone" — so only what the caller sent is checked.
pub fn validate_update_tenant(request: &UpdateTenantRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(name) = &request.name {
        validate_tenant_name(name, "name", &mut details);
    }

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

fn validate_tenant_code(raw: &str, details: &mut Vec<ValidationDetail>) {
    let code = normalize_tenant_code(raw);

    if code.is_empty() {
        details.push(ValidationDetail::new(
            "tenantCode",
            "required",
            "REQUIRED",
            "Tenant code is required",
        ));
        return;
    }

    if code.len() < MIN_TENANT_CODE_LEN || code.len() > MAX_TENANT_CODE_LEN {
        details.push(ValidationDetail::new(
            "tenantCode",
            "length",
            "INVALID_LENGTH",
            format!(
                "Tenant code must be between {MIN_TENANT_CODE_LEN} and {MAX_TENANT_CODE_LEN} characters"
            ),
        ));
    }

    // Letters, digits and dash only. Deliberately narrower than the column:
    // this value is typed into a login form by someone who was told it over the
    // phone, so a code that differs from another only by punctuation or an
    // invisible character is a support call rather than a feature.
    if !code
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        details.push(ValidationDetail::new(
            "tenantCode",
            "pattern",
            "INVALID_FORMAT",
            "Tenant code may contain only letters, digits, dash and underscore",
        ));
    }
}

fn validate_tenant_name(raw: &str, path: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "Name is required",
        ));
    } else if trimmed.chars().count() > MAX_TENANT_NAME_LEN {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("Name must be at most {MAX_TENANT_NAME_LEN} characters"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_tenants_admit_sign_in() {
        assert!(TenantStatus::Active.admits_sign_in());
        assert!(!TenantStatus::Suspended.admits_sign_in());
        assert!(!TenantStatus::Inactive.admits_sign_in());
    }

    #[test]
    fn status_round_trips_through_the_database_vocabulary() {
        for status in [
            TenantStatus::Active,
            TenantStatus::Suspended,
            TenantStatus::Inactive,
        ] {
            assert_eq!(TenantStatus::from_db(status.as_db()), status);
        }
    }

    #[test]
    fn an_unrecognised_status_does_not_admit_sign_in() {
        // Schema drift must fail closed, never open.
        assert!(!TenantStatus::from_db("PROVISIONING").admits_sign_in());
        assert!(!TenantStatus::from_db("").admits_sign_in());
    }

    #[test]
    fn normalises_case_and_surrounding_space() {
        for raw in ["acme", "ACME", "  Acme  ", "\tacme\n"] {
            assert_eq!(normalize_tenant_code(raw), "ACME", "for {raw:?}");
        }
    }

    #[test]
    fn normalisation_is_idempotent() {
        // Config normalises on load and requests normalise on arrival; applying
        // it twice must not change the answer.
        let once = normalize_tenant_code(" system ");
        assert_eq!(normalize_tenant_code(&once), once);
    }

    fn create_request() -> CreateTenantRequest {
        CreateTenantRequest {
            tenant_code: "TNT-001".to_owned(),
            name: "Acme Manufacturing".to_owned(),
            administrator: TenantAdministratorInput {
                username: "acme.admin".to_owned(),
                email: "admin@acme.example".to_owned(),
                display_name: "Acme Administrator".to_owned(),
                password: "a-sufficiently-long-password".to_owned(),
            },
        }
    }

    fn details_of(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_well_formed_tenant() {
        assert!(validate_create_tenant(&create_request()).is_ok());
    }

    #[test]
    fn accepts_a_lower_case_code_because_it_normalises() {
        // The code the caller types is normalised before it is checked, so
        // rejecting `tnt-001` here would refuse a value the lookup accepts.
        let request = CreateTenantRequest {
            tenant_code: "  tnt-001 ".to_owned(),
            ..create_request()
        };

        assert!(validate_create_tenant(&request).is_ok());
    }

    #[test]
    fn rejects_a_code_with_punctuation_a_caller_cannot_see() {
        // This value is read out over the phone and typed into a login form.
        // Two codes differing only by a space or a dot are a support call.
        for code in ["TNT 001", "TNT.001", "TNT/001", "TNT@001", "TNT\u{00A0}001"] {
            let request = CreateTenantRequest {
                tenant_code: code.to_owned(),
                ..create_request()
            };

            let details = details_of(validate_create_tenant(&request).expect_err(code));
            assert!(
                details.iter().any(|detail| detail.path == "tenantCode"),
                "{code} was accepted"
            );
        }
    }

    #[test]
    fn rejects_a_code_that_is_too_short_or_longer_than_the_column() {
        for code in ["", " ", "X", &"A".repeat(MAX_TENANT_CODE_LEN + 1)] {
            let request = CreateTenantRequest {
                tenant_code: code.to_owned(),
                ..create_request()
            };

            assert!(
                validate_create_tenant(&request).is_err(),
                "{code:?} was accepted"
            );
        }
    }

    #[test]
    fn accepts_a_code_at_both_length_boundaries() {
        for code in [
            "A".repeat(MIN_TENANT_CODE_LEN),
            "A".repeat(MAX_TENANT_CODE_LEN),
        ] {
            let request = CreateTenantRequest {
                tenant_code: code.clone(),
                ..create_request()
            };

            assert!(
                validate_create_tenant(&request).is_ok(),
                "{} characters was refused",
                code.len()
            );
        }
    }

    #[test]
    fn requires_a_name() {
        let request = CreateTenantRequest {
            name: "   ".to_owned(),
            ..create_request()
        };

        let details = details_of(validate_create_tenant(&request).expect_err("blank"));
        assert!(details.iter().any(|detail| detail.path == "name"));
    }

    #[test]
    fn reports_a_bad_code_and_a_bad_name_together() {
        // One round trip should be enough to fix the form (S10.3).
        let request = CreateTenantRequest {
            tenant_code: "!".to_owned(),
            name: String::new(),
            ..create_request()
        };

        let details = details_of(validate_create_tenant(&request).expect_err("invalid"));
        let paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();

        assert!(paths.contains(&"tenantCode"), "{paths:?}");
        assert!(paths.contains(&"name"), "{paths:?}");
    }

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        // Absent means "leave it alone", which is not a problem to report.
        let request = UpdateTenantRequest {
            name: None,
            status: None,
        };

        assert!(validate_update_tenant(&request).is_ok());
    }

    #[test]
    fn an_update_still_refuses_a_blank_name() {
        let request = UpdateTenantRequest {
            name: Some("  ".to_owned()),
            status: None,
        };

        assert!(validate_update_tenant(&request).is_err());
    }

    #[test]
    fn status_is_published_in_the_database_vocabulary() {
        // The API and the CHECK constraint must spell these the same way, or a
        // client sending the status it was given back gets a 422.
        let json = serde_json::to_string(&TenantStatus::Suspended).expect("serialises");

        assert_eq!(json, "\"SUSPENDED\"");
        assert_eq!(
            serde_json::from_str::<TenantStatus>("\"INACTIVE\"").expect("deserialises"),
            TenantStatus::Inactive
        );
    }
}
