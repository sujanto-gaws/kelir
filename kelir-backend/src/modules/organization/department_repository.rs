//! Queries for `departments` (§3.4).
//!
//! Tenant-scoped and soft-delete aware throughout. The parent and the manager
//! are **joined, not fetched**: a department carries its parent's code and its
//! manager's party code rather than their surrogate ids, and resolving each row
//! separately would turn a page of a hundred into two hundred queries — the
//! failure NFR-PERF-002 exists to prevent, and the reason `list_facilities` is
//! one statement.

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::department::{Department, DepartmentStatus};

pub struct NewDepartment<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub department_code: &'a str,
    pub name: &'a str,
    pub parent_department_id: Option<Uuid>,
    pub manager_party_id: Option<Uuid>,
    pub status: &'a str,
    pub created_by: Option<Uuid>,
}

/// What an update may change. `None` leaves the column alone; the nested
/// `Option` on a nullable reference distinguishes "leave it" from "clear it",
/// which `COALESCE` alone cannot express.
pub struct DepartmentFields<'a> {
    pub name: Option<&'a str>,
    pub parent_department_id: Option<Option<Uuid>>,
    pub manager_party_id: Option<Option<Uuid>>,
    pub status: Option<&'a str>,
}

/// The ids on the path from a department up to its root, and whether the walk
/// ran out of budget.
pub struct DepartmentAncestry {
    pub ids: Vec<Uuid>,
    pub truncated: bool,
}

pub async fn count_departments(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM departments WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_departments(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Department>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT d.id, d.department_code, d.name, d.status, d.created_at, d.updated_at,
               parent.department_code AS "parent_code?",
               manager.party_code AS "manager_code?"
        FROM departments d
        LEFT JOIN departments parent
          ON parent.id = d.parent_department_id AND parent.tenant_id = d.tenant_id
             AND parent.deleted_at IS NULL
        LEFT JOIN mdm_parties manager
          ON manager.id = d.manager_party_id AND manager.tenant_id = d.tenant_id
             AND manager.deleted_at IS NULL
        WHERE d.tenant_id = $1 AND d.deleted_at IS NULL
        ORDER BY d.department_code
        LIMIT $2 OFFSET $3
        "#,
        tenant_id,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Department {
            id: row.id,
            department_code: row.department_code,
            name: row.name,
            parent_department_id: row.parent_code,
            manager_party_id: row.manager_code,
            status: DepartmentStatus::from_db(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_department<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Department>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT d.id, d.department_code, d.name, d.status, d.created_at, d.updated_at,
               parent.department_code AS "parent_code?",
               manager.party_code AS "manager_code?"
        FROM departments d
        LEFT JOIN departments parent
          ON parent.id = d.parent_department_id AND parent.tenant_id = d.tenant_id
             AND parent.deleted_at IS NULL
        LEFT JOIN mdm_parties manager
          ON manager.id = d.manager_party_id AND manager.tenant_id = d.tenant_id
             AND manager.deleted_at IS NULL
        WHERE d.tenant_id = $1 AND d.id = $2 AND d.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Department {
        id: row.id,
        department_code: row.department_code,
        name: row.name,
        parent_department_id: row.parent_code,
        manager_party_id: row.manager_code,
        status: DepartmentStatus::from_db(&row.status),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

pub async fn find_department_id_by_code<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT id FROM departments
         WHERE tenant_id = $1 AND department_code = $2 AND deleted_at IS NULL",
        tenant_id,
        code
    )
    .fetch_optional(executor)
    .await
}

pub async fn find_party_id_by_code<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT id FROM mdm_parties
         WHERE tenant_id = $1 AND party_code = $2 AND deleted_at IS NULL",
        tenant_id,
        code
    )
    .fetch_optional(executor)
    .await
}

/// Whether a department is still live, for re-checking a reference under the
/// lock.
pub async fn department_is_live<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM departments
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        ) AS "exists!"
        "#,
        tenant_id,
        id
    )
    .fetch_one(executor)
    .await
}

/// The path from `start` up to its root, bounded by `max_depth`.
///
/// **A truncated walk is reported, not silently returned** (#134). Past the
/// bound the root is simply not in the answer, so `contains(&id)` would say
/// "not an ancestor" about a department that is one — the bound creating the
/// corruption it was there to survive. `truncated` travels with the ids and the
/// service turns it into a refusal.
///
/// A walk stopping early for any *other* reason — a parent that is soft-deleted
/// or in another tenant — is not truncation: such a parent is not in the live
/// tree, so no live traversal reaches through it, and the path returned is
/// complete with respect to the tree the product can see.
pub async fn department_ancestors<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    start: Uuid,
    max_depth: i32,
) -> Result<DepartmentAncestry, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        WITH RECURSIVE up AS (
            SELECT id, parent_department_id, 1 AS depth
            FROM departments
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            UNION ALL
            SELECT d.id, d.parent_department_id, up.depth + 1
            FROM departments d
            JOIN up ON d.id = up.parent_department_id
            WHERE d.tenant_id = $1 AND d.deleted_at IS NULL AND up.depth < $3
        )
        SELECT id AS "id!", parent_department_id, depth AS "depth!" FROM up
        "#,
        tenant_id,
        start,
        max_depth
    )
    .fetch_all(executor)
    .await?;

    // The deepest row still naming a parent is the walk running out of budget:
    // the recursive term declines to follow it precisely because `depth` has
    // reached the bound.
    let truncated = rows
        .iter()
        .any(|row| row.depth >= max_depth && row.parent_department_id.is_some());

    Ok(DepartmentAncestry {
        ids: rows.into_iter().map(|row| row.id).collect(),
        truncated,
    })
}

/// Serialises re-parenting within a tenant, for the whole transaction.
///
/// **Locking each caller's own row and its proposed parent is not enough**, and
/// the counter-example is worth keeping: with `B → C` and `D → A` stored, one
/// caller moving `A` under `B` and another moving `C` under `D` each walk a
/// path the other is about to change. Both checks pass, both writes land, and
/// the result is `A → B → C → D → A` — four departments, two disjoint lock
/// sets, nothing serialised. That is #133 on facilities, and the shape does not
/// depend on the table.
///
/// A tenant-wide lock is the version whose correctness needs no proof.
/// Re-parenting a department is a rare administrative act, and taking it one at
/// a time per tenant costs nothing anybody will measure.
///
/// Keyed on a class constant and a hash of the tenant, in the two-argument
/// form, so tenants do not wait on each other and this cannot collide with the
/// bootstrap's single-argument lock — PostgreSQL keeps the one- and
/// two-argument spaces apart.
pub async fn lock_department_hierarchy(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock($1, hashtext($2::text))",
        DEPARTMENT_HIERARCHY_LOCK_CLASS,
        tenant_id.to_string()
    )
    .execute(connection)
    .await
    .map(|_| ())
}

/// Lock class for [`lock_department_hierarchy`]. The ASCII of `DEPT`, carrying
/// no meaning beyond being unlikely to collide with another class.
const DEPARTMENT_HIERARCHY_LOCK_CLASS: i32 = 0x4445_5054;

pub async fn insert_department<'e, E: PgExecutor<'e>>(
    executor: E,
    department: &NewDepartment<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO departments
            (id, tenant_id, department_code, name, parent_department_id,
             manager_party_id, status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        department.id,
        department.tenant_id,
        department.department_code,
        department.name,
        department.parent_department_id,
        department.manager_party_id,
        department.status,
        department.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_department<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &DepartmentFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (parent_set, parent) = split(fields.parent_department_id);
    let (manager_set, manager) = split(fields.manager_party_id);

    sqlx::query!(
        r#"
        UPDATE departments
        SET name = COALESCE($3, name),
            parent_department_id = CASE WHEN $4 THEN $5 ELSE parent_department_id END,
            manager_party_id = CASE WHEN $6 THEN $7 ELSE manager_party_id END,
            status = COALESCE($8, status),
            updated_by = $9,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.name,
        parent_set,
        parent,
        manager_set,
        manager,
        fields.status,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

fn split<T>(field: Option<Option<T>>) -> (bool, Option<T>) {
    match field {
        None => (false, None),
        Some(value) => (true, value),
    }
}

/// Retires a department by soft delete.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE departments
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        deleted_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// What still points at a department, for a delete that would otherwise orphan
/// it.
pub struct Dependents {
    pub children: i64,
    pub users: i64,
    pub employee_profiles: i64,
}

impl Dependents {
    pub fn any(&self) -> bool {
        self.children > 0 || self.users > 0 || self.employee_profiles > 0
    }
}

/// Counts what a retirement would strand.
///
/// **Three references, and all three are counted in one statement.** A delete
/// that checked only children would leave users pointing at a department no
/// read returns; one that checked only users would leave a subtree orphaned.
/// The employee-profile reference is the one that is easy to forget, because it
/// lives in another module — and it is the very reference that made this issue
/// urgent, since `master_data` validates against `departments` while nothing
/// could create one.
pub async fn dependents<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Dependents, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            (SELECT count(*) FROM departments
             WHERE tenant_id = $1 AND parent_department_id = $2 AND deleted_at IS NULL)
                AS "children!",
            (SELECT count(*) FROM users
             WHERE tenant_id = $1 AND department_id = $2 AND deleted_at IS NULL)
                AS "users!",
            (SELECT count(*) FROM mdm_employee_profiles
             WHERE tenant_id = $1 AND department_id = $2 AND deleted_at IS NULL)
                AS "employee_profiles!"
        "#,
        tenant_id,
        id
    )
    .fetch_one(executor)
    .await?;

    Ok(Dependents {
        children: row.children,
        users: row.users,
        employee_profiles: row.employee_profiles,
    })
}
