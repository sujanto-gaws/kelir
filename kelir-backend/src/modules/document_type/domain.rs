//! Document type definitions and their bindings (FR-DTYPE-001, 002, 003).
//!
//! **The bindings are columns on the type, not a separate resource.** A type
//! that cannot name the form it renders is a type nothing can use, which is why
//! #157 is one item rather than two: `formId` and `listId` are fields of the
//! create and update payloads, and the workflow binding is a collection on the
//! same aggregate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
use crate::modules::master_data::domain::GovernedEntity;
use crate::response::Pagination;
use crate::utils::serde::present_or_absent;

/// Longest `typeCode` §6.2 holds — `type_code VARCHAR(64)`.
pub const MAX_TYPE_CODE_LENGTH: usize = 64;
/// Longest `name` §6.2 holds — `name VARCHAR(200)`.
pub const MAX_NAME_LENGTH: usize = 200;
/// Longest `category` and `targetEntityType` hold — `VARCHAR(64)`.
pub const MAX_CODE_LENGTH: usize = 64;

/// How sensitive a document of this type is by default (§6.2's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SecurityLevel {
    Public,
    Internal,
    Confidential,
    Restricted,
}

impl SecurityLevel {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Internal => "INTERNAL",
            Self::Confidential => "CONFIDENTIAL",
            Self::Restricted => "RESTRICTED",
        }
    }

    /// An unknown stored value reads as `Restricted`.
    ///
    /// **Fails closed, and in the strictest direction available.** The other
    /// enums in this codebase answer "deprecated" for an unknown value because
    /// that withdraws a definition from use. This one classifies *content*, so
    /// the safe answer is the most restrictive one — a value nobody recognises
    /// must not be read as `PUBLIC`.
    pub fn from_db(value: &str) -> Self {
        match value {
            "PUBLIC" => Self::Public,
            "INTERNAL" => Self::Internal,
            "CONFIDENTIAL" => Self::Confidential,
            _ => Self::Restricted,
        }
    }
}

/// Where a document type is in its life (§6.2's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentTypeStatus {
    Draft,
    Active,
    Deprecated,
}

impl DocumentTypeStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Active => "ACTIVE",
            Self::Deprecated => "DEPRECATED",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "DRAFT" => Self::Draft,
            "ACTIVE" => Self::Active,
            _ => Self::Deprecated,
        }
    }
}

/// A workflow binding on a type (§6.4).
///
/// One type may bind several workflows and pick between them by condition; the
/// engine evaluates them in `priority` order and the first match wins. Which is
/// why this is a collection rather than a column, and why `priority` is on the
/// wire — an ordering the caller cannot see is an ordering they cannot fix.
///
/// `PartialEq` is what "unchanged" means on an update (#716): a sent binding
/// equal to a stored one in all five fields is not re-checked
/// ([`changed_bindings`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowBinding {
    /// The workflow definition this type routes to.
    ///
    /// **Checked since [#187](https://github.com/sujanto-gaws/kelir/issues/187).**
    /// A binding a write sets must name a definition that exists in this tenant
    /// and is `ACTIVE`, verified in the write's transaction under a share lock
    /// on the row the check read (`service::check_workflow_bindings`), and
    /// `0025_workflow.sql` added the foreign key `0015_document.sql` deferred.
    /// Before Sprint 10 this comment said the id was unverifiable, which stopped
    /// being true the moment the workflow table existed — a comment describing a
    /// state that has ended is worse than no comment (**D-34**, one document
    /// over).
    ///
    /// **A binding an update sends back unchanged is not re-checked**
    /// ([#716](https://github.com/sujanto-gaws/kelir/issues/716)), so a type
    /// whose revision was deprecated since can still be renamed by a caller
    /// that PUTs the whole type. It stays bound to that revision, and a
    /// submission still refuses it (#187, #713).
    pub workflow_definition_id: Uuid,
    /// `null` means this is the default binding.
    pub condition_expression: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default, deserialize_with = "crate::utils::storable::option_date")]
    pub valid_from: Option<chrono::NaiveDate>,
    #[serde(default, deserialize_with = "crate::utils::storable::option_date")]
    pub valid_to: Option<chrono::NaiveDate>,
}

fn default_priority() -> i32 {
    1
}

/// A document type as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentType {
    pub id: Uuid,
    pub type_code: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    /// The published form revision this type renders.
    pub form_id: Option<Uuid>,
    pub list_id: Option<Uuid>,
    pub default_security_level: SecurityLevel,
    pub retention_policy_id: Option<Uuid>,
    pub target_entity_type: Option<String>,
    pub status: DocumentTypeStatus,
    pub workflows: Vec<WorkflowBinding>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The type list's query string: paging, a search, a status and a bound
/// workflow revision.
///
/// Unknown parameters are ignored, as [`Pagination`] ignores them everywhere;
/// a `status` outside the vocabulary is a 422 naming `status`, because it
/// deserializes into the enum. Added for the choosers of
/// [#525](https://github.com/sujanto-gaws/kelir/issues/525), which read the
/// server's match rather than filtering one page of 100.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct DocumentTypeQuery {
    /// 1-based page number; absent or below 1 is 1.
    pub page: Option<u32>,
    /// Rows per page, clamped to `response::MAX_PAGE_SIZE`.
    pub page_size: Option<u32>,
    /// Case-insensitive substring of the type code or the name. `%`, `_` and `\` in
    /// it match themselves; a blank search is no search.
    pub search: Option<String>,
    /// `DRAFT`, `ACTIVE` or `DEPRECATED`, matched exactly.
    pub status: Option<DocumentTypeStatus>,
    /// Only the types with a live binding to this workflow revision: in this
    /// tenant, not deleted and `ACTIVE`, whatever its validity window. One
    /// revision, not a workflow key; an id that names nothing is an empty page
    /// (#713, for the workflow editor's *Deprecate* warning).
    pub workflow_definition_id: Option<Uuid>,
}

impl DocumentTypeQuery {
    pub fn pagination(&self) -> Pagination {
        Pagination {
            page: self.page,
            page_size: self.page_size,
        }
    }
}

/// A document type on a list screen: without its workflow bindings.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTypeSummary {
    pub id: Uuid,
    pub type_code: String,
    pub name: String,
    pub category: Option<String>,
    pub form_id: Option<Uuid>,
    pub status: DocumentTypeStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDocumentTypeRequest {
    pub type_code: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub form_id: Option<Uuid>,
    pub list_id: Option<Uuid>,
    pub default_security_level: Option<SecurityLevel>,
    pub retention_policy_id: Option<Uuid>,
    pub target_entity_type: Option<String>,
    pub status: Option<DocumentTypeStatus>,
    #[serde(default)]
    pub workflows: Vec<WorkflowBinding>,
}

/// Editing a document type. `None` means *leave alone*; a collection that
/// **is** sent replaces the stored set wholesale.
///
/// `typeCode` is absent because it may not change: it is what a delegation
/// scopes itself to and what an integration names a type by.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDocumentTypeRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub category: Option<Option<String>>,
    /// The form this type renders; `null` clears it. A value equal to the one
    /// stored is kept without being checked again, so a form retired since it
    /// was bound does not refuse an unrelated edit; any other value must name a
    /// live, published form (#739, [`changed_reference`]).
    #[serde(default, deserialize_with = "present_or_absent")]
    pub form_id: Option<Option<Uuid>>,
    /// The list this type's documents appear on; `null` clears it. Like
    /// `formId`, a value equal to the one stored is not checked again, and any
    /// other must name a live list (#739).
    #[serde(default, deserialize_with = "present_or_absent")]
    pub list_id: Option<Option<Uuid>>,
    pub default_security_level: Option<SecurityLevel>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub retention_policy_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub target_entity_type: Option<Option<String>>,
    pub status: Option<DocumentTypeStatus>,
    /// The workflow bindings, all or nothing.
    ///
    /// When the key is absent or `null`, the stored set is left untouched:
    /// no binding is checked or rewritten, and the audit records no change to
    /// it. When an array is sent, it
    /// replaces the stored set wholesale, and `[]` clears it. There is no
    /// per-binding patch. A caller that edits one binding sends every binding
    /// it wants kept, and a caller that does not edit bindings leaves the key
    /// out (#718). Unlike the fields above, an absent key and `null` mean the
    /// same thing here, because "no bindings" is spelled `[]`.
    ///
    /// Of a sent array, only the bindings that differ from every stored one
    /// are checked; one sent back exactly as stored binds nothing new (#716,
    /// [`changed_bindings`]).
    pub workflows: Option<Vec<WorkflowBinding>>,
    /// The workflow bindings the caller read, sent with `workflows` so that a
    /// replacement built from a stale read is refused rather than applied
    /// (#728, **D-117**).
    ///
    /// `workflows` replaces the stored set wholesale, so a caller that read
    /// the type, waited, and sent its edit would delete every binding another
    /// administrator added in between. Sending the set it read lets the server
    /// see that: under the type-row lock and before anything is written, the
    /// stored set is compared with this one, and one that differs is a **409**
    /// with `error.code` `DOCUMENT_TYPE_WORKFLOWS_CHANGED` and nothing written,
    /// not the name or status either. The caller reloads the type and makes its
    /// change again.
    ///
    /// The comparison is a set comparison on all five fields of each binding,
    /// ignoring order and counting duplicates ([`same_bindings`]); `[]`
    /// matches a type with no bindings. A condition is compared as the text
    /// stored, so a caller sends back exactly what it read: the same expression
    /// re-serialised does not match.
    ///
    /// **Optional, for compatibility.** A caller that leaves it out, or sends
    /// `null`, is served exactly as before #728: its `workflows` replaces the
    /// stored set whatever that set has become, so it is unprotected against
    /// this race. Sent without `workflows`, it is a 422 at `expectedWorkflows`,
    /// code `REQUIRES_WORKFLOWS`, since there is no replacement for it to guard.
    pub expected_workflows: Option<Vec<WorkflowBinding>>,
}

/// The code a workflow replacement is refused with when the stored bindings
/// are no longer the ones the caller read (#728, **D-117**). A client
/// branches on it, not on the message, and offers to reload the type.
pub const DOCUMENT_TYPE_WORKFLOWS_CHANGED: &str = "DOCUMENT_TYPE_WORKFLOWS_CHANGED";

/// The refusal [`DOCUMENT_TYPE_WORKFLOWS_CHANGED`] names.
pub fn workflows_changed() -> AppError {
    AppError::conflict_with_code(
        DOCUMENT_TYPE_WORKFLOWS_CHANGED,
        "The workflow bindings of this document type changed since they were read, \
         so nothing was saved. Reload the document type and make the change again.",
    )
}

/// Whether the bindings a caller read are the bindings stored (#728).
///
/// A set comparison on all five fields: order does not matter, and one field
/// that differs on one binding is a mismatch. **Duplicates count**, so this
/// compares multisets: `[a, a]` is not `[a]`, and `[a, a, b]` is not
/// `[a, b, b]`. Collapsing them would let a read that saw one binding vouch
/// for a stored set holding two copies of it, which is not the set it read.
/// Two empty sets are the same set.
pub fn same_bindings(expected: &[WorkflowBinding], stored: &[WorkflowBinding]) -> bool {
    if expected.len() != stored.len() {
        return false;
    }

    // Each expected binding claims one stored binding equal to it, and a
    // claimed one cannot be claimed twice. The sets are a handful of rows, so
    // the quadratic walk is simpler than sorting a type with no `Ord`.
    let mut claimed = vec![false; stored.len()];

    expected.iter().all(|binding| {
        let unclaimed = stored
            .iter()
            .zip(claimed.iter())
            .position(|(candidate, taken)| !taken && candidate == binding);

        match unclaimed {
            Some(index) => {
                claimed[index] = true;
                true
            }
            None => false,
        }
    })
}

/// The sent bindings a write has to check, each with the index it was sent at:
/// those equal to **no** stored binding in all five fields (#716).
///
/// A binding sent back exactly as stored binds nothing the type was not
/// already bound to, so re-checking it could only refuse an unrelated edit —
/// a rename, say — because its revision was deprecated since. That is a real
/// state, and the fix for it is to change the binding, not to be unable to
/// rename the type. A binding that differs in any one field, the revision or
/// its condition, priority or window, is a binding the caller is setting, and
/// is checked like a new one.
///
/// Matched as a set rather than by position, so a caller that sends the stored
/// bindings in another order changes nothing. The index is the sent one, which
/// is the position a refusal names. Two sent bindings cannot both match one
/// stored row: they would share a priority, which `check_workflows` refuses
/// before this runs. On create, `stored` is empty and every binding is checked.
pub fn changed_bindings<'a>(
    sent: &'a [WorkflowBinding],
    stored: &'a [WorkflowBinding],
) -> impl Iterator<Item = (usize, &'a WorkflowBinding)> + 'a {
    sent.iter()
        .enumerate()
        .filter(move |(_, binding)| !stored.contains(binding))
}

/// The form or list id an update has to check: the one sent, unless it is the
/// one stored (#739).
///
/// `sent` is the request's field, `None` for a key left out and `Some(None)`
/// for `null`, which clears the binding and names nothing to check. `stored` is
/// the column as the type row holds it, read under that row's lock. An id sent
/// back as stored binds nothing the type was not already bound to, so
/// re-checking it could only refuse an unrelated edit because the form or list
/// was retired since: [`changed_bindings`]' reasoning, applied to the two
/// columns. A changed id is checked like a new one.
pub fn changed_reference(sent: Option<Option<Uuid>>, stored: Option<Uuid>) -> Option<Uuid> {
    sent.flatten().filter(|id| Some(*id) != stored)
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

/// Workflow bindings, checked for the things storage enforces and for the one
/// it cannot.
///
/// The window check is duplicated from `ck_document_type_workflows_window` so a
/// caller gets a 422 naming the field rather than a constraint violation
/// surfacing as a 500. The duplicate-priority check has no counterpart in
/// storage at all: two bindings at the same priority make "first match wins"
/// depend on row order, which is a routing decision nobody made.
fn check_workflows(workflows: &[WorkflowBinding], details: &mut Vec<ValidationDetail>) {
    let mut seen = Vec::new();

    for (index, binding) in workflows.iter().enumerate() {
        if let (Some(from), Some(to)) = (binding.valid_from, binding.valid_to) {
            if from > to {
                details.push(ValidationDetail::new(
                    format!("workflows.{index}.validTo"),
                    "range",
                    "INVALID_RANGE",
                    "validTo must not be before validFrom; a window that closes \
                     before it opens selects no workflow ever",
                ));
            }
        }

        if seen.contains(&binding.priority) {
            details.push(ValidationDetail::new(
                format!("workflows.{index}.priority"),
                "unique",
                "DUPLICATE",
                format!(
                    "priority {} is used by more than one binding; the first match \
                     wins, so two at one priority make routing depend on row order",
                    binding.priority
                ),
            ));
        } else {
            seen.push(binding.priority);
        }
    }
}

pub fn validate_create(request: &CreateDocumentTypeRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded(
        &request.type_code,
        "typeCode",
        MAX_TYPE_CODE_LENGTH,
        true,
        &mut details,
    );
    bounded(&request.name, "name", MAX_NAME_LENGTH, true, &mut details);

    if let Some(category) = &request.category {
        bounded(category, "category", MAX_CODE_LENGTH, false, &mut details);
    }

    if let Some(entity) = &request.target_entity_type {
        bounded(
            entity,
            "targetEntityType",
            MAX_CODE_LENGTH,
            false,
            &mut details,
        );
        governable(entity, &mut details);
    }

    check_workflows(&request.workflows, &mut details);

    finish(details)
}

/// Refuses a `targetEntityType` this build cannot route a change to
/// (FR-MDM-010, [#255] AC3).
///
/// **Refused at save rather than discovered at submit**, which is
/// [ADR-0028](../../../../docs/architectures/adr/0028.%20A%20Definition%20Is%20Refused%20at%20Save%20Rather%20Than%20at%20Render.md)'s
/// rule about definitions applied to configuration: a type is written once and
/// used many times, and a value that governs nothing would make every document
/// of that type an ordinary document while its administrator believed otherwise.
///
/// The column has no `CHECK` and has held free text since `0015`, so the read
/// side stays lenient — `GovernedEntity::from_db` treats a value it cannot place
/// as *governs nothing*. This stops new ones arriving; it does not rewrite what
/// is already there.
fn governable(entity: &str, details: &mut Vec<ValidationDetail>) {
    if GovernedEntity::from_db(entity).is_none() {
        details.push(ValidationDetail::new(
            "targetEntityType",
            "enum",
            "ENTITY_NOT_GOVERNABLE",
            format!(
                "`{entity}` is not a record type changes can be routed through approval for; \
                 this build governs PARTY and FACILITY"
            ),
        ));
    }
}

pub fn validate_update(request: &UpdateDocumentTypeRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(name) = &request.name {
        bounded(name, "name", MAX_NAME_LENGTH, true, &mut details);
    }

    if let Some(Some(category)) = &request.category {
        bounded(category, "category", MAX_CODE_LENGTH, false, &mut details);
    }

    if let Some(Some(entity)) = &request.target_entity_type {
        bounded(
            entity,
            "targetEntityType",
            MAX_CODE_LENGTH,
            false,
            &mut details,
        );
        governable(entity, &mut details);
    }

    if let Some(workflows) = &request.workflows {
        check_workflows(workflows, &mut details);
    }

    // The precondition guards a replacement, so without one it guards nothing,
    // and a caller sending it has misread the contract (#728 AC6).
    if request.expected_workflows.is_some() && request.workflows.is_none() {
        details.push(ValidationDetail::new(
            "expectedWorkflows",
            "consistency",
            "REQUIRES_WORKFLOWS",
            "expectedWorkflows is the set a replacement was read against, so it is \
             sent only with workflows",
        ));
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

    fn binding(priority: i32) -> WorkflowBinding {
        WorkflowBinding {
            workflow_definition_id: Uuid::now_v7(),
            condition_expression: None,
            priority,
            valid_from: None,
            valid_to: None,
        }
    }

    fn create() -> CreateDocumentTypeRequest {
        CreateDocumentTypeRequest {
            type_code: "PURCHASE_REQUISITION".to_owned(),
            name: "Purchase requisition".to_owned(),
            description: None,
            category: Some("PROCUREMENT".to_owned()),
            form_id: None,
            list_id: None,
            default_security_level: None,
            retention_policy_id: None,
            target_entity_type: None,
            status: None,
            workflows: vec![binding(1)],
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
    fn requires_a_type_code() {
        let mut request = create();
        request.type_code = "  ".to_owned();

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "typeCode"));
    }

    #[test]
    fn refuses_a_type_code_longer_than_the_column() {
        let mut request = create();
        request.type_code = "T".repeat(MAX_TYPE_CODE_LENGTH + 1);

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "typeCode" && detail.code == "TOO_LONG"));
    }

    #[test]
    fn refuses_a_validity_window_that_closes_before_it_opens() {
        let mut request = create();
        let mut window = binding(2);
        window.valid_from = Some(chrono::NaiveDate::from_ymd_opt(2026, 12, 1).expect("a date"));
        window.valid_to = Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("a date"));
        request.workflows.push(window);

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.code == "INVALID_RANGE"));
    }

    #[test]
    fn refuses_two_bindings_at_one_priority() {
        // No storage constraint covers this: "first match wins" would silently
        // depend on row order, which is a routing decision nobody made.
        let mut request = create();
        request.workflows.push(binding(1));

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.code == "DUPLICATE" && detail.path == "workflows.1.priority"));
    }

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        let request = UpdateDocumentTypeRequest {
            name: None,
            description: None,
            category: None,
            form_id: None,
            list_id: None,
            default_security_level: None,
            retention_policy_id: None,
            target_entity_type: None,
            status: None,
            workflows: None,
            expected_workflows: None,
        };

        assert!(validate_update(&request).is_ok());
    }

    fn update(
        workflows: Option<Vec<WorkflowBinding>>,
        expected_workflows: Option<Vec<WorkflowBinding>>,
    ) -> UpdateDocumentTypeRequest {
        UpdateDocumentTypeRequest {
            name: None,
            description: None,
            category: None,
            form_id: None,
            list_id: None,
            default_security_level: None,
            retention_policy_id: None,
            target_entity_type: None,
            status: None,
            workflows,
            expected_workflows,
        }
    }

    #[test]
    fn expected_workflows_without_workflows_is_refused_at_its_path() {
        let refused =
            details(validate_update(&update(None, Some(vec![binding(1)]))).expect_err("refused"));

        assert_eq!(refused.len(), 1, "{refused:?}");
        assert_eq!(refused[0].path, "expectedWorkflows");
        assert_eq!(refused[0].code, "REQUIRES_WORKFLOWS");

        // `[]` is a precondition too, "I read none", and is refused the same.
        assert!(validate_update(&update(None, Some(Vec::new()))).is_err());
    }

    #[test]
    fn expected_workflows_with_workflows_or_absent_is_valid() {
        assert!(validate_update(&update(Some(vec![binding(1)]), Some(vec![binding(2)]))).is_ok());
        assert!(validate_update(&update(Some(Vec::new()), Some(Vec::new()))).is_ok());
        assert!(validate_update(&update(Some(vec![binding(1)]), None)).is_ok());
    }

    #[test]
    fn expected_workflows_is_not_checked_like_a_replacement() {
        // It is a read, not a write: two read bindings at one priority are a
        // stored state to compare with, not a routing decision to refuse.
        let duplicate = vec![binding(1), binding(1)];

        assert!(validate_update(&update(Some(Vec::new()), Some(duplicate))).is_ok());
    }

    #[test]
    fn null_expected_workflows_reads_as_absent_and_is_camel_case() {
        let null: UpdateDocumentTypeRequest =
            serde_json::from_value(serde_json::json!({ "expectedWorkflows": null }))
                .expect("null deserializes");
        assert!(null.expected_workflows.is_none());

        let sent: UpdateDocumentTypeRequest = serde_json::from_value(serde_json::json!({
            "workflows": [],
            "expectedWorkflows": [{ "workflowDefinitionId": Uuid::now_v7(), "priority": 3 }]
        }))
        .expect("the camelCase key deserializes");
        assert_eq!(sent.expected_workflows.expect("sent")[0].priority, 3);

        assert!(serde_json::from_value::<UpdateDocumentTypeRequest>(
            serde_json::json!({ "expected_workflows": [] })
        )
        .is_err());
    }

    #[test]
    fn the_same_bindings_in_any_order_are_the_same_set() {
        let stored = vec![full_binding(1), full_binding(2), full_binding(3)];
        let shuffled = vec![stored[2].clone(), stored[0].clone(), stored[1].clone()];

        assert!(same_bindings(&stored, &stored));
        assert!(same_bindings(&shuffled, &stored));
    }

    #[test]
    fn two_empty_sets_are_the_same_and_empty_is_not_one() {
        assert!(same_bindings(&[], &[]));
        assert!(!same_bindings(&[], &[full_binding(1)]));
        assert!(!same_bindings(&[full_binding(1)], &[]));
    }

    #[test]
    fn a_set_that_lost_or_gained_a_binding_is_not_the_same() {
        let stored = vec![full_binding(1), full_binding(2)];

        // The race #728 is about: read one, another administrator added one.
        assert!(!same_bindings(&stored[..1], &stored));
        assert!(!same_bindings(&stored, &stored[..1]));
    }

    #[test]
    fn one_field_that_differs_on_one_binding_is_a_mismatch() {
        type Edit = (&'static str, fn(&mut WorkflowBinding));
        let stored = vec![full_binding(1), full_binding(2)];
        let edits: [Edit; 8] = [
            ("revision", |b| b.workflow_definition_id = Uuid::now_v7()),
            ("condition", |b| {
                b.condition_expression = Some("{}".to_owned())
            }),
            ("condition cleared", |b| b.condition_expression = None),
            ("priority", |b| b.priority = 7),
            ("start", |b| b.valid_from = Some(date(2))),
            ("start cleared", |b| b.valid_from = None),
            ("end", |b| b.valid_to = Some(date(11))),
            ("end cleared", |b| b.valid_to = None),
        ];

        for (label, edit) in edits {
            for index in 0..stored.len() {
                let mut expected = stored.clone();
                edit(&mut expected[index]);

                assert!(
                    !same_bindings(&expected, &stored),
                    "{label} on binding {index}"
                );
                assert!(
                    !same_bindings(&stored, &expected),
                    "{label} on binding {index}, the other way"
                );
            }
        }
    }

    #[test]
    fn duplicates_count() {
        let a = full_binding(1);
        let b = full_binding(2);

        assert!(same_bindings(
            &[a.clone(), a.clone(), b.clone()],
            &[b.clone(), a.clone(), a.clone()]
        ));
        // Same length, same distinct members, different counts.
        assert!(!same_bindings(
            &[a.clone(), a.clone(), b.clone()],
            &[a.clone(), b.clone(), b.clone()]
        ));
        // One copy read is not two stored, nor the other way.
        assert!(!same_bindings(
            std::slice::from_ref(&a),
            &[a.clone(), a.clone()]
        ));
        assert!(!same_bindings(
            &[a.clone(), a.clone()],
            std::slice::from_ref(&a)
        ));
        // Two copies of one stored binding cannot both claim it.
        assert!(!same_bindings(&[a.clone(), a.clone()], &[a, b]));
    }

    fn date(month: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, month, 1).expect("a date")
    }

    /// A binding with all five fields set, so each can be edited alone.
    fn full_binding(priority: i32) -> WorkflowBinding {
        WorkflowBinding {
            workflow_definition_id: Uuid::now_v7(),
            condition_expression: Some(r#"{">":[{"var":"amount"},1000]}"#.to_owned()),
            priority,
            valid_from: Some(date(1)),
            valid_to: Some(date(12)),
        }
    }

    fn changed_indexes(sent: &[WorkflowBinding], stored: &[WorkflowBinding]) -> Vec<usize> {
        changed_bindings(sent, stored)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn a_binding_sent_back_unchanged_is_not_changed_in_any_order() {
        let stored = vec![full_binding(1), full_binding(2)];
        let reversed = vec![stored[1].clone(), stored[0].clone()];

        assert!(changed_indexes(&stored, &stored).is_empty());
        assert!(changed_indexes(&reversed, &stored).is_empty());
    }

    #[test]
    fn a_binding_that_differs_in_any_one_field_is_changed() {
        // All five fields, one at a time: matching on the revision alone would
        // let the last four through unchecked (#716 AC6).
        type Edit = (&'static str, fn(&mut WorkflowBinding));
        let stored = vec![full_binding(1)];
        let edits: [Edit; 7] = [
            ("revision", |b| b.workflow_definition_id = Uuid::now_v7()),
            ("condition", |b| {
                b.condition_expression = Some("{}".to_owned())
            }),
            ("condition cleared", |b| b.condition_expression = None),
            ("priority", |b| b.priority = 2),
            ("start", |b| b.valid_from = Some(date(2))),
            ("start cleared", |b| b.valid_from = None),
            ("end cleared", |b| b.valid_to = None),
        ];

        for (label, edit) in edits {
            let mut sent = stored.clone();
            edit(&mut sent[0]);

            assert_eq!(changed_indexes(&sent, &stored), vec![0], "{label}");
        }
    }

    #[test]
    fn a_changed_binding_is_reported_at_the_index_it_was_sent_at() {
        let stored = vec![full_binding(1), full_binding(2)];
        let mut replaced = stored[0].clone();
        replaced.workflow_definition_id = Uuid::now_v7();

        assert_eq!(
            changed_indexes(&[replaced.clone(), stored[1].clone()], &stored),
            vec![0]
        );
        assert_eq!(
            changed_indexes(&[stored[1].clone(), replaced], &stored),
            vec![1]
        );
    }

    #[test]
    fn with_nothing_stored_every_binding_is_changed() {
        let sent = vec![full_binding(1), full_binding(2)];

        assert_eq!(changed_indexes(&sent, &[]), vec![0, 1]);
    }

    #[test]
    fn a_reference_left_out_or_cleared_names_nothing_to_check() {
        let stored = Some(Uuid::now_v7());

        assert_eq!(changed_reference(None, stored), None);
        assert_eq!(changed_reference(Some(None), stored), None);
        assert_eq!(changed_reference(Some(None), None), None);
    }

    #[test]
    fn a_reference_sent_at_its_stored_value_is_not_checked() {
        let stored = Uuid::now_v7();

        assert_eq!(changed_reference(Some(Some(stored)), Some(stored)), None);
    }

    #[test]
    fn a_changed_reference_is_checked() {
        let sent = Uuid::now_v7();

        assert_eq!(
            changed_reference(Some(Some(sent)), Some(Uuid::now_v7())),
            Some(sent),
            "another id stored"
        );
        assert_eq!(
            changed_reference(Some(Some(sent)), None),
            Some(sent),
            "nothing stored"
        );
    }

    #[test]
    fn an_unknown_security_level_reads_as_the_most_restrictive() {
        // The opposite direction from the other enums, and deliberately: this
        // one classifies content, so a value nobody recognises must not be read
        // as PUBLIC.
        assert_eq!(SecurityLevel::from_db("PUBLIC"), SecurityLevel::Public);
        assert_eq!(SecurityLevel::from_db("SECRET"), SecurityLevel::Restricted);
    }
}
