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

use super::super::domain::{DocumentQuery, DocumentSummary};
use super::super::repository as repo;
use super::super::DOCUMENT_READ;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::response::PageMeta;
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
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((documents, pagination.meta(total.max(0) as u64)))
}
