//! Serving a list definition to a renderer, and the rows it arranges
//! (FR-RAD-003, FR-RAD-010, [#340]).
//!
//! **The storage API has existed since Sprint 7 and nothing read it.** This is
//! the reader — and it is a *second* read of the same rows rather than a change
//! to the first, because the two answer different questions for different
//! people:
//!
//! | | `GET /rad/lists/{id}` | `GET /rad/lists/by-key/{listKey}` |
//! |---|---|---|
//! | For | the builder, editing a definition | a renderer, drawing a screen |
//! | Permission | `rad:list:read` | `document:read` |
//! | Status | any | `ACTIVE` only |
//! | Answers | what is stored | what can be drawn |
//!
//! **The permission is the one the rows behind it require, and no new row is
//! added.** [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
//! §5.13 already settled this shape for lookups: `GET
//! /rad/lookups/{source}/options` requires what the master-data endpoint it
//! projects requires, because a `rad:lookup:read` beside it would let a
//! deployment grant the projection without the thing projected. A rendered list
//! is the same shape — it is a view of documents — so requiring `rad:list:read`
//! would mean only a configuration administrator could open a screen built for
//! everybody, and adding a third permission would guard nothing `document:read`
//! does not ([ADR-0011](../../../../../docs/architectures/adr/0011.%20A%20Derived%20Surface%20Requires%20the%20Permission%20of%20What%20It%20Derives%20From.md)'s
//! converse).
//!
//! What that discloses to a document reader is the definition's column labels,
//! its filter labels and its page size. Those are the arrangement of rows the
//! caller may already read, which is the same disclosure the rendered table
//! makes by existing.
//!
//! [#340]: https://github.com/sujanto-gaws/kelir/issues/340

use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::domain::list::{ListDefinition, ListStatus};
use super::super::domain::render::{
    self, CellSource, PlannedColumn, RenderPlan, RenderableColumn, RenderableFilter, RenderableList,
};
use super::super::repository::list as repo;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::document::domain::{DocumentFilters, DocumentQuery, DocumentSort};
use crate::modules::document::service::list as document_list;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// One rendered row: the cells the definition's columns asked for.
///
/// **Keyed by the definition's own `columnKey`**, so a renderer walks the
/// columns it was given and reads each cell by the key it already holds — no
/// second mapping, and a column the definition removed cannot leave an orphan
/// cell behind.
///
/// `id` rides beside the cells rather than inside them, because a row action
/// needs the document's identity whether or not the definition happens to
/// declare an `id` column — and a list that had to declare one to be clickable
/// would be a list whose author had to know that.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
    pub id: Uuid,
    pub cells: Map<String, Value>,
}

/// A list definition, resolved for drawing.
pub async fn renderable_list(
    state: &AppState,
    caller: &Authenticated,
    list_key: &str,
) -> Result<RenderableList, AppError> {
    let definition = active_definition(state, caller, list_key).await?;
    let plan = render::plan(&definition).map_err(AppError::validation)?;

    // Asked here rather than only at the rows, so a misconfigured list fails
    // *instead of* a screen rather than under one.
    document_list::require_bound(state, caller, definition.id).await?;

    Ok(contract(&definition, &plan))
}

/// One page of the rows a list arranges.
pub async fn list_rows(
    state: &AppState,
    caller: &Authenticated,
    list_id: Uuid,
    query: &RowQuery,
) -> Result<(Vec<ListRow>, PageMeta), AppError> {
    let definition = repo::find_list(&state.pool, caller.tenant_id(), list_id)
        .await?
        .filter(|definition| render::is_renderable(definition.status))
        .ok_or_else(|| AppError::not_found("List definition"))?;
    let plan = render::plan(&definition).map_err(AppError::validation)?;

    let filters = query.filters(&plan)?;
    let sort = query.sort(&plan)?;
    // The definition's own page size, which is the whole of what `pageSize`
    // means on a configured list: a client that could widen it would be
    // deciding a number the definition exists to decide.
    let pagination = Pagination {
        page: query.page,
        page_size: Some(definition.page_size.max(1) as u32),
    };

    let (rows, meta) = document_list::list_rows_for(
        state,
        caller,
        list_id,
        filters,
        sort,
        plan.needs_form_data,
        &pagination,
    )
    .await?;

    Ok((
        rows.iter()
            .map(|row| ListRow {
                id: row.summary.id,
                cells: cells(&plan.columns, row),
            })
            .collect(),
        meta,
    ))
}

/// The stored definition, if it is one a renderer should open.
async fn active_definition(
    state: &AppState,
    caller: &Authenticated,
    list_key: &str,
) -> Result<ListDefinition, AppError> {
    // The permission of the rows behind it — see the module doc. Asked before
    // anything is read, so a 404 is never something only a permitted caller
    // could have received.
    caller.require(crate::modules::document::DOCUMENT_READ)?;

    let definition = repo::find_list_by_key(&state.pool, caller.tenant_id(), list_key)
        .await?
        .ok_or_else(|| AppError::not_found("List definition"))?;

    if !render::is_renderable(definition.status) {
        // A conflict rather than a 404: the list exists, and saying so is what
        // lets somebody fix it. **Named rather than empty** — a `DRAFT` list
        // rendered as a table with no rows is the failure AC4 is about.
        return Err(AppError::conflict(format!(
            "list `{}` is {:?} and only an {:?} list is rendered",
            definition.list_key,
            definition.status,
            ListStatus::Active
        )));
    }

    Ok(definition)
}

/// The definition and its plan, as the wire contract.
fn contract(definition: &ListDefinition, plan: &RenderPlan) -> RenderableList {
    let sortable = |key: &str| {
        plan.columns
            .iter()
            .find(|column| column.key == key)
            .is_some_and(|column| column.sortable)
    };

    RenderableList {
        id: definition.id,
        list_key: definition.list_key.clone(),
        title: definition.title.clone(),
        page_size: definition.page_size,
        columns: definition
            .columns
            .iter()
            .map(|column| RenderableColumn {
                key: column.column_key.clone(),
                label: column.label.clone(),
                data_type: column.data_type.clone(),
                format: column.format.clone(),
                width: column.width.clone(),
                sortable: sortable(&column.column_key),
            })
            .collect(),
        filters: definition
            .filters
            .iter()
            .zip(&plan.filters)
            .map(|(filter, planned)| RenderableFilter {
                key: filter.filter_key.clone(),
                label: filter.label.clone(),
                filter_type: filter.filter_type,
                options: filter.options_json.clone(),
                is_default: filter.is_default,
                parameter: planned.parameter.as_name().to_owned(),
            })
            .collect(),
        default_sort_key: plan.sort.key.as_name().to_owned(),
        default_sort_descending: plan.sort.descending,
    }
}

/// One row's cells, by the definition's column keys.
fn cells(
    columns: &[PlannedColumn],
    row: &crate::modules::document::repository::list::DocumentRow,
) -> Map<String, Value> {
    let mut cells = Map::new();

    for column in columns {
        let value = match &column.source {
            CellSource::Summary(field) => summary_cell(*field, &row.summary),
            // **`null` where the path is absent, not a missing key.** A
            // document created before the form gained a field has no value at
            // that path, and a renderer that had to distinguish *absent column*
            // from *absent value* would be reading the definition twice.
            CellSource::FormData(path) => row
                .form_data
                .as_ref()
                .map(|payload| read_path(payload, path))
                .unwrap_or(Value::Null),
        };

        cells.insert(column.key.clone(), value);
    }

    cells
}

/// A dot path into the stored payload.
///
/// Walked segment by segment rather than through `Value::pointer`, for the
/// reason `service::evaluation::scope_at` gives: a JSON pointer would need the
/// path escaped and rebuilt, and a numeric segment addresses a repeater row,
/// which is a shape `form_data.line_items.0.amount` should be able to name.
fn read_path(payload: &Value, path: &str) -> Value {
    let mut current = payload;

    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => match map.get(segment) {
                Some(value) => value,
                None => return Value::Null,
            },
            Value::Array(items) => match segment.parse::<usize>().ok().and_then(|at| items.get(at))
            {
                Some(value) => value,
                None => return Value::Null,
            },
            _ => return Value::Null,
        };
    }

    current.clone()
}

fn summary_cell(
    field: render::SummaryField,
    summary: &crate::modules::document::domain::DocumentSummary,
) -> Value {
    use render::SummaryField as F;

    match field {
        F::Id => Value::String(summary.id.to_string()),
        F::DocumentRef => Value::String(summary.document_ref.clone()),
        F::DocumentNumber => summary
            .document_number
            .clone()
            .map_or(Value::Null, Value::String),
        F::DocumentTypeId => Value::String(summary.document_type_id.to_string()),
        F::DocumentTypeCode => Value::String(summary.document_type_code.clone()),
        F::Title => Value::String(summary.title.clone()),
        F::Status => Value::String(summary.status.as_db().to_owned()),
        F::Priority => Value::String(summary.priority.as_db().to_owned()),
        F::EntityType => summary
            .entity_type
            .map_or(Value::Null, |kind| Value::String(kind.as_db().to_owned())),
        F::EntityId => summary
            .entity_id
            .map_or(Value::Null, |id| Value::String(id.to_string())),
        F::SubmittedAt => summary
            .submitted_at
            .map_or(Value::Null, |at| Value::String(at.to_rfc3339())),
        F::CreatedAt => Value::String(summary.created_at.to_rfc3339()),
        F::UpdatedAt => Value::String(summary.updated_at.to_rfc3339()),
    }
}

/// What a rows request may carry.
///
/// **Only what the definition declared.** A filter the definition does not
/// offer is refused rather than ignored, which is [#340] AC3's second half: a
/// client that could send `?status=` to a list whose author chose not to offer
/// status would be filtering by something the screen never showed, and a
/// silently-ignored parameter reads to the sender as a filter that matched
/// everything.
#[derive(Debug, Default, Clone, serde::Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RowQuery {
    pub page: Option<u32>,
    /// Accepted and refused by name.
    ///
    /// **Declared so that it does not fall into `filters` below.** `serde`'s
    /// flatten takes every parameter this struct does not name, so an
    /// undeclared `pageSize` would come back as *`pageSize` is not a filter
    /// this list declares* — true, and a poor answer to somebody asking for a
    /// bigger page. Naming it here is what lets the refusal say the real
    /// reason: the definition decides how many rows a page has.
    pub page_size: Option<u32>,
    /// A column key from the definition, which must be one it marked sortable.
    pub sort: Option<String>,
    /// `asc` or `desc`; absent is the definition's own direction when `sort` is
    /// absent too, and ascending otherwise.
    pub dir: Option<String>,
    /// The declared filters, by their **filter key** — the definition's own
    /// spelling, so a client echoes back what it was given.
    #[serde(flatten)]
    pub filters: std::collections::BTreeMap<String, String>,
}

impl RowQuery {
    /// The document filters this request sets, refusing anything undeclared.
    ///
    /// **The values are parsed by `DocumentQuery::filters`, not here.** Which
    /// strings `status` and `priority` accept is the document module's
    /// vocabulary; a second copy in this file would be a second answer to one
    /// question, and the copy that drifted would refuse a value the list is
    /// full of. So the declared controls are assembled into a `DocumentQuery`
    /// and it does the parsing — which also means a bad value earns the same
    /// `UNKNOWN_VALUE` refusal, listing the same allowed strings, that
    /// `GET /documents?status=` has always earned.
    ///
    /// What this file does add is the path: a refusal comes back naming the
    /// query *parameter*, and the client sent the definition's **filter key**,
    /// so the two are mapped back before the response goes out. A person
    /// looking at a filter labelled *Stage* should be told about `stage`.
    fn filters(&self, plan: &RenderPlan) -> Result<DocumentFilters, AppError> {
        use render::FilterParameter as P;

        if self.page_size.is_some() {
            return Err(AppError::validation(vec![ValidationDetail::new(
                "pageSize",
                "pageSize",
                render::FILTER_NOT_RENDERABLE,
                "a configured list decides its own page size, so this cannot be set per \
                 request — change `pageSize` on the list definition",
            )]));
        }

        let mut query = DocumentQuery::default();
        let mut undeclared = Vec::new();
        // Which filter key set which parameter, for renaming a refusal's path.
        let mut sent_as: Vec<(&'static str, &str)> = Vec::new();

        for (key, value) in &self.filters {
            let value = value.trim();

            if value.is_empty() {
                // An empty control is not a filter. The document list's own
                // client already drops these; dropping them here too means a
                // form that submits every box does not have to.
                continue;
            }

            let Some(planned) = plan.filters.iter().find(|filter| filter.key == *key) else {
                undeclared.push(ValidationDetail::new(
                    key.clone(),
                    "filter",
                    render::FILTER_NOT_RENDERABLE,
                    format!(
                        "`{key}` is not a filter this list declares, so it is refused rather \
                         than ignored — an ignored filter reads as one that matched everything"
                    ),
                ));
                continue;
            };

            let owned = value.to_owned();

            match planned.parameter {
                P::Search => query.search = Some(owned),
                P::Status => query.status = Some(owned),
                P::Priority => query.priority = Some(owned),
                P::EntityType => query.entity_type = Some(owned),
                P::EntityId => {
                    query.entity_id = Some(value.parse().map_err(|_| {
                        AppError::validation(vec![ValidationDetail::new(
                            key.clone(),
                            "filter",
                            render::FILTER_NOT_RENDERABLE,
                            format!("`{value}` is not an identifier"),
                        )])
                    })?)
                }
            }

            sent_as.push((planned.parameter.as_name(), key));
        }

        if !undeclared.is_empty() {
            return Err(AppError::validation(undeclared));
        }

        query.filters().map_err(|error| rename(error, &sent_as))
    }

    /// The order this request asks for, or the definition's own.
    fn sort(&self, plan: &RenderPlan) -> Result<DocumentSort, AppError> {
        let Some(requested) = self
            .sort
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            return Ok(plan.sort);
        };

        let column = plan
            .columns
            .iter()
            .find(|column| column.key == requested)
            .ok_or_else(|| {
                AppError::validation(vec![ValidationDetail::new(
                    "sort",
                    "sort",
                    render::SORT_NOT_RENDERABLE,
                    format!("`{requested}` is not a column this list declares"),
                )])
            })?;

        if !column.sortable {
            return Err(AppError::validation(vec![ValidationDetail::new(
                "sort",
                "sort",
                render::SORT_NOT_RENDERABLE,
                format!("`{requested}` is a column this list does not offer sorting on"),
            )]));
        }

        let CellSource::Summary(field) = &column.source else {
            // Unreachable while `plan` marks a `form_data.*` column unsortable,
            // and answered rather than unwrapped so a change there is a refusal
            // instead of a panic.
            return Err(AppError::validation(vec![ValidationDetail::new(
                "sort",
                "sort",
                render::SORT_NOT_RENDERABLE,
                format!("`{requested}` reads the form payload and cannot be ordered on"),
            )]));
        };

        DocumentSort::parse(field.as_name(), self.dir.as_deref()).map_err(|refusal| {
            AppError::validation(vec![ValidationDetail::new(
                "dir",
                "sort",
                render::SORT_NOT_RENDERABLE,
                refusal.to_string(),
            )])
        })
    }
}

/// A refusal from `DocumentQuery::filters`, with each path renamed to the
/// filter key the client actually sent.
///
/// **`entityType` and `entityId` are the case that makes this worth doing.**
/// `DocumentQuery::filters` refuses one of the pair without the other, and its
/// detail names the *missing* parameter — which the client may never have heard
/// of. Renaming what was sent and leaving what was not is the honest half of
/// each: the caller learns `stage` was wrong, and learns `entityId` is missing
/// under the name the document API uses for it, because their list declares no
/// filter for it at all.
fn rename(error: AppError, sent_as: &[(&'static str, &str)]) -> AppError {
    let AppError::Validation { details } = error else {
        return error;
    };

    AppError::validation(
        details
            .into_iter()
            .map(|mut detail| {
                if let Some((_, key)) = sent_as
                    .iter()
                    .find(|(parameter, _)| *parameter == detail.path)
                {
                    detail.path = (*key).to_owned();
                }

                detail
            })
            .collect(),
    )
}
