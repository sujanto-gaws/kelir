//! Departments — the organizational unit a user belongs to and a party role is
//! scoped by (FR-ORG-002, and FR-IDM-008's edge per decision **D-8**).
//!
//! **The table has existed since `0002_identity.sql` and nothing could put a
//! row in it.** That is what makes this the strongest of the four Phase 2
//! carry-overs: `master_data/repository/role.rs` validates a party role's
//! department against `departments`, so a consumer already depends on rows no
//! surface can create. Every other deferred item is unbuilt work nothing is
//! waiting on.
//!
//! **`parentDepartmentId` is a self-reference, so this is a tree that the
//! database cannot keep a tree.** It is the third appearance of that shape —
//! `mdm_facilities` (#141), and `rad_menus` / `rad_form_sections` (#191, still
//! open because nothing writes them). Here something does write it, so the
//! guard is built: a depth-bounded ancestor walk inside the caller's
//! transaction, under a tenant-wide lock, exactly as #133, #134 and #137 taught
//! facilities.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::modules::workflow::domain::jwss::RESERVED_DEPARTMENT_SCOPES;
use crate::utils::serde::present_or_absent;

/// Longest `departmentCode` §3.4 holds — `department_code VARCHAR(64)`.
pub const MAX_DEPARTMENT_CODE_LENGTH: usize = 64;
/// Longest `name` §3.4 holds — `name VARCHAR(200)`.
pub const MAX_NAME_LENGTH: usize = 200;

/// How deep a department tree may be walked when checking for a cycle.
///
/// Not a business rule about how deep an organization nests — it is the bound
/// that keeps the ancestor walk terminating if a cycle ever did reach storage.
/// The same value facilities use, for the same reason and with the same
/// consequence: a walk that hits it is **refused**, not assumed safe (#134).
pub const MAX_DEPARTMENT_DEPTH: i32 = 64;

/// Whether a department is in use (§3.4's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DepartmentStatus {
    Active,
    Inactive,
}

impl DepartmentStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Inactive => "INACTIVE",
        }
    }

    /// An unknown stored value reads as `Inactive`.
    ///
    /// Fails closed: a department nobody recognises stops being offered rather
    /// than the read returning a 500.
    pub fn from_db(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            _ => Self::Inactive,
        }
    }
}

/// A department as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Department {
    pub id: Uuid,
    /// The business code — `DEPT-PROC` (naming convention §8).
    pub department_code: String,
    pub name: String,
    /// The parent's **code**, not its surrogate id.
    ///
    /// A caller who knows a department knows its code; making them look up a
    /// UUID to move one under another would be a round trip for nothing. The
    /// same choice `parentFacilityId` makes, and for the same reason.
    pub parent_department_id: Option<String>,
    /// The manager's party code, likewise.
    pub manager_party_id: Option<String>,
    pub status: DepartmentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A department on a list screen. The same fields — a department is small
/// enough that a summary type would be a second thing to keep in step for no
/// saving.
pub type DepartmentSummary = Department;

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDepartmentRequest {
    pub department_id: String,
    pub name: String,
    pub parent_department_id: Option<String>,
    pub manager_party_id: Option<String>,
    pub status: Option<DepartmentStatus>,
}

/// Editing a department. `None` means *leave alone*; `Some(None)` clears a
/// nullable reference.
///
/// `departmentId` is absent because the code may not change: `users` and
/// `mdm_party_roles` point at the surrogate id, but a code is what an
/// integration and a person name a department by, and renaming it silently
/// re-points every report that filtered on it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDepartmentRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub parent_department_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub manager_party_id: Option<Option<String>>,
    pub status: Option<DepartmentStatus>,
}

fn bounded(
    value: &str,
    path: &str,
    max: usize,
    required: bool,
    details: &mut Vec<ValidationDetail>,
) {
    let trimmed = value.trim();

    if required && trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            format!("{path} is required"),
        ));
    } else if trimmed.chars().count() > max {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("{path} must be at most {max} characters"),
        ));
    }
}

/// Refuses a department code that is one of JWSS §5.1's `departmentScope`
/// words ([#767]).
///
/// `normalize` and the inbox clause read `OWNER_DEPARTMENT` and
/// `REQUESTED_DEPARTMENT` as scopes **before** any code lookup, so a department
/// coded with one could be created and never named by a workflow: an edge meant
/// for it resolves as the scope instead. Refused, as D-102 refuses a tenant code
/// the secret resolver could never serve, rather than documented as a trap.
///
/// **Compared on the trimmed code**, because the trimmed code is what is
/// stored, and **case-sensitively**, because a scope is matched as written:
/// `owner_department` is an ordinary code a scope can name, and stays allowed.
/// Collected with the other details, not returned early. A row written before
/// this check is left as it is (the CHANGELOG's upgrade note says how to find
/// one).
///
/// [#767]: https://github.com/sujanto-gaws/kelir/issues/767
fn refuse_reserved_code(code: &str, details: &mut Vec<ValidationDetail>) {
    let code = code.trim();

    if RESERVED_DEPARTMENT_SCOPES.contains(&code) {
        details.push(ValidationDetail::new(
            "departmentId",
            "reserved",
            "RESERVED_DEPARTMENT_CODE",
            format!(
                "departmentId {code} is a workflow departmentScope word, which is read as a \
                 scope and never as a department code, so no workflow could name this \
                 department. Choose another code"
            ),
        ));
    }
}

pub fn validate_create(request: &CreateDepartmentRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded(
        &request.department_id,
        "departmentId",
        MAX_DEPARTMENT_CODE_LENGTH,
        true,
        &mut details,
    );
    refuse_reserved_code(&request.department_id, &mut details);
    bounded(&request.name, "name", MAX_NAME_LENGTH, true, &mut details);

    // A department cannot be created under itself, because it does not exist
    // yet — but a caller naming its own code as the parent means something, and
    // "no such department" would be a confusing way to say it.
    if let Some(parent) = &request.parent_department_id {
        if parent.trim() == request.department_id.trim() && !parent.trim().is_empty() {
            details.push(ValidationDetail::new(
                "parentDepartmentId",
                "consistency",
                "CYCLE",
                "A department cannot be its own parent",
            ));
        }
    }

    finish(details)
}

pub fn validate_update(request: &UpdateDepartmentRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(name) = &request.name {
        bounded(name, "name", MAX_NAME_LENGTH, true, &mut details);
    }

    finish(details)
}

fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create() -> CreateDepartmentRequest {
        CreateDepartmentRequest {
            department_id: "DEPT-PROC".to_owned(),
            name: "Procurement".to_owned(),
            parent_department_id: None,
            manager_party_id: None,
            status: None,
        }
    }

    fn details(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_complete_request() {
        assert!(validate_create(&create()).is_ok());
    }

    #[test]
    fn requires_a_department_code() {
        let mut request = create();
        request.department_id = "  ".to_owned();

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "departmentId"));
    }

    #[test]
    fn refuses_a_code_longer_than_the_column() {
        let mut request = create();
        request.department_id = "D".repeat(MAX_DEPARTMENT_CODE_LENGTH + 1);

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "departmentId" && detail.code == "TOO_LONG"));
    }

    #[test]
    fn refuses_a_department_that_names_itself_as_its_parent() {
        // The one hop of the cycle problem that is visible before anything is
        // written. The rest needs the ancestor walk in the service.
        let mut request = create();
        request.parent_department_id = Some("DEPT-PROC".to_owned());

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.code == "CYCLE"));
    }

    fn reserved(request: &CreateDepartmentRequest) -> Vec<ValidationDetail> {
        match validate_create(request) {
            Ok(()) => Vec::new(),
            Err(error) => details(error)
                .into_iter()
                .filter(|detail| detail.code == "RESERVED_DEPARTMENT_CODE")
                .collect(),
        }
    }

    /// **#767 AC1, AC2**: both of JWSS §5.1's scope words are refused as a
    /// department code, padded or not, on `departmentId` with rule `reserved`.
    #[test]
    fn a_scope_word_is_refused_as_a_department_code() {
        for code in [
            "OWNER_DEPARTMENT",
            "REQUESTED_DEPARTMENT",
            "  OWNER_DEPARTMENT ",
        ] {
            let mut request = create();
            request.department_id = code.to_owned();
            let refused = reserved(&request);

            assert_eq!(refused.len(), 1, "{code:?} is refused once");
            assert_eq!(refused[0].path, "departmentId", "{code:?}");
            assert_eq!(refused[0].rule, "reserved", "{code:?}");
            assert!(
                refused[0].message.contains(code.trim()),
                "{code:?}: the message names the word, trimmed: {}",
                refused[0].message
            );
        }
    }

    /// **#767 AC2**: the comparison is case-sensitive and exact, because a
    /// scope is matched as written: each of these is a code a scope can name.
    #[test]
    fn a_code_that_only_resembles_a_scope_word_is_allowed() {
        for code in ["owner_department", "Owner_Department", "OWNER_DEPARTMENTS"] {
            let mut request = create();
            request.department_id = code.to_owned();

            assert!(
                validate_create(&request).is_ok(),
                "{code:?} is an ordinary department code"
            );
        }
    }

    /// **#767 AC3**: the refusal is collected with the other details rather
    /// than returned early, so a blank name is reported beside it.
    #[test]
    fn a_scope_word_is_reported_beside_the_other_defects() {
        let mut request = create();
        request.department_id = "REQUESTED_DEPARTMENT".to_owned();
        request.name = " ".to_owned();

        let details = details(validate_create(&request).expect_err("refused"));
        let codes: Vec<(&str, &str)> = details
            .iter()
            .map(|detail| (detail.path.as_str(), detail.code.as_str()))
            .collect();

        assert_eq!(
            codes,
            [
                ("departmentId", "RESERVED_DEPARTMENT_CODE"),
                ("name", "REQUIRED")
            ]
        );
    }

    /// **#767 AC4**: only the code is checked. A department may be *named*
    /// with a scope word.
    #[test]
    fn a_department_may_be_named_with_a_scope_word() {
        let mut request = create();
        request.name = "OWNER_DEPARTMENT".to_owned();

        assert!(validate_create(&request).is_ok());
    }

    /// **#767 AC6**: the list the refusal reads is exactly §5.1's two words,
    /// the two `normalize` matches.
    #[test]
    fn the_reserved_list_is_the_two_scope_words() {
        use crate::modules::workflow::domain::jwss::{
            OWNER_DEPARTMENT_SCOPE, REQUESTED_DEPARTMENT_SCOPE,
        };

        assert_eq!(OWNER_DEPARTMENT_SCOPE, "OWNER_DEPARTMENT");
        assert_eq!(REQUESTED_DEPARTMENT_SCOPE, "REQUESTED_DEPARTMENT");
        assert_eq!(
            RESERVED_DEPARTMENT_SCOPES,
            ["REQUESTED_DEPARTMENT", "OWNER_DEPARTMENT"]
        );
    }

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        let request = UpdateDepartmentRequest {
            name: None,
            parent_department_id: None,
            manager_party_id: None,
            status: None,
        };

        assert!(validate_update(&request).is_ok());
    }

    #[test]
    fn an_unknown_stored_status_reads_as_inactive() {
        assert_eq!(
            DepartmentStatus::from_db("ACTIVE"),
            DepartmentStatus::Active
        );
        assert_eq!(
            DepartmentStatus::from_db("ARCHIVED"),
            DepartmentStatus::Inactive
        );
    }
}
