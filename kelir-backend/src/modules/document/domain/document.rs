//! The document aggregate and the requests that move it (FR-DOC-001, 002, 005,
//! 006).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::link::{EntityLink, EntityType};
use super::metadata::{self, MetadataSet};
use super::status::DocumentStatus;
use crate::error::{AppError, ValidationDetail};
use crate::utils::serde::present_or_absent;

/// `documents.title` is `VARCHAR(200)`.
pub const MAX_TITLE_LENGTH: usize = 200;

/// The same bound `rad::domain::submission` puts on a submitted payload, for
/// the same reason and deliberately the same number: a document's form data
/// *is* a submitted payload, and two limits on one thing is a limit somebody
/// discovers by hitting the smaller one.
pub const MAX_FORM_DATA_BYTES: usize = crate::modules::rad::domain::submission::MAX_PAYLOAD_BYTES;

/// How urgent the requester says this is.
///
/// Storage carries it (§6.6) and Sprint 9 does nothing with it beyond recording
/// and filtering — the workflow that will route on it is Phase 5. It is here
/// rather than deferred because it is what the person creating the document
/// knows and nobody else will know later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentPriority {
    Low,
    Normal,
    High,
    Urgent,
}

impl DocumentPriority {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Normal => "NORMAL",
            Self::High => "HIGH",
            Self::Urgent => "URGENT",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "LOW" => Self::Low,
            "HIGH" => Self::High,
            "URGENT" => Self::Urgent,
            _ => Self::Normal,
        }
    }
}

/// A document, whole.
///
/// **`security_level` is deliberately absent from this struct.** The column
/// exists (§6.6) and nothing in Sprint 9 reads or writes it, because
/// FR-DTYPE-008 is the cut tail — see the visibility rule in
/// [`super::super::repository::list`]. Serializing a field the product does not
/// honour would be the overstatement #85 had to be narrowed out of the session
/// contract for.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: Uuid,
    /// The internal handle, `DOC-2026-000123`. Assigned at creation and never
    /// changed — it is what a draft is called before it has a number.
    pub document_ref: String,
    /// The business number, rendered by the type's numbering rule. `None` until
    /// a submit assigns it, which is the whole of [#168].
    ///
    /// [#168]: https://github.com/sujanto-gaws/kelir/issues/168
    pub document_number: Option<String>,
    pub document_type_id: Uuid,
    /// The exact form revision this document was created against, pinned at
    /// creation. This is what makes a published revision immutable *matter*: an
    /// old document re-renders through its own `form_id` rather than through
    /// whatever its type is bound to today (**D-30**).
    pub form_id: Option<Uuid>,
    pub title: String,
    pub status: DocumentStatus,
    pub priority: DocumentPriority,
    /// The server's answer, never the client's. Every write runs it through the
    /// Tamper-Proof Pattern first (JFSS S8.1, S10.2).
    pub form_data: Value,
    pub metadata: MetadataSet,
    /// The link's two halves, unresolved and always together. See
    /// [`super::link`] for why nothing is resolved here, and
    /// [`Document::link`] for the pair.
    pub entity_type: Option<EntityType>,
    pub entity_id: Option<Uuid>,
    pub requested_for_department_id: Option<Uuid>,
    pub requested_for_facility_id: Option<Uuid>,
    pub requested_by: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A document on a list screen: everything but the form data and the metadata.
///
/// The form data is the reason this type exists, and it is the reason
/// [`super::super::super::rad::domain::FormSummary`] exists too: a page of
/// twenty documents with their payloads inlined is twenty forms' worth of data
/// on the wire to render a table of titles and statuses.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSummary {
    pub id: Uuid,
    pub document_ref: String,
    pub document_number: Option<String>,
    pub document_type_id: Uuid,
    /// Denormalized onto the row for the list only. A list that made the client
    /// resolve twenty type ids to twenty names is twenty round trips to render
    /// one page, and the join is one.
    pub document_type_code: String,
    pub title: String,
    pub status: DocumentStatus,
    pub priority: DocumentPriority,
    pub entity_type: Option<EntityType>,
    pub entity_id: Option<Uuid>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Document {
    /// The link as a pair, when both halves are there.
    ///
    /// The columns are separate and nullable independently, so this is where
    /// "both or neither" is read back rather than assumed. A row holding one
    /// half is one the write path cannot produce and a hand-edit can, and the
    /// answer to it is "no link" rather than a panic.
    pub fn link(&self) -> Option<EntityLink> {
        match (self.entity_type, self.entity_id) {
            (Some(entity_type), Some(entity_id)) => Some(EntityLink {
                entity_type,
                entity_id,
            }),
            _ => None,
        }
    }
}

/// Creating a document from a type.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDocumentRequest {
    pub document_type_id: Uuid,
    pub title: String,
    /// Absent means an empty object, which is what "create the document, I will
    /// fill it in next" means and is the normal case from a screen.
    #[serde(default)]
    pub form_data: Option<Value>,
    #[serde(default)]
    pub metadata: Option<MetadataSet>,
    #[serde(default)]
    pub priority: Option<DocumentPriority>,
    /// Both halves or neither — [`super::link::check_pair`] is what turns that
    /// into a refusal rather than a half-written row.
    #[serde(default)]
    pub entity_type: Option<EntityType>,
    #[serde(default)]
    pub entity_id: Option<Uuid>,
    /// Required by a `DEPARTMENT_YEAR` numbering rule at submit time, and
    /// harmless before then. Accepted at creation so that the requirement is
    /// discovered when the type is chosen rather than at the moment of
    /// committing.
    #[serde(default)]
    pub requested_for_department_id: Option<Uuid>,
    #[serde(default)]
    pub requested_for_facility_id: Option<Uuid>,
}

/// Editing a draft.
///
/// Every member is `Option` and absent means *leave it alone*. There is no
/// `status` member and no `documentNumber` member, and `deny_unknown_fields` is
/// what refuses one: a transition is [`super::status`]'s route and a number is
/// #168's transaction, and letting an ordinary update carry either would put
/// both behind `document:update` — #99's AC1, which this project has now had to
/// state three times.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDocumentRequest {
    pub title: Option<String>,
    pub form_data: Option<Value>,
    /// Sent replaces the stored set; absent leaves it alone
    /// ([`super::metadata`]).
    pub metadata: Option<MetadataSet>,
    pub priority: Option<DocumentPriority>,
    /// Both halves or neither, and **sending both as `null` clears the link**.
    /// Absent leaves it alone, which is what `present_or_absent` is for and why
    /// a plain `Option` could not express this: `entityId: null` and no
    /// `entityId` at all are different instructions.
    #[serde(default, deserialize_with = "present_or_absent")]
    pub entity_type: Option<Option<EntityType>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub entity_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub requested_for_department_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    pub requested_for_facility_id: Option<Option<Uuid>>,
}

pub fn validate_create(request: &CreateDocumentRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    check_title(Some(&request.title), &mut details);

    if let Some(form_data) = &request.form_data {
        check_form_data(form_data, &mut details);
    }

    finish(details)?;

    if let Some(metadata) = &request.metadata {
        metadata::validate(metadata)?;
    }

    Ok(())
}

pub fn validate_update(request: &UpdateDocumentRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    check_title(request.title.as_deref(), &mut details);

    if let Some(form_data) = &request.form_data {
        check_form_data(form_data, &mut details);
    }

    finish(details)?;

    if let Some(metadata) = &request.metadata {
        metadata::validate(metadata)?;
    }

    Ok(())
}

fn check_title(title: Option<&str>, details: &mut Vec<ValidationDetail>) {
    let Some(title) = title else {
        return;
    };

    let trimmed = title.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            "title",
            "required",
            "REQUIRED",
            "a document's title is what it is called in every list it appears in",
        ));
        return;
    }

    // Counted in UTF-16 code units, matching the browser's `.length` — the same
    // reasoning `rad::domain::validation` gives for `maxLength`, so a title the
    // client thought fitted is not refused by a column that counted differently.
    if trimmed.encode_utf16().count() > MAX_TITLE_LENGTH {
        details.push(ValidationDetail::new(
            "title",
            "maxLength",
            "TOO_LONG",
            format!("Must be at most {MAX_TITLE_LENGTH} characters"),
        ));
    }
}

/// Refuses form data that is not an object, or is larger than a form could
/// legitimately collect.
///
/// **Checked before the re-evaluation rather than after**, which is the reason
/// `rad::service::submission::bounded` gives: the walk clones the scope once per
/// calculated field, so an unbounded payload is unbounded work as well as an
/// unbounded row.
fn check_form_data(form_data: &Value, details: &mut Vec<ValidationDetail>) {
    if !form_data.is_object() {
        details.push(ValidationDetail::new(
            "formData",
            "type",
            "PAYLOAD_NOT_AN_OBJECT",
            "form data is a JSON object keyed by the form's data keys (JFSS S10.1)",
        ));
        return;
    }

    let bytes = form_data.to_string().len();

    if bytes > MAX_FORM_DATA_BYTES {
        details.push(ValidationDetail::new(
            "formData",
            "maxLength",
            "PAYLOAD_TOO_LARGE",
            format!("the form data is {bytes} bytes and the limit is {MAX_FORM_DATA_BYTES}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create(title: &str) -> CreateDocumentRequest {
        CreateDocumentRequest {
            document_type_id: Uuid::now_v7(),
            title: title.to_owned(),
            form_data: None,
            metadata: None,
            priority: None,
            entity_type: None,
            entity_id: None,
            requested_for_department_id: None,
            requested_for_facility_id: None,
        }
    }

    #[test]
    fn a_blank_title_is_refused() {
        assert!(validate_create(&create("   ")).is_err());
    }

    #[test]
    fn a_title_is_measured_the_way_the_browser_measures_it() {
        // An emoji is one `char` and two UTF-16 code units. Counting `char`
        // here would admit a title the client's own maxLength refused, and
        // VARCHAR(200) counts characters — so this bound is the stricter of the
        // two on purpose, and the client and the server agree about which
        // titles are legal.
        let at_the_bound = "a".repeat(MAX_TITLE_LENGTH);
        assert!(validate_create(&create(&at_the_bound)).is_ok());

        let one_past = "a".repeat(MAX_TITLE_LENGTH + 1);
        assert!(validate_create(&create(&one_past)).is_err());
    }

    #[test]
    fn form_data_that_is_not_an_object_is_refused() {
        let mut request = create("A requisition");
        request.form_data = Some(json!([1, 2, 3]));

        let error = validate_create(&request).expect_err("an array is not form data");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert_eq!(details[0].code, "PAYLOAD_NOT_AN_OBJECT");
    }

    #[test]
    fn an_update_cannot_carry_a_status() {
        // #99 AC1, for the third time in this codebase. A transition has its
        // own permission and its own audit action, and `deny_unknown_fields` is
        // what stops an ordinary edit from reaching it.
        let refused: Result<UpdateDocumentRequest, _> =
            serde_json::from_str(r#"{"title": "x", "status": "APPROVED"}"#);

        assert!(refused.is_err());
    }

    #[test]
    fn an_update_cannot_carry_a_document_number() {
        // The other half: a number is #168's transaction, not a field.
        let refused: Result<UpdateDocumentRequest, _> =
            serde_json::from_str(r#"{"documentNumber": "PR-2026-000001"}"#);

        assert!(refused.is_err());
    }

    #[test]
    fn an_empty_update_is_a_legitimate_no_op_request() {
        // Every member absent means "change nothing", which is what a form that
        // was opened and closed sends. It must parse; whether the service does
        // anything with it is the service's question.
        let parsed: UpdateDocumentRequest =
            serde_json::from_str("{}").expect("an empty update parses");

        assert!(parsed.title.is_none());
        assert!(parsed.form_data.is_none());
    }

    #[test]
    fn the_form_data_bound_is_the_one_a_submission_already_had() {
        // Two limits on one thing is a limit somebody discovers by hitting the
        // smaller one. A document's form data *is* a submitted payload.
        assert_eq!(
            MAX_FORM_DATA_BYTES,
            crate::modules::rad::domain::submission::MAX_PAYLOAD_BYTES
        );
    }
}
