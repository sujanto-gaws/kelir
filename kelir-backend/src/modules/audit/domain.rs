//! What an audit row is when somebody searches for it (FR-AUD-004; [#252]).
//!
//! [#252]: https://github.com/sujanto-gaws/kelir/issues/252

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::response::Pagination;
use uuid::Uuid;

/// **What kind of thing an audit row is about**, as a closed vocabulary
/// ([#252] AC2, **D-49**; [#323], **D-61**).
///
/// # Why this is a type and was a `&str`
///
/// `AuditEntry.object_type` was `&'a str` and [`ObjectType::readable_by`] ended
/// in `_ => return None`. So a new object type was **placeable by accident**: it
/// compiled, it wrote, and its recorded values were withheld from everybody in
/// silence — including from a caller holding every permission the object has,
/// which for a `Must` requirement is a hole rather than a policy. That is
/// exactly how `EXTERNAL_REFERENCE` went unnoticed between two items a day
/// apart, and it is [#323].
///
/// **`tests/audit_object_types.rs` stood in for this and could not close it.**
/// It walked the crate's source for three write shapes and asserted each found
/// type had an arm. A fourth shape — a value built by `format!`, read from a
/// column, returned by a differently named method — was invisible to it, and
/// unlike `configuration_reference` that bluntness ran in the **unsafe**
/// direction: the scan misses it and the values are withheld in silence, which
/// is the defect the file existed for. **D-61** recorded the type as the option
/// not taken and named its cost — 69 construction sites across eleven modules.
///
/// **Now there is no fourth shape.** `AuditEntry.object_type` takes this type,
/// so a value that is not a variant cannot be written at all, and a variant
/// added without a permission does not compile. That is the move this project
/// already made for `activity::record`'s executor and `scanner::scan`'s
/// outcome: the failure becomes a compile error rather than a checklist.
///
/// # The vocabulary is `SCREAMING_SNAKE_CASE` on the wire and in the column
///
/// [`ObjectType::as_db`] is the only place a variant becomes text, and the text
/// is what `audit_events.object_type` has always held — the hash chain covers
/// those bytes, so a variant whose `as_db` changed would break verification of
/// every row already written. [`ObjectType::from_db`] reads them back.
///
/// **There is deliberately no `PARTY_ROLE` variant**, and the reasoning is kept
/// here for whoever needs it. Nothing writes that type: a party gaining the
/// SUPPLIER role is audited as [`ObjectType::Party`], because `object_id` is the
/// party and that is the object an auditor asks about
/// (`master_data::service::role`, naming convention §7). An arm existed and
/// mapped it to `master-data:party-role:read` — #81's rule, and the one **D-12**
/// was arguing about, that *this party is a supplier* is what that permission
/// exists to refuse. It was unreachable, and an arm nothing can reach is the
/// same ageing checklist as a list nothing regenerates (**D-61**). If a release
/// ever needs to audit a party role as its own object, this is where that
/// decision is taken again, with the previous one in front of it.
///
/// [#252]: https://github.com/sujanto-gaws/kelir/issues/252
/// [#323]: https://github.com/sujanto-gaws/kelir/issues/323
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObjectType {
    // Master data.
    Party,
    Facility,

    // Documents and what hangs on them.
    Document,
    Attachment,
    /// **Its own type, not [`ObjectType::Attachment`].** A trail that filed both
    /// under one name would make *which of these was a file* a question nobody
    /// can answer from the row — and two things governed by the same permission
    /// still have to be told apart.
    ExternalReference,
    Comment,

    // Configuration.
    DocumentType,
    DocumentTypeNumberingRule,
    RadForm,
    RadFormSubmission,
    RadList,
    RadMenu,

    // Workflow.
    //
    // **There is no variant for a workflow instance either, and it is a second
    // `PARTY_ROLE`** — found by this issue rather than by the test that was
    // supposed to find it. `readable_by` carried a `WORKFLOW_INSTANCE` arm and
    // `modules::workflow` declared an `INSTANCE_OBJECT_TYPE` constant, and
    // **nothing ever passed that constant to an `AuditEntry`**: a `git grep` at
    // `91d562b` finds the declaration and no use. D-61's
    // `every_arm_answers_for_a_type_something_writes` passed anyway, because a
    // declared `…OBJECT_TYPE` constant was one of the three shapes its source
    // walk counted as a *write* — so the direction-2 test was satisfied by the
    // very declaration that made the arm unreachable. Deleting the constants is
    // what exposed it.
    //
    // Nothing audits an instance's lifecycle: what a process did is
    // `workflow_history`, which is its own record with its own permission
    // (`modules::activity`'s four-record table). `workflow:instance:read` is
    // untouched and is not now unchecked — it opens
    // `GET /workflow/instances/{id}`, a different surface answering a different
    // question, which is the test **D-47** applies. If a release ever audits an
    // instance, the compiler asks for the permission at the moment the variant
    // is added, which is the whole point of this type.
    WorkflowDefinition,
    WorkflowTask,

    // Identity and organization.
    User,
    Role,
    Delegation,
    Department,
    Tenant,
}

impl ObjectType {
    /// Every variant, for the round trip and for whoever needs to enumerate the
    /// vocabulary.
    ///
    /// **This is the one thing the type does not keep honest**, and
    /// `tests/audit_object_types.rs` is what does: a variant added to the enum
    /// and not to this list compiles. A variant added and given no permission
    /// does not, which is the property #323 asks for.
    pub const ALL: &'static [Self] = &[
        Self::Party,
        Self::Facility,
        Self::Document,
        Self::Attachment,
        Self::ExternalReference,
        Self::Comment,
        Self::DocumentType,
        Self::DocumentTypeNumberingRule,
        Self::RadForm,
        Self::RadFormSubmission,
        Self::RadList,
        Self::RadMenu,
        Self::WorkflowDefinition,
        Self::WorkflowTask,
        Self::User,
        Self::Role,
        Self::Delegation,
        Self::Department,
        Self::Tenant,
    ];

    /// What the column holds and the hash chain covers (naming convention §7).
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Party => "PARTY",
            Self::Facility => "FACILITY",
            Self::Document => "DOCUMENT",
            Self::Attachment => "ATTACHMENT",
            Self::ExternalReference => "EXTERNAL_REFERENCE",
            Self::Comment => "COMMENT",
            Self::DocumentType => "DOCUMENT_TYPE",
            Self::DocumentTypeNumberingRule => "DOCUMENT_TYPE_NUMBERING_RULE",
            Self::RadForm => "RAD_FORM",
            Self::RadFormSubmission => "RAD_FORM_SUBMISSION",
            Self::RadList => "RAD_LIST",
            Self::RadMenu => "RAD_MENU",
            Self::WorkflowDefinition => "WORKFLOW_DEFINITION",
            Self::WorkflowTask => "WORKFLOW_TASK",
            Self::User => "USER",
            Self::Role => "ROLE",
            Self::Delegation => "DELEGATION",
            Self::Department => "DEPARTMENT",
            Self::Tenant => "TENANT",
        }
    }

    /// **The permission that opens this type's recorded values** ([#252] AC2,
    /// **D-49**).
    ///
    /// # Why a map here and not a permission on the row
    ///
    /// `audit_events` records `object_type` and `object_id` and nothing about
    /// who may read that object. The permission lives in the module that owns
    /// the object, so placing a row means mapping its type back to that module —
    /// and this is the map. It is here rather than in each module because a
    /// search crosses all of them: a function per module would be nineteen
    /// functions and one caller.
    ///
    /// **Exhaustive, with no wildcard** ([#323] AC1). A variant added without a
    /// permission is a compile error, which is the whole of what this issue
    /// buys: the previous `_ => return None` made *nobody may see this* the
    /// default for anything nobody had thought about.
    ///
    /// [#252]: https://github.com/sujanto-gaws/kelir/issues/252
    /// [#323]: https://github.com/sujanto-gaws/kelir/issues/323
    pub fn readable_by(self) -> &'static str {
        match self {
            Self::Party => "master-data:party:read",
            Self::Facility => "master-data:facility:read",

            Self::Document => "document:read",
            Self::Attachment => "attachment:read",
            // An external reference is a link rather than a file, and it is read
            // through the same permission its neighbours are: `list_references`
            // requires `attachment:read`, so that is the permission its recorded
            // values need (**D-49**).
            Self::ExternalReference => "attachment:read",
            Self::Comment => "comment:read",

            // A numbering rule's values are a document type's configuration, so
            // they sit behind the type's own read.
            Self::DocumentType | Self::DocumentTypeNumberingRule => "document-type:read",
            Self::RadForm | Self::RadFormSubmission => "rad:form:read",
            Self::RadList => "rad:list:read",
            Self::RadMenu => "rad:menu:read",

            Self::WorkflowDefinition => "workflow:definition:read",
            Self::WorkflowTask => "workflow:task:read",

            Self::User => "identity:user:read",
            Self::Role => "identity:role:read",
            Self::Delegation => "identity:delegation:read",
            Self::Department => "organization:department:read",
            Self::Tenant => "organization:tenant:read",
        }
    }

    /// Reads a stored value back, **refusing one this build does not know**
    /// ([#252] AC2).
    ///
    /// # The unknown type still withholds, and now that is the only way to get
    /// there
    ///
    /// `audit_events` is append-only and crosses releases: a row written by a
    /// later release or by a plugin can name a type this binary has never heard
    /// of, and `None` is what serves it as an event with no contents rather than
    /// as contents nobody decided about. That is #252 AC2's rule and the safe
    /// direction — the same choice `activity::domain::disclosable` makes for the
    /// same reason (**D-45**), and the opposite of `EventCategory::from_db`'s
    /// tolerant guess, because there a wrong answer costs a label and here it
    /// costs a record's contents.
    ///
    /// **The row is never hidden.** A search that silently omitted rows would
    /// teach an auditor that the trail is shorter than it is, which is worse
    /// than one that says *something happened here and you may not see what*.
    ///
    /// **What changed with [#323]** is which types can arrive here unplaceable.
    /// Before, a type *this crate itself wrote* could: nothing stopped a module
    /// from putting a new literal in an `AuditEntry`, and nothing said the map
    /// had to grow with it. Now this crate cannot write a value that is not a
    /// variant, so an unknown type is a row from somewhere else — which is the
    /// only case the withholding was ever for.
    ///
    /// [#252]: https://github.com/sujanto-gaws/kelir/issues/252
    /// [#323]: https://github.com/sujanto-gaws/kelir/issues/323
    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|object_type| object_type.as_db() == value)
    }
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

    // **The list this test used to hold is gone, twice over.**
    //
    // It once named nineteen object types and asserted each could be placed,
    // with a doc comment saying the list was `grep -rn 'object_type:' src/`
    // reduced to its constants and literals. That grep was run on 2026-09-01.
    // On 2026-09-02 the attachment tail added `EXTERNAL_REFERENCE`, the list did
    // not grow, and **the test still passed** while every external-reference row
    // withheld its values from everybody — the [Sprint 13 independent pass][pass],
    // finding 2.
    //
    // That is [sprint plan][plan] verification rule 6 exactly: *a test asserting
    // a project-wide property discovers its subjects rather than listing them;
    // an enumerating test fails the way the list it checks fails.* Its
    // replacement ran the grep instead of quoting it, in
    // `tests/audit_object_types.rs`.
    //
    // **And that replacement is now mostly gone too**, because the property it
    // discovered is one the type holds ([#323]). A scan looking for object types
    // in the crate's source has nothing left to find: an object type is an
    // [`ObjectType`], every variant has a permission or this file does not
    // compile, and there is no shape for the scan to miss.
    //
    // [pass]: ../../../../projects/verifications/13.%20Sprint%2013%20Independent%20Pass.md
    // [plan]: ../../../../projects/planning/01.%20Sprint%20Plan.md
    // [#323]: https://github.com/sujanto-gaws/kelir/issues/323

    /// **The guarantee, named** ([#323] AC2).
    ///
    /// This test cannot fail while the crate compiles, and saying so is the
    /// point rather than an apology for it: the assertion is a compile error,
    /// and a compile error has no test to run. What is written here is *which*
    /// compile error, so that whoever adds a variant and is refused by
    /// `readable_by` finds the reason rather than reaching for a wildcard.
    ///
    /// **Adding a variant to [`ObjectType`] without giving it a permission does
    /// not compile.** `readable_by` matches on `self` with no `_` arm, so the
    /// new variant is a non-exhaustive-match error naming itself:
    ///
    /// ```text
    /// error[E0004]: non-exhaustive patterns: `ObjectType::PurchaseRequisition` not covered
    ///    --> src/modules/audit/domain.rs:179:15
    /// ```
    ///
    /// Demonstrated that way on 2026-09-06, with `as_db` refusing on the same
    /// build. The fix is a permission; the fix is **not** a wildcard, which is
    /// what the `&str` version had and is the whole of what #323 removed —
    /// `_ => return None` made *nobody may see this* the default for every type
    /// nobody had thought about, silently.
    ///
    /// [#323]: https://github.com/sujanto-gaws/kelir/issues/323
    #[test]
    fn every_object_type_has_a_permission_because_the_match_is_exhaustive() {
        for object_type in ObjectType::ALL {
            assert!(
                !object_type.readable_by().is_empty(),
                "{object_type:?} has no permission"
            );
        }
    }

    /// **A type nobody has heard of withholds rather than opens** (#252 AC2).
    ///
    /// The row is still served; its values are not. Since #323 this is the only
    /// route to the withholding, and it is the one it was always for: a row
    /// written by a later release or by a plugin. This crate can no longer write
    /// a type it has not placed.
    #[test]
    fn an_unknown_object_type_cannot_be_read_back_and_so_withholds() {
        assert_eq!(ObjectType::from_db("SOMETHING_A_PLUGIN_WROTE"), None);
    }

    /// **What is written is what is read back**, over every variant.
    ///
    /// `as_db` is the only place a variant becomes text and `from_db` the only
    /// place text becomes a variant, so a pair that disagreed would withhold a
    /// type this crate had just written — the defect #323 closed, reintroduced
    /// one function along.
    #[test]
    fn every_variant_survives_the_round_trip_through_the_column() {
        for object_type in ObjectType::ALL {
            assert_eq!(
                ObjectType::from_db(object_type.as_db()),
                Some(*object_type),
                "{object_type:?} does not read back as itself"
            );
        }
    }

    /// **`ALL` names every variant**, which is the one thing the type does not
    /// hold for itself.
    ///
    /// The two tests above walk `ALL`, so a variant missing from it is a variant
    /// neither of them checks — and `from_db` reads `ALL`, so such a variant
    /// would also be written and never read back. The count is the guard, and
    /// it is deliberately a number a reader has to change on purpose: adding a
    /// variant, adding it to `ALL`, and bumping this is three edits in one
    /// commit, where forgetting the second is silent.
    #[test]
    fn all_names_every_variant() {
        assert_eq!(
            ObjectType::ALL.len(),
            19,
            "a variant was added to `ObjectType` and not to `ObjectType::ALL`, or the other way \
             around — `from_db` reads `ALL`, so a variant missing from it is written and never \
             read back"
        );

        let mut seen: Vec<&str> = ObjectType::ALL.iter().map(|it| it.as_db()).collect();
        seen.sort_unstable();
        seen.dedup();

        assert_eq!(
            seen.len(),
            ObjectType::ALL.len(),
            "two variants share a database value, so one of them cannot be read back"
        );
    }
}
