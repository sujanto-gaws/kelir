//! The document list (FR-DOC-013, FR-SRH-001; [#171]).
//!
//! **The visibility rule is [`super::super::repository::list`]'s**, stated there
//! and enforced in the statement rather than here. This service holds the
//! permission and the paging and nothing about which rows exist — which is
//! [#171] AC2, and the reason it reads so thinly: a handler or a service that
//! filtered rows would be a second place the rule lived, and the one in the
//! query would stop being the answer.
//!
//! **FR-SRH-001 is delivered here rather than twice.** The SRS's own note says
//! the search area surfaces the same capability, so a second endpoint would be a
//! second visibility rule over the same table — and record 03's lesson about
//! shared transition services is that two implementations of one rule are two
//! rules, one of which is untested.
//!
//! [#171]: https://github.com/sujanto-gaws/kelir/issues/171

use uuid::Uuid;

use super::super::domain::{DocumentFilters, DocumentQuery, DocumentSort, DocumentSummary};
use super::super::repository as repo;
use super::super::repository::list::DocumentRow;
use super::super::DOCUMENT_READ;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

pub async fn list_documents(
    state: &AppState,
    caller: &Authenticated,
    query: &DocumentQuery,
) -> Result<(Vec<DocumentSummary>, PageMeta), AppError> {
    caller.require(DOCUMENT_READ)?;

    // Parsed before anything is read, so a caller who got two filters wrong
    // learns both from one response rather than one per round trip.
    let filters = query.filters()?;
    let pagination = query.pagination();

    let tenant_id = caller.tenant_id();

    // The count runs the same predicates as the page. `meta.total` reporting the
    // unfiltered population beside a filtered page is a pagination control that
    // offers pages that are empty, which is how a list stops being usable at the
    // size where it matters.
    let total = repo::count_documents(&state.pool, tenant_id, &filters).await?;
    let documents = repo::list_documents(
        &state.pool,
        tenant_id,
        &filters,
        // The document list has no sort control of its own and takes the
        // default: newest first, which is what it has always served. #340 gave
        // the query a sort for the *rendered* list, whose order comes from the
        // definition; adding a `?sort=` here would be a second surface for a
        // control this screen does not offer.
        DocumentSort::default(),
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((documents, pagination.meta(total.max(0) as u64)))
}

/// The S10.3 code a list nothing renders documents *for* carries.
pub const LIST_NOT_BOUND: &str = "LIST_NOT_BOUND";

/// One page of the documents a rendered list covers ([#340]).
///
/// **The rows are the documents of every type that names this list**
/// (`document_types.list_id`, [Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
/// §6.2). That binding is a predicate in the same statement as everything else,
/// not a lookup here, for [`super::super::repository::list`]'s stated reason: a
/// filter answered anywhere but the query is a filter that could become a way
/// to see a row the query would have refused.
///
/// **The permission is `document:read` and there is no second one.** A rendered
/// list is a view of documents, so it opens exactly what `GET /documents`
/// opens — the reading `GET /rad/lookups/{source}/options` already takes
/// ([Database Schema](../../../../../docs/design/02.%20Database%20Schema.md)
/// §5.13): a surface that added its own permission would let a deployment grant
/// the view without granting the rows.
///
/// **A list nothing binds is refused rather than served empty.** No document
/// type naming this list means it has no rows *by construction*, and answering
/// with an empty page would say "no documents" to somebody looking at a
/// misconfigured screen — [#340] AC4, and [#326]'s failure one panel over.
///
/// [#340]: https://github.com/sujanto-gaws/kelir/issues/340
/// [#326]: https://github.com/sujanto-gaws/kelir/issues/326
pub async fn list_rows_for(
    state: &AppState,
    caller: &Authenticated,
    list_id: Uuid,
    filters: DocumentFilters,
    sort: DocumentSort,
    with_form_data: bool,
    pagination: &Pagination,
) -> Result<(Vec<DocumentRow>, PageMeta), AppError> {
    caller.require(DOCUMENT_READ)?;

    let tenant_id = caller.tenant_id();

    require_bound(state, caller, list_id).await?;

    // The binding is set here rather than trusted from the caller: `filters`
    // arrives carrying the definition's own controls, and this is the one field
    // that decides *which* documents the list is over.
    let filters = DocumentFilters {
        list_id: Some(list_id),
        ..filters
    };

    let total = repo::count_documents(&state.pool, tenant_id, &filters).await?;
    let rows = repo::list::list_document_rows(
        &state.pool,
        tenant_id,
        &filters,
        sort,
        with_form_data,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((rows, pagination.meta(total.max(0) as u64)))
}

/// Refuses a list no document type names, naming the list.
///
/// Separate from the page so the *render read* can ask the same question before
/// a screen is drawn: a list that fails here fails whether somebody is looking
/// at page one or page four, and finding out after the headers are on screen is
/// worse than finding out instead of them.
pub async fn require_bound(
    state: &AppState,
    caller: &Authenticated,
    list_id: Uuid,
) -> Result<(), AppError> {
    caller.require(DOCUMENT_READ)?;

    if repo::list::list_is_bound(&state.pool, caller.tenant_id(), list_id).await? {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "listId",
        "binding",
        LIST_NOT_BOUND,
        "no document type in this tenant names this list, so it has no rows to show — \
         bind it to a document type before rendering it",
    )]))
}
