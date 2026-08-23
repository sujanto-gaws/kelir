//! Facilities — the physical places master data refers to (FR-MDM-004).
//!
//! Not party-based, and the only `Must` entity in the epic that is not. A
//! facility is a building, a floor, a room, a warehouse or a site, and it
//! nests: `parentFacilityId` is what makes Building → Floor → Room a tree
//! rather than three flat lists ([concepts/03] §4.1).
//!
//! **`address` reuses the party's [`PostalAddress`].** `mdm_facilities.
//! address_json` is JSONB with nothing behind it, so the shape is a decision
//! rather than a reading: a warehouse's address and a supplier's address are
//! the same kind of thing, and two shapes for one concept would mean a client
//! rendering an address twice. Storing it inline rather than as a contact
//! mechanism is the other half of that decision — a facility *has* an address
//! the way a person has a birth date, whereas a party may have several
//! addresses for several purposes, which is what `mdm_party_contact_mechs`
//! exists to model.
//!
//! **`facilityType` is enforced here or nowhere.** §4.16 declares it
//! `VARCHAR(64)` with the vocabulary in a comment and no `CHECK`, so the
//! database will store anything. It is an enum below: a list screen that groups
//! by type cannot group by a value it has never heard of, and a typo would
//! otherwise create a silent sixth type. The column stays open in the schema so
//! that adding a type is a code change and not a migration — but an unknown
//! value arriving from a client is a 422, not a new type.
//!
//! **`recordStatus` and the two document references stay off the wire**, as
//! they do on the party aggregate and for the same reason: nothing moves them
//! until #99, and a field nothing can change reads as a control that exists.
//!
//! [concepts/03]: ../../../../../docs/concepts/03.%20Handling%20Master%20Data.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    bound_name, finish, non_empty, require_code, require_name, PostalAddress, RecordStatus,
};
use crate::error::{AppError, ValidationDetail};

/// Longest `facilityId` §4.16 holds — `facility_code VARCHAR(64)`.
pub const MAX_FACILITY_CODE_LENGTH: usize = 64;

/// How deep a facility tree may be walked when checking for a cycle.
///
/// Not a business rule about how deep a building may nest — it is the bound
/// that keeps the ancestor walk terminating if a cycle ever did reach storage.
/// Building → Floor → Room is three, and the vocabulary suggests no more than
/// five, so a request refused at 64 is a request that was wrong.
pub const MAX_FACILITY_DEPTH: i32 = 64;

/// What a facility is (§4.16's column comment, as a closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FacilityType {
    Building,
    Floor,
    Room,
    Warehouse,
    Site,
}

impl FacilityType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Building => "BUILDING",
            Self::Floor => "FLOOR",
            Self::Room => "ROOM",
            Self::Warehouse => "WAREHOUSE",
            Self::Site => "SITE",
        }
    }

    /// Reads a value already in the database.
    ///
    /// Lenient, unlike the deserializer: the column carries no `CHECK`, so a
    /// row written before this enum existed — or by a migration — could hold
    /// anything, and refusing to read a facility because of its type would
    /// hide the row rather than fix it. `None` is how such a value reaches the
    /// client, which is honest about not knowing.
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "BUILDING" => Some(Self::Building),
            "FLOOR" => Some(Self::Floor),
            "ROOM" => Some(Self::Room),
            "WAREHOUSE" => Some(Self::Warehouse),
            "SITE" => Some(Self::Site),
            _ => None,
        }
    }
}

/// A facility as the API hands it back.
///
/// `parentFacilityId` and `ownerPartyId` travel as the *business codes* their
/// referents are known by, not as surrogate ids — the same choice the party
/// aggregate makes for `managerPartyId`, so that a client reading a facility
/// sees `PARTY-ACME` rather than a UUID it would have to resolve. `id` is the
/// surrogate, because every route addresses a facility by `{id}`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Facility {
    pub id: Uuid,
    pub facility_id: String,
    pub name: String,
    pub facility_type_id: Option<FacilityType>,
    /// Where the record has got to in its governance lifecycle (FR-MDM-007).
    /// Read-only here: it is moved by `POST /facilities/{id}/transition` and by
    /// nothing else (#99).
    pub record_status_id: RecordStatus,
    /// The `facilityId` of the parent, or `null` at the root of a tree.
    pub parent_facility_id: Option<String>,
    /// The `partyId` of the party that owns the place, if one is recorded.
    pub owner_party_id: Option<String>,
    pub address: Option<PostalAddress>,
    pub additional_attributes: Value,
    pub created_stamp: DateTime<Utc>,
    pub last_updated_stamp: DateTime<Utc>,
}

/// One row of the facility list.
///
/// The same fields minus `address` and `additionalAttributes`: a list of
/// buildings is browsed by name and type, and a page of a hundred addresses is
/// payload nothing on the screen reads.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FacilitySummary {
    pub id: Uuid,
    pub facility_id: String,
    pub name: String,
    pub facility_type_id: Option<FacilityType>,
    pub parent_facility_id: Option<String>,
    pub owner_party_id: Option<String>,
    pub created_stamp: DateTime<Utc>,
    pub last_updated_stamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateFacilityRequest {
    pub facility_id: String,
    pub name: Option<String>,
    pub facility_type_id: Option<FacilityType>,
    pub parent_facility_id: Option<String>,
    pub owner_party_id: Option<String>,
    pub address: Option<PostalAddress>,
    pub additional_attributes: Option<Value>,
}

/// An update carries only what changes.
///
/// Every member is optional and an omitted one leaves the column alone, which
/// is `update_party_fields`' convention. The two references are the exception
/// worth naming: `parentFacilityId` and `ownerPartyId` can be *cleared*, so
/// they are `Option<Option<String>>` — absent means leave it, `null` means
/// detach it. Without that a facility could be given a parent and never taken
/// out from under it.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateFacilityRequest {
    pub name: Option<String>,
    pub facility_type_id: Option<FacilityType>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<String>, nullable)]
    pub parent_facility_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<String>, nullable)]
    pub owner_party_id: Option<Option<String>>,
    pub address: Option<PostalAddress>,
    pub additional_attributes: Option<Value>,
}

/// Tells *absent* from *present and null*, which `Option<String>` alone cannot.
///
/// With `#[serde(default)]` a missing key stays `None` and never reaches here;
/// a key that is present — including `null` — arrives and is wrapped in `Some`.
/// Written out rather than pulled in as a dependency: it is four lines and two
/// fields need it.
fn present_or_absent<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Validates a create payload, collecting every problem rather than stopping at
/// the first (JSON Form Schema S10.3).
pub fn validate_create_facility(request: &CreateFacilityRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    require_code(
        &request.facility_id,
        "facilityId",
        MAX_FACILITY_CODE_LENGTH,
        &mut details,
    );
    require_name(request.name.as_deref(), "name", &mut details);
    bounded_reference(
        request.parent_facility_id.as_deref(),
        "parentFacilityId",
        &mut details,
    );
    bounded_reference(
        request.owner_party_id.as_deref(),
        "ownerPartyId",
        &mut details,
    );
    self_parent(
        &request.facility_id,
        request.parent_facility_id.as_deref(),
        &mut details,
    );

    finish(details)
}

/// Validates an update payload against the facility it is aimed at.
///
/// `facility_code` is the current code, so that a facility naming *itself* as
/// its parent is refused here rather than by the ancestor walk — the cheaper
/// check, and the one that can name the field.
pub fn validate_update_facility(
    request: &UpdateFacilityRequest,
    facility_code: &str,
) -> Result<(), AppError> {
    let mut details = Vec::new();

    if request.name.is_some() {
        require_name(request.name.as_deref(), "name", &mut details);
    }
    if let Some(Some(parent)) = &request.parent_facility_id {
        bounded_reference(Some(parent), "parentFacilityId", &mut details);
        self_parent(facility_code, Some(parent), &mut details);
    }
    if let Some(Some(owner)) = &request.owner_party_id {
        bounded_reference(Some(owner), "ownerPartyId", &mut details);
    }

    finish(details)
}

/// A reference travels as a business code, so it is bounded like one. Whether
/// it resolves is the service's question — this only refuses what could not
/// name anything.
fn bounded_reference(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    match non_empty(value) {
        None if value.is_some() => details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "Must name something, or be omitted",
        )),
        _ => bound_name(value, path, details),
    }
}

/// A facility that is its own parent is a cycle of length one, and the shortest
/// one to catch.
fn self_parent(facility_code: &str, parent: Option<&str>, details: &mut Vec<ValidationDetail>) {
    if let Some(parent) = non_empty(parent) {
        if parent == facility_code.trim() {
            details.push(ValidationDetail::new(
                "parentFacilityId",
                "consistency",
                "CYCLE",
                "A facility cannot be its own parent",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(facility_id: &str) -> CreateFacilityRequest {
        CreateFacilityRequest {
            facility_id: facility_id.to_owned(),
            name: Some("Head Office".to_owned()),
            facility_type_id: Some(FacilityType::Building),
            parent_facility_id: None,
            owner_party_id: None,
            address: None,
            additional_attributes: None,
        }
    }

    #[test]
    fn every_type_round_trips_through_the_database_spelling() {
        for value in [
            FacilityType::Building,
            FacilityType::Floor,
            FacilityType::Room,
            FacilityType::Warehouse,
            FacilityType::Site,
        ] {
            assert_eq!(FacilityType::from_db(value.as_db()), Some(value));
        }
    }

    #[test]
    fn a_type_the_column_could_hold_but_this_enum_does_not_reads_as_unknown() {
        // The column carries no CHECK. Refusing to *read* such a row would hide
        // the facility rather than fix the value.
        assert_eq!(FacilityType::from_db("HANGAR"), None);
    }

    #[test]
    fn a_full_payload_passes() {
        assert!(validate_create_facility(&create("FAC-0001")).is_ok());
    }

    #[test]
    fn a_facility_needs_a_code_and_a_name() {
        let mut request = create("   ");
        request.name = None;

        let error = validate_create_facility(&request).expect_err("both are required");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        let paths: Vec<&str> = details.iter().map(|detail| detail.path.as_str()).collect();
        assert!(paths.contains(&"facilityId"), "{paths:?}");
        assert!(paths.contains(&"name"), "{paths:?}");
    }

    #[test]
    fn a_facility_code_longer_than_the_column_is_refused() {
        let request = create(&"F".repeat(MAX_FACILITY_CODE_LENGTH + 1));

        assert!(validate_create_facility(&request).is_err());
    }

    #[test]
    fn a_facility_cannot_be_created_as_its_own_parent() {
        let mut request = create("FAC-0001");
        request.parent_facility_id = Some("FAC-0001".to_owned());

        let error = validate_create_facility(&request).expect_err("a self-parent is a cycle");
        let AppError::Validation { details } = error else {
            panic!("expected a validation failure");
        };

        assert!(
            details
                .iter()
                .any(|detail| detail.path == "parentFacilityId" && detail.code == "CYCLE"),
            "{details:?}"
        );
    }

    #[test]
    fn an_update_cannot_make_a_facility_its_own_parent_either() {
        let request = UpdateFacilityRequest {
            parent_facility_id: Some(Some("FAC-0001".to_owned())),
            ..UpdateFacilityRequest::default()
        };

        assert!(validate_update_facility(&request, "FAC-0001").is_err());
    }

    #[test]
    fn clearing_a_reference_is_not_the_same_as_leaving_it_alone() {
        // `null` detaches, absent leaves it. Without the distinction a facility
        // could be given a parent and never taken out from under it.
        let cleared: UpdateFacilityRequest =
            serde_json::from_str(r#"{"parentFacilityId": null}"#).expect("parses");
        let untouched: UpdateFacilityRequest = serde_json::from_str("{}").expect("parses");

        assert_eq!(cleared.parent_facility_id, Some(None));
        assert_eq!(untouched.parent_facility_id, None);
    }

    #[test]
    fn a_blank_reference_is_refused_rather_than_read_as_a_clear() {
        // `""` is a client sending an empty form field. It is not `null`, and
        // treating it as one would detach a parent nobody asked to detach.
        let mut request = create("FAC-0001");
        request.parent_facility_id = Some("   ".to_owned());

        assert!(validate_create_facility(&request).is_err());
    }

    #[test]
    fn an_unknown_facility_type_does_not_deserialize() {
        // The other half of "enforced here or nowhere": the column would store
        // HANGAR happily.
        let refused: Result<CreateFacilityRequest, _> = serde_json::from_str(
            r#"{"facilityId": "FAC-0001", "name": "Hangar", "facilityTypeId": "HANGAR"}"#,
        );

        assert!(refused.is_err());
    }

    #[test]
    fn a_payload_with_a_misspelled_field_is_refused_rather_than_ignored() {
        // #62. `facilityType` is not `facilityTypeId`, and a request struct
        // that dropped it silently would create a typeless facility and answer
        // 201.
        let refused: Result<CreateFacilityRequest, _> = serde_json::from_str(
            r#"{"facilityId": "FAC-0001", "name": "Head Office", "facilityType": "BUILDING"}"#,
        );

        assert!(refused.is_err());
    }
}
