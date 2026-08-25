//! Form definitions — the stored JFSS documents (FR-RAD-002).
//!
//! **The definition is stored whole.** `rad_forms.definition_json` holds the
//! complete JFSS document and the component tree is the authority (Database
//! Schema §5). The projection tables beside it exist so the builder and
//! reporting can query fields relationally; nothing in this sprint writes them,
//! and a projection written from a definition nothing renders yet would be a
//! second source of truth with no reader.
//!
//! **`formKey` is immutable and `revision` is how a form changes.** JFSS has no
//! revision concept of its own — its `version` is the spec version — so a form
//! is `(formKey, revision)`, and documents pin the exact `rad_forms.id` they
//! were created against so an old document re-renders against the definition it
//! was filled in with. Which is why publishing is not an update: see
//! [`FormStatus`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::jfss;
use crate::error::{AppError, ValidationDetail};
use crate::utils::serde::present_or_absent;

/// Longest `formKey` §5.3 holds — `form_key VARCHAR(64)`.
pub const MAX_FORM_KEY_LENGTH: usize = 64;
/// Longest `title` §5.3 holds — `title VARCHAR(200)`.
pub const MAX_TITLE_LENGTH: usize = 200;

/// The largest definition this API accepts.
///
/// A bound rather than none, because `definition_json` is JSONB and PostgreSQL
/// would happily take a hundred megabytes of it — at which point every read of
/// the form, every render and every server-side re-evaluation carries it. One
/// megabyte is far above any real form: the largest example in the JFSS
/// documents is under 8 KB.
pub const MAX_DEFINITION_BYTES: usize = 1024 * 1024;

/// Where a revision is in its life (§5.3).
///
/// **`Published` is a one-way door and that is the point.** A published
/// revision is immutable: documents pin the exact revision they were created
/// against, so editing one would change what an old document renders as, years
/// later, with nothing recording that it moved. Editing a published form
/// creates the next revision as a draft instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormStatus {
    Draft,
    Published,
    Deprecated,
}

impl FormStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Published => "PUBLISHED",
            Self::Deprecated => "DEPRECATED",
        }
    }

    /// The stored value as an enum.
    ///
    /// An unknown value becomes `Deprecated` rather than panicking: the column
    /// carries a `CHECK`, so an unknown value means the check was changed
    /// without this enum, and answering "deprecated" fails closed — the form
    /// stops being offered rather than the read returning a 500.
    pub fn from_db(value: &str) -> Self {
        match value {
            "DRAFT" => Self::Draft,
            "PUBLISHED" => Self::Published,
            _ => Self::Deprecated,
        }
    }
}

/// A form definition as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Form {
    pub id: Uuid,
    pub form_key: String,
    pub title: String,
    pub revision: i32,
    /// The JFSS specification version `definition` conforms to, not the
    /// revision — those are different numbers and were conflated once already.
    pub jfss_version: String,
    pub status: FormStatus,
    pub entity_id: Option<Uuid>,
    pub definition: Value,
    pub published_at: Option<DateTime<Utc>>,
    pub published_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A form on a list screen: everything but the definition.
///
/// The definition is the reason this type exists. A page of twenty forms with
/// their documents inlined is twenty JFSS trees on the wire to render a table
/// of titles.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormSummary {
    pub id: Uuid,
    pub form_key: String,
    pub title: String,
    pub revision: i32,
    pub jfss_version: String,
    pub status: FormStatus,
    pub entity_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Creating a form definition.
///
/// `revision` is absent: a create is revision 1, and a later revision comes
/// from publishing and editing rather than from a caller choosing a number.
/// `status` is absent for the same reason — a definition is created as a draft,
/// and `publish` is the route that changes that.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateFormRequest {
    pub form_key: String,
    pub title: String,
    pub entity_id: Option<Uuid>,
    /// The complete JFSS document.
    pub definition: Value,
}

/// Editing a draft revision. Every field is optional and `None` means *leave
/// alone*, the convention `UpdateFacilityRequest` uses.
///
/// `formKey` is absent because it may not change: it is the identity a document
/// pins and what `document_types.form_id` is chosen by.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateFormRequest {
    pub title: Option<String>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub entity_id: Option<Option<Uuid>>,
    pub definition: Option<Value>,
}

fn require_key(value: &str, path: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            format!("{path} is required"),
        ));
    } else if trimmed.chars().count() > MAX_FORM_KEY_LENGTH {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("{path} must be at most {MAX_FORM_KEY_LENGTH} characters"),
        ));
    }
}

fn require_title(value: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            "title",
            "required",
            "REQUIRED",
            "title is required",
        ));
    } else if trimmed.chars().count() > MAX_TITLE_LENGTH {
        details.push(ValidationDetail::new(
            "title",
            "maxLength",
            "TOO_LONG",
            format!("title must be at most {MAX_TITLE_LENGTH} characters"),
        ));
    }
}

/// Checks the definition's size before its shape.
///
/// Order matters: the meta-schema validator walks the whole document, so
/// running it first on a 100 MB payload does the expensive thing before
/// discovering the payload should never have been accepted.
fn check_definition(definition: &Value, details: &mut Vec<ValidationDetail>) {
    let encoded = definition.to_string();

    if encoded.len() > MAX_DEFINITION_BYTES {
        details.push(ValidationDetail::new(
            "definition",
            "maxLength",
            "TOO_LARGE",
            format!(
                "definition must be at most {MAX_DEFINITION_BYTES} bytes; this one is {}",
                encoded.len()
            ),
        ));

        return;
    }

    details.extend(jfss::validate_definition(definition));
}

pub fn validate_create_form(request: &CreateFormRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    require_key(&request.form_key, "formKey", &mut details);
    require_title(&request.title, &mut details);
    check_definition(&request.definition, &mut details);

    finish(details)
}

pub fn validate_update_form(request: &UpdateFormRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(title) = &request.title {
        require_title(title, &mut details);
    }

    if let Some(definition) = &request.definition {
        check_definition(definition, &mut details);
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
    use serde_json::json;

    use super::*;

    fn definition() -> Value {
        json!({
            "formId": "purchase-requisition",
            "version": "2.0.1",
            "components": [{
                "id": "quantity",
                "role": "data",
                "type": "number",
                "key": "quantity",
                "label": "Quantity",
                "validation": { "type": "number" }
            }]
        })
    }

    fn create() -> CreateFormRequest {
        CreateFormRequest {
            form_key: "purchase-requisition".to_owned(),
            title: "Purchase requisition".to_owned(),
            entity_id: None,
            definition: definition(),
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
        assert!(validate_create_form(&create()).is_ok());
    }

    #[test]
    fn requires_a_form_key() {
        let mut request = create();
        request.form_key = "   ".to_owned();

        let details = details(validate_create_form(&request).expect_err("refused"));

        assert!(details.iter().any(|detail| detail.path == "formKey"));
    }

    #[test]
    fn refuses_a_form_key_longer_than_the_column() {
        let mut request = create();
        request.form_key = "k".repeat(MAX_FORM_KEY_LENGTH + 1);

        let details = details(validate_create_form(&request).expect_err("refused"));

        assert!(details
            .iter()
            .any(|detail| detail.path == "formKey" && detail.code == "TOO_LONG"));
    }

    #[test]
    fn refuses_a_definition_that_is_not_jfss() {
        let mut request = create();
        request.definition = json!({"nope": true});

        let details = details(validate_create_form(&request).expect_err("refused"));

        assert!(details
            .iter()
            .any(|detail| detail.code == "INVALID_DEFINITION"));
    }

    #[test]
    fn refuses_an_unregistered_operator() {
        let mut request = create();
        request.definition["components"][0]["calculate"] = json!({"flagd": ["x"]});

        let details = details(validate_create_form(&request).expect_err("refused"));

        assert!(details
            .iter()
            .any(|detail| detail.code == "OPERATOR_NOT_REGISTERED"));
    }

    #[test]
    fn refuses_an_oversized_definition_without_walking_it() {
        let mut request = create();
        request.definition = json!({ "blob": "x".repeat(MAX_DEFINITION_BYTES + 1) });

        let details = details(validate_create_form(&request).expect_err("refused"));

        assert_eq!(
            details.len(),
            1,
            "the size refusal stands alone; running the meta-schema over an \
             oversized payload does the expensive thing first"
        );
        assert_eq!(details[0].code, "TOO_LARGE");
    }

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        // Every field optional means an empty object is a legal no-op, and a
        // 422 for one would make a partial edit form awkward for no gain.
        let request = UpdateFormRequest {
            title: None,
            entity_id: None,
            definition: None,
        };

        assert!(validate_update_form(&request).is_ok());
    }

    #[test]
    fn an_update_validates_the_definition_it_carries() {
        let request = UpdateFormRequest {
            title: None,
            entity_id: None,
            definition: Some(json!({"nope": true})),
        };

        assert!(validate_update_form(&request).is_err());
    }

    #[test]
    fn an_unknown_stored_status_reads_as_deprecated() {
        // Fails closed: the form stops being offered rather than the read
        // returning a 500.
        assert_eq!(FormStatus::from_db("DRAFT"), FormStatus::Draft);
        assert_eq!(FormStatus::from_db("PUBLISHED"), FormStatus::Published);
        assert_eq!(FormStatus::from_db("RETIRED"), FormStatus::Deprecated);
    }
}
