use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::modules::workflow::domain::{DefinitionNamingRole, WorkflowDefinitionStatus};
use crate::utils::serde::present_or_absent;

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

    /// Every status. A test matches on each variant, so a fifth one that is
    /// not added here fails to compile there.
    pub const ALL: [Self; 4] = [
        Self::Active,
        Self::Inactive,
        Self::Locked,
        Self::PendingActivation,
    ];

    /// The `users.status` values [`can_sign_in`](Self::can_sign_in) accepts,
    /// as the database spells them ([#508]).
    ///
    /// A role's live-holder count reads only users with one of these, so a
    /// holder who has been deactivated no longer counts. The count is taken in
    /// SQL, and this is how it asks the rule here rather than a copy of it.
    ///
    /// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
    pub fn signing_in_db_values() -> Vec<&'static str> {
        Self::ALL
            .into_iter()
            .filter(|status| status.can_sign_in())
            .map(Self::as_db)
            .collect()
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
    /// When a failed-login lockout ends (NFR-SEC-008). `None` or a time already
    /// past means the account is not locked out. Distinct from
    /// [`UserStatus::Locked`], which is an administrator's decision and does not
    /// expire — an administrator seeing `ACTIVE` here needs this field to
    /// explain why the account is still refusing to sign in.
    pub locked_until: Option<DateTime<Utc>>,
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
    /// How many users hold the role today ([#508], **D-91** (2)): a grant not
    /// deleted and inside its `valid_from`/`valid_to` window, held by a user not
    /// deleted whose status can sign in. A user holding the role in several
    /// departments counts once.
    ///
    /// **Present only on a list or read by a caller holding
    /// `workflow:task:reassign`**, the permission that can act on it, and
    /// omitted otherwise. `0` beside a non-zero `openTasks` is a role whose
    /// last holder has left while tasks still need it.
    ///
    /// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_holders: Option<i64>,
    /// How many open tasks need the role ([#508]): the count a delete of it is
    /// refused on (`ROLE_HAS_OPEN_TASKS`, **D-89**), and the total of
    /// `GET /api/v1/identity/roles/{id}/open-tasks`. Present and omitted with
    /// `liveHolders`.
    ///
    /// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_tasks: Option<i64>,
}

/// Fills each role's `liveHolders` and `openTasks` from the two counts, keyed
/// by role id ([#508]). A role absent from a map has none, and reads `0`: each
/// count leaves out a role it found nothing for.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
pub fn with_staffing(
    roles: &mut [Role],
    live_holders: &HashMap<Uuid, i64>,
    open_tasks: &HashMap<Uuid, i64>,
) {
    for role in roles {
        role.live_holders = Some(live_holders.get(&role.id).copied().unwrap_or(0));
        role.open_tasks = Some(open_tasks.get(&role.id).copied().unwrap_or(0));
    }
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub status: Option<UserStatus>,
    /// The department the user belongs to (FR-IDM-008, per decision **D-8**).
    ///
    /// `Option<Option<_>>` because the column is nullable and *clearing* it is
    /// a real edit — a person who leaves a department has none, and a plain
    /// `Option` cannot say so: `COALESCE` reads a missing field and an explicit
    /// null identically, so the department could be set and never unset.
    #[serde(default, deserialize_with = "present_or_absent")]
    pub department_id: Option<Option<Uuid>>,
    pub role_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRoleRequest {
    pub role_code: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub permission_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

    repeated_ids("roleIds", "role", &request.role_ids, &mut details);

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

/// Refuses a list of grants that names one id twice.
///
/// **[#469](https://github.com/sujanto-gaws/kelir/issues/469): a repeated id
/// answered 500.** `replace_role_permissions` and `replace_user_roles` insert
/// one row per id, and `uq_role_permissions_role_id_permission_id` and
/// `uq_user_roles_user_id_role_id_department_id` refuse the second row. So the
/// caller's mistake reached the database as a unique violation and came back as
/// `INTERNAL_ERROR`, with the constraint's name in the log and nothing in the
/// response to say which id.
///
/// **Refused rather than deduplicated**, as `check_department` refuses an id
/// that names nothing: the list is what the caller asked to grant, and a request
/// that repeats itself is more likely a caller building it wrongly than one
/// meaning the set. The code is `DUPLICATE_IN_ARRAY`, the one JFSS §10.3 already
/// uses for an array whose items must be unique, and the path is the repeat's
/// own index, so a form can put the message on the entry to remove.
pub fn validate_distinct_ids(field: &str, noun: &str, ids: &[Uuid]) -> Result<(), AppError> {
    let mut details = Vec::new();
    repeated_ids(field, noun, ids, &mut details);

    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

/// One detail for every id that repeats an earlier one, naming both positions.
fn repeated_ids(field: &str, noun: &str, ids: &[Uuid], details: &mut Vec<ValidationDetail>) {
    for (index, id) in ids.iter().enumerate() {
        if let Some(first) = ids[..index].iter().position(|earlier| earlier == id) {
            details.push(ValidationDetail::new(
                format!("{field}.{index}"),
                "uniqueItems",
                "DUPLICATE_IN_ARRAY",
                format!("The {noun} {id} is already listed at {field}.{first}"),
            ));
        }
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

/// The code a role delete is refused with while a published workflow
/// definition names the role (**D-91** (3), [#510]). A client branches on it,
/// not on the message.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
pub const ROLE_NAMED_BY_PUBLISHED_DEFINITION: &str = "ROLE_NAMED_BY_PUBLISHED_DEFINITION";

/// Why a role open tasks still need cannot be deleted (**D-89**, [#487]), or
/// `None` when none does. `open_tasks` is
/// `workflow::service::task::open_tasks_needing_role`'s count.
///
/// [#487]: https://github.com/sujanto-gaws/kelir/issues/487
pub fn open_tasks_refusal(open_tasks: i64) -> Option<String> {
    if open_tasks <= 0 {
        return None;
    }

    let (tasks, them, they) = if open_tasks == 1 {
        ("task needs", "it", "It needs")
    } else {
        ("tasks need", "them", "They need")
    };

    // True of both ways a task needs a role: an unclaimed task offered to it is
    // left offered to nobody, and a task with an edge `allowedBy` it is left
    // with a decision nobody can make (#529). The way through is named: decide
    // the task, or reassign it (`POST /api/v1/workflow/tasks/{id}/reassign`,
    // #512, D-91). A reassign elsewhere releases this role from a task offered
    // to it. A task whose edge names it holds it until decided, and a reassign
    // moves that task to somebody who can decide it.
    Some(format!(
        "{open_tasks} open {tasks} this role to be decided. Deleting the role would leave \
         {them} offered to nobody, or with a decision nobody could make. {they} to be \
         decided or reassigned first"
    ))
}

/// Why a role published workflow revisions name cannot be deleted (**D-91**
/// (3), [#510]), or `None` when none does. `definitions` is
/// `workflow::service::definition::definitions_naming_role`'s list.
///
/// Deleting the role would have every submission routed to one of them, or the
/// next step of an approval running on one, refused as `ASSIGNMENT_UNRESOLVED`.
/// Each is named by key, name and revision, so an administrator knows which to
/// revise. A `Conflict` carries no `details` (`AppError::details` is for
/// validation only), so the list is prose.
///
/// [#510]: https://github.com/sujanto-gaws/kelir/issues/510
pub fn published_definitions_refusal(definitions: &[DefinitionNamingRole]) -> Option<String> {
    if definitions.is_empty() {
        return None;
    }

    let named = definitions
        .iter()
        .map(|definition| {
            let deprecated = match definition.status {
                WorkflowDefinitionStatus::Deprecated => ", deprecated",
                WorkflowDefinitionStatus::Active | WorkflowDefinitionStatus::Draft => "",
            };

            format!(
                "{} (\"{}\", revision {}{deprecated})",
                definition.workflow_key, definition.name, definition.version
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    Some(if definitions.len() == 1 {
        format!(
            "This role is named by 1 published workflow definition: {named}. Deleting the \
             role would leave it unable to raise its tasks. Publish a revision that does \
             not name the role, bind its document types to that revision, and delete this \
             one once its running approvals are finished"
        )
    } else {
        format!(
            "This role is named by {} published workflow definitions: {named}. Deleting \
             the role would leave them unable to raise their tasks. Publish revisions that \
             do not name the role, bind their document types to those revisions, and \
             delete these once their running approvals are finished",
            definitions.len()
        )
    })
}

/// The code a role delete is refused with while open tasks still need the role
/// (**D-89**, [#532]). A client branches on it, not on the message, and offers
/// `GET /api/v1/identity/roles/{id}/open-tasks`, which lists those tasks.
///
/// [#532]: https://github.com/sujanto-gaws/kelir/issues/532
pub const ROLE_HAS_OPEN_TASKS: &str = "ROLE_HAS_OPEN_TASKS";

#[cfg(test)]
mod tests {
    use super::*;

    fn naming(key: &str, version: i32, status: WorkflowDefinitionStatus) -> DefinitionNamingRole {
        DefinitionNamingRole {
            workflow_key: key.to_owned(),
            name: "Standard approval".to_owned(),
            version,
            status,
        }
    }

    #[test]
    fn nothing_open_and_nothing_published_is_not_refused() {
        assert_eq!(open_tasks_refusal(0), None);
        assert_eq!(published_definitions_refusal(&[]), None);
    }

    #[test]
    fn the_open_task_refusal_is_d_89s_sentence() {
        assert_eq!(
            open_tasks_refusal(2).as_deref(),
            Some(
                "2 open tasks need this role to be decided. Deleting the role would leave them \
                 offered to nobody, or with a decision nobody could make. They need to be \
                 decided or reassigned first"
            )
        );
    }

    #[test]
    fn the_definition_refusal_names_one_revision() {
        let definitions = [naming(
            "purchase_requisition",
            1,
            WorkflowDefinitionStatus::Active,
        )];

        assert_eq!(
            published_definitions_refusal(&definitions).as_deref(),
            Some(
                "This role is named by 1 published workflow definition: purchase_requisition \
                 (\"Standard approval\", revision 1). Deleting the role would leave it unable \
                 to raise its tasks. Publish a revision that does not name the role, bind its \
                 document types to that revision, and delete this one once its running \
                 approvals are finished"
            )
        );
    }

    #[test]
    fn the_definition_refusal_names_each_revision_and_marks_a_deprecated_one() {
        let definitions = [
            naming("purchase_requisition", 3, WorkflowDefinitionStatus::Active),
            naming("travel_request", 1, WorkflowDefinitionStatus::Deprecated),
        ];

        assert_eq!(
            published_definitions_refusal(&definitions).as_deref(),
            Some(
                "This role is named by 2 published workflow definitions: purchase_requisition \
                 (\"Standard approval\", revision 3), travel_request (\"Standard approval\", \
                 revision 1, deprecated). Deleting the role would leave them unable to raise \
                 their tasks. Publish revisions that do not name the role, bind their document \
                 types to those revisions, and delete these once their running approvals are \
                 finished"
            )
        );
    }

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

    /// **[#469].** Each repeat is named at its own index, beside the first
    /// occurrence, and a list with no repeat passes: the second subject.
    ///
    /// **Seen red, 2026-09-17**: `repeated_ids` pushing nothing reddens the
    /// test where the refusal is expected.
    ///
    /// [#469]: https://github.com/sujanto-gaws/kelir/issues/469
    #[test]
    fn a_repeated_id_is_refused_at_its_own_index() {
        let read = Uuid::from_u128(1);
        let create = Uuid::from_u128(2);

        let details = details_of(
            validate_distinct_ids("permissionIds", "permission", &[read, create, read, read])
                .expect_err("a repeated id is refused"),
        );
        let paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["permissionIds.2", "permissionIds.3"]);
        assert!(details.iter().all(|d| d.code == "DUPLICATE_IN_ARRAY"));
        assert!(
            details[0].message.contains(&read.to_string())
                && details[0].message.contains("permissionIds.0"),
            "{}",
            details[0].message
        );

        assert!(validate_distinct_ids("permissionIds", "permission", &[read, create]).is_ok());
        assert!(validate_distinct_ids("permissionIds", "permission", &[]).is_ok());
    }

    #[test]
    fn a_user_created_with_a_repeated_role_is_refused_with_the_other_problems() {
        let role = Uuid::from_u128(7);
        let bad = CreateUserRequest {
            display_name: "".to_owned(),
            role_ids: vec![role, role],
            ..request()
        };

        let details = details_of(validate_create_user(&bad).expect_err("invalid"));
        let paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();
        assert!(paths.contains(&"displayName"), "{paths:?}");
        assert!(paths.contains(&"roleIds.1"), "{paths:?}");
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

    /// `ALL` is every variant: this match has no wildcard, so a new status
    /// fails to compile here until it is placed, and the count below then
    /// fails until `ALL` holds it.
    #[test]
    fn all_holds_every_status_once() {
        let place = |status: UserStatus| match status {
            UserStatus::Active => 0,
            UserStatus::Inactive => 1,
            UserStatus::Locked => 2,
            UserStatus::PendingActivation => 3,
        };

        let mut placed: Vec<usize> = UserStatus::ALL.into_iter().map(place).collect();
        placed.sort_unstable();
        assert_eq!(placed, vec![0, 1, 2, 3]);
    }

    /// The live-holder count reads exactly the statuses that sign in (#508):
    /// `ACTIVE`, and none of the three a deactivation leaves.
    #[test]
    fn only_a_status_that_signs_in_is_a_live_holders() {
        assert_eq!(UserStatus::signing_in_db_values(), vec!["ACTIVE"]);

        for status in UserStatus::ALL {
            assert_eq!(
                UserStatus::signing_in_db_values().contains(&status.as_db()),
                status.can_sign_in(),
                "{status:?}"
            );
        }
    }

    fn role(id: Uuid) -> Role {
        Role {
            id,
            role_code: format!("R-{id}"),
            name: "A role".to_owned(),
            description: None,
            is_system: false,
            permissions: Vec::new(),
            live_holders: None,
            open_tasks: None,
        }
    }

    /// Each role reads its own counts, and a role either count left out reads
    /// zero rather than staying absent (#508).
    #[test]
    fn staffing_is_each_roles_own_and_absent_is_zero() {
        let (stranded, staffed, idle) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let mut roles = vec![role(stranded), role(staffed), role(idle)];

        let holders = HashMap::from([(staffed, 3)]);
        let open = HashMap::from([(stranded, 2), (staffed, 1)]);
        with_staffing(&mut roles, &holders, &open);

        let read: Vec<(Option<i64>, Option<i64>)> = roles
            .iter()
            .map(|role| (role.live_holders, role.open_tasks))
            .collect();
        assert_eq!(
            read,
            vec![(Some(0), Some(2)), (Some(3), Some(1)), (Some(0), Some(0))]
        );
    }

    /// Without the counts, the fields are not in the body at all: a caller
    /// without `workflow:task:reassign` cannot read a `0` that means nothing.
    #[test]
    fn a_role_without_staffing_serializes_no_counts() {
        let body = serde_json::to_value(role(Uuid::now_v7())).expect("serialize");
        assert!(body.get("liveHolders").is_none(), "{body}");
        assert!(body.get("openTasks").is_none(), "{body}");

        let mut roles = vec![role(Uuid::now_v7())];
        with_staffing(&mut roles, &HashMap::new(), &HashMap::new());
        let body = serde_json::to_value(&roles[0]).expect("serialize");
        assert_eq!(body["liveHolders"], 0, "{body}");
        assert_eq!(body["openTasks"], 0, "{body}");
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
