//! Party use cases — the aggregate's own create, read, update and delete
//! (FR-MDM-001, FR-MDM-003).
//!
//! Split out of `service.rs` by #112 with no behaviour change; the roles that
//! hang off a party are in [`super::role`], and `mod.rs` re-exports both, so
//! `service::create_party` still names this function.

use serde_json::{json, Value};
use uuid::Uuid;

use super::domain::{
    validate_create_party, validate_update_party, ContactMechType, CreatePartyRequest,
    PartyAggregate, PartyClassificationInput, PartyContactMechInput, PartyIdentificationInput,
    PartyRelationshipInput, PartyStatusCode, PartySummary, PartyType, UpdatePartyRequest,
    PROFILED_ROLE_TYPES,
};
use super::governance;
use super::repository::{
    self as repo, ClassificationFields, ContactMechFields, IdentificationFields, NewParty,
    PartyGroupFields, PartyRow, PersonFields, RelationshipFields,
};
use super::role::load_roles;
use super::{OBJECT_TYPE, PARTY_READ, ROLE_READ};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

pub async fn list_parties(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<PartySummary>, PageMeta), AppError> {
    caller.require(PARTY_READ)?;

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
    caller.require(PARTY_READ)?;

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
            ip_address: caller.ip_address(),
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

    // **A record with a change awaiting approval is not edited directly**
    // (FR-MDM-010, [#255](https://github.com/sujanto-gaws/kelir/issues/255) AC1,
    // **D-55**). The record's own status is what refuses it, under the
    // permission that already governs this write — not a new permission, and
    // not a query into the document module on every update.
    //
    // A governed change that could be overtaken by a direct edit would be a
    // governance nobody has to use.
    governance::refuse_if_awaiting_approval(before.record_status)?;

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

    // The row as it now is, against the row as it was, and only the fields that
    // moved (#135). A second reading of the same row rather than a projection
    // of the aggregate below: `before` is that row, and two readings of one
    // shape are what make the comparison mean anything.
    let after = repo::find_party(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;
    let (old_value, new_value) = party_changes(&before, &after).halves();

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
            ip_address: caller.ip_address(),
            reason: request.status_comments.as_deref(),
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    load_aggregate(state, caller, id).await
}

/// What one update moved, in the vocabulary the API publishes.
///
/// The party row's own updatable fields, `additionalAttributes` included — it
/// was updatable and named on neither side, so a request that changed only an
/// attribute produced a record in which nothing changed.
///
/// The aggregate's members are deliberately absent. `person`, `partyGroup`, the
/// identifications, relationships, classifications and contact mechanisms are
/// replaced wholesale by their own repository calls, they have never been in
/// the record, and putting them there is a wider question than the one #135
/// asks — what a *replacement* of a list even means as a before and an after.
/// `partyId` and `partyTypeId` are absent because no update can move them.
fn party_changes(before: &PartyRow, after: &PartyRow) -> ChangeSet {
    let mut changes = ChangeSet::new();

    changes.field("statusId", &before.status, &after.status);
    changes.field("externalId", &before.external_id, &after.external_id);
    changes.field("description", &before.description, &after.description);
    changes.field(
        "additionalAttributes",
        &before.attributes_json,
        &after.attributes_json,
    );

    changes
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
            ip_address: caller.ip_address(),
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
        record_status_id: party.record_status,
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
pub(super) fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// A unique violation on `mdm_parties` is a duplicate `partyId`, which is the
/// caller's problem to fix — not an internal error.
pub(super) fn duplicate_party_to_conflict(error: sqlx::Error) -> AppError {
    if matches!(&error, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")) {
        // "including by a deleted one" is the part a caller cannot guess. Since
        // #107 the uniqueness is total rather than partial on `deleted_at`, so a
        // code that a list does not show may still be taken — and the alternative
        // to saying so is a 409 the caller reads as a bug in the API.
        AppError::conflict(
            "That partyId is already in use, including by a deleted party — a partyId is never released",
        )
    } else {
        AppError::from(error)
    }
}
