//! Resolving a stored list definition against what a renderer can actually
//! serve (FR-RAD-003, FR-RAD-010, [#340]).
//!
//! **A list definition is generic and the thing it renders is not.**
//! `rad_lists` names columns and filters as free strings, because §5.6–§5.8
//! were written for lists over anything. What a rendered list currently *is*,
//! is documents: [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
//! §6.2 gives `document_types.list_id`, and §5.7's own example column keys are
//! `document_number` and `form_data.amount`. So between the definition and the
//! query there has to be a step that says *this column names something I can
//! fetch* — and this is it.
//!
//! # The failure is named, and it is never an empty table
//!
//! **This is [#340] AC4 and it is the whole reason this module is separate from
//! the query.** A column key nothing can resolve, a filter the documents query
//! has no parameter for, a sort naming a column that cannot be ordered — each
//! of those, left alone, renders as *a table with no rows*, which is
//! indistinguishable from a tenant that has not created any documents yet. That
//! is [#326](https://github.com/sujanto-gaws/kelir/issues/326)'s failure in a
//! different panel: a screen that says nothing while being wrong.
//!
//! So every one of them is a [`ValidationDetail`] naming the key, and the whole
//! definition is checked rather than the first problem — the same reason
//! `validate_definition` gives about a form: somebody with three mistakes
//! should learn all three at once.
//!
//! # Refused at render, not at save
//!
//! The opposite of [#338](https://github.com/sujanto-gaws/kelir/issues/338),
//! and deliberately. A *form* definition is refused when it is written, because
//! JFSS is what a form means and the check is the form's own. A **list**
//! definition's column keys mean something only once you know what the list is
//! over, and `rad_lists` does not know: refusing `form_data.amount` at the list
//! storage API would put the document module's vocabulary inside RAD's write
//! path and make a generic table un-writable for anything but documents.
//!
//! The cost is stated rather than hidden: a broken list definition is stored
//! happily and fails when somebody opens it. What makes that acceptable is that
//! it fails *loudly, naming the key* — and that the builder ([#341]) will call
//! this same function to show the author the problem before they save.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340
//! [#341]: https://github.com/sujanto-gaws/kelir/issues/341

use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use super::list::{FilterType, ListDefinition, ListStatus};
use crate::error::ValidationDetail;
use crate::modules::document::domain::{DocumentSort, DocumentSortKey, SortRefusal};

/// The prefix that says a column reads the document's form payload.
///
/// `form_data.` and not `formData.`: §5.7's example is `form_data.amount`, and
/// the segment after it is a **JFSS data key**, which is the author's own
/// spelling and is not transformed. A column key is not a wire field name.
const FORM_DATA: &str = "form_data.";

/// A field of the document row itself.
///
/// Every one of these is a column of `DocumentSummary`, which is what the list
/// query returns. The set is closed: a column key outside it and outside
/// [`FORM_DATA`] is a definition this renderer cannot serve, and saying so is
/// better than a blank cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryField {
    Id,
    DocumentRef,
    DocumentNumber,
    DocumentTypeId,
    DocumentTypeCode,
    Title,
    Status,
    Priority,
    EntityType,
    EntityId,
    SubmittedAt,
    CreatedAt,
    UpdatedAt,
}

impl SummaryField {
    /// The wire name, which is what the column key spells.
    pub fn as_name(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::DocumentRef => "documentRef",
            Self::DocumentNumber => "documentNumber",
            Self::DocumentTypeId => "documentTypeId",
            Self::DocumentTypeCode => "documentTypeCode",
            Self::Title => "title",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::EntityType => "entityType",
            Self::EntityId => "entityId",
            Self::SubmittedAt => "submittedAt",
            Self::CreatedAt => "createdAt",
            Self::UpdatedAt => "updatedAt",
        }
    }

    /// The `snake_case` spelling §5.7's example uses.
    pub fn as_column(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::DocumentRef => "document_ref",
            Self::DocumentNumber => "document_number",
            Self::DocumentTypeId => "document_type_id",
            Self::DocumentTypeCode => "document_type_code",
            Self::Title => "title",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::EntityType => "entity_type",
            Self::EntityId => "entity_id",
            Self::SubmittedAt => "submitted_at",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
        }
    }

    pub const ALL: [Self; 13] = [
        Self::Id,
        Self::DocumentRef,
        Self::DocumentNumber,
        Self::DocumentTypeId,
        Self::DocumentTypeCode,
        Self::Title,
        Self::Status,
        Self::Priority,
        Self::EntityType,
        Self::EntityId,
        Self::SubmittedAt,
        Self::CreatedAt,
        Self::UpdatedAt,
    ];

    /// Both spellings resolve, for the reason [`DocumentSortKey::parse`] gives:
    /// the schema's own example spells a column key `document_number`, so a
    /// definition written against the documentation must not be refused.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();

        Self::ALL
            .into_iter()
            .find(|field| field.as_name() == value || field.as_column() == value)
    }
}

/// Where one column's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellSource {
    /// A field of the document row.
    Summary(SummaryField),
    /// A path into `form_data_json`, dot-separated — the part after
    /// `form_data.`.
    FormData(String),
}

/// One column, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedColumn {
    /// The key the definition declared, which is what a cell is addressed by on
    /// the wire. Kept verbatim so the renderer's cells line up with the
    /// definition's columns without a second spelling rule.
    pub key: String,
    pub source: CellSource,
    /// Whether this column may be sorted on. A `form_data.*` column never can:
    /// ordering by a JSONB path needs an index that does not exist and an
    /// identifier in the `ORDER BY` that [`DocumentSort`] exists to prevent.
    pub sortable: bool,
}

/// A filter parameter the documents query understands.
///
/// **The vocabulary is `DocumentQuery`'s, minus `documentTypeId`.** That one is
/// decided by the binding — the rows *are* the documents of the types that name
/// this list — so a definition declaring it would be offering a control that
/// fights the list's own identity, and the narrower of the two would silently
/// win.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterParameter {
    Search,
    Status,
    Priority,
    EntityType,
    EntityId,
}

impl FilterParameter {
    pub fn as_name(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::EntityType => "entityType",
            Self::EntityId => "entityId",
        }
    }

    fn as_column(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Status => "status",
            Self::Priority => "priority",
            Self::EntityType => "entity_type",
            Self::EntityId => "entity_id",
        }
    }

    pub const ALL: [Self; 5] = [
        Self::Search,
        Self::Status,
        Self::Priority,
        Self::EntityType,
        Self::EntityId,
    ];

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();

        Self::ALL
            .into_iter()
            .find(|parameter| parameter.as_name() == value || parameter.as_column() == value)
    }

    fn allowed() -> String {
        Self::ALL
            .iter()
            .map(|parameter| format!("`{}`", parameter.as_name()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One filter, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFilter {
    pub key: String,
    pub parameter: FilterParameter,
}

/// A definition, resolved into everything the rows query needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPlan {
    pub columns: Vec<PlannedColumn>,
    pub filters: Vec<PlannedFilter>,
    /// The definition's own `default_sort_json`, or newest-first where it
    /// declares none.
    pub sort: DocumentSort,
    /// Whether any column reads the form payload. The query fetches
    /// `form_data_json` only when this holds, so a list of plain document
    /// columns does not pay for payloads nothing reads.
    pub needs_form_data: bool,
}

/// A column as the render contract puts it on the wire.
///
/// The definition's own column, plus the one thing the definition does not know
/// and the renderer needs: whether this particular column can be sorted on
/// *here*. `is_sortable` in the definition is the author's intent; `sortable`
/// is that intent resolved against what the query can do.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderableColumn {
    pub key: String,
    pub label: String,
    pub data_type: Option<String>,
    pub format: Option<String>,
    pub width: Option<String>,
    pub sortable: bool,
}

/// A filter as the render contract puts it on the wire.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderableFilter {
    pub key: String,
    pub label: String,
    pub filter_type: FilterType,
    pub options: Option<Value>,
    pub is_default: bool,
    /// The query parameter this filter sets. On the wire because the renderer
    /// sends it back on the rows request, and because a filter whose key and
    /// parameter differ would otherwise need the client to know the mapping.
    pub parameter: String,
}

/// What a renderer is given for one list.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenderableList {
    pub id: uuid::Uuid,
    pub list_key: String,
    pub title: String,
    pub page_size: i32,
    pub columns: Vec<RenderableColumn>,
    pub filters: Vec<RenderableFilter>,
    /// The column the list opens ordered by, and the direction — resolved, so a
    /// renderer never parses `default_sort_json` itself.
    pub default_sort_key: String,
    pub default_sort_descending: bool,
}

/// The S10.3 codes a definition this renderer cannot serve carries.
pub const COLUMN_NOT_RENDERABLE: &str = "COLUMN_NOT_RENDERABLE";
pub const FILTER_NOT_RENDERABLE: &str = "FILTER_NOT_RENDERABLE";
pub const SORT_NOT_RENDERABLE: &str = "SORT_NOT_RENDERABLE";
pub const LIST_HAS_NO_COLUMNS: &str = "LIST_HAS_NO_COLUMNS";

/// Resolves a definition, or every reason it cannot be rendered.
pub fn plan(definition: &ListDefinition) -> Result<RenderPlan, Vec<ValidationDetail>> {
    let mut details = Vec::new();
    let mut columns = Vec::new();

    // **A list with no columns is refused rather than rendered.** `list.rs`'s
    // own module doc says a list with no columns renders an empty table, and an
    // empty table is precisely the answer AC4 refuses to accept — it reads as
    // "no documents" to everybody who did not write the definition.
    if definition.columns.is_empty() {
        details.push(ValidationDetail::new(
            "columns",
            "required",
            LIST_HAS_NO_COLUMNS,
            "this list declares no columns, so it would render as an empty table — which \
             reads as `no documents` rather than as a definition that is not finished",
        ));
    }

    for (index, column) in definition.columns.iter().enumerate() {
        let key = column.column_key.trim();

        let source = if let Some(path) = key.strip_prefix(FORM_DATA) {
            if path.is_empty() {
                details.push(ValidationDetail::new(
                    format!("columns.{index}.columnKey"),
                    "columnKey",
                    COLUMN_NOT_RENDERABLE,
                    "`form_data.` names no field — the part after it is the JFSS data key \
                     the value is stored under",
                ));
                continue;
            }

            CellSource::FormData(path.to_owned())
        } else if let Some(field) = SummaryField::parse(key) {
            CellSource::Summary(field)
        } else {
            details.push(ValidationDetail::new(
                format!("columns.{index}.columnKey"),
                "columnKey",
                COLUMN_NOT_RENDERABLE,
                format!(
                    "`{key}` is neither a field of a document nor a `form_data.` path, so \
                     this column has nothing to show — a document field is one of {}",
                    summary_fields()
                ),
            ));
            continue;
        };

        // The author's intent, resolved against what the query can do: a
        // `form_data.*` column is never sortable however the definition marks
        // it, because ordering by a JSONB path needs an index nothing creates
        // and an identifier the static `ORDER BY` has no arm for. Nor is a
        // document field the sort allow-list leaves out — `entityId` displays
        // fine and orders a foreign key.
        let sortable = column.is_sortable
            && match &source {
                CellSource::Summary(field) => DocumentSortKey::parse(field.as_name()).is_some(),
                CellSource::FormData(_) => false,
            };

        columns.push(PlannedColumn {
            key: column.column_key.clone(),
            source,
            sortable,
        });
    }

    let filters = plan_filters(definition, &mut details);
    let sort = plan_sort(definition, &mut details);

    if details.is_empty() {
        Ok(RenderPlan {
            needs_form_data: columns
                .iter()
                .any(|column| matches!(column.source, CellSource::FormData(_))),
            columns,
            filters,
            sort: sort.unwrap_or_default(),
        })
    } else {
        Err(details)
    }
}

fn plan_filters(
    definition: &ListDefinition,
    details: &mut Vec<ValidationDetail>,
) -> Vec<PlannedFilter> {
    let mut filters = Vec::new();

    for (index, filter) in definition.filters.iter().enumerate() {
        let key = filter.filter_key.trim();

        let Some(parameter) = FilterParameter::parse(key) else {
            details.push(ValidationDetail::new(
                format!("filters.{index}.filterKey"),
                "filterKey",
                FILTER_NOT_RENDERABLE,
                format!(
                    "`{key}` is not something a document list can be filtered by, so this \
                     control would do nothing — the parameters are {}",
                    FilterParameter::allowed()
                ),
            ));
            continue;
        };

        filters.push(PlannedFilter {
            key: filter.filter_key.clone(),
            parameter,
        });
    }

    filters
}

/// `default_sort_json`, as §5.6 documents it: `[{"key":…,"dir":…}]`.
fn plan_sort(
    definition: &ListDefinition,
    details: &mut Vec<ValidationDetail>,
) -> Option<DocumentSort> {
    let declared = definition.default_sort.as_ref()?;

    // An explicit `null` is no declaration, which is different from a malformed
    // one and must not be reported as an error.
    if declared.is_null() {
        return None;
    }

    let Some(entries) = declared.as_array() else {
        details.push(ValidationDetail::new(
            "defaultSort",
            "defaultSort",
            SORT_NOT_RENDERABLE,
            "a default sort is an array of `{\"key\": …, \"dir\": …}` (Database Schema §5.6)",
        ));

        return None;
    };

    if entries.is_empty() {
        return None;
    }

    // **A second sort key is refused rather than dropped.** The rows query
    // orders by one column and a stable `id`; honouring the first entry and
    // ignoring the rest would give the author a list that looks sorted the way
    // they asked and is not, which is the silent half-honour this whole module
    // exists to avoid.
    if entries.len() > 1 {
        details.push(ValidationDetail::new(
            "defaultSort",
            "defaultSort",
            SORT_NOT_RENDERABLE,
            format!(
                "this list declares {} sort keys and a document list is ordered by one — \
                 the rest would be silently ignored",
                entries.len()
            ),
        ));

        return None;
    }

    let entry = &entries[0];
    let Some(key) = entry.get("key").and_then(Value::as_str) else {
        details.push(ValidationDetail::new(
            "defaultSort.0.key",
            "defaultSort",
            SORT_NOT_RENDERABLE,
            "a default sort entry names the column in `key` (Database Schema §5.6)",
        ));

        return None;
    };
    let direction = entry.get("dir").and_then(Value::as_str);

    match DocumentSort::parse(key, direction) {
        Ok(sort) => Some(sort),
        Err(refusal) => {
            let path = match refusal {
                SortRefusal::UnknownKey(_) => "defaultSort.0.key",
                SortRefusal::UnknownDirection(_) => "defaultSort.0.dir",
            };

            details.push(ValidationDetail::new(
                path,
                "defaultSort",
                SORT_NOT_RENDERABLE,
                refusal.to_string(),
            ));

            None
        }
    }
}

/// Whether a stored list is one a renderer should open at all.
///
/// **`ACTIVE` only.** `DRAFT` is a list somebody is still writing and
/// `DEPRECATED` is one a deployment has retired; rendering either would put a
/// screen in front of somebody that its owner does not consider ready or does
/// not consider current. This is the list counterpart of the published check
/// `submit_form` makes, and it is the *unpublished* half of AC4.
pub fn is_renderable(status: ListStatus) -> bool {
    status == ListStatus::Active
}

fn summary_fields() -> String {
    SummaryField::ALL
        .iter()
        .map(|field| format!("`{}`", field.as_name()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::modules::rad::domain::list::{ListColumnInput, ListFilterInput};

    fn column(key: &str) -> ListColumnInput {
        ListColumnInput {
            column_key: key.to_owned(),
            label: key.to_owned(),
            data_type: None,
            format: None,
            is_sortable: true,
            width: None,
        }
    }

    fn filter(key: &str) -> ListFilterInput {
        ListFilterInput {
            filter_key: key.to_owned(),
            label: key.to_owned(),
            filter_type: FilterType::Text,
            options_json: None,
            is_default: false,
        }
    }

    fn definition(columns: Vec<ListColumnInput>) -> ListDefinition {
        ListDefinition {
            id: uuid::Uuid::now_v7(),
            list_key: "requisitions".to_owned(),
            title: "Requisitions".to_owned(),
            entity_id: None,
            default_sort: None,
            page_size: 20,
            status: ListStatus::Active,
            columns,
            filters: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn codes(details: &[ValidationDetail]) -> Vec<&str> {
        details.iter().map(|detail| detail.code.as_str()).collect()
    }

    fn refusal(definition: &ListDefinition) -> Vec<ValidationDetail> {
        plan(definition).expect_err("the definition is refused")
    }

    // -- What resolves ----------------------------------------------------

    #[test]
    fn resolves_a_document_field_by_either_spelling() {
        // §5.7's own example spells a column key `document_number`; a builder
        // would write `documentNumber`. Refusing either would refuse a
        // definition somebody wrote from the documentation.
        let resolved = plan(&definition(vec![
            column("document_number"),
            column("documentNumber"),
        ]))
        .expect("both spellings resolve");

        assert_eq!(
            resolved.columns[0].source,
            CellSource::Summary(SummaryField::DocumentNumber)
        );
        assert_eq!(resolved.columns[0].source, resolved.columns[1].source);
    }

    #[test]
    fn a_columns_key_is_kept_verbatim_so_a_cell_can_be_found_by_it() {
        // The renderer walks the definition's columns and reads each cell by
        // the key it was given. Normalising the key here would mean the cells
        // came back under a name the definition never used.
        let resolved = plan(&definition(vec![column("document_number")])).expect("resolves");

        assert_eq!(resolved.columns[0].key, "document_number");
    }

    #[test]
    fn resolves_a_form_data_path_and_asks_for_the_payload() {
        let resolved = plan(&definition(vec![column("form_data.amount")])).expect("resolves");

        assert_eq!(
            resolved.columns[0].source,
            CellSource::FormData("amount".to_owned())
        );
        assert!(
            resolved.needs_form_data,
            "the query must fetch form_data_json when a column reads it"
        );
    }

    #[test]
    fn a_list_of_plain_columns_does_not_ask_for_the_payload() {
        // The whole point of the flag: a page of twenty documents should not
        // carry twenty form payloads to render a column of titles.
        let resolved = plan(&definition(vec![column("title")])).expect("resolves");

        assert!(!resolved.needs_form_data);
    }

    #[test]
    fn a_nested_form_data_path_keeps_every_segment() {
        let resolved =
            plan(&definition(vec![column("form_data.line_items.0.amount")])).expect("resolves");

        assert_eq!(
            resolved.columns[0].source,
            CellSource::FormData("line_items.0.amount".to_owned())
        );
    }

    // -- What is refused, and named --------------------------------------

    /// **AC4.** The failure mode this whole module exists to prevent is a table
    /// with no rows, which reads as *no documents*.
    #[test]
    fn refuses_a_column_key_nothing_can_resolve_and_names_it() {
        let details = refusal(&definition(vec![column("supplier_rating")]));

        assert_eq!(codes(&details), [COLUMN_NOT_RENDERABLE]);
        assert_eq!(details[0].path, "columns.0.columnKey");
        assert!(details[0].message.contains("supplier_rating"));
        // And says what it could have been instead.
        assert!(
            details[0].message.contains("documentNumber"),
            "{}",
            details[0].message
        );
    }

    #[test]
    fn refuses_a_form_data_prefix_with_no_path_after_it() {
        let details = refusal(&definition(vec![column("form_data.")]));

        assert_eq!(codes(&details), [COLUMN_NOT_RENDERABLE]);
    }

    #[test]
    fn refuses_a_list_that_declares_no_columns() {
        // `list.rs`'s own module doc says a list with no columns renders an
        // empty table. An empty table is the answer AC4 refuses.
        let details = refusal(&definition(Vec::new()));

        assert_eq!(codes(&details), [LIST_HAS_NO_COLUMNS]);
    }

    #[test]
    fn reports_every_bad_column_rather_than_the_first() {
        let details = refusal(&definition(vec![
            column("supplier_rating"),
            column("title"),
            column("approval_stage"),
        ]));

        assert_eq!(details.len(), 2, "{details:?}");
        assert_eq!(details[0].path, "columns.0.columnKey");
        assert_eq!(details[1].path, "columns.2.columnKey");
    }

    #[test]
    fn refuses_a_filter_the_documents_query_has_no_parameter_for() {
        let mut definition = definition(vec![column("title")]);
        definition.filters = vec![filter("supplierRating")];

        let details = refusal(&definition);

        assert_eq!(codes(&details), [FILTER_NOT_RENDERABLE]);
        assert_eq!(details[0].path, "filters.0.filterKey");
        assert!(details[0].message.contains("supplierRating"));
        assert!(
            details[0].message.contains("`search`"),
            "{}",
            details[0].message
        );
    }

    /// `documentTypeId` is the one parameter deliberately outside the
    /// vocabulary: the rows *are* the documents of the types that name this
    /// list, so a filter for it would fight the binding.
    #[test]
    fn refuses_a_filter_on_the_binding_the_list_already_is() {
        let mut definition = definition(vec![column("title")]);
        definition.filters = vec![filter("documentTypeId")];

        assert_eq!(codes(&refusal(&definition)), [FILTER_NOT_RENDERABLE]);
    }

    #[test]
    fn resolves_every_filter_the_documents_query_understands() {
        let mut definition = definition(vec![column("title")]);
        definition.filters = FilterParameter::ALL
            .iter()
            .map(|parameter| filter(parameter.as_name()))
            .collect();

        let resolved = plan(&definition).expect("resolves");

        assert_eq!(resolved.filters.len(), FilterParameter::ALL.len());
    }

    // -- Sorting ----------------------------------------------------------

    #[test]
    fn takes_the_definitions_own_default_sort() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort = Some(json!([{"key": "title", "dir": "asc"}]));

        let resolved = plan(&definition).expect("resolves");

        assert_eq!(resolved.sort.key, DocumentSortKey::Title);
        assert!(!resolved.sort.descending);
    }

    #[test]
    fn a_list_declaring_no_sort_opens_newest_first() {
        let resolved = plan(&definition(vec![column("title")])).expect("resolves");

        assert_eq!(resolved.sort, DocumentSort::default());
    }

    #[test]
    fn an_explicit_null_default_sort_is_no_declaration_rather_than_a_mistake() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort = Some(json!(null));

        assert_eq!(
            plan(&definition).expect("resolves").sort,
            DocumentSort::default()
        );
    }

    #[test]
    fn refuses_a_default_sort_naming_a_column_that_cannot_be_ordered() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort = Some(json!([{"key": "entityId", "dir": "asc"}]));

        let details = refusal(&definition);

        assert_eq!(codes(&details), [SORT_NOT_RENDERABLE]);
        assert_eq!(details[0].path, "defaultSort.0.key");
    }

    #[test]
    fn refuses_a_default_sort_direction_that_is_neither_and_says_which_half() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort = Some(json!([{"key": "title", "dir": "sideways"}]));

        let details = refusal(&definition);

        assert_eq!(details[0].path, "defaultSort.0.dir");
    }

    /// **Refused rather than half-honoured.** Taking the first key and dropping
    /// the rest would give the author a list that looks sorted the way they
    /// asked and is not.
    #[test]
    fn refuses_a_second_sort_key_rather_than_ignoring_it() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort =
            Some(json!([{"key": "title"}, {"key": "createdAt", "dir": "desc"}]));

        let details = refusal(&definition);

        assert_eq!(codes(&details), [SORT_NOT_RENDERABLE]);
        assert!(details[0].message.contains("one"), "{}", details[0].message);
    }

    #[test]
    fn refuses_a_default_sort_that_is_not_the_documented_shape() {
        let mut definition = definition(vec![column("title")]);
        definition.default_sort = Some(json!({"key": "title"}));

        assert_eq!(codes(&refusal(&definition)), [SORT_NOT_RENDERABLE]);
    }

    // -- Which columns may be sorted on -----------------------------------

    #[test]
    fn a_form_data_column_is_never_sortable_however_the_definition_marks_it() {
        // `is_sortable` defaults to true, so an author who adds a form_data
        // column gets a sortable one unless something says otherwise — and
        // ordering by a JSONB path needs an index nothing creates.
        let declared = column("form_data.amount");

        assert!(
            declared.is_sortable,
            "the definition asks for a sortable column"
        );

        let resolved = plan(&definition(vec![declared])).expect("resolves");

        assert!(
            !resolved.columns[0].sortable,
            "and the plan refuses it anyway"
        );
    }

    #[test]
    fn a_document_field_outside_the_sort_allow_list_is_not_sortable() {
        // `entityId` displays fine and orders a foreign key.
        let resolved = plan(&definition(vec![column("entityId")])).expect("resolves");

        assert!(!resolved.columns[0].sortable);
    }

    #[test]
    fn the_definition_can_still_turn_sorting_off_on_a_column_that_could_be() {
        let mut declared = column("title");
        declared.is_sortable = false;

        let resolved = plan(&definition(vec![declared])).expect("resolves");

        assert!(!resolved.columns[0].sortable);
    }

    #[test]
    fn a_sortable_document_field_stays_sortable() {
        let resolved = plan(&definition(vec![column("title")])).expect("resolves");

        assert!(resolved.columns[0].sortable);
    }

    // -- Which lists are rendered at all ----------------------------------

    #[test]
    fn only_an_active_list_is_rendered() {
        assert!(is_renderable(ListStatus::Active));
        assert!(!is_renderable(ListStatus::Draft));
        assert!(!is_renderable(ListStatus::Deprecated));
    }
}
