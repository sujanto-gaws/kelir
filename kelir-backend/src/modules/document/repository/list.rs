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

use super::super::domain::{
    DocumentFilters, DocumentPriority, DocumentStatus, DocumentSummary, EntityType,
};

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
        "#,
        tenant_id,
        filters.document_type_id,
        filters.status.map(DocumentStatus::as_db),
        filters.priority.map(DocumentPriority::as_db),
        filters.entity_type.map(EntityType::as_db),
        filters.entity_id,
        escaped(filters.search.as_deref()),
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

/// One page of documents.
///
/// **Ordered newest first**, which is what a list of documents is for: the thing
/// somebody is looking for is almost always the one they made most recently, and
/// a stable secondary key on `id` keeps two documents created in the same
/// millisecond from swapping places between pages.
pub async fn list_documents(
    pool: &PgPool,
    tenant_id: Uuid,
    filters: &DocumentFilters,
    limit: i64,
    offset: i64,
) -> Result<Vec<DocumentSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT d.id, d.document_ref, d.document_number, d.document_type_id,
               t.type_code AS document_type_code, d.title, d.status, d.priority,
               d.entity_type, d.entity_id, d.submitted_at, d.created_at, d.updated_at
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
        ORDER BY d.created_at DESC, d.id DESC
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
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| DocumentSummary {
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
        })
        .collect())
}

/// Makes `%` and `_` in a search term match themselves.
///
/// Without it, a search for `PR_2026` matches `PR-2026` and `PRX2026`, and a
/// search for `%` returns the whole population — which reads as a working search
/// that found everything. The escape character is the backslash `ILIKE` uses by
/// default, and it is escaped first so that escaping does not double itself.
///
/// The same treatment `role_view`'s search gets, and the query parameter's doc
/// comment says so on the wire: *`%` and `_` in it match themselves*.
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
