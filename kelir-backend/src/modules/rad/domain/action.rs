//! Configured actions — the buttons a list, a detail page, a document or a task
//! offers (FR-RAD-003, FR-RAD-010, [#340]).
//!
//! **`rad_actions` has been in the schema since `0014_rad.sql` and nothing has
//! ever read it.** [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
//! §5.13 says why that was right at the time — *a permission row that no route
//! checks reads as a control that exists* — and the same sentence is why this
//! file arrives with a route rather than ahead of one.
//!
//! # An action is scoped by context, not by list
//!
//! **This is the shape the table has, and it is worth stating plainly because
//! it is not the shape [#340] assumed.** The issue's scope line says *"the row
//! actions the definition declares"*, and a list definition declares none:
//! `rad_actions` carries a `context` and no `list_id`, so a `LIST` action
//! belongs to the tenant and is offered on **every** list in it. Narrowing that
//! to a particular list is a schema change rather than a query, and it is filed
//! rather than smuggled in here — §5.7 and §5.8 are what a list owns, and both
//! carry `list_id` precisely because they are the list's.
//!
//! # The permission is the action's own, and there is no second one
//!
//! **Nothing gates the catalogue as a whole, and every row is gated
//! individually.** `required_permission` is a column, so the question *may this
//! caller invoke this action* already has an answer per row; a
//! `rad:action:read` beside it would be the shape
//! [ADR-0011](../../../../../docs/architectures/adr/0011.%20A%20Derived%20Surface%20Requires%20the%20Permission%20of%20What%20It%20Derives%20From.md)
//! rejects in its own converse — *a permission that guards nothing the first
//! one does not is one permission too many* — and it would let a deployment
//! grant the button to somebody the button refuses.
//!
//! So an action the caller could not invoke is **not returned**, rather than
//! returned and disabled. A disabled button is a statement that the thing
//! exists and is not for you, which is information the row's own permission
//! already said should not be served; and a renderer that had to filter would
//! be a second copy of the rule, which is [ADR-0011](../../../../../docs/architectures/adr/0011.%20A%20Derived%20Surface%20Requires%20the%20Permission%20of%20What%20It%20Derives%20From.md)
//! §3 B's objection to filtering per reader in the layer above the query.
//!
//! A row whose `required_permission` is `NULL` is offered to every
//! authenticated caller, which is the deployment saying *everyone may*.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

/// Where an action is surfaced (§5.10's `CHECK`).
///
/// **All four, though only `List` has a reader.** The vocabulary is the
/// column's, and reading it as a closed enum is what makes a `context` nobody
/// has implemented a compile error at the match rather than a row that silently
/// appears on the wrong screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionContext {
    List,
    Detail,
    Document,
    Task,
}

impl ActionContext {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::List => "LIST",
            Self::Detail => "DETAIL",
            Self::Document => "DOCUMENT",
            Self::Task => "TASK",
        }
    }

    /// The stored value as an enum, or `None` for a value the `CHECK` does not
    /// allow.
    ///
    /// `None` rather than a fallback variant, and the difference from
    /// [`super::list::ListStatus::from_db`] is deliberate: a status has a safe
    /// closed answer (`Deprecated` — stop offering it), and a *context* does
    /// not. Guessing one would put a button on a screen it was not configured
    /// for, so an unreadable row is dropped from the catalogue instead.
    pub fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "LIST" => Self::List,
            "DETAIL" => Self::Detail,
            "DOCUMENT" => Self::Document,
            "TASK" => Self::Task,
            _ => return None,
        })
    }
}

/// What invoking an action does (§5.10's other `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActionType {
    Navigate,
    ApiCall,
    WorkflowAction,
    Plugin,
}

impl ActionType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Navigate => "NAVIGATE",
            Self::ApiCall => "API_CALL",
            Self::WorkflowAction => "WORKFLOW_ACTION",
            Self::Plugin => "PLUGIN",
        }
    }

    /// `None` for a value outside the `CHECK`, for [`ActionContext::from_db`]'s
    /// reason one step further in: an action whose *type* cannot be read is one
    /// whose behaviour is unknown, and rendering a button that does something
    /// unknown is worse than rendering none.
    pub fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "NAVIGATE" => Self::Navigate,
            "API_CALL" => Self::ApiCall,
            "WORKFLOW_ACTION" => Self::WorkflowAction,
            "PLUGIN" => Self::Plugin,
            _ => return None,
        })
    }
}

/// A configured action, as a renderer receives it.
///
/// **`requiredPermission` is not on the wire.** The caller holds it — that is
/// why the row is in the response at all — so sending it back would be telling
/// them something they proved, and it would invite a client to re-decide a
/// question the server has already closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: Uuid,
    pub action_key: String,
    pub label: String,
    pub context: ActionContext,
    pub action_type: ActionType,
    /// The action's own configuration — a route for `NAVIGATE`, an endpoint for
    /// `API_CALL`. Opaque here on purpose: `0014` declares it `JSONB DEFAULT
    /// '{}'` and no shape has been specified for it, so a type imposed now
    /// would be this file inventing one.
    pub config: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_context_the_check_allows() {
        for (stored, expected) in [
            ("LIST", ActionContext::List),
            ("DETAIL", ActionContext::Detail),
            ("DOCUMENT", ActionContext::Document),
            ("TASK", ActionContext::Task),
        ] {
            assert_eq!(ActionContext::from_db(stored), Some(expected));
            assert_eq!(expected.as_db(), stored);
        }
    }

    #[test]
    fn reads_every_action_type_the_check_allows() {
        for (stored, expected) in [
            ("NAVIGATE", ActionType::Navigate),
            ("API_CALL", ActionType::ApiCall),
            ("WORKFLOW_ACTION", ActionType::WorkflowAction),
            ("PLUGIN", ActionType::Plugin),
        ] {
            assert_eq!(ActionType::from_db(stored), Some(expected));
            assert_eq!(expected.as_db(), stored);
        }
    }

    /// **Dropped, not guessed.** A `CHECK` change that this enum did not learn
    /// about must not put a button on a screen it was never configured for.
    #[test]
    fn a_context_outside_the_check_is_unreadable_rather_than_defaulted() {
        assert_eq!(ActionContext::from_db("SIDEBAR"), None);
        assert_eq!(ActionType::from_db("SEND_EMAIL"), None);
    }
}
