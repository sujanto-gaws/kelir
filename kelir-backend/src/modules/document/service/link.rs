//! Resolving a document's link to a master-data entity (FR-DOC-011, [#170]).
//!
//! # The caller's permission for the underlying entity is required, and it is
//! required by not being checked here
//!
//! **This function holds no permission logic for master data at all.** It asks
//! the master-data module for the same record its own endpoint serves, and that
//! module's service refuses first — `master-data:party:read` exactly as
//! `GET /master-data/parties/{id}` requires it, `master-data:facility:read`
//! exactly as `GET /master-data/facilities/{id}` does.
//!
//! That is the design rather than a convenience, and it is [#161]'s design
//! because it is #161's question. A lookup field and a document link are the
//! same question wearing different clothes: *can a caller who may read documents
//! thereby read master data they could not read directly?* A resolution that
//! checked its own permission would be a second answer, and two answers drift —
//! the master-data endpoint gains a check this one does not, or a third entity
//! type is added here with the wrong string, and the result reads as a working
//! authorization check that opens more than it means to. **A document cannot
//! open what the master-data surface does not, because resolving one *is* that
//! surface.**
//!
//! #97 established the shape — a row made of two surfaces must not be reachable
//! through one of them — and **D-12** applied it again when a record's change
//! history was handing back field values without the record's own read
//! permission.
//!
//! # A caller without it is refused, not handed a blank
//!
//! **403**, which is what `master_data::service` raises and this does not catch.
//! The alternative — returning the link with the name omitted — would say "this
//! supplier has no name", and the person looking at the screen cannot tell that
//! from a record somebody entered badly. Refusing also leaks nothing the caller
//! does not already hold: they read `entityType` and `entityId` off the document
//! itself, so the disclosure a 403 makes is one they made to themselves.
//!
//! # A retired entity 404s about the entity, not about the document (AC5)
//!
//! Soft-deleting the linked party leaves the document readable and its link
//! unchanged — [`super::super::repository::link`] states why nothing cascades.
//! What a caller sees here is a 404 naming the *party*, which is a true
//! statement: the document is fine and the thing it points at is gone.
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161
//! [#170]: https://github.com/sujanto-gaws/kelir/issues/170

use uuid::Uuid;

use super::super::domain::{EntityType, ResolvedEntity};
use super::super::repository as repo;
use super::super::DOCUMENT_READ;
use crate::error::AppError;
use crate::middleware::auth::Authenticated;
use crate::modules::master_data::domain::{PartyAggregate, PartyType};
use crate::modules::master_data::service as master_data;
use crate::state::AppState;

/// Resolves the entity a document is linked to, or refuses the way the
/// entity's own endpoint would.
///
/// `document:read` is required first, because the question "is there a link at
/// all" is a fact about the document. Everything after that is the master-data
/// module's to allow or refuse.
pub async fn resolve_link(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<ResolvedEntity, AppError> {
    caller.require(DOCUMENT_READ)?;

    let document = repo::find_document(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Document"))?;

    let link = document
        .link()
        .ok_or_else(|| AppError::not_found("Linked entity"))?;

    match link.entity_type {
        EntityType::Party => {
            // `get_party` requires `master-data:party:read` and refuses before
            // it reads anything. Nothing is caught here: a 403 from there is a
            // 403 from here, unchanged.
            let party = master_data::get_party(state, caller, link.entity_id).await?;

            Ok(ResolvedEntity {
                entity_type: EntityType::Party,
                entity_id: link.entity_id,
                code: party.party_id.clone(),
                name: party_name(&party),
            })
        }
        EntityType::Facility => {
            let facility = master_data::get_facility(state, caller, link.entity_id).await?;

            Ok(ResolvedEntity {
                entity_type: EntityType::Facility,
                entity_id: link.entity_id,
                code: facility.facility_id,
                name: facility.name,
            })
        }
    }
}

/// What a person calls a party.
///
/// A party is a person or a group and the name lives in whichever profile it
/// has, so this is a fold rather than a column. The fallback to the party code
/// is the case `role_view::as_option` also has to handle: a party may exist
/// without either profile, and a chooser still has to render something that
/// identifies it.
fn party_name(party: &PartyAggregate) -> String {
    match party.party_type_id {
        PartyType::Person => party
            .person
            .as_ref()
            .map(|person| {
                [
                    person.first_name.as_str(),
                    person.middle_name.as_deref().unwrap_or_default(),
                    person.last_name.as_str(),
                ]
                .iter()
                .filter(|part| !part.trim().is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
            })
            .unwrap_or_else(|| party.party_id.clone()),
        PartyType::PartyGroup => party
            .party_group
            .as_ref()
            .map(|group| group.group_name.clone())
            .unwrap_or_else(|| party.party_id.clone()),
    }
}
