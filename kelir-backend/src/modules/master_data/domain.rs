//! Party domain types — the `PartyAggregate` of [architectures/05] in Rust.
//!
//! The aggregate is the API payload shape; Database Schema §4 is its storage.
//! Three things about the mapping are decisions rather than transcriptions, and
//! each is stated where it applies below:
//!
//! * **`id` is added to the response.** The aggregate is
//!   `additionalProperties: false` and carries no surrogate key — its `partyId`
//!   is the business code. Every route addresses a party by `{id}` (naming
//!   convention §5), so a client that had only seen `partyId` could not build a
//!   URL. Nothing else is added: `recordStatus` and the two document references
//!   exist in the schema (AC5) and stay off the wire until Phase 5 gives them a
//!   consumer, because a field nothing can change reads as a control that
//!   exists.
//! * **`roles`, `profiles`, `notes` and `tenantPartyId` are absent.** Roles and
//!   profiles are #81, in this sprint. `notes` and `tenantPartyId` have no
//!   storage in §4 at all — recorded as deviation #16 there.
//! * **Request types carry only writable fields.** A `GET` body posted back
//!   into a `PUT` is refused with a 422 naming the extra field, which is what
//!   `deny_unknown_fields` is for (#62): a request struct that quietly accepted
//!   and ignored `createdStamp` is the shape of defect that change closed.
//!
//! [architectures/05]: ../../../../docs/architectures/05.%20Core%20-%20Master%20Data%20-%20Party.md

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{AppError, ValidationDetail};

/// Longest `partyId` the aggregate allows, and the bound
/// `ck_mdm_parties_party_code_len` holds in the database.
pub const MAX_PARTY_CODE_LENGTH: usize = 60;

/// Longest value for the code columns of the child tables — `VARCHAR(64)` in
/// §4, so a longer value is a 422 rather than a database error.
pub const MAX_CODE_LENGTH: usize = 64;

/// Longest human name or title — `VARCHAR(200)` in §4.
pub const MAX_NAME_LENGTH: usize = 200;

// ---------------------------------------------------------------------------
// Vocabularies
// ---------------------------------------------------------------------------

/// `PERSON` for individual people, `PARTY_GROUP` for companies, tenants,
/// organizations and teams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartyType {
    Person,
    PartyGroup,
}

impl PartyType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Person => "PERSON",
            Self::PartyGroup => "PARTY_GROUP",
        }
    }

    /// Falls back to `PartyGroup` for an unrecognised value, which the `CHECK`
    /// on `mdm_parties.party_type` makes unreachable from a stored row.
    pub fn from_db(value: &str) -> Self {
        match value {
            "PERSON" => Self::Person,
            _ => Self::PartyGroup,
        }
    }
}

/// The party's own enabled/disabled flag (`mdm_parties.status`). Distinct from
/// `record_status`, which is the workflow lifecycle of concepts/03 §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartyStatusCode {
    PartyEnabled,
    PartyDisabled,
}

impl PartyStatusCode {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::PartyEnabled => "PARTY_ENABLED",
            Self::PartyDisabled => "PARTY_DISABLED",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "PARTY_DISABLED" => Self::PartyDisabled,
            _ => Self::PartyEnabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum Gender {
    M,
    F,
    X,
}

impl Gender {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::M => "M",
            Self::F => "F",
            Self::X => "X",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "M" => Some(Self::M),
            "F" => Some(Self::F),
            "X" => Some(Self::X),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContactMechType {
    EmailAddress,
    PhoneNumber,
    MobileNumber,
    PostalAddress,
    WebAddress,
    Other,
}

impl ContactMechType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::EmailAddress => "EMAIL_ADDRESS",
            Self::PhoneNumber => "PHONE_NUMBER",
            Self::MobileNumber => "MOBILE_NUMBER",
            Self::PostalAddress => "POSTAL_ADDRESS",
            Self::WebAddress => "WEB_ADDRESS",
            Self::Other => "OTHER",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "EMAIL_ADDRESS" => Self::EmailAddress,
            "PHONE_NUMBER" => Self::PhoneNumber,
            "MOBILE_NUMBER" => Self::MobileNumber,
            "POSTAL_ADDRESS" => Self::PostalAddress,
            "WEB_ADDRESS" => Self::WebAddress,
            _ => Self::Other,
        }
    }
}

// ---------------------------------------------------------------------------
// Shared child shapes
// ---------------------------------------------------------------------------

/// Postal address detail. Stored inside `mdm_contact_mechs.detail_json`: the
/// Party schema gives no normalization for it and the shapes differ per type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostalAddress {
    pub address_name: Option<String>,
    pub attn_name: Option<String>,
    pub address1: Option<String>,
    pub address2: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub state_province_geo_id: Option<String>,
    pub country_geo_id: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelecomNumber {
    pub country_code: Option<String>,
    pub area_code: Option<String>,
    pub contact_number: Option<String>,
    pub extension: Option<String>,
    pub ask_for: Option<String>,
}

/// The value behind a contact mechanism. Exactly one member is expected to be
/// present; [`ContactMechDetail::display_value`] is what the database stores
/// alongside it as the one-line projection lists and search read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactMechDetail {
    pub postal_address: Option<PostalAddress>,
    pub telecom_number: Option<TelecomNumber>,
    pub email_address: Option<String>,
    pub url: Option<String>,
    pub other: Option<String>,
}

impl ContactMechDetail {
    /// The denormalized one-line value for `mdm_contact_mechs.display_value`.
    ///
    /// Derived server-side and never taken from the client: it is the field a
    /// list renders, so a caller that could set it independently of `detail`
    /// could make a row display as something it is not.
    pub fn display_value(&self) -> Option<String> {
        if let Some(email) = non_empty(self.email_address.as_deref()) {
            return Some(email.to_owned());
        }
        if let Some(url) = non_empty(self.url.as_deref()) {
            return Some(url.to_owned());
        }
        if let Some(telecom) = &self.telecom_number {
            let joined = [
                telecom.country_code.as_deref(),
                telecom.area_code.as_deref(),
                telecom.contact_number.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

            if !joined.is_empty() {
                return Some(match non_empty(telecom.extension.as_deref()) {
                    Some(extension) => format!("{joined} ext. {extension}"),
                    None => joined,
                });
            }
        }
        if let Some(address) = &self.postal_address {
            let joined = [
                address.address1.as_deref(),
                address.address2.as_deref(),
                address.city.as_deref(),
                address.postal_code.as_deref(),
                address.country_geo_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

            if !joined.is_empty() {
                return Some(joined);
            }
        }

        non_empty(self.other.as_deref()).map(str::to_owned)
    }
}

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

/// A party and everything hanging off it (FR-MDM-001, FR-MDM-003).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyAggregate {
    /// Surrogate key. Not an aggregate field — see the module documentation.
    pub id: Uuid,
    /// The aggregate's `partyId`: the business code, unique per tenant.
    pub party_id: String,
    pub party_type_id: PartyType,
    pub status_id: PartyStatusCode,
    pub external_id: Option<String>,
    pub description: Option<String>,
    /// Present exactly when `partyTypeId` is `PERSON`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person: Option<Person>,
    /// Present exactly when `partyTypeId` is `PARTY_GROUP`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub party_group: Option<PartyGroup>,
    pub identifications: Vec<PartyIdentification>,
    /// Status history, oldest first. Server-written: a client changes
    /// `statusId` and the history entry follows.
    pub statuses: Vec<PartyStatus>,
    pub relationships_from: Vec<PartyRelationship>,
    pub relationships_to: Vec<PartyRelationship>,
    pub classifications: Vec<PartyClassification>,
    pub contact_mechanisms: Vec<PartyContactMech>,
    pub additional_attributes: Value,
    pub created_stamp: DateTime<Utc>,
    pub last_updated_stamp: DateTime<Utc>,
}

/// A party as a list row.
///
/// The full aggregate needs six child queries per party, which a page of a
/// hundred would multiply into six hundred. `name` is the projection a list
/// actually renders — the person's full name or the group's name — and is the
/// reason this is a separate shape rather than a trimmed aggregate. Search,
/// filter and the UI that consumes them are FR-MDM-008, in Sprint 6.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartySummary {
    pub id: Uuid,
    pub party_id: String,
    pub party_type_id: PartyType,
    pub status_id: PartyStatusCode,
    pub name: String,
    pub external_id: Option<String>,
    pub created_stamp: DateTime<Utc>,
    pub last_updated_stamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub party_id: String,
    pub first_name: String,
    pub middle_name: Option<String>,
    pub last_name: String,
    pub personal_title: Option<String>,
    pub suffix: Option<String>,
    pub gender: Option<Gender>,
    pub birth_date: Option<NaiveDate>,
    pub marital_status: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyGroup {
    pub party_id: String,
    pub group_name: String,
    pub local_name: Option<String>,
    pub office_site_name: Option<String>,
    /// `NUMERIC(18,2)` in the database, carried as a string so no precision is
    /// lost passing through a JSON double.
    pub annual_revenue: Option<String>,
    pub num_employees: Option<i32>,
    pub ticker_symbol: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyIdentification {
    pub party_identification_type_id: String,
    pub id_value: String,
    pub issued_by: Option<String>,
    pub issue_date: Option<NaiveDate>,
    pub expire_date: Option<NaiveDate>,
    pub additional_attributes: Value,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyStatus {
    pub status_id: String,
    pub status_date: DateTime<Utc>,
    /// The aggregate's `changedByUserLogin`. Null for a status a migration or a
    /// worker wrote, which has no acting user.
    pub changed_by_user_login: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyRelationship {
    pub party_relationship_id: Uuid,
    pub party_id_from: String,
    pub role_type_id_from: Option<String>,
    pub party_id_to: String,
    pub role_type_id_to: Option<String>,
    pub party_relationship_type_id: String,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub status_id: Option<String>,
    pub priority: Option<i32>,
    pub comments: Option<String>,
    pub additional_attributes: Value,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyClassification {
    pub party_class_type_id: String,
    pub party_classification_id: Option<String>,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartyContactMech {
    pub contact_mech_id: Uuid,
    pub contact_mech_type_id: ContactMechType,
    pub purpose_type_id: Option<String>,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub is_primary: bool,
    pub allow_solicitation: bool,
    pub detail: ContactMechDetail,
    pub additional_attributes: Value,
}

// ---------------------------------------------------------------------------
// Request shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePartyRequest {
    pub party_id: String,
    pub party_type_id: PartyType,
    pub status_id: Option<PartyStatusCode>,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub person: Option<PersonInput>,
    pub party_group: Option<PartyGroupInput>,
    #[serde(default)]
    pub identifications: Vec<PartyIdentificationInput>,
    #[serde(default)]
    pub relationships_from: Vec<PartyRelationshipInput>,
    #[serde(default)]
    pub relationships_to: Vec<PartyRelationshipInput>,
    #[serde(default)]
    pub classifications: Vec<PartyClassificationInput>,
    #[serde(default)]
    pub contact_mechanisms: Vec<PartyContactMechInput>,
    pub additional_attributes: Option<Value>,
}

/// A change to a party. Every field is optional and `None` means *leave alone*,
/// the same convention `UpdateUserRequest` uses.
///
/// `partyId` and `partyTypeId` are absent because neither may change: the code
/// is what `master_data_source_references` points external systems at, and the
/// type is what decides which extension table holds the party's detail. A
/// collection that *is* sent replaces the stored set wholesale — the same shape
/// as `roleIds` on a user.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePartyRequest {
    pub status_id: Option<PartyStatusCode>,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub person: Option<PersonInput>,
    pub party_group: Option<PartyGroupInput>,
    pub identifications: Option<Vec<PartyIdentificationInput>>,
    pub relationships_from: Option<Vec<PartyRelationshipInput>>,
    pub relationships_to: Option<Vec<PartyRelationshipInput>>,
    pub classifications: Option<Vec<PartyClassificationInput>>,
    pub contact_mechanisms: Option<Vec<PartyContactMechInput>>,
    pub additional_attributes: Option<Value>,
    /// Recorded on the status-history row when `statusId` changes. Ignored
    /// otherwise — there is nothing for it to annotate.
    pub status_comments: Option<String>,
}

/// Person detail. `partyId` is accepted because the aggregate carries it inside
/// `person`; when sent it must equal the party's own `partyId`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonInput {
    pub party_id: Option<String>,
    pub first_name: Option<String>,
    pub middle_name: Option<String>,
    pub last_name: Option<String>,
    pub personal_title: Option<String>,
    pub suffix: Option<String>,
    pub gender: Option<Gender>,
    pub birth_date: Option<NaiveDate>,
    pub marital_status: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyGroupInput {
    pub party_id: Option<String>,
    pub group_name: Option<String>,
    pub local_name: Option<String>,
    pub office_site_name: Option<String>,
    /// Decimal as a string, matching the response. A JSON number would round.
    pub annual_revenue: Option<String>,
    pub num_employees: Option<i32>,
    pub ticker_symbol: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyIdentificationInput {
    pub party_identification_type_id: String,
    pub id_value: String,
    pub issued_by: Option<String>,
    pub issue_date: Option<NaiveDate>,
    pub expire_date: Option<NaiveDate>,
    pub additional_attributes: Option<Value>,
}

/// One relationship. The party being written must be on the side the collection
/// names — `partyIdFrom` for `relationshipsFrom`, `partyIdTo` for
/// `relationshipsTo` — and the counterparty must already exist in the tenant.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyRelationshipInput {
    pub party_id_from: String,
    pub role_type_id_from: Option<String>,
    pub party_id_to: String,
    pub role_type_id_to: Option<String>,
    pub party_relationship_type_id: String,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub status_id: Option<String>,
    pub priority: Option<i32>,
    pub comments: Option<String>,
    pub additional_attributes: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyClassificationInput {
    pub party_class_type_id: String,
    pub party_classification_id: Option<String>,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    pub comments: Option<String>,
}

/// One contact mechanism on a party.
///
/// Either `contactMechId` names an existing mechanism in the tenant — the point
/// of `mdm_contact_mechs` being a table of its own is that two parties can
/// share a switchboard number — or `detail` supplies a new one. Sending both is
/// a validation failure rather than a silent preference.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartyContactMechInput {
    pub contact_mech_id: Option<Uuid>,
    pub contact_mech_type_id: Option<ContactMechType>,
    pub purpose_type_id: Option<String>,
    pub from_date: DateTime<Utc>,
    pub thru_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default = "default_true")]
    pub allow_solicitation: bool,
    pub detail: Option<ContactMechDetail>,
    pub additional_attributes: Option<Value>,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validates a create payload, collecting every problem rather than stopping at
/// the first — a form should be able to mark all its bad fields at once
/// (JSON Form Schema S10.3).
pub fn validate_create_party(request: &CreatePartyRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    validate_party_code(&request.party_id, &mut details);

    match request.party_type_id {
        PartyType::Person => {
            match &request.person {
                Some(person) => validate_person(person, &request.party_id, true, &mut details),
                None => details.push(ValidationDetail::new(
                    "person",
                    "required",
                    "REQUIRED",
                    "A PERSON party requires person detail",
                )),
            }
            if request.party_group.is_some() {
                details.push(ValidationDetail::new(
                    "partyGroup",
                    "conflict",
                    "NOT_ALLOWED",
                    "A PERSON party cannot carry partyGroup detail",
                ));
            }
        }
        PartyType::PartyGroup => {
            match &request.party_group {
                Some(group) => validate_party_group(group, &request.party_id, true, &mut details),
                None => details.push(ValidationDetail::new(
                    "partyGroup",
                    "required",
                    "REQUIRED",
                    "A PARTY_GROUP party requires partyGroup detail",
                )),
            }
            if request.person.is_some() {
                details.push(ValidationDetail::new(
                    "person",
                    "conflict",
                    "NOT_ALLOWED",
                    "A PARTY_GROUP party cannot carry person detail",
                ));
            }
        }
    }

    validate_children(
        &request.party_id,
        &request.identifications,
        &request.relationships_from,
        &request.relationships_to,
        &request.classifications,
        &request.contact_mechanisms,
        &mut details,
    );

    finish(details)
}

/// Validates a change. The party's own type decides which extension detail is
/// allowed, so it is passed in rather than read from the payload — the update
/// request deliberately cannot restate it.
pub fn validate_update_party(
    request: &UpdatePartyRequest,
    party_code: &str,
    party_type: PartyType,
) -> Result<(), AppError> {
    let mut details = Vec::new();

    match (party_type, &request.person, &request.party_group) {
        (PartyType::Person, Some(person), _) => {
            validate_person(person, party_code, false, &mut details);
        }
        (PartyType::Person, _, Some(_)) => details.push(ValidationDetail::new(
            "partyGroup",
            "conflict",
            "NOT_ALLOWED",
            "This party is a PERSON; it has no partyGroup detail to change",
        )),
        (PartyType::PartyGroup, _, Some(group)) => {
            validate_party_group(group, party_code, false, &mut details);
        }
        (PartyType::PartyGroup, Some(_), _) => details.push(ValidationDetail::new(
            "person",
            "conflict",
            "NOT_ALLOWED",
            "This party is a PARTY_GROUP; it has no person detail to change",
        )),
        _ => {}
    }

    validate_children(
        party_code,
        request.identifications.as_deref().unwrap_or(&[]),
        request.relationships_from.as_deref().unwrap_or(&[]),
        request.relationships_to.as_deref().unwrap_or(&[]),
        request.classifications.as_deref().unwrap_or(&[]),
        request.contact_mechanisms.as_deref().unwrap_or(&[]),
        &mut details,
    );

    finish(details)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one parameter per collection; a wrapper struct would only move the list"
)]
fn validate_children(
    party_code: &str,
    identifications: &[PartyIdentificationInput],
    relationships_from: &[PartyRelationshipInput],
    relationships_to: &[PartyRelationshipInput],
    classifications: &[PartyClassificationInput],
    contact_mechanisms: &[PartyContactMechInput],
    details: &mut Vec<ValidationDetail>,
) {
    for (index, identification) in identifications.iter().enumerate() {
        let path = format!("identifications[{index}]");
        require_code(
            &identification.party_identification_type_id,
            &format!("{path}.partyIdentificationTypeId"),
            MAX_CODE_LENGTH,
            details,
        );
        require_code(
            &identification.id_value,
            &format!("{path}.idValue"),
            MAX_CODE_LENGTH,
            details,
        );
    }

    for (index, relationship) in relationships_from.iter().enumerate() {
        validate_relationship(
            relationship,
            party_code,
            &format!("relationshipsFrom[{index}]"),
            RelationshipSide::From,
            details,
        );
    }

    for (index, relationship) in relationships_to.iter().enumerate() {
        validate_relationship(
            relationship,
            party_code,
            &format!("relationshipsTo[{index}]"),
            RelationshipSide::To,
            details,
        );
    }

    for (index, classification) in classifications.iter().enumerate() {
        require_code(
            &classification.party_class_type_id,
            &format!("classifications[{index}].partyClassTypeId"),
            MAX_CODE_LENGTH,
            details,
        );
    }

    for (index, mechanism) in contact_mechanisms.iter().enumerate() {
        validate_contact_mech(mechanism, &format!("contactMechanisms[{index}]"), details);
    }
}

enum RelationshipSide {
    From,
    To,
}

fn validate_relationship(
    relationship: &PartyRelationshipInput,
    party_code: &str,
    path: &str,
    side: RelationshipSide,
    details: &mut Vec<ValidationDetail>,
) {
    require_code(
        &relationship.party_relationship_type_id,
        &format!("{path}.partyRelationshipTypeId"),
        MAX_CODE_LENGTH,
        details,
    );

    let (own, own_field) = match side {
        RelationshipSide::From => (&relationship.party_id_from, "partyIdFrom"),
        RelationshipSide::To => (&relationship.party_id_to, "partyIdTo"),
    };

    if own.trim() != party_code.trim() {
        details.push(ValidationDetail::new(
            format!("{path}.{own_field}"),
            "consistency",
            "MISMATCH",
            format!("{own_field} must be this party's partyId in this collection"),
        ));
    }

    if relationship.party_id_from.trim() == relationship.party_id_to.trim() {
        // Self-relationships are the shape that makes an ORGANIZATION_ROLLUP
        // traversal loop forever, and no relationship type in §4 has a meaning
        // when both ends are the same party.
        details.push(ValidationDetail::new(
            format!("{path}.partyIdTo"),
            "consistency",
            "SELF_REFERENCE",
            "A party cannot be related to itself",
        ));
    }

    if let Some(thru) = relationship.thru_date {
        if thru < relationship.from_date {
            details.push(ValidationDetail::new(
                format!("{path}.thruDate"),
                "range",
                "OUT_OF_RANGE",
                "thruDate cannot be before fromDate",
            ));
        }
    }
}

fn validate_contact_mech(
    mechanism: &PartyContactMechInput,
    path: &str,
    details: &mut Vec<ValidationDetail>,
) {
    match (&mechanism.contact_mech_id, &mechanism.detail) {
        (Some(_), Some(_)) => details.push(ValidationDetail::new(
            format!("{path}.detail"),
            "conflict",
            "NOT_ALLOWED",
            "Send either contactMechId to reuse an existing mechanism, or detail to create one",
        )),
        (None, None) => details.push(ValidationDetail::new(
            format!("{path}.detail"),
            "required",
            "REQUIRED",
            "A contact mechanism needs detail, or a contactMechId to reuse",
        )),
        (None, Some(detail)) => {
            if mechanism.contact_mech_type_id.is_none() {
                details.push(ValidationDetail::new(
                    format!("{path}.contactMechTypeId"),
                    "required",
                    "REQUIRED",
                    "A new contact mechanism needs its type",
                ));
            }
            if detail.display_value().is_none() {
                details.push(ValidationDetail::new(
                    format!("{path}.detail"),
                    "required",
                    "REQUIRED",
                    "detail carries no value to display",
                ));
            }
        }
        (Some(_), None) => {}
    }

    if let Some(thru) = mechanism.thru_date {
        if thru < mechanism.from_date {
            details.push(ValidationDetail::new(
                format!("{path}.thruDate"),
                "range",
                "OUT_OF_RANGE",
                "thruDate cannot be before fromDate",
            ));
        }
    }
}

fn validate_person(
    person: &PersonInput,
    party_code: &str,
    required: bool,
    details: &mut Vec<ValidationDetail>,
) {
    echoed_party_id(person.party_id.as_deref(), party_code, "person", details);

    if required || person.first_name.is_some() {
        require_name(person.first_name.as_deref(), "person.firstName", details);
    }
    if required || person.last_name.is_some() {
        require_name(person.last_name.as_deref(), "person.lastName", details);
    }

    bound_name(person.middle_name.as_deref(), "person.middleName", details);
    bound_name(
        person.personal_title.as_deref(),
        "person.personalTitle",
        details,
    );
    bound_name(person.suffix.as_deref(), "person.suffix", details);
}

fn validate_party_group(
    group: &PartyGroupInput,
    party_code: &str,
    required: bool,
    details: &mut Vec<ValidationDetail>,
) {
    echoed_party_id(group.party_id.as_deref(), party_code, "partyGroup", details);

    if required || group.group_name.is_some() {
        require_name(group.group_name.as_deref(), "partyGroup.groupName", details);
    }

    bound_name(group.local_name.as_deref(), "partyGroup.localName", details);
    bound_name(
        group.office_site_name.as_deref(),
        "partyGroup.officeSiteName",
        details,
    );

    if let Some(revenue) = non_empty(group.annual_revenue.as_deref()) {
        if revenue.parse::<f64>().is_err() {
            details.push(ValidationDetail::new(
                "partyGroup.annualRevenue",
                "format",
                "INVALID_FORMAT",
                "annualRevenue must be a decimal number",
            ));
        }
    }
}

/// The aggregate repeats `partyId` inside `person` and `partyGroup`. A value
/// that disagrees with the party's own code is a mistake worth naming rather
/// than a field to ignore.
fn echoed_party_id(
    echoed: Option<&str>,
    party_code: &str,
    path: &str,
    details: &mut Vec<ValidationDetail>,
) {
    if let Some(value) = non_empty(echoed) {
        if value != party_code.trim() {
            details.push(ValidationDetail::new(
                format!("{path}.partyId"),
                "consistency",
                "MISMATCH",
                "partyId must match the party it belongs to",
            ));
        }
    }
}

fn validate_party_code(party_code: &str, details: &mut Vec<ValidationDetail>) {
    let trimmed = party_code.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            "partyId",
            "required",
            "REQUIRED",
            "partyId is required",
        ));
    } else if trimmed.chars().count() > MAX_PARTY_CODE_LENGTH {
        details.push(ValidationDetail::new(
            "partyId",
            "maxLength",
            "TOO_LONG",
            format!("partyId must be at most {MAX_PARTY_CODE_LENGTH} characters"),
        ));
    }
}

fn require_code(value: &str, path: &str, max: usize, details: &mut Vec<ValidationDetail>) {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "This field is required",
        ));
    } else if trimmed.chars().count() > max {
        details.push(ValidationDetail::new(
            path,
            "maxLength",
            "TOO_LONG",
            format!("Must be at most {max} characters"),
        ));
    }
}

fn require_name(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    match non_empty(value) {
        None => details.push(ValidationDetail::new(
            path,
            "required",
            "REQUIRED",
            "This field is required",
        )),
        Some(name) => bound_name(Some(name), path, details),
    }
}

fn bound_name(value: Option<&str>, path: &str, details: &mut Vec<ValidationDetail>) {
    if let Some(name) = non_empty(value) {
        if name.chars().count() > MAX_NAME_LENGTH {
            details.push(ValidationDetail::new(
                path,
                "maxLength",
                "TOO_LONG",
                format!("Must be at most {MAX_NAME_LENGTH} characters"),
            ));
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn finish(details: Vec<ValidationDetail>) -> Result<(), AppError> {
    if details.is_empty() {
        Ok(())
    } else {
        Err(AppError::validation(details))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person_request() -> CreatePartyRequest {
        CreatePartyRequest {
            party_id: "PARTY-0001".to_owned(),
            party_type_id: PartyType::Person,
            status_id: None,
            external_id: None,
            description: None,
            person: Some(PersonInput {
                first_name: Some("Jane".to_owned()),
                last_name: Some("Doe".to_owned()),
                ..PersonInput::default()
            }),
            party_group: None,
            identifications: vec![],
            relationships_from: vec![],
            relationships_to: vec![],
            classifications: vec![],
            contact_mechanisms: vec![],
            additional_attributes: None,
        }
    }

    fn group_request() -> CreatePartyRequest {
        CreatePartyRequest {
            party_type_id: PartyType::PartyGroup,
            person: None,
            party_group: Some(PartyGroupInput {
                group_name: Some("Acme Supplies".to_owned()),
                ..PartyGroupInput::default()
            }),
            ..person_request()
        }
    }

    fn paths(error: AppError) -> Vec<String> {
        match error {
            AppError::Validation { details } => {
                details.into_iter().map(|detail| detail.path).collect()
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_person_and_a_party_group() {
        assert!(validate_create_party(&person_request()).is_ok());
        assert!(validate_create_party(&group_request()).is_ok());
    }

    #[test]
    fn a_person_party_requires_person_detail_and_refuses_group_detail() {
        // The aggregate's `allOf` says exactly this; without it a PERSON party
        // could be stored with a party-group extension row and no name.
        let missing = CreatePartyRequest {
            person: None,
            ..person_request()
        };
        assert!(paths(validate_create_party(&missing).expect_err("invalid"))
            .contains(&"person".to_owned()));

        let both = CreatePartyRequest {
            party_group: Some(PartyGroupInput {
                group_name: Some("Acme".to_owned()),
                ..PartyGroupInput::default()
            }),
            ..person_request()
        };
        assert!(paths(validate_create_party(&both).expect_err("invalid"))
            .contains(&"partyGroup".to_owned()));
    }

    #[test]
    fn a_party_group_requires_group_detail_and_refuses_person_detail() {
        let missing = CreatePartyRequest {
            party_group: None,
            ..group_request()
        };
        assert!(paths(validate_create_party(&missing).expect_err("invalid"))
            .contains(&"partyGroup".to_owned()));

        let both = CreatePartyRequest {
            person: Some(PersonInput {
                first_name: Some("Jane".to_owned()),
                last_name: Some("Doe".to_owned()),
                ..PersonInput::default()
            }),
            ..group_request()
        };
        assert!(paths(validate_create_party(&both).expect_err("invalid"))
            .contains(&"person".to_owned()));
    }

    #[test]
    fn reports_every_problem_at_once() {
        let bad = CreatePartyRequest {
            party_id: "   ".to_owned(),
            person: Some(PersonInput {
                first_name: Some("".to_owned()),
                last_name: None,
                ..PersonInput::default()
            }),
            identifications: vec![PartyIdentificationInput {
                party_identification_type_id: "".to_owned(),
                id_value: "".to_owned(),
                issued_by: None,
                issue_date: None,
                expire_date: None,
                additional_attributes: None,
            }],
            ..person_request()
        };

        let reported = paths(validate_create_party(&bad).expect_err("invalid"));

        for expected in [
            "partyId",
            "person.firstName",
            "person.lastName",
            "identifications[0].partyIdentificationTypeId",
            "identifications[0].idValue",
        ] {
            assert!(
                reported.contains(&expected.to_owned()),
                "{expected} missing from {reported:?}"
            );
        }
    }

    #[test]
    fn enforces_the_party_code_bound_at_the_boundary() {
        // The database CHECK is the same 60; a payload above it must be a 422
        // rather than a constraint violation surfacing as a 500.
        let at_limit = CreatePartyRequest {
            party_id: "P".repeat(MAX_PARTY_CODE_LENGTH),
            ..person_request()
        };
        assert!(validate_create_party(&at_limit).is_ok());

        let over = CreatePartyRequest {
            party_id: "P".repeat(MAX_PARTY_CODE_LENGTH + 1),
            ..person_request()
        };
        assert!(paths(validate_create_party(&over).expect_err("invalid"))
            .contains(&"partyId".to_owned()));
    }

    #[test]
    fn an_echoed_party_id_must_agree_with_the_party() {
        let disagreeing = CreatePartyRequest {
            person: Some(PersonInput {
                party_id: Some("SOMEONE-ELSE".to_owned()),
                first_name: Some("Jane".to_owned()),
                last_name: Some("Doe".to_owned()),
                ..PersonInput::default()
            }),
            ..person_request()
        };

        assert!(
            paths(validate_create_party(&disagreeing).expect_err("invalid"))
                .contains(&"person.partyId".to_owned())
        );
    }

    #[test]
    fn a_relationship_must_name_this_party_on_its_own_side() {
        let relationship = |from: &str, to: &str| PartyRelationshipInput {
            party_id_from: from.to_owned(),
            role_type_id_from: None,
            party_id_to: to.to_owned(),
            role_type_id_to: None,
            party_relationship_type_id: "EMPLOYMENT".to_owned(),
            from_date: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is a valid timestamp"),
            thru_date: None,
            status_id: None,
            priority: None,
            comments: None,
            additional_attributes: None,
        };

        let wrong_side = CreatePartyRequest {
            relationships_from: vec![relationship("OTHER", "PARTY-0001")],
            ..person_request()
        };
        assert!(
            paths(validate_create_party(&wrong_side).expect_err("invalid"))
                .contains(&"relationshipsFrom[0].partyIdFrom".to_owned())
        );

        let right_side = CreatePartyRequest {
            relationships_from: vec![relationship("PARTY-0001", "OTHER")],
            ..person_request()
        };
        assert!(validate_create_party(&right_side).is_ok());

        let itself = CreatePartyRequest {
            relationships_from: vec![relationship("PARTY-0001", "PARTY-0001")],
            ..person_request()
        };
        assert!(paths(validate_create_party(&itself).expect_err("invalid"))
            .contains(&"relationshipsFrom[0].partyIdTo".to_owned()));
    }

    #[test]
    fn a_contact_mechanism_takes_a_reference_or_a_detail_but_not_both() {
        let mechanism = |id: Option<Uuid>, detail: Option<ContactMechDetail>| CreatePartyRequest {
            contact_mechanisms: vec![PartyContactMechInput {
                contact_mech_id: id,
                contact_mech_type_id: Some(ContactMechType::EmailAddress),
                purpose_type_id: None,
                from_date: DateTime::<Utc>::from_timestamp(0, 0)
                    .expect("epoch is a valid timestamp"),
                thru_date: None,
                is_primary: false,
                allow_solicitation: true,
                detail,
                additional_attributes: None,
            }],
            ..person_request()
        };

        let email = ContactMechDetail {
            email_address: Some("jane@acme.example".to_owned()),
            ..ContactMechDetail::default()
        };

        assert!(validate_create_party(&mechanism(None, Some(email.clone()))).is_ok());
        assert!(validate_create_party(&mechanism(Some(Uuid::now_v7()), None)).is_ok());
        assert!(validate_create_party(&mechanism(None, None)).is_err());
        assert!(validate_create_party(&mechanism(Some(Uuid::now_v7()), Some(email))).is_err());
    }

    #[test]
    fn an_update_may_not_change_the_extension_the_party_does_not_have() {
        let change = UpdatePartyRequest {
            status_id: None,
            external_id: None,
            description: None,
            person: None,
            party_group: Some(PartyGroupInput {
                group_name: Some("Acme".to_owned()),
                ..PartyGroupInput::default()
            }),
            identifications: None,
            relationships_from: None,
            relationships_to: None,
            classifications: None,
            contact_mechanisms: None,
            additional_attributes: None,
            status_comments: None,
        };

        let error = validate_update_party(&change, "PARTY-0001", PartyType::Person)
            .expect_err("a person has no party group");

        assert!(paths(error).contains(&"partyGroup".to_owned()));
    }

    #[test]
    fn an_update_may_send_one_field_of_an_extension() {
        // `None` means leave alone, so an update that renames a person must not
        // be forced to resend the name it is not changing.
        let change = UpdatePartyRequest {
            status_id: None,
            external_id: None,
            description: None,
            person: Some(PersonInput {
                last_name: Some("Roe".to_owned()),
                ..PersonInput::default()
            }),
            party_group: None,
            identifications: None,
            relationships_from: None,
            relationships_to: None,
            classifications: None,
            contact_mechanisms: None,
            additional_attributes: None,
            status_comments: None,
        };

        assert!(validate_update_party(&change, "PARTY-0001", PartyType::Person).is_ok());
    }

    #[test]
    fn the_display_value_is_derived_from_whichever_detail_is_present() {
        let email = ContactMechDetail {
            email_address: Some("jane@acme.example".to_owned()),
            ..ContactMechDetail::default()
        };
        assert_eq!(email.display_value().as_deref(), Some("jane@acme.example"));

        let telecom = ContactMechDetail {
            telecom_number: Some(TelecomNumber {
                country_code: Some("+62".to_owned()),
                area_code: Some("21".to_owned()),
                contact_number: Some("555 0100".to_owned()),
                extension: Some("12".to_owned()),
                ask_for: None,
            }),
            ..ContactMechDetail::default()
        };
        assert_eq!(
            telecom.display_value().as_deref(),
            Some("+62 21 555 0100 ext. 12")
        );

        let postal = ContactMechDetail {
            postal_address: Some(PostalAddress {
                address1: Some("1 Jalan Merdeka".to_owned()),
                city: Some("Jakarta".to_owned()),
                postal_code: Some("10110".to_owned()),
                country_geo_id: Some("IDN".to_owned()),
                ..PostalAddress::default()
            }),
            ..ContactMechDetail::default()
        };
        assert_eq!(
            postal.display_value().as_deref(),
            Some("1 Jalan Merdeka, Jakarta, 10110, IDN")
        );

        assert_eq!(ContactMechDetail::default().display_value(), None);
    }

    #[test]
    fn an_all_whitespace_detail_has_no_display_value() {
        // Otherwise a mechanism could be stored displaying as a blank line,
        // which the NOT NULL on display_value cannot catch.
        let blank = ContactMechDetail {
            email_address: Some("   ".to_owned()),
            other: Some("".to_owned()),
            ..ContactMechDetail::default()
        };

        assert_eq!(blank.display_value(), None);
    }

    #[test]
    fn vocabularies_round_trip_through_the_database() {
        for party_type in [PartyType::Person, PartyType::PartyGroup] {
            assert_eq!(PartyType::from_db(party_type.as_db()), party_type);
        }
        for status in [
            PartyStatusCode::PartyEnabled,
            PartyStatusCode::PartyDisabled,
        ] {
            assert_eq!(PartyStatusCode::from_db(status.as_db()), status);
        }
        for gender in [Gender::M, Gender::F, Gender::X] {
            assert_eq!(Gender::from_db(gender.as_db()), Some(gender));
        }
        for mech in [
            ContactMechType::EmailAddress,
            ContactMechType::PhoneNumber,
            ContactMechType::MobileNumber,
            ContactMechType::PostalAddress,
            ContactMechType::WebAddress,
            ContactMechType::Other,
        ] {
            assert_eq!(ContactMechType::from_db(mech.as_db()), mech);
        }
    }
}
