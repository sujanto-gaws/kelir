//! Validating a party payload before anything is written.
//!
//! Split out of `domain/party.rs` by #112 with no behaviour change. The types
//! being validated are next door in [`super::party`]; what is here is every
//! rule that decides whether one of them may be stored.
//!
//! Every rule collects into one `Vec<ValidationDetail>` rather than returning
//! at the first problem, so a form can mark all its bad fields at once (JSON
//! Form Schema S10.3).

use super::party::*;
use super::{
    bound_name, bounded, echoed_party_id, finish, non_empty, require_code, require_name,
    MAX_CODE_LENGTH, MAX_EXTERNAL_ID_LENGTH, MAX_PARTY_CODE_LENGTH, MAX_VOCABULARY_LENGTH,
};
use crate::error::{AppError, ValidationDetail};

/// Validates a create payload, collecting every problem rather than stopping at
/// the first — a form should be able to mark all its bad fields at once
/// (JSON Form Schema S10.3).
pub fn validate_create_party(request: &CreatePartyRequest) -> Result<(), AppError> {
    let mut details = Vec::new();

    validate_party_code(&request.party_id, &mut details);
    bounded(
        request.external_id.as_deref(),
        "externalId",
        MAX_EXTERNAL_ID_LENGTH,
        &mut details,
    );

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

    bounded(
        request.external_id.as_deref(),
        "externalId",
        MAX_EXTERNAL_ID_LENGTH,
        &mut details,
    );

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
        bound_name(
            identification.issued_by.as_deref(),
            &format!("{path}.issuedBy"),
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
        let path = format!("classifications[{index}]");
        require_code(
            &classification.party_class_type_id,
            &format!("{path}.partyClassTypeId"),
            MAX_CODE_LENGTH,
            details,
        );
        bounded(
            classification.party_classification_id.as_deref(),
            &format!("{path}.partyClassificationId"),
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

    bounded(
        relationship.status_id.as_deref(),
        &format!("{path}.statusId"),
        MAX_VOCABULARY_LENGTH,
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

    // The defect #109 reports: `purpose_type` is `VARCHAR(64)` and nothing
    // bounded the field that fills it, so an over-long value reached the INSERT
    // and came back as a 500. Every other code field on this surface already
    // went through `require_code`; this one was missed.
    bounded(
        mechanism.purpose_type_id.as_deref(),
        &format!("{path}.purposeTypeId"),
        MAX_CODE_LENGTH,
        details,
    );

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

    bounded(
        person.marital_status.as_deref(),
        "person.maritalStatus",
        MAX_VOCABULARY_LENGTH,
        details,
    );

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

    bounded(
        group.ticker_symbol.as_deref(),
        "partyGroup.tickerSymbol",
        MAX_CODE_LENGTH,
        details,
    );

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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

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

    fn epoch() -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is a valid timestamp")
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

    /// The sweep #109 asked for, as an assertion rather than a note.
    ///
    /// Each entry is a field that reaches a bounded `VARCHAR` column. Before
    /// this, seven of them had no bound in front of them and an over-long value
    /// reached the INSERT — a `sqlx` error, an `INTERNAL_ERROR`, and a 500 where
    /// the contract promises a 422 naming the field.
    ///
    /// **The boundary of the sweep.** It covers every `String` field of
    /// `CreatePartyRequest`, `UpdatePartyRequest` and their five child inputs
    /// that is written to a `VARCHAR` column. It does *not* cover: fields
    /// written to `TEXT` columns (`description`, every `comments`, a mechanism's
    /// `displayValue`), which have no width to exceed; fields that are enums
    /// here and `CHECK`-constrained there (`partyTypeId`, `statusId` on the
    /// party, `gender`, `contactMechTypeId`); and the business codes that are
    /// *resolved to a UUID* before anything is written (`partyIdFrom`/`To`,
    /// `roleTypeIdFrom`/`To`, and the three party references on the role
    /// profiles) — an over-long value there does not match a row, which is a
    /// 422 already.
    #[test]
    fn every_string_field_that_reaches_a_varchar_column_is_bounded() {
        let over_64 = "x".repeat(MAX_CODE_LENGTH + 1);
        let over_40 = "x".repeat(MAX_VOCABULARY_LENGTH + 1);
        let over_200 = "x".repeat(crate::modules::master_data::domain::MAX_NAME_LENGTH + 1);
        let over_255 = "x".repeat(MAX_EXTERNAL_ID_LENGTH + 1);

        let request = CreatePartyRequest {
            external_id: Some(over_255),
            person: Some(PersonInput {
                first_name: Some("Jane".to_owned()),
                last_name: Some("Doe".to_owned()),
                marital_status: Some(over_40.clone()),
                ..PersonInput::default()
            }),
            identifications: vec![PartyIdentificationInput {
                party_identification_type_id: "PASSPORT_NUMBER".to_owned(),
                id_value: "X1".to_owned(),
                issued_by: Some(over_200),
                issue_date: None,
                expire_date: None,
                additional_attributes: None,
            }],
            classifications: vec![PartyClassificationInput {
                party_class_type_id: "CONTACT_TIER".to_owned(),
                party_classification_id: Some(over_64.clone()),
                from_date: epoch(),
                thru_date: None,
                comments: None,
            }],
            relationships_from: vec![PartyRelationshipInput {
                party_id_from: "PARTY-0001".to_owned(),
                role_type_id_from: None,
                party_id_to: "PARTY-0002".to_owned(),
                role_type_id_to: None,
                party_relationship_type_id: "EMPLOYMENT".to_owned(),
                from_date: epoch(),
                thru_date: None,
                status_id: Some(over_40),
                priority: None,
                comments: None,
                additional_attributes: None,
            }],
            contact_mechanisms: vec![PartyContactMechInput {
                contact_mech_id: Some(Uuid::nil()),
                contact_mech_type_id: None,
                purpose_type_id: Some(over_64),
                from_date: epoch(),
                thru_date: None,
                is_primary: false,
                allow_solicitation: true,
                detail: None,
                additional_attributes: None,
            }],
            ..person_request()
        };

        let reported = paths(validate_create_party(&request).expect_err("invalid"));

        for field in [
            "externalId",
            "person.maritalStatus",
            "identifications[0].issuedBy",
            "classifications[0].partyClassificationId",
            "relationshipsFrom[0].statusId",
            "contactMechanisms[0].purposeTypeId",
        ] {
            assert!(
                reported.contains(&field.to_owned()),
                "{field} reaches its column unbounded; reported {reported:?}"
            );
        }
    }

    /// `tickerSymbol` is on the group input, so it needs the group request.
    #[test]
    fn a_ticker_symbol_longer_than_its_column_is_refused() {
        let request = CreatePartyRequest {
            party_group: Some(PartyGroupInput {
                group_name: Some("Acme Supplies".to_owned()),
                ticker_symbol: Some("x".repeat(MAX_CODE_LENGTH + 1)),
                ..PartyGroupInput::default()
            }),
            ..group_request()
        };

        assert!(paths(validate_create_party(&request).expect_err("invalid"))
            .contains(&"partyGroup.tickerSymbol".to_owned()));
    }

    /// An update takes the same payload shapes and had the same hole.
    #[test]
    fn an_update_bounds_the_same_fields_a_create_does() {
        let request = UpdatePartyRequest {
            status_id: None,
            external_id: Some("x".repeat(MAX_EXTERNAL_ID_LENGTH + 1)),
            description: None,
            person: None,
            party_group: None,
            identifications: None,
            relationships_from: None,
            relationships_to: None,
            classifications: None,
            contact_mechanisms: Some(vec![PartyContactMechInput {
                contact_mech_id: Some(Uuid::nil()),
                contact_mech_type_id: None,
                purpose_type_id: Some("x".repeat(MAX_CODE_LENGTH + 1)),
                from_date: epoch(),
                thru_date: None,
                is_primary: false,
                allow_solicitation: true,
                detail: None,
                additional_attributes: None,
            }]),
            additional_attributes: None,
            status_comments: None,
        };

        let reported = paths(
            validate_update_party(&request, "PARTY-0001", PartyType::Person).expect_err("invalid"),
        );

        assert!(reported.contains(&"externalId".to_owned()), "{reported:?}");
        assert!(
            reported.contains(&"contactMechanisms[0].purposeTypeId".to_owned()),
            "{reported:?}"
        );
    }

    /// A value exactly at the bound is accepted. Off-by-one in the other
    /// direction turns a fix into a new refusal of valid data.
    #[test]
    fn a_value_exactly_at_the_bound_is_accepted() {
        let request = CreatePartyRequest {
            external_id: Some("x".repeat(MAX_EXTERNAL_ID_LENGTH)),
            contact_mechanisms: vec![PartyContactMechInput {
                contact_mech_id: Some(Uuid::nil()),
                contact_mech_type_id: None,
                purpose_type_id: Some("x".repeat(MAX_CODE_LENGTH)),
                from_date: epoch(),
                thru_date: None,
                is_primary: false,
                allow_solicitation: true,
                detail: None,
                additional_attributes: None,
            }],
            ..person_request()
        };

        assert!(validate_create_party(&request).is_ok());
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
