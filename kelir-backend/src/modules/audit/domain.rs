//! What an audit row is when somebody searches for it (FR-AUD-004; [#252]).
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::response::Pagination;
use uuid::Uuid;

/// **The permission that opens an object type's recorded values, or `None` for
/// a type this build cannot place** ([#252] AC2, **D-49**).
///
/// # Why a table and not a permission on the row
///
/// `audit_events` records `object_type` and `object_id` and nothing about who
/// may read that object. The permission lives in the module that owns the
/// object, so placing a row means mapping its type back to that module — and
/// this is the map. It is here rather than in each module because a search
/// crosses all of them: a function per module would be nineteen functions and
/// one caller.
///
/// # The unknown type withholds
///
/// A type this build does not know is one written by a release that did not
/// consult this table, or by a plugin. **`None` withholds the values and keeps
/// the row**, which is #252 AC2's rule and is the safe direction — the same
/// choice `activity::domain::disclosable` makes for the same reason (**D-45**),
/// and the opposite of `EventCategory::from_db`'s tolerant guess, because there
/// a wrong answer costs a label and here it costs a record's contents.
///
/// **The row is never hidden.** A search that silently omitted rows would teach
/// an auditor that the trail is shorter than it is, which is worse than one
/// that says *something happened here and you may not see what*.
///
/// [#252]: https://github.com/sujanto-gaws/kelir/issues/252
pub fn readable_by(object_type: &str) -> Option<&'static str> {
    Some(match object_type {
        // Master data. `PARTY_ROLE` is `master-data:party-role:read` rather
        // than the party's own, which is #81's rule and the one D-12 was
        // arguing about: *this party is a supplier* is what that permission
        // exists to refuse.
        "PARTY" => "master-data:party:read",
        "PARTY_ROLE" => "master-data:party-role:read",
        "FACILITY" => "master-data:facility:read",

        // Documents and what hangs on them.
        "DOCUMENT" => "document:read",
        "ATTACHMENT" => "attachment:read",
        "COMMENT" => "comment:read",

        // Configuration. A numbering rule's values are a document type's
        // configuration, so they sit behind the type's own read.
        "DOCUMENT_TYPE" | "DOCUMENT_TYPE_NUMBERING_RULE" => "document-type:read",
        "RAD_FORM" | "RAD_FORM_SUBMISSION" => "rad:form:read",
        "RAD_LIST" => "rad:list:read",

        // Workflow.
        "WORKFLOW_DEFINITION" => "workflow:definition:read",
        "WORKFLOW_INSTANCE" => "workflow:instance:read",
        "WORKFLOW_TASK" => "workflow:task:read",

        // Identity and organization.
        "USER" => "identity:user:read",
        "ROLE" => "identity:role:read",
        "DELEGATION" => "identity:delegation:read",
        "DEPARTMENT" => "organization:department:read",
        "TENANT" => "organization:tenant:read",

        _ => return None,
    })
}

/// One audit row, as a search reports it.
///
/// **The metadata is always here and the values may not be.** Who did what to
/// which object, and when, is the trail; `old_value` and `new_value` are the
/// object's own contents and are served only to a caller who may read that
/// object (#252 AC2).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: Uuid,

    pub event_type: String,
    pub action: String,
    pub object_type: String,
    pub object_id: Uuid,

    pub actor_user_id: Option<Uuid>,
    pub ip_address: Option<String>,
    pub reason: Option<String>,

    /// **`None` when withheld and `None` when the row never had one**, which
    /// this type deliberately does not distinguish — see [`values_withheld`],
    /// which is the field that says which happened. Two nullable payloads and a
    /// boolean is a smaller contract than four states encoded in the payloads.
    pub old_value: Option<Value>,
    pub new_value: Option<Value>,

    /// **True when this caller may not read the object these values describe.**
    ///
    /// Stated rather than left to be inferred from a null: an auditor looking
    /// at a row with no values needs to know whether nothing was recorded or
    /// whether they are not allowed to see it, and those are different facts
    /// about the trail.
    pub values_withheld: bool,

    pub occurred_at: DateTime<Utc>,
}

/// What a caller may narrow a search by (#252 AC1).
///
/// **No free-text.** Every field here is an exact match or a bound, which is
/// what the existing indexes on `audit_events` answer — `(object_type,
/// object_id, created_at)`, `(actor_user_id, created_at)`, `(tenant_id,
/// created_at)`. A `LIKE` over `old_value_json` would be a sequential scan of
/// the one table nobody may delete from, and it would search content this
/// surface will not always show.
///
/// **Paging is a field rather than a second extractor**, which is
/// `DocumentQuery`'s shape: two `QueryParams` over one query string means two
/// structs each seeing the other's parameters, and the clamping stays in
/// [`Pagination`] either way.
///
/// Unknown parameters are ignored rather than refused, for the reason
/// `DocumentQuery` gives: `deny_unknown_fields` on one endpoint and nowhere
/// else is a difference between endpoints with nothing behind it (coding
/// standard §1.1).
#[derive(Debug, Default, Clone, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct AuditSearch {
    /// 1-based page number; values below 1 are treated as 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,

    /// Who acted.
    pub actor_user_id: Option<Uuid>,
    /// What kind of thing they acted on — `DOCUMENT`, `PARTY`, `USER`.
    pub object_type: Option<String>,
    /// Which one, exactly.
    pub object_id: Option<Uuid>,
    /// Naming convention §7's dotted vocabulary — `Document.Approved`.
    pub event_type: Option<String>,
    /// Inclusive lower bound on when it happened.
    pub from: Option<DateTime<Utc>>,
    /// Inclusive upper bound.
    pub to: Option<DateTime<Utc>>,
}

impl AuditSearch {
    /// The paging half, so clamping and the 1-based page live in one place.
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every object type this codebase writes has an answer**, and the test
    /// names them rather than counting them — a count passes when a type is
    /// renamed and this does not.
    ///
    /// The list is `grep -rn 'object_type:' src/` reduced to its constants and
    /// literals. A type added later without a row here withholds its values,
    /// which is the safe direction and is what the `_ => None` arm is for; this
    /// test is what stops that from happening silently to a type we already
    /// have.
    #[test]
    fn every_object_type_this_release_writes_can_be_placed() {
        for object_type in [
            "PARTY",
            "PARTY_ROLE",
            "FACILITY",
            "DOCUMENT",
            "ATTACHMENT",
            "COMMENT",
            "DOCUMENT_TYPE",
            "DOCUMENT_TYPE_NUMBERING_RULE",
            "RAD_FORM",
            "RAD_FORM_SUBMISSION",
            "RAD_LIST",
            "WORKFLOW_DEFINITION",
            "WORKFLOW_INSTANCE",
            "WORKFLOW_TASK",
            "USER",
            "ROLE",
            "DELEGATION",
            "DEPARTMENT",
            "TENANT",
        ] {
            assert!(
                readable_by(object_type).is_some(),
                "`{object_type}` is written by this codebase and cannot be placed, \
                 so its values are withheld from everybody"
            );
        }
    }

    /// And a type nobody has heard of withholds rather than opens.
    #[test]
    fn an_unknown_object_type_has_no_permission_and_so_withholds() {
        assert_eq!(readable_by("SOMETHING_A_PLUGIN_WROTE"), None);
    }
}
