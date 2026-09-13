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

use super::super::domain::{
    DocumentFilters, DocumentQuery, DocumentSort, DocumentSummary, RecentlyTouchedDocument,
};
use super::super::repository as repo;
use super::super::repository::list::DocumentRow;
use super::super::DOCUMENT_READ;
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::activity::domain as activity_domain;
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

/// How many documents this caller raised and has not sent yet (FR-RPT-001,
/// [#431]).
///
/// # This function requires no permission, and that is the decision
///
/// Every other function in this file opens with `caller.require(DOCUMENT_READ)`,
/// because every other function serves document **rows** — a title, a number, a
/// reference, sometimes a form payload. `document:read` is the permission for
/// *the document surface*, and rows are what it protects.
///
/// **This returns one integer about the caller's own unsent work.** It names no
/// document, says nothing about the tenant's population, and tells the caller
/// nothing they do not already know first-hand: they raised these rows. There
/// is nothing here for `document:read` to protect, and requiring it would make
/// the dashboard refuse somebody a count of their own drafts.
///
/// **The alternative was asking for `document:read` as well, and
/// `0041_activity_read_dropped.sql` is what that costs when it is wrong.**
/// `activity:read` guarded a surface whose every fact was already behind the
/// document's own read; it outlived the check by a release and then left the
/// catalogue (**D-45**, **D-47**, [#301]). The mirror-image mistake is the one
/// available here — a second permission in front of a fact that needs none —
/// and the dashboard's own gate, [`crate::modules::reporting::DASHBOARD_READ`],
/// is where that surface is decided.
///
/// **What this does not license.** A count over rows the caller did *not* raise
/// is the document population, and that is `document:read`'s to gate.
/// FR-RPT-004's tenant-wide status summary is exactly that, and it cannot be
/// served by widening this function.
///
/// [#301]: https://github.com/sujanto-gaws/kelir/issues/301
/// [#431]: https://github.com/sujanto-gaws/kelir/issues/431
pub async fn count_own_drafts(state: &AppState, caller: &Authenticated) -> Result<i64, AppError> {
    let count =
        repo::list::count_own_drafts(&state.pool, caller.tenant_id(), caller.user_id()).await?;

    Ok(count)
}

/// The documents this caller touched most recently (FR-RPT-003, [#433]).
///
/// # This function requires no permission either, and the line is *whose work*
///
/// [`count_own_drafts`] above argues that at length for a number. **This returns
/// rows**, and that difference is worth meeting head-on rather than leaving to
/// the reader, because it is the one an author reviewing this file will stop at.
///
/// **The line the dashboard's invariant draws is *whose work*, not *rows versus
/// counts***, and FR-RPT-002 settled it one item ago: the pending-task widget
/// serves task rows behind `reporting:dashboard:read` alone, because a task's own
/// holder is the last party its name needs keeping from. [ADR-0039] had already
/// taken it — each FR-RPT row "inherits FR-RPT-001's endpoint, its permission
/// and its card".
///
/// The same holds here and more plainly: **the caller is the actor on every
/// event that put a document in this list.** They raised it, edited it,
/// commented on it, or decided it. There is no row here whose title they are
/// learning for the first time, so `document:read` in front of it would be a
/// check in front of a fact its subject already has first-hand — the shape
/// `0041_activity_read_dropped.sql` is this project's record of (**D-45**,
/// **D-47**, [#301]). It would also make the widget refuse somebody a list of
/// their own work on the one screen built to show it.
///
/// **What the frontend does with that is a courtesy, not a control.** The rows
/// link into the document screen, that route holds `document:read`, so a viewer
/// without it sees the rows and not the links. The server is not asked to know
/// this.
///
/// **What this does not license.** Documents the caller did *not* touch are the
/// document population, and that is [`DOCUMENT_READ`]'s to gate. FR-RPT-004's
/// tenant-wide status summary is exactly that, and **it cannot be served by
/// widening this function** — an author reaching for it is standing here, which
/// is why the sentence is here rather than only in the module doc.
///
/// # What "touched" means, and where it is written
///
/// [`activity::domain::TOUCH_EVENT_TYPES`] — *you touched a document when you
/// raised or changed it, said something on it, attached something to it, or
/// moved its workflow*. [#433] AC2 required that sentence before the query
/// existed, because **`recent` is the word that hides an unstated join**: every
/// row a widget shows is consistent with *some* definition, so one that is not
/// written down cannot be tested against.
///
/// The visibility and the ordering are both
/// [`super::super::repository::list::list_recent_touched`]'s, in one statement,
/// for this file's usual reason.
///
/// [ADR-0039]: ../../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
/// [#301]: https://github.com/sujanto-gaws/kelir/issues/301
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
/// [`activity::domain::TOUCH_EVENT_TYPES`]: crate::modules::activity::domain::TOUCH_EVENT_TYPES
pub async fn recent_documents(
    state: &AppState,
    caller: &Authenticated,
    limit: i64,
) -> Result<Vec<RecentlyTouchedDocument>, AppError> {
    let documents = repo::list::list_recent_touched(
        &state.pool,
        caller.tenant_id(),
        caller.user_id(),
        activity_domain::TOUCH_EVENT_TYPES,
        limit,
    )
    .await?;

    Ok(documents)
}
