//! Resolving a lookup field's options (FR-RAD-007, [#161]).
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161

use super::super::domain::{LookupOption, LookupQuery, LookupSource};
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::master_data::domain::{MasterDataOption, RoleView};
use crate::modules::master_data::service as master_data;
use crate::response::PageMeta;
use crate::state::AppState;

/// One page of the options a lookup field offers.
///
/// # The caller's permission for the underlying entity is required (#161 AC2)
///
/// **And it is required by not being checked here.** This function holds no
/// permission logic at all: it asks the master-data module for a page of the
/// same list the master-data endpoint serves, and that module's service refuses
/// first, before anything is read — `master-data:party:read` plus
/// `master-data:party-role:read` for the three role-backed sources, exactly as
/// `GET /master-data/suppliers` requires them, and `master-data:facility:read`
/// for facilities, exactly as `GET /master-data/facilities` does.
///
/// That is the design rather than a convenience. A lookup that checked its own
/// permission would be a second answer to the same question, and two answers
/// drift: the master-data endpoint would gain a check this one did not, or a new
/// source would be added here with the wrong string, and the result reads as a
/// working authorization check that opens more than it means to. **A lookup
/// cannot open what the master-data surface does not, because it is that
/// surface.** #97 established the shape — a row made of two surfaces must not be
/// reachable through one of them — and **D-12** applied it again when a record's
/// change history was handing back field values without the record's own read
/// permission.
///
/// A new `rad:lookup:read` was the alternative and it is worse in the exact way
/// this issue is about: a deployment could then grant the lookup without
/// granting the list, which is a way to enumerate suppliers to somebody who may
/// not read suppliers. It would also be the per-endpoint permission shape
/// decision **D-6** rejected for the catalogue.
///
/// # A caller without it is refused, not handed an empty list (#161 AC2)
///
/// The AC asks for the choice to be made and the reason stated here, because
/// "empty" and "refused" leak different things. **403**, for three reasons:
///
/// **An empty list is a false statement about the data.** It says "this source
/// holds nothing", and the person filling in the form cannot tell that from a
/// tenant that has not entered its suppliers yet. They will pick nothing, submit
/// the document with the field blank, and the failure surfaces days later as a
/// requisition nobody can route — a long way from its cause. A 403 is the only
/// answer that a renderer can turn into something true on the screen.
///
/// **Refusing leaks nothing the caller does not already hold.** The disclosure a
/// 403 makes here is that the named source exists — and the caller learned that
/// from the form definition they just read, whose `settings.lookups` names it;
/// the set of sources is a static allow-list this API prints in its own
/// validation messages. So the confidentiality argument for emptiness buys
/// nothing, while the cost of the lie is paid by every user who hits it.
///
/// **It is the answer this project has already given twice.** #97 refused a role
/// view to a caller missing either of its two permissions rather than returning
/// rows with the numbers stripped out, and D-12 refused a change history rather
/// than handing back the field values inside it. Filtering-to-empty would be a
/// third answer neither precedent supports.
///
/// # Paging and filtering are the server's (#161 AC4)
///
/// The caller pages and searches; which records may be offered at all is decided
/// in `master_data::service`, where the knowledge of what an enabled party or a
/// live role assignment means already lives.
///
/// # An over-long search is refused before the permission is, and that is the
/// product's order rather than an exception to it
///
/// Worth stating because `list_role_view` states the opposite for itself — "the
/// refusal comes before the query parameters are parsed" — and a reader moving
/// between the two would otherwise file the difference as a defect. The rule the
/// product actually follows is that malformed input is refused at the boundary,
/// before any service runs: `JsonBody` and `QueryParams` answer 422 on a
/// mis-typed field without a handler ever being reached, which is why the
/// permission sweep in `rad_permissions.rs` has to send structurally valid
/// bodies to assert a 403 at all. `LookupQuery::search` is that same boundary
/// check, made by hand only because the bound is a character count.
///
/// **Nothing is read either way**, which is what #161 AC2 asks for: the 422 says
/// the caller's own `search` was too long and says nothing about the source, the
/// tenant, or whether either holds any rows. `list_role_view`'s stronger order is
/// a local refinement for its four hand-parsed *filters*, where the refusal names
/// a vocabulary; there is no vocabulary here to name.
pub async fn list_options(
    state: &AppState,
    caller: &Authenticated,
    source: LookupSource,
    query: &LookupQuery,
) -> Result<(Vec<LookupOption>, PageMeta), AppError> {
    let search = query.search()?;
    let pagination = query.pagination();

    let (options, meta) = match source {
        LookupSource::Supplier => {
            master_data::list_role_view_options(
                state,
                caller,
                RoleView::Supplier,
                search.as_deref(),
                &pagination,
            )
            .await?
        }
        LookupSource::Customer => {
            master_data::list_role_view_options(
                state,
                caller,
                RoleView::Customer,
                search.as_deref(),
                &pagination,
            )
            .await?
        }
        LookupSource::Employee => {
            master_data::list_role_view_options(
                state,
                caller,
                RoleView::Employee,
                search.as_deref(),
                &pagination,
            )
            .await?
        }
        LookupSource::Facility => {
            master_data::list_facility_options(state, caller, search.as_deref(), &pagination)
                .await?
        }
    };

    Ok((options.into_iter().map(as_lookup_option).collect(), meta))
}

/// A master-data record as an option a form may offer.
///
/// The id travels as a string because that is what a JFSS payload holds: a
/// component's `validation.type` is one of JSON Schema's, and a UUID is a
/// `string` among them. Rendering it as a JSON string here rather than leaving
/// the renderer to stringify it keeps the value a document stores identical to
/// the value the chooser offered.
fn as_lookup_option(option: MasterDataOption) -> LookupOption {
    LookupOption {
        value: option.id.to_string(),
        label: option.name,
        description: option.code,
    }
}
