//! Role and role-profile use cases (FR-MDM-002).
//!
//! Split out of `service.rs` by #112 with no behaviour change. The three
//! role-filtered list endpoints are in [`super::role_view`]; what is here is
//! the roles of one party.

use serde_json::json;
use uuid::Uuid;

use super::domain::{
    validate_assign_role, AssignRoleRequest, EmploymentType, PartyProfiles, PartyRole,
    PartyRoleStatus, PartyRoles, RoleProfileInput, SupplierApprovalStatus,
};
use super::party::trimmed;
use super::repository::{
    self as repo, ContactProfileFields, CustomerProfileFields, EmployeeProfileFields,
    PartyRoleFields, SupplierProfileFields,
};
use super::{OBJECT_TYPE, ROLE_READ};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry};
use crate::state::AppState;

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

    // Asked before the profile references are resolved, so that a request aimed
    // at a party that does not exist is answered with that and not with which
    // of its profile references was wrong. It is the ordering `list_role_view`
    // argues for one screen away — refuse on the resource before reading the
    // request — and the one every version of this function has had; hoisting
    // the resolve out of the transaction for #118 would otherwise have swapped
    // it silently.
    //
    // A hint, not the authority. The party is found again under `FOR UPDATE`
    // below and a 404 from there is the one that counts: this read is on the
    // pool and a party deleted between the two is exactly what #116 made the
    // lock re-ask.
    if repo::find_party(&state.pool, tenant_id, party_id)
        .await?
        .is_none()
    {
        return Err(AppError::not_found("Party"));
    }

    // Resolved before the transaction opens, and that ordering is the point.
    //
    // This runs on `state.pool`, so it takes a connection of its own. Called
    // between `begin()` and `commit()` it would take a *second* connection
    // while the transaction still holds the first, and enough concurrent
    // assignments would then wait on connections held by each other: the pool
    // ceiling is 10 and the acquire timeout 5 seconds, so ten of them stalled
    // and answered 500 (#118). It is a self-deadlock, not contention — the
    // requests are not waiting on the database, they are waiting on each other.
    //
    // Nothing here needs the lock. It resolves what the *request* points at, so
    // it belongs where `create_party` and `update_party` already put it, ahead
    // of their own `begin()`. What must stay inside is `validate_assign_role`,
    // which reads `creating`.
    //
    // The failure it exists for is unchanged: a manager or a department that
    // does not exist is a 422 naming the path, rather than a foreign-key
    // violation surfacing as a 500.
    let references = resolve_profile_references(state, tenant_id, request.profile.as_ref()).await?;

    let mut transaction = state.pool.begin().await?;

    // The party is found and held in the same statement, and everything that
    // decides between an insert and an update is read after it.
    //
    // This used to read the party and its existing role on the pool and then
    // open a transaction to act on what it read — check-then-act across a
    // connection boundary. Two concurrent assignments both read *no such role*
    // and both inserted, and the database did not catch it because
    // `uq_mdm_party_roles_party_id_role_type_id_starts_at` includes `starts_at`,
    // so two rows with different `fromDate` do not collide. It left the party
    // holding one role twice, 28 times in 30 (#105).
    //
    // Waiting on the lock rather than trying it, unlike the bootstrap's: both
    // requests are legitimate and both must finish, one creating and one
    // updating. Standing down would fail a request that did nothing wrong.
    let party = repo::lock_party(&mut transaction, tenant_id, party_id)
        .await?
        .ok_or_else(|| AppError::not_found("Party"))?;

    let existing =
        repo::find_live_party_role(&mut *transaction, tenant_id, party_id, role_type_id).await?;
    let creating = existing.is_none();

    // Validated against the *locked* answer, not a hint read earlier. Whether a
    // profile is required depends on whether this is a create, and reading that
    // outside the lock is what this change exists to stop doing.
    validate_assign_role(&request, role_type, &party.party_code, creating)?;

    let role_fields = PartyRoleFields {
        starts_at: request.from_date,
        ends_at: request.thru_date,
        status: request.status_id.map(PartyRoleStatus::as_db),
        comments: request.comments.as_deref(),
        attributes_json: request.additional_attributes.as_ref(),
    };

    // The row's own id, kept so the read-back below is a primary-key lookup
    // rather than a second search for the row this just wrote (#121).
    let assignment_id = match existing {
        Some(id) => {
            repo::update_party_role(&mut *transaction, id, &role_fields, actor).await?;
            id
        }
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
    };

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

    // Read back inside the transaction, so what the route answers with is the
    // row as this call left it rather than as a later one may have.
    let assignment = repo::find_party_role_by_id(&mut *transaction, assignment_id)
        .await?
        .ok_or_else(|| AppError::not_found("Party role"))?;

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
pub(super) async fn load_roles(
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
