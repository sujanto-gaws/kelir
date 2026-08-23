//! The party itself: its type, its person or party-group detail, and the
//! identifications, statuses, relationships, classifications and contact
//! mechanisms that hang off it.
//!
//! The aggregate this builds up to, and the three decisions behind how it maps
//! onto Database Schema §4, are documented on the parent module.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{non_empty, PartyProfiles, PartyRole};

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
    /// The roles this party holds, and the profiles behind them.
    ///
    /// Absent — not empty — for a caller who holds `master-data:party:read`
    /// without `master-data:party-role:read`. The distinction is the point:
    /// `[]` says this party holds no roles, and absence says you cannot see.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<PartyRole>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<PartyProfiles>,
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
