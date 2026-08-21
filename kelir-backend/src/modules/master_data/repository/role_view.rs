//! The one query behind `/suppliers`, `/customers` and `/employees`, and the
//! count that pages it.
//!
//! The conventions these follow — tenant scoping, soft delete — are on the
//! parent module. Two things here are decisions rather than transcription:
//!
//! **One statement per page.** The role view joins the party, its person or
//! group name, and the role profile in a single query. Reading the page and
//! then fetching each party's name or supplier number would turn a hundred rows
//! into three hundred queries, which is the failure FR-MDM-008 and NFR-PERF-002
//! exist to prevent (#97 AC6).
//!
//! **Every join carries `tenant_id`**, not only the driving table. The party is
//! reachable from the role by `party_id` alone and the profile from the party,
//! so the extra predicate is redundant *given* the data — which is exactly what
//! made the joins in `repository::party` easy to write without it and is filed
//! as #108. A join that states the scope it depends on cannot be made wrong by
//! a later change to what reaches it.

use sqlx::PgPool;
use uuid::Uuid;

use crate::modules::master_data::domain::{
    PartyRoleStatus, PartyStatusCode, PartyType, RoleViewFilters, RoleViewRow,
};

/// The filters as the three text parameters the SQL binds.
///
/// `None` means *do not filter*: every predicate is written
/// `($n::text IS NULL OR column = $n)`, so one static statement serves all
/// sixteen combinations and nothing is assembled from strings (coding standard
/// §2.5).
struct Bound {
    party_status: Option<&'static str>,
    party_type: Option<&'static str>,
    role_status: Option<&'static str>,
    search: Option<String>,
}

impl Bound {
    fn from(filters: &RoleViewFilters) -> Self {
        Self {
            party_status: filters.status.map(PartyStatusCode::as_db),
            party_type: filters.party_type.map(PartyType::as_db),
            role_status: filters.role_status.map(PartyRoleStatus::as_db),
            search: filters.search.as_deref().map(like_contains),
        }
    }
}

/// How many parties the view holds, before paging.
///
/// **This and [`list_role_view`] must be changed together.** Their `matched`
/// blocks are the same query, and a `meta.total` produced by different criteria
/// than the rows is not a smaller version of the same answer — it is a number
/// that describes a population the caller never sees. That divergence is what
/// `the_total_counts_the_same_rows_the_page_shows` is aimed at.
pub async fn count_role_view(
    pool: &PgPool,
    tenant_id: Uuid,
    role_type_code: &str,
    filters: &RoleViewFilters,
) -> Result<i64, sqlx::Error> {
    let bound = Bound::from(filters);

    sqlx::query_scalar!(
        r#"
        WITH matched AS (
            SELECT p.party_code,
                   COALESCE(
                       g.group_name,
                       NULLIF(btrim(concat_ws(' ', pe.first_name, pe.middle_name, pe.last_name)), ''),
                       p.party_code
                   ) AS name,
                   COALESCE(sp.supplier_number, cp.customer_number, ep.employee_number) AS role_number
            FROM mdm_party_roles r
            JOIN mdm_role_types t
              ON t.id = r.role_type_id AND t.tenant_id = r.tenant_id AND t.deleted_at IS NULL
            JOIN mdm_parties p
              ON p.id = r.party_id AND p.tenant_id = r.tenant_id AND p.deleted_at IS NULL
            LEFT JOIN mdm_persons pe
              ON pe.party_id = p.id AND pe.tenant_id = p.tenant_id AND pe.deleted_at IS NULL
            LEFT JOIN mdm_party_groups g
              ON g.party_id = p.id AND g.tenant_id = p.tenant_id AND g.deleted_at IS NULL
            LEFT JOIN mdm_supplier_profiles sp
              ON $2 = 'SUPPLIER' AND sp.party_id = p.id AND sp.tenant_id = p.tenant_id
                 AND sp.deleted_at IS NULL
            LEFT JOIN mdm_customer_profiles cp
              ON $2 = 'CUSTOMER' AND cp.party_id = p.id AND cp.tenant_id = p.tenant_id
                 AND cp.deleted_at IS NULL
            LEFT JOIN mdm_employee_profiles ep
              ON $2 = 'EMPLOYEE' AND ep.party_id = p.id AND ep.tenant_id = p.tenant_id
                 AND ep.deleted_at IS NULL
            WHERE r.tenant_id = $1
              AND t.role_type_code = $2
              AND r.deleted_at IS NULL
              AND ($3::text IS NULL OR p.status = $3)
              AND ($4::text IS NULL OR p.party_type = $4)
              AND ($5::text IS NULL OR r.status = $5)
        )
        SELECT count(*) FROM matched
        WHERE $6::text IS NULL
           OR party_code ILIKE $6
           OR name ILIKE $6
           OR role_number ILIKE $6
        "#,
        tenant_id,
        role_type_code,
        bound.party_status,
        bound.party_type,
        bound.role_status,
        bound.search
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of the parties holding `role_type_code`.
///
/// **`role_type_code` never comes from a caller.** It is
/// `RoleView::role_type_code`, which is three literals behind an enum — the
/// allow-list coding standard §2.5 requires of anything that reaches a query as
/// an identifier.
///
/// Only one of the three profile joins is live per call: `$2 = 'SUPPLIER'` in
/// the join condition is false for a customer view, so PostgreSQL never reads
/// that table. Writing it once with three conditional joins rather than three
/// near-identical queries is what keeps the search, the filters and the paging
/// from drifting apart between the views.
///
/// A party appears once per live role row. `mdm_party_roles` is meant to hold
/// at most one per (party, role type) and #105 records that concurrent
/// assignment can break that — deliberately not papered over with `DISTINCT`
/// here, because a list that hides the duplicate would leave the defect
/// invisible from the surface most likely to reveal it.
pub async fn list_role_view(
    pool: &PgPool,
    tenant_id: Uuid,
    role_type_code: &str,
    filters: &RoleViewFilters,
    limit: i64,
    offset: i64,
) -> Result<Vec<RoleViewRow>, sqlx::Error> {
    let bound = Bound::from(filters);

    let rows = sqlx::query!(
        r#"
        WITH matched AS (
            SELECT p.id,
                   p.party_code,
                   p.party_type,
                   p.status AS party_status,
                   p.external_id,
                   p.created_at,
                   p.updated_at,
                   r.starts_at,
                   r.ends_at,
                   r.status AS role_status,
                   COALESCE(
                       g.group_name,
                       NULLIF(btrim(concat_ws(' ', pe.first_name, pe.middle_name, pe.last_name)), ''),
                       p.party_code
                   ) AS name,
                   COALESCE(sp.supplier_number, cp.customer_number, ep.employee_number) AS role_number
            FROM mdm_party_roles r
            JOIN mdm_role_types t
              ON t.id = r.role_type_id AND t.tenant_id = r.tenant_id AND t.deleted_at IS NULL
            JOIN mdm_parties p
              ON p.id = r.party_id AND p.tenant_id = r.tenant_id AND p.deleted_at IS NULL
            LEFT JOIN mdm_persons pe
              ON pe.party_id = p.id AND pe.tenant_id = p.tenant_id AND pe.deleted_at IS NULL
            LEFT JOIN mdm_party_groups g
              ON g.party_id = p.id AND g.tenant_id = p.tenant_id AND g.deleted_at IS NULL
            LEFT JOIN mdm_supplier_profiles sp
              ON $2 = 'SUPPLIER' AND sp.party_id = p.id AND sp.tenant_id = p.tenant_id
                 AND sp.deleted_at IS NULL
            LEFT JOIN mdm_customer_profiles cp
              ON $2 = 'CUSTOMER' AND cp.party_id = p.id AND cp.tenant_id = p.tenant_id
                 AND cp.deleted_at IS NULL
            LEFT JOIN mdm_employee_profiles ep
              ON $2 = 'EMPLOYEE' AND ep.party_id = p.id AND ep.tenant_id = p.tenant_id
                 AND ep.deleted_at IS NULL
            WHERE r.tenant_id = $1
              AND t.role_type_code = $2
              AND r.deleted_at IS NULL
              AND ($3::text IS NULL OR p.status = $3)
              AND ($4::text IS NULL OR p.party_type = $4)
              AND ($5::text IS NULL OR r.status = $5)
        )
        SELECT id AS "id!",
               party_code AS "party_code!",
               party_type AS "party_type!",
               party_status AS "party_status!",
               external_id,
               created_at AS "created_at!",
               updated_at AS "updated_at!",
               starts_at AS "starts_at!",
               ends_at,
               role_status AS "role_status!",
               name AS "name!",
               role_number
        FROM matched
        WHERE $6::text IS NULL
           OR party_code ILIKE $6
           OR name ILIKE $6
           OR role_number ILIKE $6
        ORDER BY party_code
        LIMIT $7 OFFSET $8
        "#,
        tenant_id,
        role_type_code,
        bound.party_status,
        bound.party_type,
        bound.role_status,
        bound.search,
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| RoleViewRow {
            id: row.id,
            party_id: row.party_code,
            party_type_id: PartyType::from_db(&row.party_type),
            status_id: PartyStatusCode::from_db(&row.party_status),
            name: row.name,
            external_id: row.external_id,
            role_type_id: role_type_code.to_owned(),
            role_number: row.role_number,
            role_status_id: PartyRoleStatus::from_db(&row.role_status),
            from_date: row.starts_at,
            thru_date: row.ends_at,
            created_stamp: row.created_at,
            last_updated_stamp: row.updated_at,
        })
        .collect())
}

/// Wraps a search term in a `%…%` pattern that matches it literally.
///
/// `LIKE` reads `%` as *any run of characters* and `_` as *any character*, so a
/// caller searching for the literal string `100%` would otherwise match every
/// row beginning `100`, and `_` would match a scan of the whole table. Both are
/// escaped, and so is the escape character itself — `\` last would double the
/// backslashes this function had just introduced.
///
/// PostgreSQL's default `LIKE` escape is the backslash, which is why no
/// `ESCAPE` clause appears in the queries above.
fn like_contains(search: &str) -> String {
    let mut pattern = String::with_capacity(search.len() + 2);
    pattern.push('%');

    for character in search.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }

    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_a_plain_search_in_a_contains_pattern() {
        assert_eq!(like_contains("ACME"), "%ACME%");
    }

    #[test]
    fn escapes_a_percent_so_it_matches_itself() {
        // Without this, searching `100%` returns every code starting `100`, and
        // the caller reads the extra rows as matches.
        assert_eq!(like_contains("100%"), "%100\\%%");
    }

    #[test]
    fn escapes_an_underscore_so_it_matches_itself() {
        assert_eq!(like_contains("A_B"), "%A\\_B%");
    }

    #[test]
    fn escapes_the_escape_character_itself() {
        // `\%` from a caller is a literal backslash followed by a literal
        // percent, not an escaped percent.
        assert_eq!(like_contains("\\%"), "%\\\\\\%%");
    }

    #[test]
    fn leaves_a_search_that_needs_no_escaping_alone() {
        assert_eq!(like_contains("SUP-0001"), "%SUP-0001%");
    }
}
