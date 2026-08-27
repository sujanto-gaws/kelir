//! The master-data entity a document concerns (FR-DOC-011, [#170]).
//!
//! A purchase order concerns a supplier; a facility transfer concerns a
//! facility. The SRS says *"when applicable"*, so the link is optional — but
//! when it is there it is a **pair**, and that is the whole of AC1.
//!
//! # `entityType` is recorded, never inferred
//!
//! `documents.entity_id` is a bare `UUID` with no foreign key, because the thing
//! it points at is polymorphic — `0015_document.sql` created it that way
//! deliberately. A bare id that could mean a party or a facility is a bug
//! waiting for the second entity type, and Kelir already has the second entity
//! type. So the pair is all-or-nothing: `entityType` without `entityId`, and
//! `entityId` without `entityType`, are both refused.
//!
//! The column has no `CHECK`, so the vocabulary is this enum. It holds the two
//! kinds that have a read endpoint to delegate to; products and services are
//! one variant each when they have services to delegate to.
//!
//! # Reading a document hands back the pair and nothing else
//!
//! **This is [#161]'s answer, because it is #161's question.** A lookup field
//! and a document link are the same question wearing different clothes: can a
//! caller who may read documents thereby read master data they could not read
//! directly? #161 answered by *not* holding a permission check at all — it asks
//! the master-data module for the same page the master-data endpoint serves, and
//! that module refuses first. *A lookup cannot open what the master-data surface
//! does not, because it is that surface.*
//!
//! So a document's own read returns `entityType` and `entityId` — no name, no
//! code, no fields — and resolving them is a sub-resource that calls
//! [`master_data::service`][ms]. A caller holding `document:read` and not
//! `master-data:party:read` gets the document, sees that it concerns a party,
//! and gets **403** from the resolution. That is #161's choice between refusing
//! and answering empty, taken the same way and for its second reason: refusing
//! leaks nothing the caller does not already hold, because they read the
//! identifier on the document itself.
//!
//! [#161]: https://github.com/sujanto-gaws/kelir/issues/161
//! [#170]: https://github.com/sujanto-gaws/kelir/issues/170
//! [ms]: crate::modules::master_data::service

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// A kind of master-data record a document may concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntityType {
    Party,
    Facility,
}

impl EntityType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Party => "PARTY",
            Self::Facility => "FACILITY",
        }
    }

    /// Reads a value already in the database, refusing one this build does not
    /// know.
    ///
    /// **Not lenient, unlike [`DocumentStatus::from_db`][from_db].** That column
    /// has a `CHECK` vouching for its contents and this one does not, so
    /// `entity_type` really can hold a string no variant matches — a row
    /// written by a future release, or by hand. Guessing `Party` would then
    /// resolve a facility id against the party table and hand back somebody
    /// else's record, which is a wrong answer where the status column's
    /// fallback is only ever an unreachable one.
    ///
    /// [from_db]: super::status::DocumentStatus::from_db
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "PARTY" => Some(Self::Party),
            "FACILITY" => Some(Self::Facility),
            _ => None,
        }
    }

    /// What a refusal calls this entity.
    pub fn missing(self) -> &'static str {
        match self {
            Self::Party => "Party",
            Self::Facility => "Facility",
        }
    }
}

/// The link as a document carries it: the pair, and nothing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntityLink {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
}

/// The pair rule: both halves, or neither, or a refusal naming the one that is
/// missing.
///
/// A free function over the two members rather than a struct with a method,
/// because the two members are what the wire carries — a create request, an
/// update request and a list filter each hold them separately, and a wrapper
/// type would exist only to be unwrapped three times.
///
/// This is [#170]'s AC1 enforced rather than described: `entityType` without
/// `entityId` names no record, and `entityId` without `entityType` is the bare
/// id that could mean a party or a facility.
pub fn check_pair(
    entity_type: Option<EntityType>,
    entity_id: Option<Uuid>,
) -> Result<Option<EntityLink>, AppError> {
    match (entity_type, entity_id) {
        (Some(entity_type), Some(entity_id)) => Ok(Some(EntityLink {
            entity_type,
            entity_id,
        })),
        (None, None) => Ok(None),
        (Some(_), None) => Err(half("entityId", "an entityType names no record without one")),
        (None, Some(_)) => Err(half(
            "entityType",
            "an entityId alone could mean a party or a facility, and the two \n             are different records",
        )),
    }
}

fn half(path: &str, message: &str) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        path,
        "required",
        "INCOMPLETE_ENTITY_LINK",
        message,
    )])
}

/// A resolved link, as the sub-resource answers it.
///
/// Deliberately thin. It is what a workspace header needs to render *"Supplier:
/// PT Sumber Makmur"* and no more — the entity's own endpoint is where a caller
/// who wants its fields goes, and that endpoint is the one that decides what
/// they may see. Widening this struct is how the permission this design
/// delegates would start being answered here instead.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEntity {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    /// The code a person recognises — a party code, a facility code.
    pub code: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_halves_of_a_link_are_required_together() {
        // AC1: a bare id that could mean a party or a facility is a bug waiting
        // for the second entity type, and there already is one.
        assert!(check_pair(Some(EntityType::Party), None).is_err());
        assert!(check_pair(None, Some(Uuid::now_v7())).is_err());
    }

    #[test]
    fn no_link_at_all_is_legitimate() {
        // FR-DOC-011 says "when applicable". Most documents concern no master
        // record at all.
        assert!(check_pair(None, None)
            .expect("neither half is not a failure")
            .is_none());
    }

    #[test]
    fn an_unrecognised_entity_type_in_the_column_is_refused_rather_than_guessed() {
        // The column carries no CHECK, so this string really can be there.
        // Guessing would resolve the id against the wrong table and hand back
        // somebody else's record.
        assert_eq!(EntityType::from_db("PRODUCT"), None);
        assert_eq!(EntityType::from_db(""), None);
    }

    #[test]
    fn every_entity_type_round_trips_through_the_database_spelling() {
        for entity_type in [EntityType::Party, EntityType::Facility] {
            assert_eq!(EntityType::from_db(entity_type.as_db()), Some(entity_type));
        }
    }
}
