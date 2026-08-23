//! Queries for the party itself and everything §4.1-4.11 hangs off it.
//!
//! The conventions these follow — tenant scoping, soft delete, and why decimal
//! columns travel as text — are on the parent module.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::master_data::domain::{PartyStatusCode, PartySummary, PartyType};

/// A party's own row, before its children are loaded.
pub struct PartyRow {
    pub id: Uuid,
    pub party_code: String,
    pub party_type: PartyType,
    pub status: PartyStatusCode,
    pub external_id: Option<String>,
    pub description: Option<String>,
    pub attributes_json: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Columns of a new party row.
pub struct NewParty<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub party_code: &'a str,
    pub party_type: &'a str,
    pub status: &'a str,
    pub external_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub attributes_json: &'a Value,
    pub created_by: Option<Uuid>,
}

/// Person detail as the database holds it. `None` means *leave alone* on an
/// update and *store nothing* on an insert.
#[derive(Default)]
pub struct PersonFields<'a> {
    pub first_name: Option<&'a str>,
    pub middle_name: Option<&'a str>,
    pub last_name: Option<&'a str>,
    pub personal_title: Option<&'a str>,
    pub suffix: Option<&'a str>,
    pub gender: Option<&'a str>,
    pub birth_date: Option<NaiveDate>,
    pub marital_status: Option<&'a str>,
    pub comments: Option<&'a str>,
}

#[derive(Default)]
pub struct PartyGroupFields<'a> {
    pub group_name: Option<&'a str>,
    pub local_name: Option<&'a str>,
    pub office_site_name: Option<&'a str>,
    pub annual_revenue: Option<&'a str>,
    pub num_employees: Option<i32>,
    pub ticker_symbol: Option<&'a str>,
    pub comments: Option<&'a str>,
}

pub struct IdentificationFields<'a> {
    pub identification_type: &'a str,
    pub id_value: &'a str,
    pub issued_by: Option<&'a str>,
    pub issue_date: Option<NaiveDate>,
    pub expire_date: Option<NaiveDate>,
    pub attributes_json: &'a Value,
}

/// A relationship with both ends already resolved to surrogate keys.
pub struct RelationshipFields<'a> {
    pub from_party_id: Uuid,
    pub to_party_id: Uuid,
    pub relationship_type: &'a str,
    pub from_role_type_id: Option<Uuid>,
    pub to_role_type_id: Option<Uuid>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub status: Option<&'a str>,
    pub priority: Option<i32>,
    pub comments: Option<&'a str>,
    pub attributes_json: &'a Value,
}

pub struct ClassificationFields<'a> {
    pub class_type: &'a str,
    pub classification_code: Option<&'a str>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub comments: Option<&'a str>,
}

/// One contact mechanism on a party: either a reference to a mechanism that
/// already exists, or the value of one to create.
pub struct ContactMechFields<'a> {
    pub existing_contact_mech_id: Option<Uuid>,
    pub contact_mech_type: Option<&'a str>,
    pub display_value: Option<&'a str>,
    pub detail_json: &'a Value,
    pub purpose_type: Option<&'a str>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub is_primary: bool,
    pub allow_solicitation: bool,
    pub attributes_json: &'a Value,
}

// ---------------------------------------------------------------------------
// Parties
// ---------------------------------------------------------------------------

pub async fn count_parties(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM mdm_parties WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One row per party, with the name a list renders already projected.
///
/// The joins are what keep this a single query: loading the six child
/// collections per row would turn a page of a hundred into six hundred queries.
pub async fn list_parties(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<PartySummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT p.id,
               p.party_code,
               p.party_type,
               p.status,
               p.external_id,
               p.created_at,
               p.updated_at,
               COALESCE(
                   g.group_name,
                   NULLIF(btrim(concat_ws(' ', pe.first_name, pe.middle_name, pe.last_name)), ''),
                   p.party_code
               ) AS "name!"
        FROM mdm_parties p
        LEFT JOIN mdm_persons pe ON pe.party_id = p.id AND pe.deleted_at IS NULL
        LEFT JOIN mdm_party_groups g ON g.party_id = p.id AND g.deleted_at IS NULL
        WHERE p.tenant_id = $1 AND p.deleted_at IS NULL
        ORDER BY p.party_code
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
        .map(|row| PartySummary {
            id: row.id,
            party_id: row.party_code,
            party_type_id: PartyType::from_db(&row.party_type),
            status_id: PartyStatusCode::from_db(&row.status),
            name: row.name,
            external_id: row.external_id,
            created_stamp: row.created_at,
            last_updated_stamp: row.updated_at,
        })
        .collect())
}

/// [`find_party`], holding the row until the transaction ends.
///
/// Takes `&mut PgConnection` rather than an executor because `FOR UPDATE`
/// outside a transaction locks nothing worth having: the lock would be released
/// with the statement. The caller must be inside one.
///
/// This is how "a party holds a role once" is kept true under concurrency
/// (#105). Assigning a role is check-then-act — read whether the party already
/// holds it, then insert or update — and the two halves ran on different
/// connections with nothing between them, so two requests both read *no* and
/// both inserted. The party row is what they contend on; taking it first means
/// the second request reads what the first wrote.
///
/// It also re-asks a question the caller answered before the transaction began:
/// a party soft-deleted in between is gone by the time this runs, and the role
/// is not written onto it.
pub async fn lock_party(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<PartyRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, party_code, party_type, status, external_id, description,
               attributes_json, created_at, updated_at
        FROM mdm_parties
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        id
    )
    .fetch_optional(connection)
    .await?;

    Ok(row.map(|row| PartyRow {
        id: row.id,
        party_code: row.party_code,
        party_type: PartyType::from_db(&row.party_type),
        status: PartyStatusCode::from_db(&row.status),
        external_id: row.external_id,
        description: row.description,
        attributes_json: row.attributes_json,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

pub async fn find_party(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<PartyRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, party_code, party_type, status, external_id, description,
               attributes_json, created_at, updated_at
        FROM mdm_parties
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| PartyRow {
        id: row.id,
        party_code: row.party_code,
        party_type: PartyType::from_db(&row.party_type),
        status: PartyStatusCode::from_db(&row.status),
        external_id: row.external_id,
        description: row.description,
        attributes_json: row.attributes_json,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// The surrogate key behind a business code, for resolving the far end of a
/// relationship. Tenant-scoped: a code from another tenant must not resolve.
pub async fn find_party_id_by_code(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM mdm_parties
        WHERE tenant_id = $1 AND party_code = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_code
    )
    .fetch_optional(executor)
    .await
}

pub async fn insert_party(
    executor: impl PgExecutor<'_>,
    party: NewParty<'_>,
) -> Result<(), sqlx::Error> {
    // record_status is left to its DRAFT default: nothing in this phase moves a
    // party through the workflow lifecycle (FR-MDM-010 is Phase 5+), and a
    // value written here would be a value nothing maintains.
    sqlx::query!(
        r#"
        INSERT INTO mdm_parties (
            id, tenant_id, party_code, party_type, status, external_id,
            description, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        party.id,
        party.tenant_id,
        party.party_code,
        party.party_type,
        party.status,
        party.external_id,
        party.description,
        party.attributes_json,
        party.created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Updates the party's own columns. `COALESCE` keeps every unsupplied field at
/// its current value, so a partial update cannot blank a column it did not
/// mention — the same convention `update_user_fields` uses.
#[allow(
    clippy::too_many_arguments,
    reason = "one parameter per updatable column, each independently optional"
)]
pub async fn update_party_fields(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    status: Option<&str>,
    external_id: Option<&str>,
    description: Option<&str>,
    attributes_json: Option<&Value>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_parties
        SET status = COALESCE($3, status),
            external_id = COALESCE($4, external_id),
            description = COALESCE($5, description),
            attributes_json = COALESCE($6, attributes_json),
            updated_by = $7,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        status,
        external_id,
        description,
        attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Soft-deletes the party. The child rows are left as they are: they are only
/// reachable through the party, and hard-deleting them would discard the record
/// a restore would need.
pub async fn soft_delete_party(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_parties
        SET deleted_at = now(), status = 'PARTY_DISABLED', updated_by = $3, updated_at = now()
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
