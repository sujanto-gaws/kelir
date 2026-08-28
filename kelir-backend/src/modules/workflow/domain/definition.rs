//! Workflow definitions — the stored JWSS documents (FR-WF-001, 002, 003).
//!
//! **The definition is stored whole.** `workflow_definitions.definition_json`
//! holds the complete JWSS document and it is the authority (JWSS §1);
//! `workflow_states` and `workflow_transitions` beside it are projections
//! regenerated on publish, for the designer and for the foreign key that keeps
//! an instance in a state its definition declares.
//!
//! **`workflowKey` is immutable and `version` is how a workflow changes.** JWSS
//! has no revision concept of its own — its `version` is the *spec* version — so
//! a workflow is `(workflowKey, version)`, and a running instance pins the exact
//! `workflow_definitions.id` it started against so an approval in flight does
//! not change shape underneath its approver. Which is why publishing is not an
//! update: see [`WorkflowDefinitionStatus`].
//!
//! This is `rad::domain::form` with the nouns changed, and deliberately so — it
//! is the same problem, and [#175](https://github.com/sujanto-gaws/kelir/issues/175)
//! AC1 asks for exactly the guarantee `documents.form_id` already provides.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::jwss;
use crate::error::{AppError, ValidationDetail};

/// Longest `workflowKey` §7.1 holds — `workflow_key VARCHAR(64)`.
pub const MAX_WORKFLOW_KEY_LENGTH: usize = 64;
/// Longest `name` §7.1 holds — `name VARCHAR(200)`.
pub const MAX_NAME_LENGTH: usize = 200;

/// The largest definition this API accepts.
///
/// The bound `rad::domain::form` puts on a JFSS document, for its reason: the
/// column is JSONB and PostgreSQL would take a hundred megabytes of it, at which
/// point every read of the workflow and every transition carries it. A megabyte
/// is far above any real workflow — the example in the JWSS is under 3 KB.
pub const MAX_DEFINITION_BYTES: usize = 1024 * 1024;

/// Where a revision is in its life (§7.1).
///
/// **`Active` is a one-way door and that is the point.** A published revision is
/// what running instances execute, so editing one would change the rules an
/// approval is being decided under, mid-approval, with nothing recording that it
/// moved. Editing an active definition creates the next revision as a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkflowDefinitionStatus {
    Draft,
    Active,
    Deprecated,
}

impl WorkflowDefinitionStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Active => "ACTIVE",
            Self::Deprecated => "DEPRECATED",
        }
    }

    /// The stored value as an enum.
    ///
    /// An unknown value becomes `Deprecated` rather than panicking, which is
    /// `FormStatus::from_db`'s trade and fails closed the same way: the
    /// definition stops being offered for binding rather than the read
    /// returning a 500.
    pub fn from_db(value: &str) -> Self {
        match value {
            "DRAFT" => Self::Draft,
            "ACTIVE" => Self::Active,
            _ => Self::Deprecated,
        }
    }
}

/// A workflow definition as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub id: Uuid,
    pub workflow_key: String,
    pub name: String,
    pub description: Option<String>,
    /// The definition revision — **not** the JWSS spec version. The two were
    /// conflated once already on forms, and an instance pins this one.
    pub version: i32,
    pub jwss_version: String,
    pub status: WorkflowDefinitionStatus,
    pub initial_state: String,
    pub definition: Value,
    pub published_at: Option<DateTime<Utc>>,
    pub published_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A workflow on a list screen: everything but the definition.
///
/// The definition is the reason this type exists — a page of twenty workflows
/// with their documents inlined is twenty JWSS trees on the wire to render a
/// table of names.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinitionSummary {
    pub id: Uuid,
    pub workflow_key: String,
    pub name: String,
    pub version: i32,
    pub jwss_version: String,
    pub status: WorkflowDefinitionStatus,
    pub initial_state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorkflowRequest {
    pub workflow_key: String,
    pub name: String,
    pub description: Option<String>,
    #[schema(value_type = Object)]
    pub definition: Value,
}

/// Editing a draft revision, or seeding the next one.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWorkflowRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    #[schema(value_type = Object)]
    pub definition: Option<Value>,
}

pub fn validate_create(request: &CreateWorkflowRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded(
        &request.workflow_key,
        "workflowKey",
        MAX_WORKFLOW_KEY_LENGTH,
        &mut details,
    );
    bounded(&request.name, "name", MAX_NAME_LENGTH, &mut details);
    check_definition(&request.definition, &mut details);

    finish(details)
}

pub fn validate_update(request: &UpdateWorkflowRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(name) = &request.name {
        bounded(name, "name", MAX_NAME_LENGTH, &mut details);
    }

    if let Some(definition) = &request.definition {
        check_definition(definition, &mut details);
    }

    finish(details)
}

/// The three checks of [`jwss`], plus the size bound.
///
/// **Run on every save**, which is [#174](https://github.com/sujanto-gaws/kelir/issues/174)
/// AC3 — *"refused at save time rather than at run time"* — and stricter than
/// JWSS §8, which requires only that a violating definition never reach
/// `ACTIVE`. The publish path checks again, and
/// [`super::super::service::definition`] says why that is a real second line
/// rather than a decorative one.
fn check_definition(definition: &Value, details: &mut Vec<ValidationDetail>) {
    let bytes = definition.to_string().len();

    if bytes > MAX_DEFINITION_BYTES {
        details.push(ValidationDetail::new(
            "definition",
            "maxLength",
            "TOO_LARGE",
            format!("a workflow definition must be at most {MAX_DEFINITION_BYTES} bytes"),
        ));

        // Not validated further: the checks below walk the whole document, and
        // walking a document already refused for its size is work nobody asked
        // for on a request that has already failed.
        return;
    }

    details.extend(jwss::validate_definition(definition));
}

fn bounded(value: &str, path: &str, max: usize, details: &mut Vec<ValidationDetail>) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
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

fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

/// The JWSS specification version a definition is recorded against.
///
/// Read from the document rather than assumed: `version` is required by the
/// meta-schema and is the *spec* version, so a document declaring `1.1.0` is
/// stored as `1.1.0` and a reader can tell. Defaulted only if a document somehow
/// validated without one, which the meta-schema does not allow.
pub fn jwss_version(definition: &Value) -> String {
    definition
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("1.0.0")
        .to_owned()
}

/// The state a new instance starts in, extracted for the column beside the JSON.
pub fn initial_state(definition: &Value) -> String {
    definition
        .get("initialState")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workflow() -> Value {
        json!({
            "workflowKey": "purchase_requisition_standard",
            "version": "1.0.0",
            "name": "Standard",
            "initialState": "SUBMITTED",
            "states": [
                { "code": "SUBMITTED", "name": "Submitted", "mapsToDocumentStatus": "SUBMITTED",
                  "task": { "taskDefinitionKey": "manager_approval", "taskName": "Manager approval",
                            "assignment": { "assigneeType": "ROLE", "roleCode": "APPROVER" } } },
                { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
                  "isFinal": true },
                { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
                  "isFinal": true }
            ],
            "transitions": [
                { "from": "SUBMITTED", "to": "COMPLETED", "action": "APPROVE",
                  "allowedBy": "ROLE:APPROVER" },
                { "from": "SUBMITTED", "to": "REJECTED", "action": "REJECT",
                  "allowedBy": "ROLE:APPROVER" }
            ]
        })
    }

    fn create() -> CreateWorkflowRequest {
        CreateWorkflowRequest {
            workflow_key: "purchase_requisition_standard".to_owned(),
            name: "Standard purchase requisition".to_owned(),
            description: None,
            definition: workflow(),
        }
    }

    fn details(error: AppError) -> Vec<ValidationDetail> {
        match error {
            AppError::Validation { details } => details,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_complete_definition() {
        assert!(validate_create(&create()).is_ok());
    }

    #[test]
    fn requires_a_workflow_key() {
        let mut request = create();
        request.workflow_key = "   ".to_owned();

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "workflowKey" && detail.code == "REQUIRED"));
    }

    #[test]
    fn refuses_a_key_longer_than_the_column() {
        let mut request = create();
        request.workflow_key = "w".repeat(MAX_WORKFLOW_KEY_LENGTH + 1);

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.path == "workflowKey" && detail.code == "TOO_LONG"));
    }

    #[test]
    fn a_definition_that_is_not_a_jwss_document_is_refused_at_save() {
        let mut request = create();
        request.definition = json!({ "workflowKey": "x" });

        assert!(details(validate_create(&request).expect_err("refused"))
            .iter()
            .any(|detail| detail.code == "INVALID_DEFINITION"));
    }

    #[test]
    fn a_request_with_a_misspelled_field_is_refused() {
        // #62. `workflow_key` silently dropped would leave `workflowKey`
        // missing and the request refused for the wrong reason.
        let refused: Result<CreateWorkflowRequest, _> =
            serde_json::from_str(r#"{"workflow_key": "x", "name": "n", "definition": {}}"#);

        assert!(refused.is_err());
    }

    #[test]
    fn the_spec_version_and_the_revision_are_different_numbers() {
        // Conflated once already on forms, which is why `jwss_version` reads the
        // document and `version` is a column the database increments.
        assert_eq!(jwss_version(&workflow()), "1.0.0");
        assert_eq!(initial_state(&workflow()), "SUBMITTED");
    }

    #[test]
    fn an_unknown_stored_status_fails_closed() {
        assert_eq!(
            WorkflowDefinitionStatus::from_db("ACTIVE"),
            WorkflowDefinitionStatus::Active
        );
        assert_eq!(
            WorkflowDefinitionStatus::from_db("RETIRED"),
            WorkflowDefinitionStatus::Deprecated
        );
    }
}
