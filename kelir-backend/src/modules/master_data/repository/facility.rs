//! Queries for `mdm_facilities` (§4.16).
//!
//! The conventions these follow — tenant scoping, soft delete — are on the
//! parent module.
//!
//! **The parent and the owner are joined, not fetched.** A facility carries its
//! parent's `facility_code` and its owner's `party_code` rather than their
//! surrogate ids, so every read here left-joins both. Reading a page and then
//! resolving each row's two references would turn a hundred rows into two
//! hundred queries, which is the failure NFR-PERF-002 exists to prevent — the
//! same reason `list_role_view` is one statement.

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::master_data::domain::{
    Facility, FacilitySummary, FacilityType, PostalAddress, RecordStatus,
};

/// The columns a create writes. `record_status` is absent deliberately: it is
/// left at its `DRAFT` default because nothing moves it until #99, and a value
/// written here would be a value nothing maintains.
pub struct NewFacility<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub facility_code: &'a str,
    pub name: &'a str,
    pub facility_type: Option<&'a str>,
    pub parent_facility_id: Option<Uuid>,
    pub owner_party_id: Option<Uuid>,
    pub address_json: &'a Value,
    pub attributes_json: &'a Value,
    pub created_by: Option<Uuid>,
}

/// What an update may change, and whether it is changing it.
///
/// `Option<Option<Uuid>>` for the two references carries the distinction the
/// request shape makes: `None` leaves the column alone, `Some(None)` clears it,
/// `Some(Some(id))` points it somewhere. `COALESCE` alone cannot express the
/// middle one.
pub struct FacilityFields<'a> {
    pub name: Option<&'a str>,
    pub facility_type: Option<&'a str>,
    pub parent_facility_id: Option<Option<Uuid>>,
    pub owner_party_id: Option<Option<Uuid>>,
    pub address_json: Option<&'a Value>,
    pub attributes_json: Option<&'a Value>,
}

pub async fn count_facilities(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM mdm_facilities WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_facilities(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<FacilitySummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT f.id, f.facility_code, f.name, f.facility_type,
               parent.facility_code AS "parent_code?",
               owner.party_code AS "owner_code?",
               f.created_at, f.updated_at
        FROM mdm_facilities f
        LEFT JOIN mdm_facilities parent
          ON parent.id = f.parent_facility_id AND parent.tenant_id = f.tenant_id
             AND parent.deleted_at IS NULL
        LEFT JOIN mdm_parties owner
          ON owner.id = f.owner_party_id AND owner.tenant_id = f.tenant_id
             AND owner.deleted_at IS NULL
        WHERE f.tenant_id = $1 AND f.deleted_at IS NULL
        ORDER BY f.facility_code
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
        .map(|row| FacilitySummary {
            id: row.id,
            facility_id: row.facility_code,
            name: row.name,
            facility_type_id: row.facility_type.as_deref().and_then(FacilityType::from_db),
            parent_facility_id: row.parent_code,
            owner_party_id: row.owner_code,
            created_stamp: row.created_at,
            last_updated_stamp: row.updated_at,
        })
        .collect())
}

pub async fn find_facility(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Facility>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT f.id, f.facility_code, f.name, f.facility_type, f.record_status,
               parent.facility_code AS "parent_code?",
               owner.party_code AS "owner_code?",
               f.address_json, f.attributes_json, f.created_at, f.updated_at
        FROM mdm_facilities f
        LEFT JOIN mdm_facilities parent
          ON parent.id = f.parent_facility_id AND parent.tenant_id = f.tenant_id
             AND parent.deleted_at IS NULL
        LEFT JOIN mdm_parties owner
          ON owner.id = f.owner_party_id AND owner.tenant_id = f.tenant_id
             AND owner.deleted_at IS NULL
        WHERE f.tenant_id = $1 AND f.id = $2 AND f.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Facility {
        id: row.id,
        facility_id: row.facility_code,
        name: row.name,
        facility_type_id: row.facility_type.as_deref().and_then(FacilityType::from_db),
        record_status_id: RecordStatus::from_db(&row.record_status),
        parent_facility_id: row.parent_code,
        owner_party_id: row.owner_code,
        address: address_from(row.address_json),
        additional_attributes: row.attributes_json,
        created_stamp: row.created_at,
        last_updated_stamp: row.updated_at,
    }))
}

/// The surrogate id behind a `facilityId`, for resolving a parent reference.
pub async fn find_facility_id_by_code(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    facility_code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM mdm_facilities
        WHERE tenant_id = $1 AND facility_code = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        facility_code
    )
    .fetch_optional(executor)
    .await
}

/// Every facility on the path from `start` up to its root, `start` included.
///
/// This is what makes the hierarchy a tree rather than a graph. `parent_facility_id`
/// is a self-reference and no database constraint can say "and no cycles", so
/// the service walks up from the proposed parent before writing: if the
/// facility being re-parented is anywhere on that path, the write would close a
/// loop and is refused.
///
/// **Depth-bounded, and the bound is not a business rule.** A cycle already in
/// storage would make an unbounded `WITH RECURSIVE` spin until the connection
/// died; stopping at `max_depth` means the worst case is a wrong answer rather
/// than a hung request. No such cycle can be written through this module, which
/// is exactly the claim that needs a limit behind it rather than a comment.
pub async fn facility_ancestors(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    start: Uuid,
    max_depth: i32,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query_scalar!(
        r#"
        WITH RECURSIVE up AS (
            SELECT id, parent_facility_id, 1 AS depth
            FROM mdm_facilities
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            UNION ALL
            SELECT f.id, f.parent_facility_id, up.depth + 1
            FROM mdm_facilities f
            JOIN up ON f.id = up.parent_facility_id
            WHERE f.tenant_id = $1 AND f.deleted_at IS NULL AND up.depth < $3
        )
        SELECT id AS "id!" FROM up
        "#,
        tenant_id,
        start,
        max_depth
    )
    .fetch_all(executor)
    .await?;

    Ok(rows)
}

pub async fn insert_facility(
    executor: impl PgExecutor<'_>,
    facility: &NewFacility<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_facilities (
            id, tenant_id, facility_code, name, facility_type, parent_facility_id,
            owner_party_id, address_json, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
        facility.id,
        facility.tenant_id,
        facility.facility_code,
        facility.name,
        facility.facility_type,
        facility.parent_facility_id,
        facility.owner_party_id,
        facility.address_json,
        facility.attributes_json,
        facility.created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Updates a facility's own columns.
///
/// `COALESCE` for the fields that can only be replaced, and an explicit
/// "changing / not changing" flag for the two references, because they can also
/// be cleared: `$6` says whether `parent_facility_id` is being written at all
/// and `$7` is the value, so `NULL` means *detach* rather than *leave alone*.
#[allow(
    clippy::too_many_arguments,
    reason = "one parameter per updatable column, each independently optional"
)]
pub async fn update_facility_fields(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    fields: &FacilityFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (set_parent, parent) = match fields.parent_facility_id {
        Some(value) => (true, value),
        None => (false, None),
    };
    let (set_owner, owner) = match fields.owner_party_id {
        Some(value) => (true, value),
        None => (false, None),
    };

    sqlx::query!(
        r#"
        UPDATE mdm_facilities
        SET name = COALESCE($3, name),
            facility_type = COALESCE($4, facility_type),
            address_json = COALESCE($5, address_json),
            attributes_json = COALESCE($6, attributes_json),
            parent_facility_id = CASE WHEN $7 THEN $8 ELSE parent_facility_id END,
            owner_party_id = CASE WHEN $9 THEN $10 ELSE owner_party_id END,
            updated_by = $11,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.name,
        fields.facility_type,
        fields.address_json,
        fields.attributes_json,
        set_parent,
        parent,
        set_owner,
        owner,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Soft-deletes a facility.
///
/// Its children are left alone rather than cascaded. A room in a demolished
/// building is still a room, and re-parenting it is a decision the deleter has
/// to make — [`children_of`] is what lets the service refuse the delete instead
/// of guessing.
pub async fn soft_delete_facility(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_facilities
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// How many live facilities name this one as their parent.
pub async fn children_of(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*) FROM mdm_facilities
        WHERE tenant_id = $1 AND parent_facility_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}

/// `address_json` back into the shape it was stored from.
///
/// An empty object is `None` rather than an all-`null` address: the column
/// defaults to `{}`, so every facility created without one would otherwise
/// carry a hollow `address` member that a client has to test field by field.
fn address_from(value: Value) -> Option<PostalAddress> {
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        return None;
    }

    serde_json::from_value(value).ok()
}
