//! Document type definitions and their bindings (FR-DTYPE-001, 002, 003).
//!
//! **The bindings are columns on the type, not a separate resource.** A type
//! that cannot name the form it renders is a type nothing can use, which is why
//! #157 is one item rather than two: `formId` and `listId` are fields of the
//! create and update payloads, and the workflow binding is a collection on the
//! same aggregate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};
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
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowBinding {
    /// The workflow definition this type routes to.
    ///
    /// **Checked since [#187](https://github.com/sujanto-gaws/kelir/issues/187).**
    /// It must name a definition that exists in this tenant and is `ACTIVE`,
    /// verified in the write's transaction under a share lock on the row the
    /// check read (`service::check_workflow_bindings`), and `0025_workflow.sql`
    /// added the foreign key `0015_document.sql` deferred. Before Sprint 10 this
    /// comment said the id was unverifiable, which stopped being true the moment
    /// the workflow table existed — a comment describing a state that has ended
    /// is worse than no comment (**D-34**, one document over).
    pub workflow_definition_id: Uuid,
    /// `null` means this is the default binding.
    pub condition_expression: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    pub valid_from: Option<chrono::NaiveDate>,
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
    #[serde(default, deserialize_with = "present_or_absent")]
    pub form_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub list_id: Option<Option<Uuid>>,
    pub default_security_level: Option<SecurityLevel>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub retention_policy_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub target_entity_type: Option<Option<String>>,
    pub status: Option<DocumentTypeStatus>,
    pub workflows: Option<Vec<WorkflowBinding>>,
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
    }

    check_workflows(&request.workflows, &mut details);

    finish(details)
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
    }

    if let Some(workflows) = &request.workflows {
        check_workflows(workflows, &mut details);
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
        };

        assert!(validate_update(&request).is_ok());
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
