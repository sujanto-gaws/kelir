//! Queries for the party itself and everything §4.1-4.11 hangs off it.
//!
//! The conventions these follow — tenant scoping, soft delete, and why decimal
//! columns travel as text — are on the parent module.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::master_data::domain::{
    ContactMechType, Gender, PartyClassification, PartyContactMech, PartyGroup,
    PartyIdentification, PartyRelationship, PartyStatus, PartyStatusCode, PartySummary, PartyType,
    Person,
};

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

// ---------------------------------------------------------------------------
// Person and party group extensions
// ---------------------------------------------------------------------------

pub async fn insert_person(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &PersonFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_persons (
            id, tenant_id, party_id, first_name, middle_name, last_name,
            personal_title, suffix, gender, birth_date, marital_status, comments, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        "#,
        id,
        tenant_id,
        party_id,
        fields.first_name.unwrap_or_default(),
        fields.middle_name,
        fields.last_name.unwrap_or_default(),
        fields.personal_title,
        fields.suffix,
        fields.gender,
        fields.birth_date,
        fields.marital_status,
        fields.comments,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_person(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &PersonFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_persons
        SET first_name = COALESCE($3, first_name),
            middle_name = COALESCE($4, middle_name),
            last_name = COALESCE($5, last_name),
            personal_title = COALESCE($6, personal_title),
            suffix = COALESCE($7, suffix),
            gender = COALESCE($8, gender),
            birth_date = COALESCE($9, birth_date),
            marital_status = COALESCE($10, marital_status),
            comments = COALESCE($11, comments),
            updated_by = $12,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.first_name,
        fields.middle_name,
        fields.last_name,
        fields.personal_title,
        fields.suffix,
        fields.gender,
        fields.birth_date,
        fields.marital_status,
        fields.comments,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn find_person(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<Person>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT first_name, middle_name, last_name, personal_title, suffix,
               gender, birth_date, marital_status, comments
        FROM mdm_persons
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Person {
        party_id: party_code.to_owned(),
        first_name: row.first_name,
        middle_name: row.middle_name,
        last_name: row.last_name,
        personal_title: row.personal_title,
        suffix: row.suffix,
        gender: row.gender.as_deref().and_then(Gender::from_db),
        birth_date: row.birth_date,
        marital_status: row.marital_status,
        comments: row.comments,
    }))
}

pub async fn insert_party_group(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &PartyGroupFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_party_groups (
            id, tenant_id, party_id, group_name, local_name, office_site_name,
            annual_revenue, num_employees, ticker_symbol, comments, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, ($7::text)::numeric, $8, $9, $10, $11)
        "#,
        id,
        tenant_id,
        party_id,
        fields.group_name.unwrap_or_default(),
        fields.local_name,
        fields.office_site_name,
        fields.annual_revenue,
        fields.num_employees,
        fields.ticker_symbol,
        fields.comments,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_party_group(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &PartyGroupFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_party_groups
        SET group_name = COALESCE($3, group_name),
            local_name = COALESCE($4, local_name),
            office_site_name = COALESCE($5, office_site_name),
            annual_revenue = COALESCE(($6::text)::numeric, annual_revenue),
            num_employees = COALESCE($7, num_employees),
            ticker_symbol = COALESCE($8, ticker_symbol),
            comments = COALESCE($9, comments),
            updated_by = $10,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.group_name,
        fields.local_name,
        fields.office_site_name,
        fields.annual_revenue,
        fields.num_employees,
        fields.ticker_symbol,
        fields.comments,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

pub async fn find_party_group(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<PartyGroup>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT group_name, local_name, office_site_name,
               annual_revenue::text AS annual_revenue,
               num_employees, ticker_symbol, comments
        FROM mdm_party_groups
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| PartyGroup {
        party_id: party_code.to_owned(),
        group_name: row.group_name,
        local_name: row.local_name,
        office_site_name: row.office_site_name,
        annual_revenue: row.annual_revenue,
        num_employees: row.num_employees,
        ticker_symbol: row.ticker_symbol,
        comments: row.comments,
    }))
}

// ---------------------------------------------------------------------------
// Identifications
// ---------------------------------------------------------------------------

pub async fn list_identifications(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<PartyIdentification>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT identification_type, id_value, issued_by, issue_date, expire_date, attributes_json
        FROM mdm_party_identifications
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        ORDER BY identification_type, id_value
        "#,
        tenant_id,
        party_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PartyIdentification {
            party_identification_type_id: row.identification_type,
            id_value: row.id_value,
            issued_by: row.issued_by,
            issue_date: row.issue_date,
            expire_date: row.expire_date,
            additional_attributes: row.attributes_json,
        })
        .collect())
}

/// Replaces the party's identifications with exactly the set given.
///
/// The old rows are deleted rather than soft-deleted. These are the aggregate's
/// content, not independently addressable entities: a soft-deleted
/// identification would linger invisibly forever, and nothing would ever ask
/// for it again — the same reasoning `replace_user_roles` follows.
pub async fn replace_identifications(
    transaction: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    identifications: &[IdentificationFields<'_>],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM mdm_party_identifications WHERE tenant_id = $1 AND party_id = $2",
        tenant_id,
        party_id
    )
    .execute(&mut *transaction)
    .await?;

    for identification in identifications {
        sqlx::query!(
            r#"
            INSERT INTO mdm_party_identifications (
                id, tenant_id, party_id, identification_type, id_value,
                issued_by, issue_date, expire_date, attributes_json, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            Uuid::now_v7(),
            tenant_id,
            party_id,
            identification.identification_type,
            identification.id_value,
            identification.issued_by,
            identification.issue_date,
            identification.expire_date,
            identification.attributes_json,
            actor
        )
        .execute(&mut *transaction)
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Status history
// ---------------------------------------------------------------------------

pub async fn list_statuses(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<PartyStatus>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT s.status, s.status_at, s.comments, u.username AS "username?"
        FROM mdm_party_statuses s
        LEFT JOIN users u ON u.id = s.changed_by
        WHERE s.tenant_id = $1 AND s.party_id = $2
        ORDER BY s.status_at, s.id
        "#,
        tenant_id,
        party_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PartyStatus {
            status_id: row.status,
            status_date: row.status_at,
            changed_by_user_login: row.username,
            comments: row.comments,
        })
        .collect())
}

/// Appends a status-history row. The table is append-only (§4.7), so this is
/// the only write it has.
pub async fn insert_status(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    status: &str,
    changed_by: Option<Uuid>,
    comments: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_party_statuses (id, tenant_id, party_id, status, status_at, changed_by, comments)
        VALUES ($1, $2, $3, $4, now(), $5, $6)
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        status,
        changed_by,
        comments
    )
    .execute(executor)
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Relationships
// ---------------------------------------------------------------------------

/// One direction of the party's relationships, with both ends projected back to
/// the business codes the aggregate carries.
pub async fn list_relationships(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    outgoing: bool,
) -> Result<Vec<PartyRelationship>, sqlx::Error> {
    // One statement for both directions: the side is a bind parameter rather
    // than two near-identical queries that could drift apart.
    let rows = sqlx::query!(
        r#"
        SELECT r.id,
               f.party_code AS party_id_from,
               t.party_code AS party_id_to,
               rf.role_type_code AS "role_type_id_from?",
               rt.role_type_code AS "role_type_id_to?",
               r.relationship_type,
               r.starts_at,
               r.ends_at,
               r.status,
               r.priority,
               r.comments,
               r.attributes_json
        FROM mdm_party_relationships r
        JOIN mdm_parties f ON f.id = r.from_party_id
        JOIN mdm_parties t ON t.id = r.to_party_id
        LEFT JOIN mdm_role_types rf ON rf.id = r.from_role_type_id
        LEFT JOIN mdm_role_types rt ON rt.id = r.to_role_type_id
        WHERE r.tenant_id = $1
          AND r.deleted_at IS NULL
          AND (($3 AND r.from_party_id = $2) OR (NOT $3 AND r.to_party_id = $2))
        ORDER BY r.starts_at, r.id
        "#,
        tenant_id,
        party_id,
        outgoing
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PartyRelationship {
            party_relationship_id: row.id,
            party_id_from: row.party_id_from,
            role_type_id_from: row.role_type_id_from,
            party_id_to: row.party_id_to,
            role_type_id_to: row.role_type_id_to,
            party_relationship_type_id: row.relationship_type,
            from_date: row.starts_at,
            thru_date: row.ends_at,
            status_id: row.status,
            priority: row.priority,
            comments: row.comments,
            additional_attributes: row.attributes_json,
        })
        .collect())
}

/// Replaces one direction of the party's relationships.
///
/// Only the named side is touched: replacing `relationshipsFrom` must not
/// silently discard a relationship some *other* party owns and this one is
/// merely the target of.
pub async fn replace_relationships(
    transaction: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    outgoing: bool,
    relationships: &[RelationshipFields<'_>],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        DELETE FROM mdm_party_relationships
        WHERE tenant_id = $1
          AND (($3 AND from_party_id = $2) OR (NOT $3 AND to_party_id = $2))
        "#,
        tenant_id,
        party_id,
        outgoing
    )
    .execute(&mut *transaction)
    .await?;

    for relationship in relationships {
        sqlx::query!(
            r#"
            INSERT INTO mdm_party_relationships (
                id, tenant_id, from_party_id, to_party_id, relationship_type,
                from_role_type_id, to_role_type_id, starts_at, ends_at, status,
                priority, comments, attributes_json, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#,
            Uuid::now_v7(),
            tenant_id,
            relationship.from_party_id,
            relationship.to_party_id,
            relationship.relationship_type,
            relationship.from_role_type_id,
            relationship.to_role_type_id,
            relationship.starts_at,
            relationship.ends_at,
            relationship.status,
            relationship.priority,
            relationship.comments,
            relationship.attributes_json,
            actor
        )
        .execute(&mut *transaction)
        .await?;
    }

    Ok(())
}

/// Resolves a role-type code to its surrogate key, within the tenant.
pub async fn find_role_type_id(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    role_type_code: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM mdm_role_types
        WHERE tenant_id = $1 AND role_type_code = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        role_type_code
    )
    .fetch_optional(executor)
    .await
}

// ---------------------------------------------------------------------------
// Classifications
// ---------------------------------------------------------------------------

pub async fn list_classifications(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<PartyClassification>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT class_type, classification_code, starts_at, ends_at, comments
        FROM mdm_party_classifications
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        ORDER BY class_type, starts_at
        "#,
        tenant_id,
        party_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PartyClassification {
            party_class_type_id: row.class_type,
            party_classification_id: row.classification_code,
            from_date: row.starts_at,
            thru_date: row.ends_at,
            comments: row.comments,
        })
        .collect())
}

pub async fn replace_classifications(
    transaction: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    classifications: &[ClassificationFields<'_>],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM mdm_party_classifications WHERE tenant_id = $1 AND party_id = $2",
        tenant_id,
        party_id
    )
    .execute(&mut *transaction)
    .await?;

    for classification in classifications {
        sqlx::query!(
            r#"
            INSERT INTO mdm_party_classifications (
                id, tenant_id, party_id, class_type, classification_code,
                starts_at, ends_at, comments, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            Uuid::now_v7(),
            tenant_id,
            party_id,
            classification.class_type,
            classification.classification_code,
            classification.starts_at,
            classification.ends_at,
            classification.comments,
            actor
        )
        .execute(&mut *transaction)
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Contact mechanisms
// ---------------------------------------------------------------------------

pub async fn list_contact_mechs(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<PartyContactMech>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT l.contact_mech_id,
               m.contact_mech_type,
               m.detail_json,
               l.purpose_type,
               l.starts_at,
               l.ends_at,
               l.is_primary,
               l.allow_solicitation,
               l.attributes_json
        FROM mdm_party_contact_mechs l
        JOIN mdm_contact_mechs m ON m.id = l.contact_mech_id AND m.deleted_at IS NULL
        WHERE l.tenant_id = $1 AND l.party_id = $2 AND l.deleted_at IS NULL
        ORDER BY l.is_primary DESC, l.starts_at, l.id
        "#,
        tenant_id,
        party_id
    )
    .fetch_all(executor)
    .await?;

    let mut mechanisms = Vec::with_capacity(rows.len());
    for row in rows {
        // A detail that no longer deserializes is data this build cannot
        // describe; an empty detail is a better answer than a 500 on a read.
        let detail = serde_json::from_value(row.detail_json).unwrap_or_default();

        mechanisms.push(PartyContactMech {
            contact_mech_id: row.contact_mech_id,
            contact_mech_type_id: ContactMechType::from_db(&row.contact_mech_type),
            purpose_type_id: row.purpose_type,
            from_date: row.starts_at,
            thru_date: row.ends_at,
            is_primary: row.is_primary,
            allow_solicitation: row.allow_solicitation,
            detail,
            additional_attributes: row.attributes_json,
        });
    }

    Ok(mechanisms)
}

/// Whether a contact mechanism exists in this tenant, for a link that reuses
/// one rather than supplying a value.
pub async fn contact_mech_exists(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    contact_mech_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM mdm_contact_mechs
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        ) AS "exists!"
        "#,
        tenant_id,
        contact_mech_id
    )
    .fetch_one(executor)
    .await
}

/// Replaces the party's contact mechanisms, creating the mechanism rows that
/// the links supply values for.
///
/// A mechanism that was linked only to this party and is not linked again is
/// deleted with the link. Mechanisms are shared on purpose — two parties can
/// point at one switchboard number — so the cleanup is scoped to the ids this
/// party held and checks each is now unreferenced. Without it every edit to a
/// party's contact details would leave a row behind that nothing can reach.
pub async fn replace_contact_mechs(
    transaction: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    mechanisms: &[ContactMechFields<'_>],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let previous: Vec<Uuid> = sqlx::query_scalar!(
        "SELECT contact_mech_id FROM mdm_party_contact_mechs WHERE tenant_id = $1 AND party_id = $2",
        tenant_id,
        party_id
    )
    .fetch_all(&mut *transaction)
    .await?;

    sqlx::query!(
        "DELETE FROM mdm_party_contact_mechs WHERE tenant_id = $1 AND party_id = $2",
        tenant_id,
        party_id
    )
    .execute(&mut *transaction)
    .await?;

    for mechanism in mechanisms {
        let contact_mech_id = match mechanism.existing_contact_mech_id {
            Some(id) => id,
            None => {
                let id = Uuid::now_v7();
                sqlx::query!(
                    r#"
                    INSERT INTO mdm_contact_mechs (
                        id, tenant_id, contact_mech_type, display_value, detail_json, created_by
                    )
                    VALUES ($1, $2, $3, $4, $5, $6)
                    "#,
                    id,
                    tenant_id,
                    mechanism.contact_mech_type.unwrap_or("OTHER"),
                    mechanism.display_value.unwrap_or_default(),
                    mechanism.detail_json,
                    actor
                )
                .execute(&mut *transaction)
                .await?;
                id
            }
        };

        sqlx::query!(
            r#"
            INSERT INTO mdm_party_contact_mechs (
                id, tenant_id, party_id, contact_mech_id, purpose_type,
                starts_at, ends_at, is_primary, allow_solicitation, attributes_json, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            Uuid::now_v7(),
            tenant_id,
            party_id,
            contact_mech_id,
            mechanism.purpose_type,
            mechanism.starts_at,
            mechanism.ends_at,
            mechanism.is_primary,
            mechanism.allow_solicitation,
            mechanism.attributes_json,
            actor
        )
        .execute(&mut *transaction)
        .await?;
    }

    if !previous.is_empty() {
        sqlx::query!(
            r#"
            DELETE FROM mdm_contact_mechs m
            WHERE m.tenant_id = $1
              AND m.id = ANY($2)
              AND NOT EXISTS (
                  SELECT 1 FROM mdm_party_contact_mechs l WHERE l.contact_mech_id = m.id
              )
            "#,
            tenant_id,
            &previous
        )
        .execute(&mut *transaction)
        .await?;
    }

    Ok(())
}
