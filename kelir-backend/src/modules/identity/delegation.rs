//! Delegation windows: one person's approvals, reaching another for a while
//! (FR-IDM-006; [#184]).
//!
//! # What a window is, and what it is not
//!
//! A window says *from this moment until that one, work that would reach me
//! reaches them instead*. It is not a grant of permission, not a role, and not
//! a copy of anything: the delegate acts with their own account, holding their
//! own permissions, on a task the engine addressed to them.
//! [`crate::modules::workflow::service::assignment`] is where it is applied —
//! after a rule has resolved and before the task's columns are written, which is
//! [JWSS](../../../../docs/schema/JSON%20Workflow%20Schema.md) §5.1's own
//! sentence — and that file carries the routing reasoning in full.
//!
//! # The delegator is always the caller
//!
//! **Nobody hands over another person's authority.** `identity:delegation:create`
//! answers *may this account delegate its work at all*; it is deliberately not a
//! permission to create a window out of somebody else's name, because that is
//! the one shape of this feature that would be an escalation — a holder of the
//! permission could point the finance director's approvals at themselves and
//! then take them, and every row would look exactly like a legitimate window.
//! [#184] AC5 asks that delegation does not escalate permission; this is the
//! half of it that has to be true at the *creation* end rather than at the
//! decision end.
//!
//! Reading and ending are administrative and are not restricted that way: a
//! window whose owner has gone on leave without ending it is exactly the row
//! somebody else has to be able to see and stop.
//!
//! # `ROLE` scope is refused, and the reason is in the assignment resolver
//!
//! §3.8's `scope` has three values. Two of them are honoured here — `ALL`, and
//! `DOCUMENT_TYPE` narrowing to one type of document. `ROLE` is refused at the
//! API with a message that says why: a window redirects a task that resolves to
//! **a person**, and a task offered to a role is not one person's to hand over —
//! it has no assignee until somebody claims it, and every other holder of the
//! role is still being offered it. A `ROLE`-scoped window could therefore never
//! match anything the resolver looks at, and accepting one would store a row
//! that silently routes nothing.
//!
//! The value stays in the column's `CHECK`. That is the standing `STARTED` has
//! on `workflow_instances`: it was specified, and the product does not produce
//! it.
//!
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// How wide a window is (§3.8's `scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DelegationScope {
    /// Everything that would reach the delegator personally.
    All,
    /// Only work on documents of one type.
    DocumentType,
}

impl DelegationScope {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::DocumentType => "DOCUMENT_TYPE",
        }
    }

    /// An unrecognised stored value reads as [`Self::All`].
    ///
    /// **A fallback for a row this product cannot create.** `from_db` has to be
    /// total, and the only other value the column's `CHECK` permits is `ROLE`,
    /// which the API refuses — so reaching this arm means somebody wrote the row
    /// by hand.
    ///
    /// It renders such a window as covering everything, which is **wrong about
    /// what it covers**: the resolver matches no `ROLE` row at all, so it covers
    /// nothing. It is the fallback anyway, because the alternative is refusing
    /// to render the row — and that would hide the one window a person cannot
    /// find any other way from the list they would end it from. Being wrong
    /// about the scope of a window that is visible beats being silent about one
    /// that is not.
    pub fn from_db(value: &str) -> Self {
        match value {
            "DOCUMENT_TYPE" => Self::DocumentType,
            _ => Self::All,
        }
    }
}

/// A window as the API returns it.
///
/// Both parties carry their display name as well as their id. A list of
/// delegations that named people by UUID would be a screen nobody can read, and
/// the join is one per row against a table the list already has to be inside the
/// tenant of.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Delegation {
    pub id: Uuid,
    pub delegator_user_id: Uuid,
    pub delegator_display_name: String,
    pub delegate_user_id: Uuid,
    pub delegate_display_name: String,
    pub scope: DelegationScope,
    pub document_type_id: Option<Uuid>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: Option<String>,
    /// Whether the window is still standing.
    ///
    /// **Not the same as whether it is routing.** A window ends by being
    /// switched off *or* by its `endsAt` passing, and a screen that showed only
    /// this flag would report a finished window as live.
    pub is_active: bool,
    /// Whether this window would redirect work **right now**.
    ///
    /// **Computed in the statement that reads the row**, against the same
    /// `now()` — and there is deliberately no Rust copy of the predicate here to
    /// go with it. *Is this window open* is answered once, in SQL, by
    /// [`delegation_repository::active_delegate_of`][a]; a second definition
    /// beside it would be two answers to the question this whole feature turns
    /// on, and the day they disagreed the list would be describing routing that
    /// was not happening.
    ///
    /// It is a field rather than something the client derives, for the reason
    /// `workflow::domain::Assignment` is one screen over: two clients deriving
    /// it would derive it differently.
    ///
    /// [a]: super::delegation_repository::active_delegate_of
    pub is_routing: bool,
    pub created_at: DateTime<Utc>,
}

/// What opening a window asks for.
///
/// **There is no `delegatorUserId`.** The delegator is the caller — see the
/// module documentation, which is where the escalation this omission prevents is
/// written down.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDelegationRequest {
    pub delegate_user_id: Uuid,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    /// `ALL` when absent.
    #[serde(default)]
    pub scope: Option<String>,
    /// Required by, and only meaningful to, a `DOCUMENT_TYPE` window.
    #[serde(default)]
    pub document_type_id: Option<Uuid>,
    /// Why the window exists — "annual leave", and the like. Free text, bounded
    /// by [`MAX_REASON_LENGTH`].
    #[serde(default)]
    pub reason: Option<String>,
}

/// Longest reason this API stores against a window.
///
/// `delegations.reason` is `TEXT`, so nothing in the schema bounds it and the
/// bound has to be here — `workflow::domain::task::MAX_COMMENT_LENGTH`'s
/// reasoning, at a length that matches what this field is for. A window's reason
/// is a sentence, not a decision's justification.
pub const MAX_REASON_LENGTH: usize = 500;

/// What the request means, once it has been checked against itself.
///
/// The parsed form the service writes from, so the validation below is the only
/// place a string becomes a scope and the only place the two dates are compared.
#[derive(Debug, Clone)]
pub struct ValidatedDelegation {
    pub delegate_user_id: Uuid,
    pub scope: DelegationScope,
    pub document_type_id: Option<Uuid>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Everything about the request that can be decided without the database.
///
/// The three refusals here are each also a database constraint or a
/// database-shaped absence — `ck_delegations_window`, `ck_delegations_not_self`,
/// and a `document_type_id` the foreign key would reject. They are checked here
/// so that the caller is told which field is wrong instead of being handed a
/// constraint name, which is `master_data::domain`'s reason for the same
/// duplication; the constraints stay because a check in a service is a check the
/// next writer of a row can step around.
pub fn validate_create(
    request: &CreateDelegationRequest,
    delegator: Uuid,
    now: DateTime<Utc>,
) -> Result<ValidatedDelegation, AppError> {
    let mut details = Vec::new();

    if request.delegate_user_id == delegator {
        details.push(ValidationDetail::new(
            "delegateUserId",
            "notSelf",
            "DELEGATE_IS_DELEGATOR",
            "a delegation hands work to somebody else; delegating to yourself \
             would change nothing and would read, to anybody looking at the \
             list, as cover that is not there",
        ));
    }

    if request.ends_at <= request.starts_at {
        details.push(ValidationDetail::new(
            "endsAt",
            "window",
            "WINDOW_INVERTED",
            "a delegation window ends after it starts",
        ));
    } else if request.ends_at <= now {
        // Distinct from the inversion above, and worth its own message: the
        // dates are in the right order and the window is simply over. Almost
        // always a timezone or a year typed wrong, and stored it would be a
        // handover somebody believes is in place and which routes nothing.
        details.push(ValidationDetail::new(
            "endsAt",
            "window",
            "WINDOW_ALREADY_OVER",
            "this window has already ended, so it would redirect nothing; \
             check the date, and end an existing window rather than opening a \
             closed one",
        ));
    }

    let scope = match request.scope.as_deref().map(str::trim) {
        None | Some("") | Some("ALL") => DelegationScope::All,
        Some("DOCUMENT_TYPE") => DelegationScope::DocumentType,
        Some("ROLE") => {
            // The one refusal here that is a design decision rather than a typo
            // caught early. The module documentation carries it in full.
            details.push(ValidationDetail::new(
                "scope",
                "enum",
                "SCOPE_UNSUPPORTED",
                "a ROLE-scoped window is not something Kelir can honour: a \
                 delegation redirects a task that resolves to a person, and a \
                 task offered to a role has no assignee to redirect — every \
                 other holder of the role is still being offered it. Must be \
                 one of: ALL, DOCUMENT_TYPE",
            ));

            DelegationScope::All
        }
        Some(other) => {
            details.push(ValidationDetail::new(
                "scope",
                "enum",
                "UNKNOWN_VALUE",
                format!("`{other}` is not a delegation scope. Must be one of: ALL, DOCUMENT_TYPE"),
            ));

            DelegationScope::All
        }
    };

    let document_type_id = match (scope, request.document_type_id) {
        (DelegationScope::DocumentType, None) => {
            details.push(ValidationDetail::new(
                "documentTypeId",
                "required",
                "REQUIRED",
                "a DOCUMENT_TYPE window names the type of document it covers",
            ));

            None
        }
        (DelegationScope::DocumentType, Some(id)) => Some(id),
        // **Dropped rather than stored** on an `ALL` window. A row carrying a
        // document type its scope tells the resolver to ignore is a row two
        // readers will disagree about, and the one who reads the column wins
        // silently.
        (DelegationScope::All, _) => None,
    };

    let reason = normalize_reason(request.reason.as_deref(), &mut details);

    if !details.is_empty() {
        return Err(AppError::validation(details));
    }

    Ok(ValidatedDelegation {
        delegate_user_id: request.delegate_user_id,
        scope,
        document_type_id,
        starts_at: request.starts_at,
        ends_at: request.ends_at,
        reason,
    })
}

/// Trimmed, with whitespace treated as absent, and bounded.
///
/// `workflow::domain::task::normalize_comment`'s two normalizations, for the
/// same two reasons: a box full of spaces is not a reason, and a `TEXT` column
/// with no bound is a column somebody can fill.
fn normalize_reason(reason: Option<&str>, details: &mut Vec<ValidationDetail>) -> Option<String> {
    let trimmed = reason?.trim();

    if trimmed.is_empty() {
        return None;
    }

    if trimmed.chars().count() > MAX_REASON_LENGTH {
        details.push(ValidationDetail::new(
            "reason",
            "maxLength",
            "TOO_LONG",
            format!("a delegation reason is at most {MAX_REASON_LENGTH} characters"),
        ));

        return None;
    }

    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-01T09:00:00Z")
            .expect("a fixed instant")
            .with_timezone(&Utc)
    }

    fn request() -> CreateDelegationRequest {
        CreateDelegationRequest {
            delegate_user_id: Uuid::now_v7(),
            starts_at: now(),
            ends_at: now() + TimeDelta::days(7),
            scope: None,
            document_type_id: None,
            reason: None,
        }
    }

    fn codes(error: AppError) -> Vec<String> {
        match error {
            AppError::Validation { details } => {
                details.into_iter().map(|detail| detail.code).collect()
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_window_with_no_scope_covers_everything() {
        let validated = validate_create(&request(), Uuid::now_v7(), now()).expect("a window");

        assert_eq!(validated.scope, DelegationScope::All);
        assert!(validated.document_type_id.is_none());
    }

    #[test]
    fn delegating_to_yourself_is_refused() {
        // `ck_delegations_not_self` would refuse it too. This is what turns the
        // constraint's name into a message naming the field.
        let request = request();

        let error =
            validate_create(&request, request.delegate_user_id, now()).expect_err("refused");

        assert_eq!(codes(error), ["DELEGATE_IS_DELEGATOR"]);
    }

    #[test]
    fn a_window_that_ends_before_it_starts_is_refused() {
        let mut request = request();
        request.ends_at = request.starts_at - TimeDelta::hours(1);

        let error = validate_create(&request, Uuid::now_v7(), now()).expect_err("refused");

        assert_eq!(codes(error), ["WINDOW_INVERTED"]);
    }

    #[test]
    fn a_window_that_is_already_over_is_refused_in_its_own_words() {
        // Ordered correctly and useless, which is a different mistake from the
        // one above and needs a different sentence: it is what a year typed
        // wrong looks like.
        let mut request = request();
        request.starts_at = now() - TimeDelta::days(30);
        request.ends_at = now() - TimeDelta::days(23);

        let error = validate_create(&request, Uuid::now_v7(), now()).expect_err("refused");

        assert_eq!(codes(error), ["WINDOW_ALREADY_OVER"]);
    }

    #[test]
    fn a_role_scoped_window_is_refused_with_the_reason() {
        let mut request = request();
        request.scope = Some("ROLE".to_owned());

        let error = validate_create(&request, Uuid::now_v7(), now()).expect_err("refused");

        assert_eq!(codes(error), ["SCOPE_UNSUPPORTED"]);
    }

    #[test]
    fn a_document_type_window_names_the_type() {
        let mut request = request();
        request.scope = Some("DOCUMENT_TYPE".to_owned());

        let error = validate_create(&request, Uuid::now_v7(), now()).expect_err("refused");

        assert_eq!(codes(error), ["REQUIRED"]);
    }

    #[test]
    fn a_document_type_on_a_window_that_covers_everything_is_dropped() {
        // Stored, it would be a column the resolver's `scope` check tells it to
        // ignore and a column a screen would render. One of the two readers
        // would be wrong and neither would say so.
        let mut request = request();
        request.document_type_id = Some(Uuid::now_v7());

        let validated = validate_create(&request, Uuid::now_v7(), now()).expect("a window");

        assert!(validated.document_type_id.is_none());
    }

    #[test]
    fn a_reason_of_spaces_is_no_reason() {
        let mut request = request();
        request.reason = Some("   ".to_owned());

        let validated = validate_create(&request, Uuid::now_v7(), now()).expect("a window");

        assert!(validated.reason.is_none());
    }

    #[test]
    fn a_reason_longer_than_the_bound_is_refused() {
        let mut request = request();
        request.reason = Some("x".repeat(MAX_REASON_LENGTH + 1));

        let error = validate_create(&request, Uuid::now_v7(), now()).expect_err("refused");

        assert_eq!(codes(error), ["TOO_LONG"]);
    }
}
