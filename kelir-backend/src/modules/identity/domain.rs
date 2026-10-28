use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::modules::workflow::domain::{DefinitionNamingRole, WorkflowDefinitionStatus};
use crate::response::Pagination;
use crate::utils::serde::present_or_absent;
use crate::utils::text::is_blank_text;

/// Whether a row an access token names is there, and whether it is deleted
/// (#650, decision D-105; ADR-0045).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Live,
    /// `deleted_at` is set. Nothing else makes a row deleted: not `status`.
    Deleted,
    /// No such row. For a user, also a row that belongs to another tenant
    /// than the one the token names.
    Missing,
}

impl RowState {
    /// From `deleted_at IS NOT NULL` read by primary key: `None` is no row.
    pub fn from_deleted(deleted: Option<bool>) -> Self {
        match deleted {
            Some(false) => Self::Live,
            Some(true) => Self::Deleted,
            None => Self::Missing,
        }
    }

    /// For the operator's log line. Never for a response.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Deleted => "deleted",
            Self::Missing => "missing",
        }
    }
}

/// The tenant and the user an access token names, as they are now.
///
/// **`deleted_at` and nothing else.** A status, a name, a role or a
/// permission is not read: those stay what the token carried when it was
/// issued, for the token's life (D-104).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSubject {
    pub tenant: RowState,
    pub user: RowState,
}

impl TokenSubject {
    /// Whether a token naming this pair is accepted: both rows there, and
    /// neither deleted.
    pub fn is_live(self) -> bool {
        self.tenant == RowState::Live && self.user == RowState::Live
    }
}

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

/// Why a refresh token was revoked: `refresh_tokens.revoked_reason` (#710).
///
/// The strings are the stored vocabulary, so [`as_str`](Self::as_str) never
/// changes a spelling a row already carries. A refresh that meets a revoked
/// token reads the reason back and asks [`is_replay_evidence`] what it means
/// (SDD §11.1). Each revoke site passes one of these, so a new reason cannot
/// be written until it has a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    /// The refresh that spent the token issued its successor.
    Rotated,
    /// Its holder signed out.
    SignedOut,
    /// The reuse sweep ended it with the rest of its user's family.
    RefreshTokenReuse,
    /// The holder changed their own password, or an administrator set it.
    PasswordChanged,
    /// A reset link was redeemed.
    PasswordReset,
    /// `update_tenant` took the tenant out of `ACTIVE`.
    TenantSuspended,
    /// `delete_tenant`.
    TenantDeleted,
    /// `update_user` set a status that cannot sign in.
    AccountDeactivated,
    /// `delete_user`.
    AccountDeleted,
    /// A refresh found its user could not sign in.
    AccountNotActive,
    /// A refresh found its tenant refuses sessions (#649).
    TenantNotActive,
}

impl RevokeReason {
    /// Every reason. A test matches on each variant, so a twelfth that is not
    /// added here fails to compile there.
    pub const ALL: [Self; 11] = [
        Self::Rotated,
        Self::SignedOut,
        Self::RefreshTokenReuse,
        Self::PasswordChanged,
        Self::PasswordReset,
        Self::TenantSuspended,
        Self::TenantDeleted,
        Self::AccountDeactivated,
        Self::AccountDeleted,
        Self::AccountNotActive,
        Self::TenantNotActive,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rotated => "rotated",
            Self::SignedOut => "signed out",
            Self::RefreshTokenReuse => "refresh token reuse",
            Self::PasswordChanged => "password changed",
            Self::PasswordReset => "password reset",
            Self::TenantSuspended => "tenant suspended",
            Self::TenantDeleted => "tenant deleted",
            Self::AccountDeactivated => "account deactivated",
            Self::AccountDeleted => "account deleted",
            Self::AccountNotActive => "account not active",
            Self::TenantNotActive => "tenant not active",
        }
    }

    /// The reason a stored string spells, or `None` for one this code does
    /// not know: a row written by a later release, or by hand.
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }

    /// Whether a token revoked for this reason, presented again, means a copy
    /// of it exists. Rotation, sign-out and the reuse sweep each revoke a
    /// token its client should no longer hold. Every other reason is an
    /// administrative act on a token its client still holds in good faith.
    ///
    /// No wildcard arm, so a new reason does not compile until it is
    /// classified here.
    fn marks_a_copy(self) -> bool {
        match self {
            Self::Rotated | Self::SignedOut | Self::RefreshTokenReuse => true,
            Self::PasswordChanged
            | Self::PasswordReset
            | Self::TenantSuspended
            | Self::TenantDeleted
            | Self::AccountDeactivated
            | Self::AccountDeleted
            | Self::AccountNotActive
            | Self::TenantNotActive => false,
        }
    }
}

/// Whether a revoked refresh token presented again is evidence of replay,
/// given the `revoked_reason` stored with it (#710, SDD §11.1).
///
/// **Fails safe.** `None` and a string [`RevokeReason::from_db`] does not
/// recognise are evidence: a missed alarm is the worse error, and a false one
/// costs the user a sign-in.
pub fn is_replay_evidence(reason: Option<&str>) -> bool {
    reason
        .and_then(RevokeReason::from_db)
        .is_none_or(RevokeReason::marks_a_copy)
}

/// The user list's query string: paging, a search and a status.
///
/// Unknown parameters are ignored, as [`Pagination`] ignores them everywhere;
/// a `status` outside the vocabulary is a 422 naming `status`, because it
/// deserializes into the enum. Added for the choosers of
/// [#525](https://github.com/sujanto-gaws/kelir/issues/525), which read the
/// server's match rather than filtering one page of 100.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct UserQuery {
    /// 1-based page number; absent or below 1 is 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the username, the email or the display name.
    /// `%`, `_` and `\` in it match themselves; a blank search is no search.
    pub search: Option<String>,
    /// `ACTIVE`, `INACTIVE`, `LOCKED` or `PENDING_ACTIVATION`, matched exactly.
    pub status: Option<UserStatus>,
}

impl UserQuery {
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }
}

/// The role list's query string: paging and a search.
///
/// A role has no status, so there is no `status` filter: an unknown parameter
/// is ignored, as [`Pagination`] ignores them everywhere. Added for the choosers of
/// [#525](https://github.com/sujanto-gaws/kelir/issues/525), which read the
/// server's match rather than filtering one page of 100.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct RoleQuery {
    /// 1-based page number; absent or below 1 is 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the role code or the name. `%`, `_` and `\` in
    /// it match themselves; a blank search is no search.
    pub search: Option<String>,
}

impl RoleQuery {
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
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
    /// Required; at most 64 characters once trimmed; letters, digits, dot,
    /// dash and underscore. Stored trimmed.
    pub username: String,
    /// At most 320 characters once trimmed, and shaped like an address.
    /// Stored trimmed.
    pub email: String,
    pub password: String,
    /// Required; at most 200 characters once trimmed. Stored trimmed.
    pub display_name: String,
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub role_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateUserRequest {
    /// Absent leaves the address as it is; present, it is held to the create
    /// rules: at most 320 characters once trimmed, and shaped like an address.
    pub email: Option<String>,
    /// Absent leaves the name as it is; present, it is held to the create
    /// rules: not blank, at most 200 characters once trimmed.
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
    /// Required; at most 64 characters once trimmed. Stored trimmed, and
    /// unique among the tenant's live roles.
    pub role_code: String,
    /// Required; at most 200 characters once trimmed.
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub permission_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRoleRequest {
    /// Absent leaves the name as it is; present, it is held to the create
    /// rules: not blank, at most 200 characters once trimmed.
    pub name: Option<String>,
    pub description: Option<String>,
    pub permission_ids: Option<Vec<Uuid>>,
}

/// Minimum password length. Deliberately a length floor rather than a
/// composition rule: length is what resists guessing, and character-class rules
/// mostly produce predictable substitutions.
pub const MIN_PASSWORD_LENGTH: usize = 12;

/// `users.username VARCHAR(64)` (`0004_string_lengths.sql`).
pub const MAX_USERNAME_LENGTH: usize = 64;

/// `users.email VARCHAR(320)` (`0004_string_lengths.sql`), RFC 5321's maximum.
pub const MAX_USER_EMAIL_LENGTH: usize = 320;

/// `users.display_name VARCHAR(200)` (`0004_string_lengths.sql`).
pub const MAX_USER_DISPLAY_NAME_LENGTH: usize = 200;

/// Validates a create-user payload, collecting every problem rather than
/// stopping at the first — a form should be able to mark all its bad fields at
/// once (JSON Form Schema S10.3).
///
/// **[#610]: a long username answered 500.** Nothing bounded `username`,
/// `email` or `displayName`, so a value over its column reached the insert and
/// came back as `INTERNAL_ERROR`. Each is now counted as [#575] counts a role's
/// code and name: in characters of the trimmed value, the value
/// `create_user` stores. A username is checked for blank, then length, then
/// its characters, and reports the first that fails.
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
pub fn validate_create_user(request: &CreateUserRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    validate_username(&request.username, &mut details);
    validate_email(&request.email, &mut details);
    validate_password(&request.password, &mut details);
    bounded_text(
        &request.display_name,
        "displayName",
        "Display name",
        MAX_USER_DISPLAY_NAME_LENGTH,
        &mut details,
    );
    repeated_ids("roleIds", "role", &request.role_ids, &mut details);

    finish(details)
}

/// Validates an update-user payload. An absent `email` or `displayName` is
/// left as it is; a present one is held to the create rules, so an edit cannot
/// store what a create could not ([#610]). Before this, a blank display name
/// was stored and an over-long one answered 500. A repeated role id is
/// reported beside them ([#469]).
///
/// [#469]: https://github.com/sujanto-gaws/kelir/issues/469
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
pub fn validate_update_user(request: &UpdateUserRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(email) = &request.email {
        validate_email(email, &mut details);
    }

    if let Some(display_name) = &request.display_name {
        bounded_text(
            display_name,
            "displayName",
            "Display name",
            MAX_USER_DISPLAY_NAME_LENGTH,
            &mut details,
        );
    }

    if let Some(role_ids) = &request.role_ids {
        repeated_ids("roleIds", "role", role_ids, &mut details);
    }

    finish(details)
}

/// One detail for every id that repeats an earlier one, naming both positions:
/// a list of grants that names one id twice is refused.
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

/// `roles.role_code VARCHAR(64)` (`0004_string_lengths.sql`).
pub const MAX_ROLE_CODE_LENGTH: usize = 64;

/// `roles.name VARCHAR(200)` (`0004_string_lengths.sql`).
pub const MAX_ROLE_NAME_LENGTH: usize = 200;

/// Validates a create-role payload, collecting every problem at once.
///
/// **[#575]: nothing checked a role's code or name.** An empty or blank code
/// was stored — the column is `NOT NULL`, and `''` is not null — and one over
/// 64 characters reached `VARCHAR(64)` and came back as a 500. The server is
/// the validator of a hand-built form (coding standard §3.4), so each is a 422
/// `VALIDATION_ERROR` with a detail at the field's own path.
///
/// Lengths are counted in characters of the **trimmed** value, because the
/// trimmed value is what `create_role` stores and `VARCHAR(n)` counts
/// characters, not bytes. No format is imposed on the code: none is specified,
/// and the seeded and documented codes already differ in shape
/// (`ROLE-ADMIN`, `FINANCE_APPROVER`).
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
pub fn validate_create_role(request: &CreateRoleRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded_text(
        &request.role_code,
        "roleCode",
        "Role code",
        MAX_ROLE_CODE_LENGTH,
        &mut details,
    );
    bounded_text(
        &request.name,
        "name",
        "Name",
        MAX_ROLE_NAME_LENGTH,
        &mut details,
    );
    repeated_ids(
        "permissionIds",
        "permission",
        &request.permission_ids,
        &mut details,
    );

    finish(details)
}

/// Validates an update-role payload. An absent `name` is left as it is; a
/// present one is held to the create rules, so an edit cannot blank what a
/// create could not ([#575]).
///
/// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
pub fn validate_update_role(request: &UpdateRoleRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(name) = &request.name {
        bounded_text(name, "name", "Name", MAX_ROLE_NAME_LENGTH, &mut details);
    }

    if let Some(permission_ids) = &request.permission_ids {
        repeated_ids("permissionIds", "permission", permission_ids, &mut details);
    }

    finish(details)
}

/// Required, and at most `max` characters once trimmed. One detail at most:
/// blank (`REQUIRED`) is reported before too long (`TOO_LONG`). Returns whether
/// the value passed, so a caller with a further rule checks it only then.
///
/// Blank is [`is_blank_text`]: whitespace, or a code point that shows nothing,
/// such as U+200B ([#616]). The length is counted on `str::trim`'s value,
/// which is what the services store: an invisible character at the edge is
/// kept, so it is counted.
///
/// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
fn bounded_text(
    value: &str,
    path: &str,
    label: &str,
    max: usize,
    details: &mut Vec<ValidationDetail>,
) -> bool {
    if is_blank_text(value) {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            format!("{label} is required"),
        ));
        false
    } else if value.trim().chars().count() > max {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("{label} must be at most {max} characters"),
        ));
        false
    } else {
        true
    }
}

fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
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

/// Blank, then too long, then a character outside the set: one detail, the
/// first that fails ([#610]). A 65-character username with a space in it is
/// `TOO_LONG`, not `INVALID_FORMAT`.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
fn validate_username(username: &str, details: &mut Vec<ValidationDetail>) {
    if !bounded_text(
        username,
        "username",
        "Username",
        MAX_USERNAME_LENGTH,
        details,
    ) {
        return;
    }

    if !username
        .trim()
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

/// Too long, then the shape: one detail, the first that fails ([#610]). A
/// blank address is `INVALID_FORMAT`, as it was: it has no `@`.
///
/// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
fn validate_email(email: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = email.trim();

    if trimmed.chars().count() > MAX_USER_EMAIL_LENGTH {
        details.push(ValidationDetail::new(
            "email",
            "maxLength",
            "TOO_LONG",
            format!("Email must be at most {MAX_USER_EMAIL_LENGTH} characters"),
        ));
        return;
    }

    // Deliberately shallow: the only reliable proof an address exists is
    // delivering to it, and strict patterns reject valid addresses.
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
             not name the role, bind its document types to that revision, and deprecate \
             it if it is still published. A deprecated revision stops holding the role \
             once its running approvals are finished"
        )
    } else {
        format!(
            "This role is named by {} published workflow definitions: {named}. Deleting \
             the role would leave them unable to raise their tasks. Publish revisions that \
             do not name the role, bind their document types to those revisions, and \
             deprecate any still published. A deprecated revision stops holding the role \
             once its running approvals are finished",
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
                 document types to that revision, and deprecate it if it is still published. \
                 A deprecated revision stops holding the role once its running approvals are \
                 finished"
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
                 types to those revisions, and deprecate any still published. A deprecated \
                 revision stops holding the role once its running approvals are finished"
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
    /// Held through `validate_update_user`, which has carried the update
    /// route's check since #610 (it was `validate_distinct_ids`, called alone).
    #[test]
    fn a_repeated_id_is_refused_at_its_own_index() {
        let read = Uuid::from_u128(1);
        let create = Uuid::from_u128(2);
        let update = |role_ids: Vec<Uuid>| UpdateUserRequest {
            email: None,
            display_name: None,
            status: None,
            department_id: None,
            role_ids: Some(role_ids),
        };

        let details = details_of(
            validate_update_user(&update(vec![read, create, read, read]))
                .expect_err("a repeated id is refused"),
        );
        let paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["roleIds.2", "roleIds.3"]);
        assert!(details.iter().all(|d| d.code == "DUPLICATE_IN_ARRAY"));
        assert!(
            details[0].message.contains(&read.to_string())
                && details[0].message.contains("roleIds.0"),
            "{}",
            details[0].message
        );

        assert!(validate_update_user(&update(vec![read, create])).is_ok());
        assert!(validate_update_user(&update(Vec::new())).is_ok());
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

    /// The strings are what stored rows carry, so renaming one strands every
    /// token revoked under the old spelling in the fail-safe arm (#710). The
    /// classification is the table in SDD §11.1. A twelfth reason fails to
    /// compile in the `match` until it is placed, and the count of
    /// administrative reasons then fails until it is classified on purpose.
    #[test]
    fn revoke_reason_classification_and_strings() {
        let expected = |reason: RevokeReason| match reason {
            RevokeReason::Rotated => ("rotated", true),
            RevokeReason::SignedOut => ("signed out", true),
            RevokeReason::RefreshTokenReuse => ("refresh token reuse", true),
            RevokeReason::PasswordChanged => ("password changed", false),
            RevokeReason::PasswordReset => ("password reset", false),
            RevokeReason::TenantSuspended => ("tenant suspended", false),
            RevokeReason::TenantDeleted => ("tenant deleted", false),
            RevokeReason::AccountDeactivated => ("account deactivated", false),
            RevokeReason::AccountDeleted => ("account deleted", false),
            RevokeReason::AccountNotActive => ("account not active", false),
            RevokeReason::TenantNotActive => ("tenant not active", false),
        };

        let mut spellings = std::collections::HashSet::new();
        let mut administrative = 0;
        for reason in RevokeReason::ALL {
            let (text, replay) = expected(reason);
            assert_eq!(reason.as_str(), text, "{reason:?}");
            assert_eq!(RevokeReason::from_db(text), Some(reason), "{text}");
            assert_eq!(is_replay_evidence(Some(text)), replay, "{text}");
            assert!(spellings.insert(text), "{text} is spelled twice");
            if !replay {
                administrative += 1;
            }
        }
        assert_eq!(spellings.len(), 11);
        assert_eq!(administrative, 8);

        // Fails safe: no reason, or one this code does not know, is evidence.
        assert!(is_replay_evidence(None));
        assert!(is_replay_evidence(Some("anything else")));
        assert_eq!(RevokeReason::from_db("anything else"), None);
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

    fn create_role_request(code: &str, name: &str) -> CreateRoleRequest {
        CreateRoleRequest {
            role_code: code.to_owned(),
            name: name.to_owned(),
            description: None,
            permission_ids: Vec::new(),
        }
    }

    fn placed(error: AppError) -> Vec<(String, String)> {
        details_of(error)
            .into_iter()
            .map(|detail| (detail.path, detail.code))
            .collect()
    }

    /// **[#575].** Blank and over-long are refused at the field's own path,
    /// both fields at once; the column's own length, counted in characters
    /// of the trimmed value, passes.
    ///
    /// [#575]: https://github.com/sujanto-gaws/kelir/issues/575
    #[test]
    fn a_role_code_and_name_are_required_and_bounded() {
        assert_eq!(
            placed(
                validate_create_role(&create_role_request("  ", "")).expect_err("blank is refused")
            ),
            [
                ("roleCode".to_owned(), "REQUIRED".to_owned()),
                ("name".to_owned(), "REQUIRED".to_owned()),
            ]
        );

        let long_code = "C".repeat(MAX_ROLE_CODE_LENGTH + 1);
        let long_name = "N".repeat(MAX_ROLE_NAME_LENGTH + 1);
        assert_eq!(
            placed(
                validate_create_role(&create_role_request(&long_code, &long_name))
                    .expect_err("too long")
            ),
            [
                ("roleCode".to_owned(), "TOO_LONG".to_owned()),
                ("name".to_owned(), "TOO_LONG".to_owned()),
            ]
        );

        // At the bound exactly, padded with whitespace the store trims, and in
        // characters wider than a byte: `VARCHAR(n)` counts characters.
        let at_code = format!("  {}  ", "é".repeat(MAX_ROLE_CODE_LENGTH));
        let at_name = format!(" {} ", "é".repeat(MAX_ROLE_NAME_LENGTH));
        assert!(validate_create_role(&create_role_request(&at_code, &at_name)).is_ok());
    }

    #[test]
    fn a_role_update_checks_a_present_name_only() {
        let update = |name: Option<&str>| UpdateRoleRequest {
            name: name.map(str::to_owned),
            description: None,
            permission_ids: None,
        };

        assert!(validate_update_role(&update(None)).is_ok());
        assert!(validate_update_role(&update(Some("Approver"))).is_ok());
        assert_eq!(
            placed(validate_update_role(&update(Some(" "))).expect_err("blank")),
            [("name".to_owned(), "REQUIRED".to_owned())]
        );
        assert_eq!(
            placed(
                validate_update_role(&update(Some(&"N".repeat(MAX_ROLE_NAME_LENGTH + 1))))
                    .expect_err("too long")
            ),
            [("name".to_owned(), "TOO_LONG".to_owned())]
        );
    }

    /// The field problems and a repeated permission id are reported together,
    /// so a form marks all of them from one response.
    #[test]
    fn a_role_create_reports_a_repeated_permission_beside_the_fields() {
        let read = Uuid::from_u128(1);
        let mut request = create_role_request("", "Approver");
        request.permission_ids = vec![read, read];

        assert_eq!(
            placed(validate_create_role(&request).expect_err("refused")),
            [
                ("roleCode".to_owned(), "REQUIRED".to_owned()),
                (
                    "permissionIds.1".to_owned(),
                    "DUPLICATE_IN_ARRAY".to_owned()
                ),
            ]
        );
    }

    /// The five code points #616 names. None is `char::is_whitespace`, so
    /// `str::trim` kept each.
    const ZERO_WIDTH: [char; 5] = ['\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}'];

    fn create_user_request(username: &str, email: &str, display_name: &str) -> CreateUserRequest {
        CreateUserRequest {
            username: username.to_owned(),
            email: email.to_owned(),
            display_name: display_name.to_owned(),
            ..request()
        }
    }

    fn update_user_request(email: Option<&str>, display_name: Option<&str>) -> UpdateUserRequest {
        UpdateUserRequest {
            email: email.map(str::to_owned),
            display_name: display_name.map(str::to_owned),
            status: None,
            department_id: None,
            role_ids: None,
        }
    }

    /// An address of exactly `length` characters, `local` repeated before
    /// `@kelir.test`.
    fn email_of(local: char, length: usize) -> String {
        let domain = "@kelir.test";
        format!(
            "{}{domain}",
            local.to_string().repeat(length - domain.chars().count())
        )
    }

    /// **[#610]: a username over `users.username VARCHAR(64)` is `TOO_LONG`.**
    /// 64 characters pass, padded or not, because the check counts the
    /// trimmed value the store keeps; 65 are refused, padded or not, with a
    /// message naming the bound.
    ///
    /// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
    #[test]
    fn a_username_is_bounded_in_characters_of_the_trimmed_value() {
        let at = "u".repeat(MAX_USERNAME_LENGTH);
        let over = "u".repeat(MAX_USERNAME_LENGTH + 1);

        for username in [at.clone(), format!("  {at}  "), format!("\t{at}\u{00A0}")] {
            assert!(
                validate_create_user(&create_user_request(&username, "u@kelir.test", "U")).is_ok(),
                "{username:?}"
            );
        }

        for username in [over.clone(), format!("  {over}  ")] {
            let details = details_of(
                validate_create_user(&create_user_request(&username, "u@kelir.test", "U"))
                    .expect_err("too long"),
            );
            assert_eq!(details.len(), 1, "{details:?}");
            assert_eq!(details[0].path, "username");
            assert_eq!(details[0].rule, "maxLength");
            assert_eq!(details[0].code, "TOO_LONG");
            assert!(details[0].message.contains("64"), "{}", details[0].message);
        }
    }

    /// **[#610]: one detail per path, blank then long then characters.** A
    /// 65-character username with a space is too long, not badly formed; a
    /// username of zero-width characters is blank, not badly formed ([#616]),
    /// even 65 of them; a zero-width character at the edge of 64 is kept by
    /// `str::trim`, so it is counted and makes 65.
    ///
    /// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
    /// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
    #[test]
    fn a_username_is_checked_for_blank_then_length_then_characters() {
        let placed_username = |username: &str| {
            placed(
                validate_create_user(&create_user_request(username, "u@kelir.test", "U"))
                    .expect_err("refused"),
            )
        };
        let at_username = |code: &str| [("username".to_owned(), code.to_owned())];

        assert_eq!(placed_username(""), at_username("REQUIRED"));
        for c in ZERO_WIDTH {
            assert_eq!(placed_username(&c.to_string()), at_username("REQUIRED"));
            assert_eq!(
                placed_username(&format!(" {c}{c} ")),
                at_username("REQUIRED")
            );
        }
        assert_eq!(
            placed_username(&"\u{200B}".repeat(MAX_USERNAME_LENGTH + 1)),
            at_username("REQUIRED")
        );

        let spaced = format!("john doe{}", "x".repeat(MAX_USERNAME_LENGTH - 7));
        assert_eq!(spaced.chars().count(), MAX_USERNAME_LENGTH + 1);
        assert_eq!(placed_username(&spaced), at_username("TOO_LONG"));
        assert_eq!(placed_username("john doe"), at_username("INVALID_FORMAT"));

        let edged = format!("\u{200B}{}", "u".repeat(MAX_USERNAME_LENGTH));
        assert_eq!(placed_username(&edged), at_username("TOO_LONG"));
    }

    /// **[#610]: an email over 320 and a display name over 200 characters are
    /// `TOO_LONG`.** Counted in characters, not bytes, of the trimmed value:
    /// `é` is two bytes and each bound is met exactly. The email's length is
    /// checked before its shape, one detail; a blank email stays
    /// `INVALID_FORMAT`, as before, and a display name of zero-width
    /// characters is blank ([#616]).
    ///
    /// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
    /// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
    #[test]
    fn an_email_and_a_display_name_are_bounded_in_characters_of_the_trimmed_value() {
        let at_email = email_of('é', MAX_USER_EMAIL_LENGTH);
        let at_name = "é".repeat(MAX_USER_DISPLAY_NAME_LENGTH);
        assert!(at_email.len() > MAX_USER_EMAIL_LENGTH);
        assert!(validate_create_user(&create_user_request(
            "u",
            &format!(" {at_email} "),
            &format!("\u{3000}{at_name}\u{3000}")
        ))
        .is_ok());

        let over_email = email_of('é', MAX_USER_EMAIL_LENGTH + 1);
        let over_name = "é".repeat(MAX_USER_DISPLAY_NAME_LENGTH + 1);
        let details = details_of(
            validate_create_user(&create_user_request("u", &over_email, &over_name))
                .expect_err("too long"),
        );
        assert_eq!(
            details
                .iter()
                .map(|d| (d.path.as_str(), d.rule.as_str(), d.code.as_str()))
                .collect::<Vec<_>>(),
            [
                ("email", "maxLength", "TOO_LONG"),
                ("displayName", "maxLength", "TOO_LONG"),
            ]
        );
        assert!(details[0].message.contains("320"), "{}", details[0].message);
        assert!(details[1].message.contains("200"), "{}", details[1].message);

        // Too long and badly shaped: the length is the one reported.
        let spaced = format!("a b{}", email_of('e', MAX_USER_EMAIL_LENGTH - 2));
        assert_eq!(spaced.chars().count(), MAX_USER_EMAIL_LENGTH + 1);
        assert_eq!(
            placed(
                validate_create_user(&create_user_request("u", &spaced, "U")).expect_err("long")
            ),
            [("email".to_owned(), "TOO_LONG".to_owned())]
        );

        for blank in ["", "  ", "\u{200B}"] {
            assert_eq!(
                placed(
                    validate_create_user(&create_user_request("u", blank, "U")).expect_err("blank")
                ),
                [("email".to_owned(), "INVALID_FORMAT".to_owned())],
                "{blank:?}"
            );
        }

        for c in ZERO_WIDTH {
            assert_eq!(
                placed(
                    validate_create_user(&create_user_request("u", "u@kelir.test", &c.to_string()))
                        .expect_err("blank")
                ),
                [("displayName".to_owned(), "REQUIRED".to_owned())],
                "U+{:04X}",
                c as u32
            );
        }
        assert_eq!(
            placed(
                validate_create_user(&create_user_request(
                    "u",
                    "u@kelir.test",
                    &"\u{FEFF}".repeat(MAX_USER_DISPLAY_NAME_LENGTH + 1)
                ))
                .expect_err("blank")
            ),
            [("displayName".to_owned(), "REQUIRED".to_owned())]
        );
    }

    /// The three sized fields over their bounds are reported together, in
    /// field order, not the first alone.
    #[test]
    fn every_sized_user_field_over_its_bound_is_reported_at_once() {
        let request = create_user_request(
            &"u".repeat(MAX_USERNAME_LENGTH + 1),
            &email_of('e', MAX_USER_EMAIL_LENGTH + 1),
            &"d".repeat(MAX_USER_DISPLAY_NAME_LENGTH + 1),
        );

        assert_eq!(
            placed(validate_create_user(&request).expect_err("too long")),
            [
                ("username".to_owned(), "TOO_LONG".to_owned()),
                ("email".to_owned(), "TOO_LONG".to_owned()),
                ("displayName".to_owned(), "TOO_LONG".to_owned()),
            ]
        );
    }

    /// **[#610], the update route.** An absent field is left alone; a present
    /// one is held to the create rules. A blank display name, which was
    /// stored, is `REQUIRED`; a repeated role id is reported beside it.
    ///
    /// [#610]: https://github.com/sujanto-gaws/kelir/issues/610
    #[test]
    fn a_user_update_checks_a_present_email_and_display_name_only() {
        assert!(validate_update_user(&update_user_request(None, None)).is_ok());
        assert!(validate_update_user(&update_user_request(
            Some(&format!(" {} ", email_of('é', MAX_USER_EMAIL_LENGTH))),
            Some(&format!(" {} ", "é".repeat(MAX_USER_DISPLAY_NAME_LENGTH)))
        ))
        .is_ok());

        let refused = |email: Option<&str>, display_name: Option<&str>| {
            placed(
                validate_update_user(&update_user_request(email, display_name))
                    .expect_err("refused"),
            )
        };
        let one = |path: &str, code: &str| [(path.to_owned(), code.to_owned())];

        assert_eq!(
            refused(Some(&email_of('e', MAX_USER_EMAIL_LENGTH + 1)), None),
            one("email", "TOO_LONG")
        );
        assert_eq!(refused(Some(""), None), one("email", "INVALID_FORMAT"));
        assert_eq!(
            refused(Some("not-an-email"), None),
            one("email", "INVALID_FORMAT")
        );
        assert_eq!(refused(None, Some("")), one("displayName", "REQUIRED"));
        assert_eq!(refused(None, Some(" \t ")), one("displayName", "REQUIRED"));
        for c in ZERO_WIDTH {
            assert_eq!(
                refused(None, Some(&c.to_string())),
                one("displayName", "REQUIRED")
            );
        }
        assert_eq!(
            refused(None, Some(&"d".repeat(MAX_USER_DISPLAY_NAME_LENGTH + 1))),
            one("displayName", "TOO_LONG")
        );

        let role = Uuid::from_u128(9);
        let mut request = update_user_request(Some("x"), Some(" "));
        request.role_ids = Some(vec![role, role]);
        assert_eq!(
            placed(validate_update_user(&request).expect_err("refused")),
            [
                ("email".to_owned(), "INVALID_FORMAT".to_owned()),
                ("displayName".to_owned(), "REQUIRED".to_owned()),
                ("roleIds.1".to_owned(), "DUPLICATE_IN_ARRAY".to_owned()),
            ]
        );
    }

    /// **[#616]: a role code or name of zero-width characters alone is
    /// blank**, on create and on update, and wins over its length.
    ///
    /// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
    #[test]
    fn a_role_code_or_name_of_zero_width_characters_is_blank() {
        for c in ZERO_WIDTH {
            let blank = format!("{c} {c}");
            assert_eq!(
                placed(
                    validate_create_role(&create_role_request(&blank, &blank)).expect_err("blank")
                ),
                [
                    ("roleCode".to_owned(), "REQUIRED".to_owned()),
                    ("name".to_owned(), "REQUIRED".to_owned()),
                ],
                "U+{:04X}",
                c as u32
            );
            assert_eq!(
                placed(
                    validate_update_role(&UpdateRoleRequest {
                        name: Some(c.to_string()),
                        description: None,
                        permission_ids: None,
                    })
                    .expect_err("blank")
                ),
                [("name".to_owned(), "REQUIRED".to_owned())],
                "U+{:04X}",
                c as u32
            );
        }

        let long_blank = "\u{200B}".repeat(MAX_ROLE_CODE_LENGTH + 1);
        assert_eq!(
            placed(
                validate_create_role(&create_role_request(&long_blank, "Name")).expect_err("blank")
            ),
            [("roleCode".to_owned(), "REQUIRED".to_owned())]
        );
    }

    /// **What is stored is unchanged ([#616]).** A visible code with a
    /// zero-width character at its edge is not blank and is accepted, as it
    /// came. `str::trim` keeps that character, so the length counts it: one at
    /// the edge of a 64-character code makes 65.
    ///
    /// [#616]: https://github.com/sujanto-gaws/kelir/issues/616
    #[test]
    fn a_zero_width_character_at_the_edge_is_kept_and_counted() {
        assert!(
            validate_create_role(&create_role_request("\u{200B}ADMIN", "Admin\u{FEFF}")).is_ok()
        );

        let edged = format!("{}\u{200B}", "C".repeat(MAX_ROLE_CODE_LENGTH));
        assert_eq!(edged.trim().chars().count(), MAX_ROLE_CODE_LENGTH + 1);
        assert_eq!(
            placed(validate_create_role(&create_role_request(&edged, "Name")).expect_err("long")),
            [("roleCode".to_owned(), "TOO_LONG".to_owned())]
        );
    }
}
