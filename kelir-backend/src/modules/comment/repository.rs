//! The statements behind `comments` (Database Schema §9.1; [#249]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::Comment;

pub struct NewComment<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub body: &'a str,
    pub created_by: Option<Uuid>,
}

/// Records a comment.
///
/// **`comment_type`, `visibility` and `status` are not parameters.** They take
/// their column defaults, and writing them as literals here would be this
/// module claiming to decide something it does not offer — the create surface
/// takes a body. When FR-CMT-004 makes `status` move, it moves in a statement
/// written for that.
pub async fn insert_comment<'e, E: PgExecutor<'e>>(
    executor: E,
    comment: &NewComment<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO comments (id, tenant_id, document_id, body, created_by)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        comment.id,
        comment.tenant_id,
        comment.document_id,
        comment.body,
        comment.created_by,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// One comment, scoped by tenant in the statement.
pub async fn find_comment<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<Comment>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT c.id, c.document_id, c.body, c.created_by, c.created_at,
               u.username AS "author_username?"
        FROM comments c
        LEFT JOIN users u ON u.id = c.created_by AND u.tenant_id = c.tenant_id
        WHERE c.tenant_id = $1 AND c.id = $2 AND c.deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| Comment {
        id: row.id,
        document_id: row.document_id,
        body: row.body,
        author_user_id: row.created_by,
        author_username: row.author_username,
        created_at: row.created_at,
    }))
}

/// A document's comments, oldest first.
///
/// **Oldest first, which is the opposite of every other list in this product.**
/// A conversation is read in the order it was said; a list of records is read
/// newest-first because the newest is the one you came for. The index
/// `idx_comments_document_id (document_id, created_at)` is ordered to match.
///
/// **Replies are not excluded**, because nothing writes `parent_comment_id` in
/// this release. When FR-CMT-002 does, this statement is where the tree is
/// assembled and the filter belongs — said here so the next reader knows the
/// flat list is a fact about the data rather than a choice about the display.
pub async fn list_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Comment>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT c.id, c.document_id, c.body, c.created_by, c.created_at,
               u.username AS "author_username?"
        FROM comments c
        LEFT JOIN users u ON u.id = c.created_by AND u.tenant_id = c.tenant_id
        WHERE c.tenant_id = $1 AND c.document_id = $2 AND c.deleted_at IS NULL
        ORDER BY c.created_at, c.id
        LIMIT $3 OFFSET $4
        "#,
        tenant_id,
        document_id,
        limit,
        offset
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Comment {
            id: row.id,
            document_id: row.document_id,
            body: row.body,
            author_user_id: row.created_by,
            author_username: row.author_username,
            created_at: row.created_at,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
///
/// Written out rather than shared, and the duplication is deliberate for
/// `workflow::repository::inbox`'s stated reason: `meta.total` and the page must
/// agree. That file is also where the duplication drifted — a join added to one
/// and not the other ([#279](https://github.com/sujanto-gaws/kelir/issues/279))
/// — so the predicate here is kept to the three clauses both statements need and
/// the author join stays out of the count, where it changes nothing.
pub async fn count_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM comments
        WHERE tenant_id = $1 AND document_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}
