//! List definitions — configured list screens (FR-RAD-003).
//!
//! A list is its own row plus two ordered child collections, columns and
//! filters, and the three are one thing: a list with no columns renders an
//! empty table, so they are created together and **replaced wholesale on
//! update**, the same convention `roleIds` follows on a user. Patching a single
//! column by index would make the client's idea of the order load-bearing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Longest `listKey` §5.6 holds — `list_key VARCHAR(64)`.
pub const MAX_LIST_KEY_LENGTH: usize = 64;
/// Longest `title` and `label` §5.6–§5.8 hold — `VARCHAR(200)`.
pub const MAX_TITLE_LENGTH: usize = 200;
/// Longest `columnKey` / `filterKey` §5.7–§5.8 hold — `VARCHAR(64)`.
pub const MAX_KEY_LENGTH: usize = 64;

/// The page-size window `ck_rad_lists_page_size` enforces.
///
/// Checked here as well as in the database so the caller gets a 422 naming the
/// field rather than a constraint violation surfacing as a 500. The database
/// keeps its copy because a list definition is configuration and may be written
/// by something that is not this API.
pub const MIN_PAGE_SIZE: i32 = 1;
pub const MAX_PAGE_SIZE: i32 = 100;

/// Where a list definition is in its life (§5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ListStatus {
    Draft,
    Active,
    Deprecated,
}

impl ListStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Active => "ACTIVE",
            Self::Deprecated => "DEPRECATED",
        }
    }

    /// An unknown stored value reads as `Deprecated`, for the reason
    /// `FormStatus::from_db` gives: fail closed rather than 500.
    pub fn from_db(value: &str) -> Self {
        match value {
            "DRAFT" => Self::Draft,
            "ACTIVE" => Self::Active,
            _ => Self::Deprecated,
        }
    }
}

/// What a filter control is (§5.8's `CHECK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FilterType {
    Text,
    Enum,
    Lookup,
    DateRange,
    NumberRange,
    Boolean,
}

impl FilterType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Enum => "ENUM",
            Self::Lookup => "LOOKUP",
            Self::DateRange => "DATE_RANGE",
            Self::NumberRange => "NUMBER_RANGE",
            Self::Boolean => "BOOLEAN",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "TEXT" => Self::Text,
            "ENUM" => Self::Enum,
            "LOOKUP" => Self::Lookup,
            "DATE_RANGE" => Self::DateRange,
            "NUMBER_RANGE" => Self::NumberRange,
            "BOOLEAN" => Self::Boolean,
            _ => return None,
        })
    }
}

/// A column on the wire, in and out.
///
/// One type for both directions because a column has no server-assigned field —
/// no id a client would need back, since a column is addressed by its key
/// within its list. Two near-identical types would be two places to add the
/// next property to.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListColumnInput {
    pub column_key: String,
    pub label: String,
    pub data_type: Option<String>,
    pub format: Option<String>,
    #[serde(default = "default_true")]
    pub is_sortable: bool,
    pub width: Option<String>,
}

/// A filter on the wire, in and out.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListFilterInput {
    pub filter_key: String,
    pub label: String,
    pub filter_type: FilterType,
    pub options_json: Option<Value>,
    #[serde(default)]
    pub is_default: bool,
}

fn default_true() -> bool {
    true
}

/// A list definition as the API returns it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDefinition {
    pub id: Uuid,
    pub list_key: String,
    pub title: String,
    pub entity_id: Option<Uuid>,
    pub default_sort: Option<Value>,
    pub page_size: i32,
    pub status: ListStatus,
    /// In `sortOrder`, which is storage's business and not the caller's — the
    /// caller sent an ordered array and gets an ordered array back.
    pub columns: Vec<ListColumnInput>,
    pub filters: Vec<ListFilterInput>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A list on a list-of-lists screen: without its children.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSummary {
    pub id: Uuid,
    pub list_key: String,
    pub title: String,
    pub entity_id: Option<Uuid>,
    pub page_size: i32,
    pub status: ListStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateListRequest {
    pub list_key: String,
    pub title: String,
    pub entity_id: Option<Uuid>,
    pub default_sort: Option<Value>,
    pub page_size: Option<i32>,
    pub status: Option<ListStatus>,
    #[serde(default)]
    pub columns: Vec<ListColumnInput>,
    #[serde(default)]
    pub filters: Vec<ListFilterInput>,
}

/// Editing a list definition. `None` means *leave alone*; a collection that
/// **is** sent replaces the stored set wholesale.
///
/// `listKey` is absent because it may not change: it is what a menu route and a
/// document type name a list by.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateListRequest {
    pub title: Option<String>,
    pub entity_id: Option<Option<Uuid>>,
    pub default_sort: Option<Option<Value>>,
    pub page_size: Option<i32>,
    pub status: Option<ListStatus>,
    pub columns: Option<Vec<ListColumnInput>>,
    pub filters: Option<Vec<ListFilterInput>>,
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

fn check_page_size(page_size: i32, details: &mut Vec<ValidationDetail>) {
    if !(MIN_PAGE_SIZE..=MAX_PAGE_SIZE).contains(&page_size) {
        details.push(ValidationDetail::new(
            "pageSize",
            "range",
            "OUT_OF_RANGE",
            format!("pageSize must be between {MIN_PAGE_SIZE} and {MAX_PAGE_SIZE}"),
        ));
    }
}

/// Columns and filters, checked for the two things storage enforces and the
/// caller cannot see: bounded keys and no duplicates within the list.
///
/// The duplicate check is here rather than left to
/// `uq_rad_list_columns_list_id_column_key`, because a unique-index violation
/// arrives after several rows are already written and reads as a 500. Two
/// columns with one key is also a mistake the caller can fix, which is what a
/// 422 is for.
fn check_columns(columns: &[ListColumnInput], details: &mut Vec<ValidationDetail>) {
    let mut seen = Vec::new();

    for (index, column) in columns.iter().enumerate() {
        bounded(
            &column.column_key,
            &format!("columns.{index}.columnKey"),
            MAX_KEY_LENGTH,
            true,
            details,
        );
        bounded(
            &column.label,
            &format!("columns.{index}.label"),
            MAX_TITLE_LENGTH,
            true,
            details,
        );

        let key = column.column_key.trim();

        if seen.contains(&key) {
            details.push(ValidationDetail::new(
                format!("columns.{index}.columnKey"),
                "unique",
                "DUPLICATE",
                format!("`{key}` appears more than once in this list"),
            ));
        } else {
            seen.push(key);
        }
    }
}

fn check_filters(filters: &[ListFilterInput], details: &mut Vec<ValidationDetail>) {
    let mut seen = Vec::new();

    for (index, filter) in filters.iter().enumerate() {
        bounded(
            &filter.filter_key,
            &format!("filters.{index}.filterKey"),
            MAX_KEY_LENGTH,
            true,
            details,
        );
        bounded(
            &filter.label,
            &format!("filters.{index}.label"),
            MAX_TITLE_LENGTH,
            true,
            details,
        );

        let key = filter.filter_key.trim();

        if seen.contains(&key) {
            details.push(ValidationDetail::new(
                format!("filters.{index}.filterKey"),
                "unique",
                "DUPLICATE",
                format!("`{key}` appears more than once in this list"),
            ));
        } else {
            seen.push(key);
        }
    }
}

pub fn validate_create_list(request: &CreateListRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    bounded(
        &request.list_key,
        "listKey",
        MAX_LIST_KEY_LENGTH,
        true,
        &mut details,
    );
    bounded(
        &request.title,
        "title",
        MAX_TITLE_LENGTH,
        true,
        &mut details,
    );

    if let Some(page_size) = request.page_size {
        check_page_size(page_size, &mut details);
    }

    check_columns(&request.columns, &mut details);
    check_filters(&request.filters, &mut details);

    finish(details)
}

pub fn validate_update_list(request: &UpdateListRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    if let Some(title) = &request.title {
        bounded(title, "title", MAX_TITLE_LENGTH, true, &mut details);
    }

    if let Some(page_size) = request.page_size {
        check_page_size(page_size, &mut details);
    }

    if let Some(columns) = &request.columns {
        check_columns(columns, &mut details);
    }

    if let Some(filters) = &request.filters {
        check_filters(filters, &mut details);
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

    fn column(key: &str) -> ListColumnInput {
        ListColumnInput {
            column_key: key.to_owned(),
            label: "Column".to_owned(),
            data_type: Some("STRING".to_owned()),
            format: None,
            is_sortable: true,
            width: None,
        }
    }

    fn filter(key: &str) -> ListFilterInput {
        ListFilterInput {
            filter_key: key.to_owned(),
            label: "Filter".to_owned(),
            filter_type: FilterType::Text,
            options_json: None,
            is_default: false,
        }
    }

    fn create() -> CreateListRequest {
        CreateListRequest {
            list_key: "purchase-requisitions".to_owned(),
            title: "Purchase requisitions".to_owned(),
            entity_id: None,
            default_sort: Some(json!([{"key": "created_at", "dir": "desc"}])),
            page_size: Some(20),
            status: None,
            columns: vec![column("document_number")],
            filters: vec![filter("status")],
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
        assert!(validate_create_list(&create()).is_ok());
    }

    #[test]
    fn requires_a_list_key() {
        let mut request = create();
        request.list_key = "  ".to_owned();

        assert!(
            details(validate_create_list(&request).expect_err("refused"))
                .iter()
                .any(|detail| detail.path == "listKey")
        );
    }

    #[test]
    fn refuses_a_page_size_outside_the_stored_bounds() {
        for page_size in [0, -1, MAX_PAGE_SIZE + 1] {
            let mut request = create();
            request.page_size = Some(page_size);

            let details = details(validate_create_list(&request).expect_err("refused"));

            assert!(
                details
                    .iter()
                    .any(|detail| detail.path == "pageSize" && detail.code == "OUT_OF_RANGE"),
                "page size {page_size} must be refused by name, not by the database"
            );
        }
    }

    #[test]
    fn refuses_two_columns_with_one_key() {
        // Caught here rather than by the unique index, which fires after some
        // rows are written and arrives as a 500.
        let mut request = create();
        request.columns = vec![column("code"), column("code")];

        let details = details(validate_create_list(&request).expect_err("refused"));

        assert!(details
            .iter()
            .any(|detail| detail.code == "DUPLICATE" && detail.path == "columns.1.columnKey"));
    }

    #[test]
    fn refuses_two_filters_with_one_key() {
        let mut request = create();
        request.filters = vec![filter("status"), filter("status")];

        let details = details(validate_create_list(&request).expect_err("refused"));

        assert!(details
            .iter()
            .any(|detail| detail.code == "DUPLICATE" && detail.path == "filters.1.filterKey"));
    }

    #[test]
    fn a_list_with_no_columns_is_accepted() {
        // It renders an empty table, which is a configuration mistake and not a
        // malformed request — and a list is built up over several edits.
        let mut request = create();
        request.columns = Vec::new();

        assert!(validate_create_list(&request).is_ok());
    }

    #[test]
    fn an_update_that_changes_nothing_is_valid() {
        let request = UpdateListRequest {
            title: None,
            entity_id: None,
            default_sort: None,
            page_size: None,
            status: None,
            columns: None,
            filters: None,
        };

        assert!(validate_update_list(&request).is_ok());
    }

    #[test]
    fn an_unknown_stored_status_reads_as_deprecated() {
        assert_eq!(ListStatus::from_db("ACTIVE"), ListStatus::Active);
        assert_eq!(ListStatus::from_db("RETIRED"), ListStatus::Deprecated);
    }

    #[test]
    fn a_column_defaults_to_sortable() {
        // Matching `is_sortable BOOLEAN NOT NULL DEFAULT true`. A client that
        // omits the field gets the stored default rather than false, which is
        // what `#[serde(default)]` alone would have given.
        let parsed: ListColumnInput = serde_json::from_value(json!({
            "columnKey": "code",
            "label": "Code"
        }))
        .expect("parses");

        assert!(parsed.is_sortable);
    }
}
