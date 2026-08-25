//! Department use cases (FR-ORG-002).
//!
//! **The cycle guard is the part worth reading.** `parent_department_id` is a
//! self-reference, so the database can express *points at a department* and
//! cannot express *and not at one of its own descendants*. [`refuse_cycle`] is
//! that stop, and it is written against three lessons this project already
//! paid for on `mdm_facilities`:
//!
//! - **#133** — the check and the write it guards are one transaction, under a
//!   lock. On the pool it was a read whose answer expired before the write.
//! - **#134** — a path the walk could not finish is *refused*, not assumed
//!   safe. A prefix is not evidence of absence, and treating it as one is the
//!   depth bound creating the corruption it was there to survive.
//! - **#137** — a reference resolved before the transaction is re-read inside
//!   it, because a delete can land in between.

use serde_json::json;
use uuid::Uuid;

use super::department::{
    validate_create, validate_update, CreateDepartmentRequest, Department, DepartmentStatus,
    UpdateDepartmentRequest, MAX_DEPARTMENT_DEPTH,
};
use super::department_repository::{self as repo, DepartmentFields, NewDepartment};
use super::{DEPARTMENT_MANAGE, DEPARTMENT_READ};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, AuditEntry, ChangeSet};
use crate::response::{PageMeta, Pagination};
use crate::state::AppState;

/// What the audit trail calls a department (naming convention §7).
const OBJECT_TYPE: &str = "DEPARTMENT";

pub async fn list_departments(
    state: &AppState,
    caller: &Authenticated,
    pagination: &Pagination,
) -> Result<(Vec<Department>, PageMeta), AppError> {
    caller.require(DEPARTMENT_READ)?;

    let tenant_id = caller.tenant_id();
    let total = repo::count_departments(&state.pool, tenant_id).await?;
    let departments = repo::list_departments(
        &state.pool,
        tenant_id,
        pagination.limit(),
        pagination.offset(),
    )
    .await?;

    Ok((departments, pagination.meta(total.max(0) as u64)))
}

pub async fn get_department(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<Department, AppError> {
    caller.require(DEPARTMENT_READ)?;

    repo::find_department(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Department"))
}

pub async fn create_department(
    state: &AppState,
    caller: &Authenticated,
    request: CreateDepartmentRequest,
) -> Result<Department, AppError> {
    caller.require(DEPARTMENT_MANAGE)?;
    validate_create(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    // Both references resolve on the pool, before the transaction opens, so
    // this call holds one connection at a time (coding standard §2.5, #118).
    let parent = resolve_parent(state, tenant_id, request.parent_department_id.as_deref()).await?;
    let manager = resolve_manager(state, tenant_id, request.manager_party_id.as_deref()).await?;

    let id = Uuid::now_v7();

    let mut transaction = state.pool.begin().await?;

    // A create cannot close a loop — nothing points at a department that does
    // not exist yet — so the hierarchy lock is taken only to re-read the
    // parent, which is #137's lesson: resolving it above answered "is there
    // such a department" a moment ago, and a delete landing in between would
    // leave this row under a department that no longer exists.
    if let Some(parent_id) = parent {
        repo::lock_department_hierarchy(&mut transaction, tenant_id).await?;

        if !repo::department_is_live(&mut *transaction, tenant_id, parent_id).await? {
            return Err(parent_no_longer_there());
        }
    }

    repo::insert_department(
        &mut *transaction,
        &NewDepartment {
            id,
            tenant_id,
            department_code: request.department_id.trim(),
            name: request.name.trim(),
            parent_department_id: parent,
            manager_party_id: manager,
            status: request.status.unwrap_or(DepartmentStatus::Active).as_db(),
            created_by: actor,
        },
    )
    .await
    .map_err(duplicate_to_conflict)?;

    transaction.commit().await?;

    // Read back before the record is written (#135): the code is trimmed on the
    // way in, and the references are stored against the codes the resolver
    // found.
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Department.Created",
            action: "CREATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "departmentId": created.department_code,
                "name": created.name,
                "parentDepartmentId": created.parent_department_id,
                "managerPartyId": created.manager_party_id,
                "status": created.status,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_department(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: UpdateDepartmentRequest,
) -> Result<Department, AppError> {
    caller.require(DEPARTMENT_MANAGE)?;
    validate_update(&request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_department(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Department"))?;

    // Both references resolve on the pool, before the transaction opens (#118).
    let parent = match &request.parent_department_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(code)) => Some(resolve_parent(state, tenant_id, Some(code)).await?),
    };

    let manager = match &request.manager_party_id {
        None => None,
        Some(None) => Some(None),
        Some(Some(code)) => Some(resolve_manager(state, tenant_id, Some(code)).await?),
    };

    // The cycle check and the write it guards, in one transaction (#133).
    let mut transaction = state.pool.begin().await?;

    if let Some(Some(parent_id)) = parent {
        repo::lock_department_hierarchy(&mut transaction, tenant_id).await?;

        // Re-read under the lock, for the same reason a create does (#137).
        if !repo::department_is_live(&mut *transaction, tenant_id, parent_id).await? {
            return Err(parent_no_longer_there());
        }

        if parent_id == id {
            return Err(AppError::validation(vec![ValidationDetail::new(
                "parentDepartmentId",
                "consistency",
                "CYCLE",
                "A department cannot be its own parent",
            )]));
        }

        refuse_cycle(&mut *transaction, tenant_id, id, parent_id).await?;
    }

    let affected = repo::update_department(
        &mut *transaction,
        tenant_id,
        id,
        &DepartmentFields {
            name: request.name.as_deref().map(str::trim),
            parent_department_id: parent,
            manager_party_id: manager,
            status: request.status.map(DepartmentStatus::as_db),
        },
        actor,
    )
    .await?;

    if affected == 0 {
        return Err(AppError::not_found("Department"));
    }

    transaction.commit().await?;

    let after = load(state, tenant_id, id).await?;

    // What changed, not what was requested (#135).
    let mut changes = ChangeSet::new();
    changes.field("name", &before.name, &after.name);
    changes.field(
        "parentDepartmentId",
        &before.parent_department_id,
        &after.parent_department_id,
    );
    changes.field(
        "managerPartyId",
        &before.manager_party_id,
        &after.manager_party_id,
    );
    changes.field("status", &before.status, &after.status);

    let (old_value, new_value) = changes.halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Department.Updated",
            action: "UPDATE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(old_value),
            new_value: Some(new_value),
        },
    )
    .await;

    Ok(after)
}

pub async fn delete_department(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(DEPARTMENT_MANAGE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());

    let before = repo::find_department(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Department"))?;

    // Refused rather than cascaded, and refused with a count of each kind of
    // thing that would be stranded. `ON DELETE RESTRICT` would refuse too, as a
    // 500 saying "foreign key violation" — which tells an administrator nothing
    // about what to move first.
    let dependents = repo::dependents(&state.pool, tenant_id, id).await?;

    if dependents.any() {
        return Err(AppError::conflict(format!(
            "`{}` still has {} sub-department(s), {} user(s) and {} employee \
             profile(s) pointing at it; move or reassign them first, or set the \
             department to INACTIVE, which stops it being chosen",
            before.department_code,
            dependents.children,
            dependents.users,
            dependents.employee_profiles
        )));
    }

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("Department"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "Department.Deleted",
            action: "DELETE",
            object_type: OBJECT_TYPE,
            object_id: id,
            actor_user_id: actor,
            ip_address: None,
            reason: None,
            old_value: Some(json!({
                "departmentId": before.department_code,
                "name": before.name,
                "status": before.status,
            })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Refuses a move that would make `id` its own ancestor.
///
/// Walking up from the proposed parent is the check: if `id` is on that path,
/// then making `parent` the parent of `id` closes a loop. It needs a test more
/// than most rules here do, because the failure is not a wrong answer — a cycle
/// in storage makes any traversal loop until something times out.
///
/// **Runs inside the caller's transaction, under
/// [`repo::lock_department_hierarchy`]** (#133), and **a path it could not walk
/// to the end is refused rather than assumed safe** (#134). The caller is told
/// the depth is the reason, because "no" without one is indistinguishable from
/// a defect.
async fn refuse_cycle(
    executor: impl sqlx::PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    parent: Uuid,
) -> Result<(), AppError> {
    let ancestry =
        repo::department_ancestors(executor, tenant_id, parent, MAX_DEPARTMENT_DEPTH).await?;

    if ancestry.ids.contains(&id) {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentDepartmentId",
            "consistency",
            "CYCLE",
            "That department is under this one; moving it there would close a loop",
        )]));
    }

    if ancestry.truncated {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentDepartmentId",
            "consistency",
            "TOO_DEEP",
            format!(
                "That department sits more than {MAX_DEPARTMENT_DEPTH} levels deep, so this \
                 move cannot be checked for a loop. Move it nearer the root first"
            ),
        )]));
    }

    Ok(())
}

/// The parent named by the request went away between resolving it and writing.
///
/// The same 422 `resolve_parent` gives, deliberately: from the caller's side
/// nothing distinguishes a parent that never existed from one deleted while
/// their request was in flight.
fn parent_no_longer_there() -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "parentDepartmentId",
        "exists",
        "NOT_FOUND",
        "No department with that departmentId",
    )])
}

/// The surrogate id behind a `parentDepartmentId`, or a 422 naming the field.
async fn resolve_parent(
    state: &AppState,
    tenant_id: Uuid,
    code: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_department_id_by_code(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => Err(parent_no_longer_there()),
    }
}

/// The surrogate id behind a `managerPartyId`, or a 422 naming the field.
async fn resolve_manager(
    state: &AppState,
    tenant_id: Uuid,
    code: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    let Some(code) = code.map(str::trim).filter(|code| !code.is_empty()) else {
        return Ok(None);
    };

    match repo::find_party_id_by_code(&state.pool, tenant_id, code).await? {
        Some(id) => Ok(Some(id)),
        None => Err(AppError::validation(vec![ValidationDetail::new(
            "managerPartyId",
            "exists",
            "NOT_FOUND",
            "No party with that partyId",
        )])),
    }
}

fn duplicate_to_conflict(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            AppError::conflict("a department with this departmentId already exists")
        }
        _ => error.into(),
    }
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<Department, AppError> {
    repo::find_department(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::Internal {
            source: anyhow::anyhow!("department {id} vanished after it was written"),
        })
}
