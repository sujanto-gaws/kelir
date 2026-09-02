//! A master-data change that has to be approved before it applies
//! (FR-MDM-010; [#255], **D-55**, [ADR-0033]).
//!
//! # What a governed change is
//!
//! A document of a type carrying `target_entity_type`, linked to the record it
//! proposes to change, whose form data **is** the change. Submitting it parks
//! the record at `PENDING_APPROVAL`; the process approves or refuses it; the
//! record is written or put back.
//!
//! # The change is the update request's own shape, and it is checked at raise
//!
//! `form_data_json` is arbitrary JSON and a master-data record is typed columns,
//! so something has to say what maps to what. **The document's form data is
//! deserialized into the entity's own update request** — the same type
//! `PUT /master-data/parties/{id}` takes — which means the mapping is the one
//! that already exists rather than a second one this module would have to keep
//! in step.
//!
//! **It is checked when the change is raised, not when it is approved**
//! ([ADR-0028]'s rule about definitions, applied to a payload): a change whose
//! shape is wrong should be refused to the person proposing it, at the moment
//! they propose it, rather than to an approver a week later — or worse,
//! discovered inside the transaction that closes the process, where the only
//! options left are failing an approval that was legitimately given or applying
//! less than was approved.
//!
//! # What a governed change may carry, and what it may not
//!
//! **The record's own scalar fields**: `statusId`, `externalId`, `description`
//! and `additionalAttributes` for a party; the equivalent for a facility. Those
//! are what `repository::update_party_fields` and `update_facility_fields`
//! write in one statement, which is what makes applying them inside the closing
//! transaction possible at all.
//!
//! **Not the sub-aggregates** — `person`, `identifications`, `relationships*`,
//! `classifications`, `contactMechanisms`. They are separate tables written by
//! multi-statement service logic that resolves references and re-checks
//! permissions, and reproducing that inside a workflow's closing transaction
//! would be a second copy of the update path drifting from the first.
//!
//! **A change naming one is refused at raise**, with the field named. That is
//! the difference between a bounded feature and one that quietly does less than
//! it appears to: the alternative — accepting them and applying the scalars —
//! would approve a change to a supplier's contacts and then not make it.
//!
//! [#255]: https://github.com/sujanto-gaws/kelir/issues/255
//! [ADR-0028]: ../../../../docs/architectures/adr/0028.%20A%20Definition%20Is%20Refused%20at%20Save%20Rather%20Than%20at%20Render.md
//! [ADR-0033]: ../../../../docs/architectures/adr/0033.%20A%20Governed%20Record%20Parks%20at%20Pending%20Approval.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RecordStatus;
use crate::error::{AppError, ValidationDetail};

/// Which record a change is about — the same two entities the document link
/// resolves, named here so this module does not depend on the document's enum.
///
/// **A third entity is a migration and a match arm**, and both are in this
/// module: `mdm_products` and `mdm_services` carry `record_status` too, and the
/// day one of them is governed, this is the type that grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernedEntity {
    Party,
    Facility,
}

impl GovernedEntity {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Party => "PARTY",
            Self::Facility => "FACILITY",
        }
    }

    /// Reads a configured or stored value, **refusing one this build does not
    /// know**.
    ///
    /// `document_types.target_entity_type` is free text with no `CHECK` — it has
    /// been since `0015` — so a value no variant matches really can be there. A
    /// type carrying one governs **nothing**, which is the safe reading: the
    /// alternative, guessing `Party`, would route a facility's change through
    /// the party table. `document::domain::EntityType::from_db` refuses for the
    /// same reason one module over.
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "PARTY" => Some(Self::Party),
            "FACILITY" => Some(Self::Facility),
            _ => None,
        }
    }

    pub fn missing(self) -> &'static str {
        match self {
            Self::Party => "Party",
            Self::Facility => "Facility",
        }
    }
}

/// An open or resolved change, as this module reads it back.
#[derive(Debug, Clone)]
pub struct ChangeRequest {
    pub id: Uuid,
    pub document_id: Uuid,
    pub entity: GovernedEntity,
    pub entity_id: Uuid,
    pub previous_record_status: RecordStatus,
}

/// One change that was proposed for a record, as its history reports it
/// ([#255] AC4).
///
/// **A refused attempt is on this list.** What was asked of a record is part of
/// what happened to it, and a history that showed only the applied changes would
/// answer a different question from the one somebody auditing a supplier is
/// asking.
///
/// The document is named and not opened: reading what the change actually said
/// is `document:read`, on the document surface, under that module's own rules.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAttempt {
    pub id: Uuid,
    pub document_id: Uuid,
    /// `null` while the change is still being decided.
    pub outcome: Option<String>,
    pub previous_record_status: RecordStatus,
    pub raised_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub raised_by: Option<Uuid>,
}

/// The scalar change a governed document carries, **one shape per entity**.
///
/// A party and a facility do not have the same fields, and a single struct with
/// the union of both would accept `facilityType` on a party's change and then
/// not apply it. The entity is known before the payload is read — it comes from
/// the type's configuration — so the shape is chosen rather than guessed.
///
/// **Every field optional, and absent means unchanged**, which is the update
/// request's own semantics because each of these is a subset of one.
/// `additionalAttributes` replaces wholesale where it is present, as the update
/// path does with it today.
#[derive(Debug, Clone)]
pub enum ProposedChange {
    Party(PartyChange),
    Facility(FacilityChange),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyChange {
    pub status_id: Option<String>,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub additional_attributes: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FacilityChange {
    pub name: Option<String>,
    pub facility_type: Option<String>,
    pub additional_attributes: Option<Value>,
}

impl ProposedChange {
    /// Whether it would write anything at all.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Party(change) => {
                change.status_id.is_none()
                    && change.external_id.is_none()
                    && change.description.is_none()
                    && change.additional_attributes.is_none()
            }
            Self::Facility(change) => {
                change.name.is_none()
                    && change.facility_type.is_none()
                    && change.additional_attributes.is_none()
            }
        }
    }
}

/// The sub-aggregate keys a governed change may not carry, named so the refusal
/// can say which one was sent.
///
/// A party's, and a facility's `address` beside them: each is written by
/// multi-statement service logic that this process does not reproduce.
const UNSUPPORTED_KEYS: &[&str] = &[
    "person",
    "partyGroup",
    "identifications",
    "relationshipsFrom",
    "relationshipsTo",
    "classifications",
    "contactMechanisms",
    "address",
];

/// Reads a document's form data as a change, or refuses and says why.
///
/// Three refusals, and each names the field:
///
/// * a key this module does not apply — the sub-aggregates above;
/// * a shape that is not an object, or a value of the wrong type;
/// * a change that would write nothing, which is an approval nobody needs.
pub fn read_change(entity: GovernedEntity, form_data: &Value) -> Result<ProposedChange, AppError> {
    let Some(fields) = form_data.as_object() else {
        return Err(refusal(
            "formData",
            "type",
            "CHANGE_NOT_AN_OBJECT",
            "a governed change is an object of the record's own fields",
        ));
    };

    for key in UNSUPPORTED_KEYS {
        if fields.contains_key(*key) {
            return Err(refusal(
                &format!("formData.{key}"),
                "unsupported",
                "CHANGE_FIELD_NOT_GOVERNED",
                format!(
                    "`{key}` cannot be changed through an approval yet: it is written across \
                     several tables by the record's own update path, and this process applies \
                     the record's own fields in one statement. Change it directly, or raise a \
                     change carrying only statusId, externalId, description or \
                     additionalAttributes"
                ),
            ));
        }
    }

    let unreadable = |error: serde_json::Error| {
        refusal(
            "formData",
            "shape",
            "CHANGE_NOT_READABLE",
            format!("this change cannot be read as a change to the record: {error}"),
        )
    };

    let change = match entity {
        GovernedEntity::Party => {
            ProposedChange::Party(serde_json::from_value(form_data.clone()).map_err(unreadable)?)
        }
        GovernedEntity::Facility => {
            ProposedChange::Facility(serde_json::from_value(form_data.clone()).map_err(unreadable)?)
        }
    };

    if change.is_empty() {
        return Err(refusal(
            "formData",
            "required",
            "CHANGE_EMPTY",
            "this change would alter nothing; there is nothing for an approver to approve",
        ));
    }

    Ok(change)
}

/// The refusal for a second change over a record that already has one.
///
/// A **409** rather than a 422: the request is fine and the record's state is
/// what refuses it, which is what a person needs to be told — there is a change
/// in flight, and it is not this one.
pub fn already_in_flight() -> AppError {
    AppError::conflict(
        "this record already has a change awaiting approval; that one has to be decided before \
         another can be raised",
    )
}

/// The refusal for a record whose status cannot take a change.
pub fn not_changeable(status: RecordStatus) -> AppError {
    AppError::conflict(format!(
        "a record at {} cannot have a change raised against it",
        status.as_db()
    ))
}

/// The refusal for a direct edit of a record with a change in flight
/// ([#255] AC1).
///
/// **The record's own status is what refuses it**, under the permission that
/// already governs the write — rather than a new permission, or a query into
/// the document module on every master-data update.
pub fn awaiting_approval() -> AppError {
    AppError::conflict(
        "this record has a change awaiting approval and cannot be edited directly until that \
         change is decided",
    )
}

fn refusal(path: &str, rule: &str, code: &str, message: impl Into<String>) -> AppError {
    AppError::validation(vec![ValidationDetail::new(path, rule, code, message)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_scalar_change_is_read() {
        let change = read_change(
            GovernedEntity::Party,
            &json!({
                "externalId": "SUP-0009",
                "description": "moved to the new industrial estate",
            }),
        )
        .expect("a change");

        let ProposedChange::Party(party) = change else {
            panic!("a party's change is read as a party's change");
        };

        assert_eq!(party.external_id.as_deref(), Some("SUP-0009"));
        assert!(party.status_id.is_none(), "absent is unchanged");
    }

    /// **A facility's change is not a party's**, and the shape is chosen from
    /// the configuration rather than from what the payload happens to carry.
    #[test]
    fn a_partys_field_on_a_facilitys_change_is_refused() {
        assert!(read_change(
            GovernedEntity::Facility,
            &json!({ "name": "North warehouse", "facilityType": "WAREHOUSE" }),
        )
        .is_ok());

        assert!(
            read_change(GovernedEntity::Facility, &json!({ "externalId": "FAC-1" })).is_err(),
            "a facility has no externalId on this surface, and accepting one would \
             be a change that is approved and then not made"
        );
    }

    #[test]
    fn a_change_naming_a_sub_aggregate_is_refused_and_names_it() {
        // Accepting this and applying the scalars would approve a change to the
        // supplier's contacts and then not make it.
        let refused = read_change(
            GovernedEntity::Party,
            &json!({
                "description": "new contact",
                "contactMechanisms": [{ "purpose": "PRIMARY_PHONE" }],
            }),
        )
        .expect_err("a refusal");

        assert_eq!(refused.code(), "VALIDATION_ERROR");
    }

    #[test]
    fn a_change_that_would_write_nothing_is_refused() {
        assert!(read_change(GovernedEntity::Party, &json!({})).is_err());
    }

    #[test]
    fn a_change_that_is_not_an_object_is_refused() {
        assert!(read_change(GovernedEntity::Party, &json!(["externalId"])).is_err());
    }

    #[test]
    fn an_unknown_governed_entity_governs_nothing() {
        assert_eq!(
            GovernedEntity::from_db("PARTY"),
            Some(GovernedEntity::Party)
        );
        assert_eq!(GovernedEntity::from_db("SUPPLIER"), None);
    }
}
