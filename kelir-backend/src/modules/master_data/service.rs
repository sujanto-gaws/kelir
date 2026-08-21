//! Party use cases. Owns transactions and permission checks (coding standard
//! §2.2/§2.6): handlers call these, never the repository.

use serde_json::{json, Value};
use uuid::Uuid;

use super::domain::{
    validate_assign_role, validate_create_party, validate_update_party, AssignRoleRequest,
    ContactMechType, CreatePartyRequest, EmploymentType, PartyAggregate, PartyClassificationInput,
    PartyContactMechInput, PartyIdentificationInput, PartyProfiles, PartyRelationshipInput,
    PartyRole, PartyRoleStatus, PartyRoles, PartyStatusCode, PartySummary, PartyType,
    RoleProfileInput, RoleView, RoleViewQuery, RoleViewRow, SupplierApprovalStatus,
    UpdatePartyRequest, PROFILED_ROLE_TYPES,
};
use super::repository::{
    self as repo, ClassificationFields, ContactMechFields, ContactProfileFields,
    CustomerProfileFields, EmployeeProfileFields, IdentificationFields, NewParty, PartyGroupFields,
    PartyRoleFields, PersonFields, RelationshipFields, SupplierProfileFields,
};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a party (naming convention §7).
const OBJECT_TYPE: &str = "PARTY";

pub async fn list_parties(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<PartySummary>, PageMeta), AppError> {
    caller.require("master-data:party:read")?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_parties(&state.pool, tenant_id).await?;
    let parties = repo::list_parties(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((parties, pagination.meta(total.max(0) as u64)))
}

pub async fn get_party(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<PartyAggregate, AppError> {
    caller.require("master-data:party:read")?;

    load_aggregate(state, caller, id).await
}

pub async fn create_party(
    state: &AppState,
    caller: &Authenticated,
    request: CreatePartyRequest,
) -> Result<PartyAggregate, AppError> {
    caller.require("master-data:party:create")?;
    validate_create_party(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let id = Uuid::now_v7();
    let party_code = request.party_id.trim();
    let status = request.status_id.unwrap_or(PartyStatusCode::PartyEnabled);

    // Everything the payload names has to be resolvable before anything is
    // written: a party stored with half its relationships, because the second
    // one pointed at a party that does not exist, is worse than a refusal.
    let relationships_from = resolve_relationships(
        state,
        tenant_id,
        &request.relationships_from,
        RelationshipSide::From,
        id,
        "relationshipsFrom",
    )
    .await?;
    let relationships_to = resolve_relationships(
        state,
        tenant_id,
        &request.relationships_to,
        RelationshipSide::To,
        id,
        "relationshipsTo",
    )
    .await?;
    let contact_mechs =
        resolve_contact_mechs(state, tenant_id, &request.contact_mechanisms).await?;

    let empty = json!({});
    let attributes = request.additional_attributes.clone().unwrap_or(json!({}));

    let mut transaction = state.pool.begin().await?;

    repo::insert_party(
        &mut *transaction,
        NewParty {
            id,
            tenant_id,
            party_code,
            party_type: request.party_type_id.as_db(),
            status: status.as_db(),
            external_id: trimmed(request.external_id.as_deref()),
            description: request.description.as_deref(),
            attributes_json: &attributes,
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_party_to_conflict)?;

    match (&request.person, &request.party_group) {
        (Some(person), _) if request.party_type_id == PartyType::Person => {
            repo::insert_person(
                &mut *transaction,
                Uuid::now_v7(),
                tenant_id,
                id,
                &PersonFields {
                    first_name: trimmed(person.first_name.as_deref()),
                    middle_name: trimmed(person.middle_name.as_deref()),
                    last_name: trimmed(person.last_name.as_deref()),
                    personal_title: trimmed(person.personal_title.as_deref()),
                    suffix: trimmed(person.suffix.as_deref()),
                    gender: person.gender.map(|gender| gender.as_db()),
                    birth_date: person.birth_date,
                    marital_status: trimmed(person.marital_status.as_deref()),
                    comments: person.comments.as_deref(),
                },
                actor,
            )
            .await?;
        }
        (_, Some(group)) => {
            repo::insert_party_group(
                &mut *transaction,
                Uuid::now_v7(),
                tenant_id,
                id,
                &PartyGroupFields {
                    group_name: trimmed(group.group_name.as_deref()),
                    local_name: trimmed(group.local_name.as_deref()),
                    office_site_name: trimmed(group.office_site_name.as_deref()),
                    annual_revenue: trimmed(group.annual_revenue.as_deref()),
                    num_employees: group.num_employees,
                    ticker_symbol: trimmed(group.ticker_symbol.as_deref()),
                    comments: group.comments.as_deref(),
                },
                actor,
            )
            .await?;
        }
        // Unreachable: validation refuses a party whose type and detail
        // disagree, and both arms above are keyed on the type.
        _ => {}
    }

    repo::replace_identifications(
        &mut transaction,
        tenant_id,
        id,
        &identification_fields(&request.identifications, &empty),
        actor,
    )
    .await?;

    repo::replace_relationships(
        &mut transaction,
        tenant_id,
        id,
        true,
        &relationship_fields(&request.relationships_from, &relationships_from, &empty),
        actor,
    )
    .await?;
    repo::replace_relationships(
        &mut transaction,
        tenant_id,
        id,
        false,
        &relationship_fields(&request.relationships_to, &relationships_to, &empty),
        actor,
    )
    .await?;

    repo::replace_classifications(
        &mut transaction,
        tenant_id,
        id,
        &classification_fields(&request.classifications),
        actor,
    )
    .await?;

    repo::replace_contact_mechs(
        &mut transaction,
        tenant_id,
        id,
        &contact_mech_fields(&request.contact_mechanisms, &contact_mechs, &empty),
        actor,
    )
    .await?;

    // The status history starts where the party does: without this row the
    // aggregate's `statuses` would be empty until the first change, and the
    // record of who created the party in which state would live only in the
    // audit trail.
    repo::insert_status(
        &mut *transaction,
        tenant_id,
        id,
        status.as_db(),
        actor,
        None,
    )
    .await?;

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Party.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "partyId": party_code,
                "partyTypeId": request.party_type_id,
                "statusId": status,
            })),
        },
    )
    .await;

    load_aggregate(state, caller, id).await
}

pub async fn update_party(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdatePartyRequest,
) -> Result<PartyAggregate, AppError> {
    caller.require("master-data:party:update")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let before = repo::find_party(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    validate_update_party(&request, &before.party_code, before.party_type)?;

    let relationships_from = match &request.relationships_from {
        Some(inputs) => Some(
            resolve_relationships(
                state,
                tenant_id,
                inputs,
                RelationshipSide::From,
                id,
                "relationshipsFrom",
            )
            .await?,
        ),
        None => None,
    };
    let relationships_to = match &request.relationships_to {
        Some(inputs) => Some(
            resolve_relationships(
                state,
                tenant_id,
                inputs,
                RelationshipSide::To,
                id,
                "relationshipsTo",
            )
            .await?,
        ),
        None => None,
    };
    let contact_mechs = match &request.contact_mechanisms {
        Some(inputs) => Some(resolve_contact_mechs(state, tenant_id, inputs).await?),
        None => None,
    };

    let empty = json!({});
    let mut transaction = state.pool.begin().await?;

    let updated = repo::update_party_fields(
        &mut *transaction,
        tenant_id,
        id,
        request.status_id.map(PartyStatusCode::as_db),
        trimmed(request.external_id.as_deref()),
        request.description.as_deref(),
        request.additional_attributes.as_ref(),
        actor,
    )
    .await?;

    if updated == 0 {
        return Err(AppError::not_found("Party"));
    }

    if let Some(person) = &request.person {
        repo::update_person(
            &mut *transaction,
            tenant_id,
            id,
            &PersonFields {
                first_name: trimmed(person.first_name.as_deref()),
                middle_name: trimmed(person.middle_name.as_deref()),
                last_name: trimmed(person.last_name.as_deref()),
                personal_title: trimmed(person.personal_title.as_deref()),
                suffix: trimmed(person.suffix.as_deref()),
                gender: person.gender.map(|gender| gender.as_db()),
                birth_date: person.birth_date,
                marital_status: trimmed(person.marital_status.as_deref()),
                comments: person.comments.as_deref(),
            },
            actor,
        )
        .await?;
    }

    if let Some(group) = &request.party_group {
        repo::update_party_group(
            &mut *transaction,
            tenant_id,
            id,
            &PartyGroupFields {
                group_name: trimmed(group.group_name.as_deref()),
                local_name: trimmed(group.local_name.as_deref()),
                office_site_name: trimmed(group.office_site_name.as_deref()),
                annual_revenue: trimmed(group.annual_revenue.as_deref()),
                num_employees: group.num_employees,
                ticker_symbol: trimmed(group.ticker_symbol.as_deref()),
                comments: group.comments.as_deref(),
            },
            actor,
        )
        .await?;
    }

    if let Some(identifications) = &request.identifications {
        repo::replace_identifications(
            &mut transaction,
            tenant_id,
            id,
            &identification_fields(identifications, &empty),
            actor,
        )
        .await?;
    }

    if let (Some(inputs), Some(resolved)) = (&request.relationships_from, &relationships_from) {
        repo::replace_relationships(
            &mut transaction,
            tenant_id,
            id,
            true,
            &relationship_fields(inputs, resolved, &empty),
            actor,
        )
        .await?;
    }
    if let (Some(inputs), Some(resolved)) = (&request.relationships_to, &relationships_to) {
        repo::replace_relationships(
            &mut transaction,
            tenant_id,
            id,
            false,
            &relationship_fields(inputs, resolved, &empty),
            actor,
        )
        .await?;
    }

    if let Some(classifications) = &request.classifications {
        repo::replace_classifications(
            &mut transaction,
            tenant_id,
            id,
            &classification_fields(classifications),
            actor,
        )
        .await?;
    }

    if let (Some(inputs), Some(resolved)) = (&request.contact_mechanisms, &contact_mechs) {
        repo::replace_contact_mechs(
            &mut transaction,
            tenant_id,
            id,
            &contact_mech_fields(inputs, resolved, &empty),
            actor,
        )
        .await?;
    }

    // A status change is history, not a field edit: the row is what makes the
    // previous state and who left it recoverable (FR-MDM-003).
    let status_changed = matches!(request.status_id, Some(status) if status != before.status);
    if status_changed {
        let status = request.status_id.unwrap_or(before.status);
        repo::insert_status(
            &mut *transaction,
            tenant_id,
            id,
            status.as_db(),
            actor,
            request.status_comments.as_deref(),
        )
        .await?;
    }

    transaction.commit().await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Party.Updated",
            action: if status_changed {
                "STATUS_CHANGE"
            } else {
                "UPDATE"
            },
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: request.status_comments.as_deref(),
            old_value: Some(json!({
                "statusId": before.status,
                "externalId": before.external_id,
                "description": before.description,
            })),
            new_value: Some(json!({
                "statusId": request.status_id,
                "externalId": request.external_id,
                "description": request.description,
            })),
        },
    )
    .await;

    load_aggregate(state, caller, id).await
}

pub async fn delete_party(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require("master-data:party:delete")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    // The party and everything it holds close together, or nothing does. A
    // party soft-deleted while its roles stayed live is what left a supplier
    // number occupied by a row no route could reach (#103): the unique index
    // on `supplier_number` is partial on `deleted_at IS NULL`, so the orphan
    // kept the number while `remove_role` had stopped being able to find it.
    let mut transaction = state.pool.begin().await?;

    let removed = repo::soft_delete_party(&mut *transaction, tenant_id, id, actor).await?;

    if removed == 0 {
        return Err(AppError::not_found("Party"));
    }

    repo::soft_delete_party_roles(&mut *transaction, tenant_id, id, actor).await?;

    // Every profile table, rather than only the ones behind the roles just
    // closed. A profile can only exist where its role did, so the extra
    // statements match nothing in the ordinary case — and in the case this
    // change exists for, an orphan left by an earlier delete, matching nothing
    // is not what we want.
    for role_type in PROFILED_ROLE_TYPES {
        repo::soft_delete_role_profile(&mut transaction, tenant_id, id, role_type, actor).await?;
    }

    transaction.commit().await?;

    // Append to the history before the audit event, for the same reason the
    // create path writes one: `deleted_at` records that it happened, and this
    // records who and when in the vocabulary the aggregate reads back.
    repo::insert_status(
        &state.pool,
        tenant_id,
        id,
        PartyStatusCode::PartyDisabled.as_db(),
        actor,
        Some("party deleted"),
    )
    .await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Party.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: None,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Reading the aggregate
// ---------------------------------------------------------------------------

async fn load_aggregate(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<PartyAggregate, AppError> {
    let tenant_id = caller.tenant_id();
    let party = repo::find_party(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    // The roles and their profiles are a separate authorization surface; a
    // caller without `master-data:party-role:read` gets an aggregate with
    // neither member rather than one with two empty ones.
    let (roles, profiles) = if caller.claims.has_permission(ROLE_READ) {
        let held = load_roles(state, tenant_id, id, &party.party_code).await?;
        (Some(held.roles), Some(held.profiles))
    } else {
        (None, None)
    };

    let (person, party_group) = match party.party_type {
        PartyType::Person => (
            repo::find_person(&state.pool, tenant_id, id, &party.party_code).await?,
            None,
        ),
        PartyType::PartyGroup => (
            None,
            repo::find_party_group(&state.pool, tenant_id, id, &party.party_code).await?,
        ),
    };

    Ok(PartyAggregate {
        id: party.id,
        party_id: party.party_code,
        party_type_id: party.party_type,
        status_id: party.status,
        external_id: party.external_id,
        description: party.description,
        person,
        party_group,
        identifications: repo::list_identifications(&state.pool, tenant_id, id).await?,
        statuses: repo::list_statuses(&state.pool, tenant_id, id).await?,
        relationships_from: repo::list_relationships(&state.pool, tenant_id, id, true).await?,
        relationships_to: repo::list_relationships(&state.pool, tenant_id, id, false).await?,
        classifications: repo::list_classifications(&state.pool, tenant_id, id).await?,
        contact_mechanisms: repo::list_contact_mechs(&state.pool, tenant_id, id).await?,
        roles,
        profiles,
        additional_attributes: party.attributes_json,
        created_stamp: party.created_at,
        last_updated_stamp: party.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Resolving what the payload names
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum RelationshipSide {
    From,
    To,
}

/// A relationship's two ends and two role types, as surrogate keys.
struct ResolvedRelationship {
    from_party_id: Uuid,
    to_party_id: Uuid,
    from_role_type_id: Option<Uuid>,
    to_role_type_id: Option<Uuid>,
}

/// Turns the business codes in a relationship payload into keys, failing with a
/// 422 that names the offending path rather than a foreign-key violation.
async fn resolve_relationships(
    state: &AppState,
    tenant_id: Uuid,
    inputs: &[PartyRelationshipInput],
    side: RelationshipSide,
    own_party_id: Uuid,
    collection: &str,
) -> Result<Vec<ResolvedRelationship>, AppError> {
    let mut resolved = Vec::with_capacity(inputs.len());
    let mut details = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        let path = format!("{collection}[{index}]");

        let (counterparty_code, counterparty_field) = match side {
            RelationshipSide::From => (input.party_id_to.trim(), "partyIdTo"),
            RelationshipSide::To => (input.party_id_from.trim(), "partyIdFrom"),
        };

        let counterparty =
            repo::find_party_id_by_code(&state.pool, tenant_id, counterparty_code).await?;

        let Some(counterparty) = counterparty else {
            details.push(ValidationDetail::new(
                format!("{path}.{counterparty_field}"),
                "exists",
                "NOT_FOUND",
                format!("No party with partyId '{counterparty_code}'"),
            ));
            continue;
        };

        let from_role_type_id = resolve_role_type(
            state,
            tenant_id,
            input.role_type_id_from.as_deref(),
            &format!("{path}.roleTypeIdFrom"),
            &mut details,
        )
        .await?;
        let to_role_type_id = resolve_role_type(
            state,
            tenant_id,
            input.role_type_id_to.as_deref(),
            &format!("{path}.roleTypeIdTo"),
            &mut details,
        )
        .await?;

        let (from_party_id, to_party_id) = match side {
            RelationshipSide::From => (own_party_id, counterparty),
            RelationshipSide::To => (counterparty, own_party_id),
        };

        resolved.push(ResolvedRelationship {
            from_party_id,
            to_party_id,
            from_role_type_id,
            to_role_type_id,
        });
    }

    if details.is_empty() {
        Ok(resolved)
    } else {
        Err(AppError::validation(details))
    }
}

async fn resolve_role_type(
    state: &AppState,
    tenant_id: Uuid,
    code: Option<&str>,
    path: &str,
    details: &mut Vec<ValidationDetail>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_role_type_id(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => {
            details.push(ValidationDetail::new(
                path,
                "exists",
                "NOT_FOUND",
                format!("No role type '{code}'"),
            ));
            Ok(None)
        }
    }
}

/// A contact mechanism's storage form: an existing id to link, or the type and
/// display value of a new one.
struct ResolvedContactMech {
    existing: Option<Uuid>,
    contact_mech_type: Option<&'static str>,
    display_value: Option<String>,
    detail_json: Value,
}

async fn resolve_contact_mechs(
    state: &AppState,
    tenant_id: Uuid,
    inputs: &[PartyContactMechInput],
) -> Result<Vec<ResolvedContactMech>, AppError> {
    let mut resolved = Vec::with_capacity(inputs.len());
    let mut details = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        let path = format!("contactMechanisms[{index}]");

        if let Some(existing) = input.contact_mech_id {
            if !repo::contact_mech_exists(&state.pool, tenant_id, existing).await? {
                details.push(ValidationDetail::new(
                    format!("{path}.contactMechId"),
                    "exists",
                    "NOT_FOUND",
                    "No contact mechanism with that id",
                ));
                continue;
            }

            resolved.push(ResolvedContactMech {
                existing: Some(existing),
                contact_mech_type: None,
                display_value: None,
                detail_json: json!({}),
            });
            continue;
        }

        // Validation has already established that a link without an id carries
        // detail with a value in it.
        let detail = input.detail.clone().unwrap_or_default();

        resolved.push(ResolvedContactMech {
            existing: None,
            contact_mech_type: input
                .contact_mech_type_id
                .map(ContactMechType::as_db)
                .or(Some("OTHER")),
            display_value: detail.display_value(),
            detail_json: serde_json::to_value(&detail).unwrap_or_else(|_| json!({})),
        });
    }

    if details.is_empty() {
        Ok(resolved)
    } else {
        Err(AppError::validation(details))
    }
}

// ---------------------------------------------------------------------------
// Payload to column mapping
// ---------------------------------------------------------------------------

fn identification_fields<'a>(
    inputs: &'a [PartyIdentificationInput],
    empty: &'a Value,
) -> Vec<IdentificationFields<'a>> {
    inputs
        .iter()
        .map(|input| IdentificationFields {
            identification_type: input.party_identification_type_id.trim(),
            id_value: input.id_value.trim(),
            issued_by: trimmed(input.issued_by.as_deref()),
            issue_date: input.issue_date,
            expire_date: input.expire_date,
            attributes_json: input.additional_attributes.as_ref().unwrap_or(empty),
        })
        .collect()
}

fn relationship_fields<'a>(
    inputs: &'a [PartyRelationshipInput],
    resolved: &'a [ResolvedRelationship],
    empty: &'a Value,
) -> Vec<RelationshipFields<'a>> {
    inputs
        .iter()
        .zip(resolved)
        .map(|(input, keys)| RelationshipFields {
            from_party_id: keys.from_party_id,
            to_party_id: keys.to_party_id,
            relationship_type: input.party_relationship_type_id.trim(),
            from_role_type_id: keys.from_role_type_id,
            to_role_type_id: keys.to_role_type_id,
            starts_at: input.from_date,
            ends_at: input.thru_date,
            status: trimmed(input.status_id.as_deref()),
            priority: input.priority,
            comments: input.comments.as_deref(),
            attributes_json: input.additional_attributes.as_ref().unwrap_or(empty),
        })
        .collect()
}

fn classification_fields(inputs: &[PartyClassificationInput]) -> Vec<ClassificationFields<'_>> {
    inputs
        .iter()
        .map(|input| ClassificationFields {
            class_type: input.party_class_type_id.trim(),
            classification_code: trimmed(input.party_classification_id.as_deref()),
            starts_at: input.from_date,
            ends_at: input.thru_date,
            comments: input.comments.as_deref(),
        })
        .collect()
}

fn contact_mech_fields<'a>(
    inputs: &'a [PartyContactMechInput],
    resolved: &'a [ResolvedContactMech],
    empty: &'a Value,
) -> Vec<ContactMechFields<'a>> {
    inputs
        .iter()
        .zip(resolved)
        .map(|(input, storage)| ContactMechFields {
            existing_contact_mech_id: storage.existing,
            contact_mech_type: storage.contact_mech_type,
            display_value: storage.display_value.as_deref(),
            detail_json: &storage.detail_json,
            purpose_type: trimmed(input.purpose_type_id.as_deref()),
            starts_at: input.from_date,
            ends_at: input.thru_date,
            is_primary: input.is_primary,
            allow_solicitation: input.allow_solicitation,
            attributes_json: input.additional_attributes.as_ref().unwrap_or(empty),
        })
        .collect()
}

/// Trims a supplied string and treats an all-whitespace one as absent, so a
/// `COALESCE` update cannot overwrite a value with blanks.
fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// A unique violation on `mdm_parties` is a duplicate `partyId`, which is the
/// caller's problem to fix — not an internal error.
fn duplicate_party_to_conflict(error: sqlx::Error) -> AppError {
    if matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) {
        AppError::conflict("That partyId is already in use")
    } else {
        AppError::from(error)
    }
}

// ---------------------------------------------------------------------------
// Roles and role profiles (FR-MDM-002)
// ---------------------------------------------------------------------------

/// The permission that makes a party's roles and profiles visible.
///
/// Separate from `master-data:party:read` because the data is: a supplier
/// profile carries a bank account number and a customer profile a credit limit,
/// so seeing that a party exists and seeing what it is worth are different
/// questions. The aggregate omits both members entirely without it.
const ROLE_READ: &str = "master-data:party-role:read";

pub async fn get_party_roles(
    state: &AppState,
    caller: &Authenticated,
    party_id: Uuid,
) -> Result<PartyRoles, AppError> {
    caller.require(ROLE_READ)?;

    let tenant_id = caller.tenant_id();
    let party = repo::find_party(&state.pool, tenant_id, party_id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    load_roles(state, tenant_id, party_id, &party.party_code).await
}

/// One page of the parties holding a role — `/suppliers`, `/customers`,
/// `/employees` (FR-MDM-002, FR-MDM-008).
///
/// **Two permissions, and the reason is the row.** A role-view row is a party
/// summary with a supplier number on it, so it is made of both surfaces: the
/// party half is what `master-data:party:read` gates on `/parties`, and the
/// number is role data, which is what [`ROLE_READ`] gates on the aggregate. A
/// view that asked for only one of them would be a way around the other — a
/// caller holding `master-data:party:read` alone would read the supplier
/// numbers the aggregate withholds from them one URL away (#97 AC3), and a
/// caller holding only [`ROLE_READ`] would gain the ability to enumerate
/// parties, which today needs `master-data:party:read`.
///
/// No new permission string is minted for the views. Three endpoints over data
/// two existing permissions already govern is the per-endpoint permission shape
/// D-6 rejected for the catalogue.
///
/// The refusal comes before the query parameters are parsed: a caller who may
/// not see this list learns that, not which of their filters was misspelled.
pub async fn list_role_view(
    state: &AppState,
    caller: &Authenticated,
    view: RoleView,
    query: &RoleViewQuery,
) -> Result<(Vec<RoleViewRow>, PageMeta), AppError> {
    caller.require("master-data:party:read")?;
    caller.require(ROLE_READ)?;

    let filters = query.filters()?;
    let pagination = query.pagination();
    let tenant_id = caller.tenant_id();
    let role_type_code = view.role_type_code();

    let total = repo::count_role_view(&state.pool, tenant_id, role_type_code, &filters).await?;
    let rows = repo::list_role_view(
        &state.pool,
        tenant_id,
        role_type_code,
        &filters,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((rows, pagination.meta(total.max(0) as u64)))
}

/// Gives a party a role, or restates the one it already holds.
///
/// Returns whether the assignment was created, so the handler can answer 201
/// rather than 200 — `PUT` is idempotent, and a client that repeats it needs to
/// be able to tell the first call from the rest.
pub async fn assign_role(
    state: &AppState,
    caller: &Authenticated,
    party_id: Uuid,
    role_type_code: &str,
    request: AssignRoleRequest,
) -> Result<(bool, PartyRole), AppError> {
    caller.require("master-data:party-role:assign")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let party = repo::find_party(&state.pool, tenant_id, party_id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    let role_type = role_type_code.trim();
    let role_type_id = repo::find_role_type_id(&state.pool, tenant_id, role_type)
        .await?
        .ok_or_else(|| {
            // Not a 404: the party exists and the request is well-formed, the
            // role type in it is not one this tenant has. A tenant adds its own
            // by inserting a row in mdm_role_types — no migration (#81 AC4).
            AppError::validation(vec![ValidationDetail::new(
                "roleTypeId",
                "exists",
                "NOT_FOUND",
                format!("No role type '{role_type}'"),
            )])
        })?;

    let existing =
        repo::find_live_party_role(&state.pool, tenant_id, party_id, role_type_id).await?;
    let creating = existing.is_none();

    validate_assign_role(&request, role_type, &party.party_code, creating)?;

    // Everything the profile points at is resolved before anything is written,
    // so a manager or a department that does not exist is a 422 naming the path
    // rather than a foreign-key violation surfacing as a 500.
    let references = resolve_profile_references(state, tenant_id, request.profile.as_ref()).await?;

    let mut transaction = state.pool.begin().await?;

    let role_fields = PartyRoleFields {
        starts_at: request.from_date,
        ends_at: request.thru_date,
        status: request.status_id.map(PartyRoleStatus::as_db),
        comments: request.comments.as_deref(),
        attributes_json: request.additional_attributes.as_ref(),
    };

    match existing {
        Some(id) => repo::update_party_role(&mut *transaction, id, &role_fields, actor).await?,
        None => {
            repo::insert_party_role(
                &mut *transaction,
                tenant_id,
                party_id,
                role_type_id,
                &role_fields,
                actor,
            )
            .await?
        }
    }

    if let Some(profile) = &request.profile {
        write_profile(
            &mut transaction,
            tenant_id,
            party_id,
            profile,
            &references,
            creating,
            actor,
        )
        .await
        .map_err(duplicate_profile_to_conflict)?;
    }

    transaction.commit().await?;

    // The event is named for the business subject, not the table: a party
    // gaining the SUPPLIER role is a supplier coming into existence (naming
    // convention §7, which gives `Supplier.Created` as its example for exactly
    // this party-based storage). `object_type` stays PARTY because `object_id`
    // is the party — that is the object an auditor asks about.
    let entity = event_entity(role_type);
    let event_type = format!("{entity}.{}", if creating { "Created" } else { "Updated" });

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: &event_type,
            action: if creating {
                "ROLE_ASSIGNED"
            } else {
                "ROLE_UPDATED"
            },
            object_type: OBJECT_TYPE,
            object_id: party_id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "partyId": party.party_code,
                "roleTypeId": role_type,
                "hasProfile": request.profile.is_some(),
            })),
        },
    )
    .await;

    // The assignment that was written, and only it.
    //
    // This route used to answer with `load_roles` — every role the party holds
    // and every profile behind them — while requiring only
    // `master-data:party-role:assign`. That handed a caller the bank account
    // and the credit limit that `master-data:party-role:read` exists to gate,
    // one route away from the aggregate that withholds them (#104).
    //
    // Gating the collection here would have closed the leak too. Returning the
    // assignment the URL addresses closes it without a second gate to keep in
    // step with the first — and it is the smaller contract: a caller who wants
    // the profiles asks `GET .../roles`, under the permission that governs
    // them.
    let assignment = repo::find_party_role(&state.pool, tenant_id, party_id, role_type)
        .await?
        .ok_or_else(|| AppError::not_found("Party role"))?;

    Ok((creating, assignment))
}

/// Ends a role assignment and closes the profile behind it.
///
/// The party is untouched (#81 AC3): a supplier that stops being a supplier is
/// still a party, and may still be a customer.
pub async fn remove_role(
    state: &AppState,
    caller: &Authenticated,
    party_id: Uuid,
    role_type_code: &str,
) -> Result<(), AppError> {
    caller.require("master-data:party-role:remove")?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let party = repo::find_party(&state.pool, tenant_id, party_id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    let role_type = role_type_code.trim();
    let role_type_id = repo::find_role_type_id(&state.pool, tenant_id, role_type)
        .await?
        .ok_or_else(|| AppError::not_found("Party role"))?;

    let mut transaction = state.pool.begin().await?;

    let removed =
        repo::soft_delete_party_role(&mut *transaction, tenant_id, party_id, role_type_id, actor)
            .await?;

    if removed == 0 {
        return Err(AppError::not_found("Party role"));
    }

    // The profile goes with the role rather than being left behind: a supplier
    // profile on a party that is not a supplier describes nothing, and an
    // orphan would still hold the supplier number that stops the next party
    // from using it (#81 AC3).
    repo::soft_delete_role_profile(&mut transaction, tenant_id, party_id, role_type, actor).await?;

    transaction.commit().await?;

    let event_type = format!("{}.Removed", event_entity(role_type));

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: &event_type,
            action: "ROLE_REMOVED",
            object_type: OBJECT_TYPE,
            object_id: party_id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(json!({
                "partyId": party.party_code,
                "roleTypeId": role_type,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// A party's roles, and only the profiles whose role it actually holds.
///
/// Keying the profile reads off the role list is what keeps the two consistent:
/// a profile row that outlived its role could not appear here even if one
/// existed.
async fn load_roles(
    state: &AppState,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<PartyRoles, AppError> {
    let roles = repo::list_party_roles(&state.pool, tenant_id, party_id).await?;
    let holds = |code: &str| roles.iter().any(|role| role.role_type_id == code);

    let profiles = PartyProfiles {
        supplier: if holds("SUPPLIER") {
            repo::find_supplier_profile(&state.pool, tenant_id, party_id, party_code).await?
        } else {
            None
        },
        customer: if holds("CUSTOMER") {
            repo::find_customer_profile(&state.pool, tenant_id, party_id, party_code).await?
        } else {
            None
        },
        employee: if holds("EMPLOYEE") {
            repo::find_employee_profile(&state.pool, tenant_id, party_id, party_code).await?
        } else {
            None
        },
        contact: if holds("CONTACT") {
            repo::find_contact_profile(&state.pool, tenant_id, party_id, party_code).await?
        } else {
            None
        },
    };

    Ok(PartyRoles { roles, profiles })
}

/// The party and department keys a profile names, resolved from the business
/// codes the aggregate carries.
#[derive(Default)]
struct ProfileReferences {
    department_id: Option<Uuid>,
    manager_party_id: Option<Uuid>,
    billing_party_id: Option<Uuid>,
    assistant_party_id: Option<Uuid>,
}

async fn resolve_profile_references(
    state: &AppState,
    tenant_id: Uuid,
    profile: Option<&RoleProfileInput>,
) -> Result<ProfileReferences, AppError> {
    let Some(profile) = profile else {
        return Ok(ProfileReferences::default());
    };

    let mut resolved = ProfileReferences::default();
    let mut details = Vec::new();

    if let Some(customer) = &profile.customer {
        resolved.billing_party_id = resolve_party_reference(
            state,
            tenant_id,
            customer.billing_party_id.as_deref(),
            "profile.customer.billingPartyId",
            &mut details,
        )
        .await?;
    }

    if let Some(employee) = &profile.employee {
        resolved.manager_party_id = resolve_party_reference(
            state,
            tenant_id,
            employee.manager_party_id.as_deref(),
            "profile.employee.managerPartyId",
            &mut details,
        )
        .await?;

        if let Some(department_id) = employee.department_id {
            if repo::department_exists(&state.pool, tenant_id, department_id).await? {
                resolved.department_id = Some(department_id);
            } else {
                details.push(ValidationDetail::new(
                    "profile.employee.departmentId",
                    "exists",
                    "NOT_FOUND",
                    "No department with that id",
                ));
            }
        }
    }

    if let Some(contact) = &profile.contact {
        resolved.assistant_party_id = resolve_party_reference(
            state,
            tenant_id,
            contact.assistant_party_id.as_deref(),
            "profile.contact.assistantPartyId",
            &mut details,
        )
        .await?;
    }

    if details.is_empty() {
        Ok(resolved)
    } else {
        Err(AppError::validation(details))
    }
}

async fn resolve_party_reference(
    state: &AppState,
    tenant_id: Uuid,
    party_code: Option<&str>,
    path: &str,
    details: &mut Vec<ValidationDetail>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = party_code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_party_id_by_code(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => {
            details.push(ValidationDetail::new(
                path,
                "exists",
                "NOT_FOUND",
                format!("No party with partyId '{code}'"),
            ));
            Ok(None)
        }
    }
}

/// Writes whichever profile the request carries. Validation has already
/// established that it is the one belonging to the role being assigned.
async fn write_profile(
    transaction: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    profile: &RoleProfileInput,
    references: &ProfileReferences,
    creating: bool,
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    if let Some(supplier) = &profile.supplier {
        let fields = SupplierProfileFields {
            supplier_number: trimmed(supplier.supplier_number.as_deref()),
            supplier_category: trimmed(supplier.supplier_category.as_deref()),
            payment_term_days: supplier.payment_term_days,
            default_currency_uom: trimmed(supplier.default_currency_uom.as_deref()),
            tax_number: trimmed(supplier.tax_number.as_deref()),
            bank_name: trimmed(supplier.bank_name.as_deref()),
            bank_account: trimmed(supplier.bank_account.as_deref()),
            bank_account_name: trimmed(supplier.bank_account_name.as_deref()),
            approval_status: supplier.approval_status.map(SupplierApprovalStatus::as_db),
            status: trimmed(supplier.status_id.as_deref()),
            attributes_json: supplier.additional_attributes.as_ref(),
        };

        if creating {
            repo::insert_supplier_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        } else {
            repo::update_supplier_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        }
    }

    if let Some(customer) = &profile.customer {
        let fields = CustomerProfileFields {
            customer_number: trimmed(customer.customer_number.as_deref()),
            customer_category: trimmed(customer.customer_category.as_deref()),
            customer_since_date: customer.customer_since,
            credit_limit: trimmed(customer.credit_limit.as_deref()),
            payment_term_days: customer.payment_term_days,
            default_currency_uom: trimmed(customer.default_currency_uom.as_deref()),
            tax_number: trimmed(customer.tax_number.as_deref()),
            billing_party_id: references.billing_party_id,
            status: trimmed(customer.status_id.as_deref()),
            attributes_json: customer.additional_attributes.as_ref(),
        };

        if creating {
            repo::insert_customer_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        } else {
            repo::update_customer_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        }
    }

    if let Some(employee) = &profile.employee {
        let fields = EmployeeProfileFields {
            employee_number: trimmed(employee.employee_number.as_deref()),
            department_id: references.department_id,
            manager_party_id: references.manager_party_id,
            position: trimmed(employee.position.as_deref()),
            job_grade: trimmed(employee.job_grade.as_deref()),
            employment_type: employee.employment_type.map(EmploymentType::as_db),
            join_date: employee.join_date,
            resign_date: employee.resign_date,
            status: trimmed(employee.status_id.as_deref()),
            attributes_json: employee.additional_attributes.as_ref(),
        };

        if creating {
            repo::insert_employee_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        } else {
            repo::update_employee_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        }
    }

    if let Some(contact) = &profile.contact {
        let fields = ContactProfileFields {
            contact_type: trimmed(contact.contact_type.as_deref()),
            preferred_contact_mech_type: trimmed(contact.preferred_contact_mech_type_id.as_deref()),
            do_not_contact: contact.do_not_contact,
            assistant_party_id: references.assistant_party_id,
            attributes_json: contact.additional_attributes.as_ref(),
        };

        if creating {
            repo::insert_contact_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        } else {
            repo::update_contact_profile(&mut *transaction, tenant_id, party_id, &fields, actor)
                .await?;
        }
    }

    Ok(())
}

/// The business subject a role type names, in the event vocabulary of naming
/// convention §7: `SUPPLIER` becomes `Supplier`, `ORGANIZATION_UNIT` becomes
/// `OrganizationUnit`, and a role type a tenant invented becomes whatever it
/// spelled.
fn event_entity(role_type_code: &str) -> String {
    role_type_code
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut characters = segment.chars();
            match characters.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &characters.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect()
}

/// A unique violation while writing a profile is a business number already in
/// use — the caller's problem to fix, not an internal error.
fn duplicate_profile_to_conflict(error: sqlx::Error) -> AppError {
    if matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) {
        AppError::conflict("That profile number is already in use")
    } else {
        AppError::from(error)
    }
}
