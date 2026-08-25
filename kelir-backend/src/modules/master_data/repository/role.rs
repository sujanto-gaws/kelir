//! Queries for the roles a party holds (§4.4-4.5) and the four role profiles
//! (§4.12-4.15).
//!
//! The conventions these follow — tenant scoping, soft delete, and why decimal
//! columns travel as text — are on the parent module.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::modules::master_data::domain::{
    ContactProfile, CustomerProfile, EmployeeProfile, EmploymentType, PartyRole, PartyRoleStatus,
    SupplierApprovalStatus, SupplierProfile,
};

// ---------------------------------------------------------------------------
// Party roles (§4.5)
// ---------------------------------------------------------------------------

/// Columns of a role assignment. `None` means *leave alone* on an update.
pub struct PartyRoleFields<'a> {
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub status: Option<&'a str>,
    pub comments: Option<&'a str>,
    pub attributes_json: Option<&'a Value>,
}

/// Every live role the party holds, with the role type projected back to the
/// code the aggregate carries.
pub async fn list_party_roles(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
) -> Result<Vec<PartyRole>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT t.role_type_code, r.starts_at, r.ends_at, r.status, r.comments, r.attributes_json
        FROM mdm_party_roles r
        JOIN mdm_role_types t ON t.id = r.role_type_id
        WHERE r.tenant_id = $1 AND r.party_id = $2 AND r.deleted_at IS NULL
        ORDER BY t.role_type_code
        "#,
        tenant_id,
        party_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PartyRole {
            role_type_id: row.role_type_code,
            from_date: row.starts_at,
            thru_date: row.ends_at,
            status_id: PartyRoleStatus::from_db(&row.status),
            comments: row.comments,
            additional_attributes: row.attributes_json,
        })
        .collect())
}

/// The live assignment of one role type to one party, if there is one.
///
/// "Live" is what keeps a party holding a role once rather than twice: the
/// unique index covers `starts_at` as well, so a second assignment with a
/// different start date would be accepted by the database. The service asks
/// here first and updates in place instead.
pub async fn find_live_party_role(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    role_type_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT id FROM mdm_party_roles
        WHERE tenant_id = $1 AND party_id = $2 AND role_type_id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        role_type_id
    )
    .fetch_optional(executor)
    .await
}

/// Writes a new assignment and hands back its id.
///
/// The id is what a caller needing to address the row it just wrote would use.
/// `assign_role` no longer does: it answers with the request it was given
/// (#119), so it has nothing to read back and discards this.
pub async fn insert_party_role(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    role_type_id: Uuid,
    fields: &PartyRoleFields<'_>,
    created_by: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO mdm_party_roles (
            id, tenant_id, party_id, role_type_id, starts_at, ends_at,
            status, comments, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, 'ACTIVE'), $8, COALESCE($9, '{}'::jsonb), $10)
        RETURNING id
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        role_type_id,
        fields.starts_at,
        fields.ends_at,
        fields.status,
        fields.comments,
        fields.attributes_json,
        created_by
    )
    .fetch_one(executor)
    .await
}

/// Updates a role the party already holds.
///
/// `starts_at` and `ends_at` are written unconditionally rather than through
/// `COALESCE`: they are the assignment's period, `PUT` states the period the
/// assignment should end in, and a `thruDate` that could be set but never
/// cleared would make an ended role impossible to reopen.
pub async fn update_party_role(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    fields: &PartyRoleFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_party_roles
        SET starts_at = $2,
            ends_at = $3,
            status = COALESCE($4, status),
            comments = COALESCE($5, comments),
            attributes_json = COALESCE($6, attributes_json),
            updated_by = $7,
            updated_at = now()
        WHERE id = $1
        "#,
        id,
        fields.starts_at,
        fields.ends_at,
        fields.status,
        fields.comments,
        fields.attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Ends a role assignment.
///
/// Soft delete, and `ends_at` closes at the same time: a role that ended is not
/// a role that never existed, and "was a supplier until March" lives in those
/// two columns. The partial unique index excludes soft-deleted rows, so the
/// same role can be assigned again later.
pub async fn soft_delete_party_role(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    role_type_id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_party_roles
        SET deleted_at = now(),
            ends_at = COALESCE(ends_at, now()),
            status = 'INACTIVE',
            updated_by = $4,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND role_type_id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        role_type_id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Closes every live role a party holds, in one statement.
///
/// The same close as [`soft_delete_party_role`], without naming a role type —
/// what a deleted party needs, because leaving a role live behind a party that
/// is not is what left the supplier number occupied by a row nothing could
/// reach (#103).
pub async fn soft_delete_party_roles(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_party_roles
        SET deleted_at = now(),
            ends_at = COALESCE(ends_at, now()),
            status = 'INACTIVE',
            updated_by = $3,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        updated_by
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Whether a department exists in this tenant, for an employee profile that
/// names one.
pub async fn department_exists(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    department_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM departments
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        ) AS "exists!"
        "#,
        tenant_id,
        department_id
    )
    .fetch_one(executor)
    .await
}

// ---------------------------------------------------------------------------
// Supplier profile (§4.12)
// ---------------------------------------------------------------------------

/// Columns of a supplier profile. `None` means *leave alone* on an update; on
/// an insert the business number is required and validation has already run.
#[derive(Default)]
pub struct SupplierProfileFields<'a> {
    pub supplier_number: Option<&'a str>,
    pub supplier_category: Option<&'a str>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<&'a str>,
    pub tax_number: Option<&'a str>,
    pub bank_name: Option<&'a str>,
    pub bank_account: Option<&'a str>,
    pub bank_account_name: Option<&'a str>,
    pub approval_status: Option<&'a str>,
    pub status: Option<&'a str>,
    pub attributes_json: Option<&'a Value>,
}

pub async fn insert_supplier_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &SupplierProfileFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_supplier_profiles (
            id, tenant_id, party_id, supplier_number, supplier_category, payment_term_days,
            default_currency_uom, tax_number, bank_name, bank_account, bank_account_name,
            approval_status, status, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                COALESCE($12, 'DRAFT'), $13, COALESCE($14, '{}'::jsonb), $15)
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        fields.supplier_number.unwrap_or_default(),
        fields.supplier_category,
        fields.payment_term_days,
        fields.default_currency_uom,
        fields.tax_number,
        fields.bank_name,
        fields.bank_account,
        fields.bank_account_name,
        fields.approval_status,
        fields.status,
        fields.attributes_json,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_supplier_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &SupplierProfileFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_supplier_profiles
        SET supplier_number = COALESCE($3, supplier_number),
            supplier_category = COALESCE($4, supplier_category),
            payment_term_days = COALESCE($5, payment_term_days),
            default_currency_uom = COALESCE($6, default_currency_uom),
            tax_number = COALESCE($7, tax_number),
            bank_name = COALESCE($8, bank_name),
            bank_account = COALESCE($9, bank_account),
            bank_account_name = COALESCE($10, bank_account_name),
            approval_status = COALESCE($11, approval_status),
            status = COALESCE($12, status),
            attributes_json = COALESCE($13, attributes_json),
            updated_by = $14,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.supplier_number,
        fields.supplier_category,
        fields.payment_term_days,
        fields.default_currency_uom,
        fields.tax_number,
        fields.bank_name,
        fields.bank_account,
        fields.bank_account_name,
        fields.approval_status,
        fields.status,
        fields.attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn find_supplier_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<SupplierProfile>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT supplier_number, supplier_category, payment_term_days, default_currency_uom,
               tax_number, bank_name, bank_account, bank_account_name, approval_status,
               status, attributes_json
        FROM mdm_supplier_profiles
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| SupplierProfile {
        party_id: party_code.to_owned(),
        supplier_number: row.supplier_number,
        supplier_category: row.supplier_category,
        payment_term_days: row.payment_term_days,
        default_currency_uom: row.default_currency_uom,
        tax_number: row.tax_number,
        bank_name: row.bank_name,
        bank_account: row.bank_account,
        bank_account_name: row.bank_account_name,
        approval_status: SupplierApprovalStatus::from_db(&row.approval_status),
        status_id: row.status,
        additional_attributes: row.attributes_json,
    }))
}

// ---------------------------------------------------------------------------
// Customer profile (§4.13)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CustomerProfileFields<'a> {
    pub customer_number: Option<&'a str>,
    pub customer_category: Option<&'a str>,
    pub customer_since_date: Option<NaiveDate>,
    /// Decimal as text; see the module note on `NUMERIC`.
    pub credit_limit: Option<&'a str>,
    pub payment_term_days: Option<i32>,
    pub default_currency_uom: Option<&'a str>,
    pub tax_number: Option<&'a str>,
    pub billing_party_id: Option<Uuid>,
    pub status: Option<&'a str>,
    pub attributes_json: Option<&'a Value>,
}

pub async fn insert_customer_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &CustomerProfileFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_customer_profiles (
            id, tenant_id, party_id, customer_number, customer_category, customer_since_date,
            credit_limit, payment_term_days, default_currency_uom, tax_number,
            billing_party_id, status, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, ($7::text)::numeric, $8, $9, $10, $11, $12,
                COALESCE($13, '{}'::jsonb), $14)
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        fields.customer_number.unwrap_or_default(),
        fields.customer_category,
        fields.customer_since_date,
        fields.credit_limit,
        fields.payment_term_days,
        fields.default_currency_uom,
        fields.tax_number,
        fields.billing_party_id,
        fields.status,
        fields.attributes_json,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_customer_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &CustomerProfileFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_customer_profiles
        SET customer_number = COALESCE($3, customer_number),
            customer_category = COALESCE($4, customer_category),
            customer_since_date = COALESCE($5, customer_since_date),
            credit_limit = COALESCE(($6::text)::numeric, credit_limit),
            payment_term_days = COALESCE($7, payment_term_days),
            default_currency_uom = COALESCE($8, default_currency_uom),
            tax_number = COALESCE($9, tax_number),
            billing_party_id = COALESCE($10, billing_party_id),
            status = COALESCE($11, status),
            attributes_json = COALESCE($12, attributes_json),
            updated_by = $13,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.customer_number,
        fields.customer_category,
        fields.customer_since_date,
        fields.credit_limit,
        fields.payment_term_days,
        fields.default_currency_uom,
        fields.tax_number,
        fields.billing_party_id,
        fields.status,
        fields.attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn find_customer_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<CustomerProfile>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT c.customer_number, c.customer_category, c.customer_since_date,
               c.credit_limit::text AS credit_limit, c.payment_term_days,
               c.default_currency_uom, c.tax_number, c.status, c.attributes_json,
               b.party_code AS "billing_party_id?"
        FROM mdm_customer_profiles c
        LEFT JOIN mdm_parties b ON b.id = c.billing_party_id
        WHERE c.tenant_id = $1 AND c.party_id = $2 AND c.deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| CustomerProfile {
        party_id: party_code.to_owned(),
        customer_number: row.customer_number,
        customer_category: row.customer_category,
        customer_since: row.customer_since_date,
        credit_limit: row.credit_limit,
        payment_term_days: row.payment_term_days,
        default_currency_uom: row.default_currency_uom,
        tax_number: row.tax_number,
        billing_party_id: row.billing_party_id,
        status_id: row.status,
        additional_attributes: row.attributes_json,
    }))
}

// ---------------------------------------------------------------------------
// Employee profile (§4.14)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct EmployeeProfileFields<'a> {
    pub employee_number: Option<&'a str>,
    pub department_id: Option<Uuid>,
    pub manager_party_id: Option<Uuid>,
    pub position: Option<&'a str>,
    pub job_grade: Option<&'a str>,
    pub employment_type: Option<&'a str>,
    pub join_date: Option<NaiveDate>,
    pub resign_date: Option<NaiveDate>,
    pub status: Option<&'a str>,
    pub attributes_json: Option<&'a Value>,
}

pub async fn insert_employee_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &EmployeeProfileFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_employee_profiles (
            id, tenant_id, party_id, employee_number, department_id, manager_party_id,
            position, job_grade, employment_type, join_date, resign_date, status,
            attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                COALESCE($13, '{}'::jsonb), $14)
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        fields.employee_number.unwrap_or_default(),
        fields.department_id,
        fields.manager_party_id,
        fields.position,
        fields.job_grade,
        fields.employment_type,
        fields.join_date,
        fields.resign_date,
        fields.status,
        fields.attributes_json,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_employee_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &EmployeeProfileFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_employee_profiles
        SET employee_number = COALESCE($3, employee_number),
            department_id = COALESCE($4, department_id),
            manager_party_id = COALESCE($5, manager_party_id),
            position = COALESCE($6, position),
            job_grade = COALESCE($7, job_grade),
            employment_type = COALESCE($8, employment_type),
            join_date = COALESCE($9, join_date),
            resign_date = COALESCE($10, resign_date),
            status = COALESCE($11, status),
            attributes_json = COALESCE($12, attributes_json),
            updated_by = $13,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.employee_number,
        fields.department_id,
        fields.manager_party_id,
        fields.position,
        fields.job_grade,
        fields.employment_type,
        fields.join_date,
        fields.resign_date,
        fields.status,
        fields.attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn find_employee_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<EmployeeProfile>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT e.employee_number, e.department_id, e.position, e.job_grade,
               e.employment_type, e.join_date, e.resign_date, e.status, e.attributes_json,
               m.party_code AS "manager_party_id?"
        FROM mdm_employee_profiles e
        LEFT JOIN mdm_parties m ON m.id = e.manager_party_id
        WHERE e.tenant_id = $1 AND e.party_id = $2 AND e.deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| EmployeeProfile {
        party_id: party_code.to_owned(),
        employee_number: row.employee_number,
        department_id: row.department_id,
        manager_party_id: row.manager_party_id,
        position: row.position,
        job_grade: row.job_grade,
        employment_type: row
            .employment_type
            .as_deref()
            .and_then(EmploymentType::from_db),
        join_date: row.join_date,
        resign_date: row.resign_date,
        status_id: row.status,
        additional_attributes: row.attributes_json,
    }))
}

// ---------------------------------------------------------------------------
// Contact profile (§4.15)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ContactProfileFields<'a> {
    pub contact_type: Option<&'a str>,
    pub preferred_contact_mech_type: Option<&'a str>,
    pub do_not_contact: Option<bool>,
    pub assistant_party_id: Option<Uuid>,
    pub attributes_json: Option<&'a Value>,
}

pub async fn insert_contact_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &ContactProfileFields<'_>,
    created_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO mdm_contact_profiles (
            id, tenant_id, party_id, contact_type, preferred_contact_mech_type,
            do_not_contact, assistant_party_id, attributes_json, created_by
        )
        VALUES ($1, $2, $3, $4, $5, COALESCE($6, false), $7, COALESCE($8, '{}'::jsonb), $9)
        "#,
        Uuid::now_v7(),
        tenant_id,
        party_id,
        fields.contact_type,
        fields.preferred_contact_mech_type,
        fields.do_not_contact,
        fields.assistant_party_id,
        fields.attributes_json,
        created_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_contact_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    fields: &ContactProfileFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE mdm_contact_profiles
        SET contact_type = COALESCE($3, contact_type),
            preferred_contact_mech_type = COALESCE($4, preferred_contact_mech_type),
            do_not_contact = COALESCE($5, do_not_contact),
            assistant_party_id = COALESCE($6, assistant_party_id),
            attributes_json = COALESCE($7, attributes_json),
            updated_by = $8,
            updated_at = now()
        WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        party_id,
        fields.contact_type,
        fields.preferred_contact_mech_type,
        fields.do_not_contact,
        fields.assistant_party_id,
        fields.attributes_json,
        updated_by
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn find_contact_profile(
    executor: impl PgExecutor<'_>,
    tenant_id: Uuid,
    party_id: Uuid,
    party_code: &str,
) -> Result<Option<ContactProfile>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT c.contact_type, c.preferred_contact_mech_type, c.do_not_contact,
               c.attributes_json, a.party_code AS "assistant_party_id?"
        FROM mdm_contact_profiles c
        LEFT JOIN mdm_parties a ON a.id = c.assistant_party_id
        WHERE c.tenant_id = $1 AND c.party_id = $2 AND c.deleted_at IS NULL
        "#,
        tenant_id,
        party_id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| ContactProfile {
        party_id: party_code.to_owned(),
        contact_type: row.contact_type,
        preferred_contact_mech_type_id: row.preferred_contact_mech_type,
        do_not_contact: row.do_not_contact,
        assistant_party_id: row.assistant_party_id,
        additional_attributes: row.attributes_json,
    }))
}

// ---------------------------------------------------------------------------
// Ending a profile with its role
// ---------------------------------------------------------------------------

/// Soft-deletes whichever profile belongs to `role_type_code`, if the role type
/// has one.
///
/// Four statements rather than one, because `sqlx::query!` verifies against a
/// real schema at compile time and cannot take a table name at runtime; coding
/// standard §2.5 forbids interpolating one. The match is the allow-list that
/// rule asks for.
pub async fn soft_delete_role_profile(
    executor: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    party_id: Uuid,
    role_type_code: &str,
    updated_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    match role_type_code {
        "SUPPLIER" => {
            sqlx::query!(
                r#"
                UPDATE mdm_supplier_profiles
                SET deleted_at = now(), updated_by = $3, updated_at = now()
                WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                party_id,
                updated_by
            )
            .execute(&mut *executor)
            .await?;
        }
        "CUSTOMER" => {
            sqlx::query!(
                r#"
                UPDATE mdm_customer_profiles
                SET deleted_at = now(), updated_by = $3, updated_at = now()
                WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                party_id,
                updated_by
            )
            .execute(&mut *executor)
            .await?;
        }
        "EMPLOYEE" => {
            sqlx::query!(
                r#"
                UPDATE mdm_employee_profiles
                SET deleted_at = now(), updated_by = $3, updated_at = now()
                WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                party_id,
                updated_by
            )
            .execute(&mut *executor)
            .await?;
        }
        "CONTACT" => {
            sqlx::query!(
                r#"
                UPDATE mdm_contact_profiles
                SET deleted_at = now(), updated_by = $3, updated_at = now()
                WHERE tenant_id = $1 AND party_id = $2 AND deleted_at IS NULL
                "#,
                tenant_id,
                party_id,
                updated_by
            )
            .execute(&mut *executor)
            .await?;
        }
        // A tenant-defined role type has no profile table to close.
        _ => {}
    }

    Ok(())
}
