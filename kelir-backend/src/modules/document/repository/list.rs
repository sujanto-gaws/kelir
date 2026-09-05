//! The one filtered statement behind the document list (FR-DOC-013,
//! FR-SRH-001, [#171]).
//!
//! # The visibility rule, stated once and enforced here
//!
//! > **A caller sees a document when it is in their tenant and they hold
//! > `document:read`. There is no third condition in Sprint 9.**
//!
//! Not the creator, not the department, not a security level.
//! `documents.security_level` exists in the column (§6.6) and **nothing reads
//! it**, because FR-DTYPE-008 is Sprint 9's cut tail and **D-16** expects the
//! tail to go. A filter half-honouring a requirement nobody wrote is worse than
//! no filter: it reads as a control and enforces an accident. When FR-DTYPE-008
//! lands it adds a finer grain *on top of* this floor, and this comment is where
//! it goes.
//!
//! [#171]'s AC2 asks for the rule to be enforced **in the query and not in the
//! handler**, and every filter below is a predicate in this same statement for
//! AC4's reason: a filter answered anywhere else could become a way to confirm
//! that a document exists without being allowed to see it. There is nowhere
//! else for it to be answered.
//!
//! A list is also the surface where a leak is least visible. A detail endpoint
//! that refuses is obvious; a list that quietly includes one row too many is
//! not — which is why `tests/documents_list.rs` puts a second tenant's document
//! in the database and asserts against the *query* rather than around it, the
//! #106 / #121 lesson that cost this project three sprints of coverage findings.
//!
//! [#171]: https://github.com/sujanto-gaws/kelir/issues/171

use sqlx::PgPool;
use uuid::Uuid;

use serde_json::Value;

use super::super::domain::{
    DocumentFilters, DocumentPriority, DocumentSort, DocumentStatus, DocumentSummary, EntityType,
};

/// A document as the list serves it, with the form payload beside it when
/// something asked for it.
///
/// **`form_data` is `None` unless the caller said it needed it**, and that is a
/// query-level decision rather than a projection afterwards: a page of twenty
/// documents carries twenty form payloads, which is the reason
/// [`DocumentSummary`] exists at all (NFR-PERF-002). The rendered-list path
/// ([#340]) asks for it only when the list definition declares a `form_data.*`
/// column, and it never puts the payload on the wire — it reads the declared
/// paths out of it and sends the cells.
///
/// [#340]: https://github.com/sujanto-gaws/kelir/issues/340
pub struct DocumentRow {
    pub summary: DocumentSummary,
    pub form_data: Option<Value>,
}

/// How many documents match, for the `meta.total` the envelope carries.
///
/// The same predicates as [`list_documents`], deliberately duplicated rather
/// than factored into a string both share: coding standard §2.5 keeps a
/// statement static so `sqlx::query!` can check it, and two checked statements
/// beat one assembled one. The pairing is held by
/// `a_filtered_page_reports_the_filtered_total`, which is what would catch them
/// drifting.
pub async fn count_documents(
    pool: &PgPool,
    tenant_id: Uuid,
    filters: &DocumentFilters,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM documents d
        JOIN document_types t
          ON t.id = d.document_type_id AND t.tenant_id = d.tenant_id
        WHERE d.tenant_id = $1
          AND d.deleted_at IS NULL
          AND ($2::uuid IS NULL OR d.document_type_id = $2)
          AND ($3::text IS NULL OR d.status = $3)
          AND ($4::text IS NULL OR d.priority = $4)
          AND ($5::text IS NULL OR d.entity_type = $5)
          AND ($6::uuid IS NULL OR d.entity_id = $6)
          AND ($7::text IS NULL
               OR d.title ILIKE '%' || $7 || '%'
               OR d.document_ref ILIKE '%' || $7 || '%'
               OR d.document_number ILIKE '%' || $7 || '%')
          AND ($8::uuid IS NULL OR t.list_id = $8)
        "#,
        tenant_id,
        filters.document_type_id,
        filters.status.map(DocumentStatus::as_db),
        filters.priority.map(DocumentPriority::as_db),
        filters.entity_type.map(EntityType::as_db),
        filters.entity_id,
        escaped(filters.search.as_deref()),
        filters.list_id,
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of documents.
///
/// **Ordered by `sort`, and a stable secondary key on `id` keeps two documents
/// created in the same millisecond from swapping places between pages.** The
/// default is newest first, which is what a list of documents is for: the thing
/// somebody is looking for is almost always the one they made most recently.
///
/// Delegates to [`list_document_rows`] rather than carrying a second statement,
/// which is the one place this file departs from the duplication
/// [`count_documents`] argues for — and for the reason that argument gives.
/// Those two statements duplicate a `WHERE`, where drift is a wrong *number*
/// beside a right page; these would duplicate a `WHERE`, an `ORDER BY` and a
/// `LIMIT`, where drift is two pages of the same query in different orders.
pub async fn list_documents(
    pool: &PgPool,
    tenant_id: Uuid,
    filters: &DocumentFilters,
    sort: DocumentSort,
    limit: i64,
    offset: i64,
) -> Result<Vec<DocumentSummary>, sqlx::Error> {
    let rows = list_document_rows(pool, tenant_id, filters, sort, false, limit, offset).await?;

    Ok(rows.into_iter().map(|row| row.summary).collect())
}

/// The same page, with the form payload where one was asked for.
///
/// # The `ORDER BY` is static, and that is the whole of why it looks like this
///
/// Coding standard §2.5 keeps a statement static so `sqlx::query!` can check
/// it, and §6.4 requires a dynamic identifier to be allow-listed. Both hold
/// here without an assembled string: [`DocumentSort`] is a closed enum, its
/// `as_db` token arrives as a **bound parameter**, and every column this query
/// could order by is written out in the statement's own text. Nothing from a
/// request reaches the SQL as an identifier.
///
/// The cost is nine pairs of `CASE` arms. It is worth paying: the alternative
/// that reads better is `format!` around a validated identifier, and that
/// trades a checked statement for an unchecked one plus a promise that the
/// validation is never bypassed — which is a promise about every future edit
/// rather than about this one.
///
/// **`form_data_json` is read only when `with_form_data` holds**, through the
/// `CASE` on `$10`, so a caller that does not need the payload does not pay to
/// fetch it.
pub async fn list_document_rows(
    pool: &PgPool,
    tenant_id: Uuid,
    filters: &DocumentFilters,
    sort: DocumentSort,
    with_form_data: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<DocumentRow>, sqlx::Error> {
    let sort_key = sort.key.as_db();
    let descending = sort.descending;

    let rows = sqlx::query!(
        r#"
        SELECT d.id, d.document_ref, d.document_number, d.document_type_id,
               t.type_code AS document_type_code, d.title, d.status, d.priority,
               d.entity_type, d.entity_id, d.submitted_at, d.created_at, d.updated_at,
               CASE WHEN $10 THEN d.form_data_json END AS form_data_json
        FROM documents d
        JOIN document_types t
          ON t.id = d.document_type_id AND t.tenant_id = d.tenant_id
        WHERE d.tenant_id = $1
          AND d.deleted_at IS NULL
          AND ($2::uuid IS NULL OR d.document_type_id = $2)
          AND ($3::text IS NULL OR d.status = $3)
          AND ($4::text IS NULL OR d.priority = $4)
          AND ($5::text IS NULL OR d.entity_type = $5)
          AND ($6::uuid IS NULL OR d.entity_id = $6)
          AND ($7::text IS NULL
               OR d.title ILIKE '%' || $7 || '%'
               OR d.document_ref ILIKE '%' || $7 || '%'
               OR d.document_number ILIKE '%' || $7 || '%')
          AND ($13::uuid IS NULL OR t.list_id = $13)
        ORDER BY
          CASE WHEN $11 = 'document_ref'       AND NOT $12 THEN d.document_ref END ASC,
          CASE WHEN $11 = 'document_ref'       AND     $12 THEN d.document_ref END DESC,
          CASE WHEN $11 = 'document_number'    AND NOT $12 THEN d.document_number END ASC,
          CASE WHEN $11 = 'document_number'    AND     $12 THEN d.document_number END DESC,
          CASE WHEN $11 = 'document_type_code' AND NOT $12 THEN t.type_code END ASC,
          CASE WHEN $11 = 'document_type_code' AND     $12 THEN t.type_code END DESC,
          CASE WHEN $11 = 'title'              AND NOT $12 THEN d.title END ASC,
          CASE WHEN $11 = 'title'              AND     $12 THEN d.title END DESC,
          CASE WHEN $11 = 'status'             AND NOT $12 THEN d.status END ASC,
          CASE WHEN $11 = 'status'             AND     $12 THEN d.status END DESC,
          CASE WHEN $11 = 'priority'           AND NOT $12 THEN d.priority END ASC,
          CASE WHEN $11 = 'priority'           AND     $12 THEN d.priority END DESC,
          CASE WHEN $11 = 'submitted_at'       AND NOT $12 THEN d.submitted_at END ASC,
          CASE WHEN $11 = 'submitted_at'       AND     $12 THEN d.submitted_at END DESC,
          CASE WHEN $11 = 'created_at'         AND NOT $12 THEN d.created_at END ASC,
          CASE WHEN $11 = 'created_at'         AND     $12 THEN d.created_at END DESC,
          CASE WHEN $11 = 'updated_at'         AND NOT $12 THEN d.updated_at END ASC,
          CASE WHEN $11 = 'updated_at'         AND     $12 THEN d.updated_at END DESC,
          d.id DESC
        LIMIT $8 OFFSET $9
        "#,
        tenant_id,
        filters.document_type_id,
        filters.status.map(DocumentStatus::as_db),
        filters.priority.map(DocumentPriority::as_db),
        filters.entity_type.map(EntityType::as_db),
        filters.entity_id,
        escaped(filters.search.as_deref()),
        limit,
        offset,
        with_form_data,
        sort_key,
        descending,
        filters.list_id,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| DocumentRow {
            summary: DocumentSummary {
                id: row.id,
                document_ref: row.document_ref,
                document_number: row.document_number,
                document_type_id: row.document_type_id,
                document_type_code: row.document_type_code,
                title: row.title,
                status: DocumentStatus::from_db(&row.status),
                priority: DocumentPriority::from_db(&row.priority),
                entity_type: row.entity_type.as_deref().and_then(EntityType::from_db),
                entity_id: row.entity_id,
                submitted_at: row.submitted_at,
                created_at: row.created_at,
                updated_at: row.updated_at,
            },
            form_data: row.form_data_json,
        })
        .collect())
}

/// Whether any document type in this tenant names `list_id`.
///
/// `EXISTS` rather than a count: the question is *is there one*, and a count
/// over a table with a hundred types would read every row to answer a yes.
pub async fn list_is_bound(
    pool: &PgPool,
    tenant_id: Uuid,
    list_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM document_types
            WHERE tenant_id = $1 AND list_id = $2 AND deleted_at IS NULL
        ) AS "bound!"
        "#,
        tenant_id,
        list_id,
    )
    .fetch_one(pool)
    .await
}

fn escaped(search: Option<&str>) -> Option<String> {
    search.map(|term| {
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wildcard_in_a_search_term_matches_itself() {
        assert_eq!(escaped(Some("PR_2026")).as_deref(), Some("PR\\_2026"));
        assert_eq!(escaped(Some("%")).as_deref(), Some("\\%"));
    }

    #[test]
    fn the_escape_character_is_escaped_first() {
        // Escaping `%` before `\` would turn `\` into `\\` and then into
        // `\\\\`, and the term would stop matching a literal backslash.
        assert_eq!(escaped(Some("a\\%b")).as_deref(), Some("a\\\\\\%b"));
    }

    #[test]
    fn no_search_is_not_an_empty_search() {
        // `Some("")` would be `ILIKE '%%'`, which matches every row including
        // ones whose column is empty — indistinguishable from no filter, and
        // the caller would read the whole population as their result.
        assert_eq!(escaped(None), None);
    }
}
