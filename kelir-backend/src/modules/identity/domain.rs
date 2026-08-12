use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Account lifecycle (SRS FR-IDM-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserStatus {
    Active,
    Inactive,
    Locked,
    PendingActivation,
}

impl UserStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
            Self::Locked => "LOCKED",
            Self::PendingActivation => "PENDING_ACTIVATION",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "INACTIVE" => Self::Inactive,
            "LOCKED" => Self::Locked,
            _ => Self::PendingActivation,
        }
    }

    /// Only an active account may authenticate. Every other state is a refusal,
    /// and the login path must not distinguish between them to the caller.
    pub fn can_sign_in(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// A user as returned by the API. Never carries the password hash.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub status: UserStatus,
    pub department_id: Option<Uuid>,
    pub must_change_password: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub roles: Vec<RoleSummary>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleSummary {
    pub id: Uuid,
    pub role_code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    pub id: Uuid,
    pub role_code: String,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub id: Uuid,
    pub permission_code: String,
    pub module: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<UserStatus>,
    pub department_id: Option<Uuid>,
    pub role_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    pub role_code: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub permission_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub permission_ids: Option<Vec<Uuid>>,
}

/// Minimum password length. Deliberately a length floor rather than a
/// composition rule: length is what resists guessing, and character-class rules
/// mostly produce predictable substitutions.
pub const MIN_PASSWORD_LENGTH: usize = 12;

/// Validates a create-user payload, collecting every problem rather than
/// stopping at the first — a form should be able to mark all its bad fields at
/// once (JSON Form Schema S10.3).
pub fn validate_create_user(request: &CreateUserRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    validate_username(&request.username, &mut details);
    validate_email(&request.email, &mut details);
    validate_password(&request.password, &mut details);

    if request.display_name.trim().is_empty() {
        details.push(ValidationDetail::new(
            "displayName",
            "required",
            "REQUIRED",
            "Display name is required",
        ));
    }

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

pub fn validate_password_value(password: &str) -> Result<(), AppError> {
    let mut details = Vec::new();
    validate_password(password, &mut details);

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

fn validate_username(username: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = username.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            "username",
            "required",
            "REQUIRED",
            "Username is required",
        ));
    } else if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        details.push(ValidationDetail::new(
            "username",
            "pattern",
            "INVALID_FORMAT",
            "Username may contain only letters, digits, dot, dash and underscore",
        ));
    }
}

fn validate_email(email: &str, details: &mut Vec<ValidationDetail>) {
    // Deliberately shallow: the only reliable proof an address exists is
    // delivering to it, and strict patterns reject valid addresses.
    let trimmed = email.trim();
    let plausible = trimmed.contains('@')
        && !trimmed.starts_with('@')
        && !trimmed.ends_with('@')
        && !trimmed.contains(char::is_whitespace);

    if !plausible {
        details.push(ValidationDetail::new(
            "email",
            "format",
            "INVALID_FORMAT",
            "Enter a valid email address",
        ));
    }
}

fn validate_password(password: &str, details: &mut Vec<ValidationDetail>) {
    if password.chars().count() < MIN_PASSWORD_LENGTH {
        details.push(ValidationDetail::new(
            "password",
            "minLength",
            "TOO_SHORT",
            format!("Password must be at least {MIN_PASSWORD_LENGTH} characters"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CreateUserRequest {
        CreateUserRequest {
            username: "user.john".to_owned(),
            email: "john@example.com".to_owned(),
            password: "a-sufficiently-long-password".to_owned(),
            display_name: "John".to_owned(),
            department_id: None,
            role_ids: vec![],
        }
    }

    fn details_of(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_valid_request() {
        assert!(validate_create_user(&request()).is_ok());
    }

    #[test]
    fn reports_every_problem_at_once() {
        // One round trip should be enough to fix a form.
        let bad = CreateUserRequest {
            username: "  ".to_owned(),
            email: "not-an-email".to_owned(),
            password: "short".to_owned(),
            display_name: "".to_owned(),
            ..request()
        };

        let details = details_of(validate_create_user(&bad).expect_err("invalid"));
        let paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();

        assert!(paths.contains(&"username"), "{paths:?}");
        assert!(paths.contains(&"email"), "{paths:?}");
        assert!(paths.contains(&"password"), "{paths:?}");
        assert!(paths.contains(&"displayName"), "{paths:?}");
    }

    #[test]
    fn rejects_a_username_with_spaces_or_symbols() {
        for username in ["john doe", "john@example", "john/../admin"] {
            let bad = CreateUserRequest {
                username: username.to_owned(),
                ..request()
            };
            assert!(
                validate_create_user(&bad).is_err(),
                "{username} should be rejected"
            );
        }
    }

    #[test]
    fn enforces_password_length_at_the_boundary() {
        let just_short = "a".repeat(MIN_PASSWORD_LENGTH - 1);
        let just_long = "a".repeat(MIN_PASSWORD_LENGTH);

        assert!(validate_password_value(&just_short).is_err());
        assert!(validate_password_value(&just_long).is_ok());
    }

    #[test]
    fn only_active_accounts_may_sign_in() {
        assert!(UserStatus::Active.can_sign_in());
        for status in [
            UserStatus::Inactive,
            UserStatus::Locked,
            UserStatus::PendingActivation,
        ] {
            assert!(!status.can_sign_in(), "{status:?} must not sign in");
        }
    }

    #[test]
    fn status_round_trips_through_the_database_vocabulary() {
        for status in [
            UserStatus::Active,
            UserStatus::Inactive,
            UserStatus::Locked,
            UserStatus::PendingActivation,
        ] {
            assert_eq!(UserStatus::from_db(status.as_db()), status);
        }
    }
}
