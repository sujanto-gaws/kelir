//! The statements behind `comments` (Database Schema §9.1; [#249], [#253]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249
//! [#253]: https://github.com/sujanto-gaws/kelir/issues/253

use sqlx::PgExecutor;
use uuid::Uuid;

use super::domain::Comment;

pub struct NewComment<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub document_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
    pub body: &'a str,
    pub created_by: Option<Uuid>,
}

/// What the write paths need to know about a comment before they touch it.
///
/// **Not [`Comment`]**, and the difference is the point: this carries
/// `created_by` and the body as they are *on the row*, where `Comment` is what
/// the API may say about it. An edit needs the old length for the audit trail
/// and the author for the refusal, and neither is something the read boundary
/// serves.
pub struct CommentRow {
    pub id: Uuid,
    pub created_by: Option<Uuid>,
    pub parent_comment_id: Option<Uuid>,
    pub body: String,
}

/// Records a comment, or a reply to one.
///
/// **`comment_type`, `visibility` and `status` are not parameters.** They take
/// their column defaults, and writing them as literals here would be this
/// module claiming to decide something it does not offer — the create surface
/// takes a body and, since [#253], the comment it answers. When FR-CMT-005
/// makes `status` move, it moves in a statement written for that.
pub async fn insert_comment<'e, E: PgExecutor<'e>>(
    executor: E,
    comment: &NewComment<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO comments (id, tenant_id, document_id, parent_comment_id, body, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        comment.id,
        comment.tenant_id,
        comment.document_id,
        comment.parent_comment_id,
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
        SELECT c.id, c.document_id, c.body, c.parent_comment_id, c.created_by, c.created_at,
               c.edited_at, c.deleted_at,
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
        body: Some(row.body),
        parent_comment_id: row.parent_comment_id,
        author_user_id: row.created_by,
        author_username: row.author_username,
        created_at: row.created_at,
        edited_at: row.edited_at,
        deleted_at: row.deleted_at,
    }))
}

/// The comment an edit or a delete is about, **locked for the transaction**.
///
/// **Scoped by document as well as by tenant**, because the surface is
/// `/documents/{id}/comments/{commentId}` and a comment reached through the
/// wrong document is not a comment this caller asked for. Whether they may see
/// *that* document has already been answered by its own module's service; this
/// makes the answer apply to the row.
///
/// **`FOR UPDATE`, for `document::service::delete_document`'s reason**: two
/// deletes of the same comment would otherwise both find it undeleted, and the
/// second would write a second `Comment.Deleted` over a row already gone. The
/// lock makes the second one see the first one's work and answer 404.
pub async fn lock_comment(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
) -> Result<Option<CommentRow>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, created_by, parent_comment_id, body
        FROM comments
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        FOR UPDATE
        "#,
        tenant_id,
        document_id,
        id
    )
    .fetch_optional(&mut **transaction)
    .await?;

    Ok(row.map(|row| CommentRow {
        id: row.id,
        created_by: row.created_by,
        parent_comment_id: row.parent_comment_id,
        body: row.body,
    }))
}

/// The comment a reply names, if this document has it and it is still there.
///
/// Returns the parent's **own** `parent_comment_id`, which is the whole of what
/// the depth rule needs: a non-null one means the caller is replying to a reply,
/// which **D-50** refuses.
///
/// **Not locked.** A parent deleted between this read and the insert leaves a
/// reply under a tombstone, which is precisely the state **D-51** designs for —
/// the thread keeps its shape and the deleted comment is served with no body. A
/// lock here would buy nothing and hold a row somebody else is reading.
pub async fn find_parent<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
) -> Result<Option<Option<Uuid>>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT parent_comment_id
        FROM comments
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| row.parent_comment_id))
}

/// Replaces a comment's text, and stamps the edit as an edit.
///
/// **`edited_at` and `updated_at` both move, and they are not the same fact.**
/// `updated_at` is *this row was written*, which the delete below also causes;
/// `edited_at` is *what this comment says changed*, which only this statement
/// causes. #253 AC3 is about the second one.
pub async fn update_body<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
    body: &str,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE comments
        SET body = $4, edited_at = now(), updated_at = now(), updated_by = $5
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id,
        body,
        actor,
    )
    .execute(executor)
    .await?;

    Ok(result.rows_affected())
}

/// Marks a comment deleted without taking its text out of the row.
///
/// **A soft delete, and the body stays** (#253 AC4). What the API serves is
/// decided at the read boundary — a tombstone with no body while replies hang
/// from it, and nothing at all once they do not. Scrubbing the column here would
/// make that a one-way door and would take the row's own audit evidence with it.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
        UPDATE comments
        SET deleted_at = now(), updated_at = now(), updated_by = $4
        WHERE tenant_id = $1 AND document_id = $2 AND id = $3 AND deleted_at IS NULL
        "#,
        tenant_id,
        document_id,
        id,
        actor,
    )
    .execute(executor)
    .await?;

    Ok(result.rows_affected())
}

/// A document's conversation, oldest first, **replies under the comment they
/// answer**.
///
/// **Oldest first, which is the opposite of every other list in this product.**
/// A conversation is read in the order it was said; a list of records is read
/// newest-first because the newest is the one you came for.
///
/// # The order is the thread's, and the sort is a join rather than a `COALESCE`
///
/// A root sorts by when it was said and a reply sorts with its root, so
/// `ORDER BY` runs over the *parent's* `created_at` where there is one. The
/// obvious shorthand — `COALESCE(parent_comment_id, id)` — would be one
/// expression and one index, and it is not written here because it is true only
/// while every id is a UUIDv7: it orders threads by the **bytes of a uuid**, and
/// a writer that ever used `Uuid::new_v4` would scramble the conversation with
/// nothing failing. The join says what is meant.
///
/// # Which rows appear, and the one that is a tombstone (**D-51**)
///
/// A deleted comment is served **only while an undeleted reply hangs from it**,
/// with `body` withheld — deleting a root must not take other people's words
/// with it, and a reply whose parent vanished is an orphan the screen cannot
/// place. A deleted comment nobody answered is not served at all: there is no
/// shape left for it to hold.
///
/// **The `EXISTS` is one level deep because threading is** (D-50). It asks for
/// replies to this comment and does not recurse, which is a statement that
/// stops being right the day the depth rule changes — and that day it fails
/// visibly here rather than quietly somewhere else.
pub async fn list_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Comment>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT c.id, c.document_id, c.body, c.parent_comment_id, c.created_by, c.created_at,
               c.edited_at, c.deleted_at,
               u.username AS "author_username?"
        FROM comments c
        LEFT JOIN users u ON u.id = c.created_by AND u.tenant_id = c.tenant_id
        LEFT JOIN comments p ON p.id = c.parent_comment_id AND p.tenant_id = c.tenant_id
        WHERE c.tenant_id = $1 AND c.document_id = $2
          AND (
            c.deleted_at IS NULL
            OR EXISTS (
              SELECT 1 FROM comments r
              WHERE r.parent_comment_id = c.id AND r.deleted_at IS NULL
            )
          )
        ORDER BY coalesce(p.created_at, c.created_at), coalesce(p.id, c.id), c.created_at, c.id
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
            // **The withholding is here**, at the one place every reader comes
            // through, rather than in a caller that has to remember.
            body: row.deleted_at.is_none().then_some(row.body),
            document_id: row.document_id,
            parent_comment_id: row.parent_comment_id,
            author_user_id: row.created_by,
            author_username: row.author_username,
            created_at: row.created_at,
            edited_at: row.edited_at,
            deleted_at: row.deleted_at,
        })
        .collect())
}

/// How many the page is drawn from, **under the same predicate**.
///
/// Written out rather than shared, and the duplication is deliberate for
/// `workflow::repository::inbox`'s stated reason: `meta.total` and the page must
/// agree. That file is also where the duplication drifted — a join added to one
/// and not the other ([#279](https://github.com/sujanto-gaws/kelir/issues/279))
/// — so the predicate here is kept to what both statements need and the author
/// join stays out, where it changes nothing.
///
/// **The tombstone clause is not one of the things that may drift**, which is
/// why it is repeated verbatim rather than approximated: a count that omitted it
/// would report a total the page could exceed, on any document where somebody
/// deleted a comment that had been answered.
///
/// **Seen red, 2026-09-01**, with the clause dropped from this statement alone:
/// `deleting_a_comment_that_has_replies_leaves_a_tombstone_and_keeps_them` kept
/// its two rows and reported a total of one.
pub async fn count_for_document<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    document_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*)
        FROM comments c
        WHERE c.tenant_id = $1 AND c.document_id = $2
          AND (
            c.deleted_at IS NULL
            OR EXISTS (
              SELECT 1 FROM comments r
              WHERE r.parent_comment_id = c.id AND r.deleted_at IS NULL
            )
          )
        "#,
        tenant_id,
        document_id
    )
    .fetch_one(executor)
    .await
    .map(|count| count.unwrap_or(0))
}
