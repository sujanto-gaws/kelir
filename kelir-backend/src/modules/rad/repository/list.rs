//! Queries for `rad_lists` and its two child collections (§5.6–§5.8).
//!
//! **The children are read in one statement each, not per row.** A list's
//! columns and filters are two array reads keyed on the list, so a page of
//! twenty lists costs three queries rather than forty-one — the failure
//! NFR-PERF-002 exists to prevent, and the reason `list_role_view` is one
//! statement.

use serde_json::Value;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::modules::rad::domain::list::FilterType;
use crate::modules::rad::domain::{
    ListColumnInput, ListDefinition, ListFilterInput, ListStatus, ListSummary,
};

pub struct NewList<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub list_key: &'a str,
    pub title: &'a str,
    pub entity_id: Option<Uuid>,
    pub default_sort_json: Option<&'a Value>,
    pub page_size: i32,
    pub status: &'a str,
    pub created_by: Option<Uuid>,
}

pub struct ListFields<'a> {
    pub title: Option<&'a str>,
    pub entity_id: Option<Option<Uuid>>,
    pub default_sort_json: Option<Option<&'a Value>>,
    pub page_size: Option<i32>,
    pub status: Option<&'a str>,
}

pub async fn count_lists(pool: &PgPool, tenant_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT count(*) FROM rad_lists WHERE tenant_id = $1 AND deleted_at IS NULL",
        tenant_id
    )
    .fetch_one(pool)
    .await
    .map(|count| count.unwrap_or(0))
}

pub async fn list_lists(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<ListSummary>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, list_key, title, entity_id, page_size, status, created_at, updated_at
        FROM rad_lists
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY list_key
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
        .map(|row| ListSummary {
            id: row.id,
            list_key: row.list_key,
            title: row.title,
            entity_id: row.entity_id,
            page_size: row.page_size,
            status: ListStatus::from_db(&row.status),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_list<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<ListDefinition>, sqlx::Error> {
    let Some(row) = sqlx::query!(
        r#"
        SELECT id, list_key, title, entity_id, default_sort_json, page_size,
               status, created_at, updated_at
        FROM rad_lists
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id
    )
    .fetch_optional(executor)
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(ListDefinition {
        id: row.id,
        list_key: row.list_key,
        title: row.title,
        entity_id: row.entity_id,
        default_sort: row.default_sort_json,
        page_size: row.page_size,
        status: ListStatus::from_db(&row.status),
        columns: columns_of(executor, tenant_id, id).await?,
        filters: filters_of(executor, tenant_id, id).await?,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// The same, by the key a menu route and a document type name a list by.
///
/// **A second read rather than a parameter on the first**, because the two are
/// addressed differently and mean different things: `id` is a row and
/// `list_key` is the tenant-unique name §5.6's index enforces. A renderer opens
/// a list by the name in its URL — `/lists/purchase_requisition_list` — and an
/// id in a route would make every bookmark a reference to a row somebody could
/// replace.
///
/// # The `tenant_id` here is defence in depth, and a mutation says so
///
/// **Removing it from this statement comes back green** (#340's campaign, M14),
/// and the cause is a gate rather than a missing test — coding standard §2.9's
/// stated shape. [`find_list`] below re-scopes by tenant, so a key resolved to
/// another tenant's id yields `None` there and the caller gets a 404 either
/// way. What the predicate changes is only the case where **two tenants hold
/// the same key**: without it, the `SELECT` picks one of the two rows in no
/// defined order, and the answer becomes 200-or-404 by luck.
///
/// It stays, and the reason it cannot be held by a test is written down rather
/// than papered over: an `ORDER BY` added here so a test could predict which
/// row wins would be production code shaped by a test. **The load-bearing scope
/// is [`find_list`]'s**, and that one is covered — `rad_list_render.rs` asks
/// this tenant for another tenant's list id and asserts a 404, and
/// `rad_permissions.rs` asserts the same for the storage read.
pub async fn find_list_by_key<'e, E: PgExecutor<'e> + Copy>(
    executor: E,
    tenant_id: Uuid,
    list_key: &str,
) -> Result<Option<ListDefinition>, sqlx::Error> {
    let Some(id) = sqlx::query_scalar!(
        r#"
        SELECT id
        FROM rad_lists
        WHERE tenant_id = $1 AND list_key = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        list_key,
    )
    .fetch_optional(executor)
    .await?
    else {
        return Ok(None);
    };

    // Resolved to an id and then read through `find_list`, so the children come
    // back through exactly one statement each. Two full readers would be two
    // places to forget a column when §5.7 grows one.
    find_list(executor, tenant_id, id).await
}

async fn columns_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    list_id: Uuid,
) -> Result<Vec<ListColumnInput>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT column_key, label, data_type, format, is_sortable, width
        FROM rad_list_columns
        WHERE tenant_id = $1 AND list_id = $2 AND deleted_at IS NULL
        ORDER BY sort_order, column_key
        "#,
        tenant_id,
        list_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ListColumnInput {
            column_key: row.column_key,
            label: row.label,
            data_type: row.data_type,
            format: row.format,
            is_sortable: row.is_sortable,
            width: row.width,
        })
        .collect())
}

async fn filters_of<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    list_id: Uuid,
) -> Result<Vec<ListFilterInput>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT filter_key, label, filter_type, options_json, is_default
        FROM rad_list_filters
        WHERE tenant_id = $1 AND list_id = $2 AND deleted_at IS NULL
        ORDER BY sort_order, filter_key
        "#,
        tenant_id,
        list_id
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            // A stored `filter_type` outside the enum means the column's CHECK
            // was widened without this code. Dropping the filter renders a list
            // missing one control, where a panic would render nothing at all.
            FilterType::from_db(&row.filter_type).map(|filter_type| ListFilterInput {
                filter_key: row.filter_key,
                label: row.label,
                filter_type,
                options_json: row.options_json,
                is_default: row.is_default,
            })
        })
        .collect())
}

pub async fn insert_list<'e, E: PgExecutor<'e>>(
    executor: E,
    list: &NewList<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO rad_lists
            (id, tenant_id, list_key, title, entity_id, default_sort_json,
             page_size, status, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
        list.id,
        list.tenant_id,
        list.list_key,
        list.title,
        list.entity_id,
        list.default_sort_json,
        list.page_size,
        list.status,
        list.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

pub async fn update_list<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &ListFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (entity_id_set, entity_id) = match fields.entity_id {
        None => (false, None),
        Some(value) => (true, value),
    };
    let (sort_set, sort) = match fields.default_sort_json {
        None => (false, None),
        Some(value) => (true, value),
    };

    sqlx::query!(
        r#"
        UPDATE rad_lists
        SET title = COALESCE($3, title),
            entity_id = CASE WHEN $4 THEN $5 ELSE entity_id END,
            default_sort_json = CASE WHEN $6 THEN $7 ELSE default_sort_json END,
            page_size = COALESCE($8, page_size),
            status = COALESCE($9, status),
            updated_by = $10,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.title,
        entity_id_set,
        entity_id,
        sort_set,
        sort,
        fields.page_size,
        fields.status,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

/// Replaces a list's columns wholesale.
///
/// **A hard delete, and it is the exception coding standard §4 allows rather
/// than the practice it forbids.** These rows are not business data with a
/// lifetime — they are the stored form of an array the caller just sent, and
/// the caller sent the whole array. Soft-deleting them instead would make
/// `uq_rad_list_columns_list_id_column_key`, which is partial on
/// `deleted_at IS NULL`, accumulate a dead row per edit per column, and the
/// list read would have to filter around them forever.
pub async fn replace_columns(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    list_id: Uuid,
    columns: &[ListColumnInput],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM rad_list_columns WHERE tenant_id = $1 AND list_id = $2",
        tenant_id,
        list_id
    )
    .execute(&mut **transaction)
    .await?;

    for (index, column) in columns.iter().enumerate() {
        sqlx::query!(
            r#"
            INSERT INTO rad_list_columns
                (id, tenant_id, list_id, column_key, label, data_type, format,
                 is_sortable, width, sort_order, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            Uuid::now_v7(),
            tenant_id,
            list_id,
            column.column_key.trim(),
            column.label.trim(),
            column.data_type.as_deref(),
            column.format.as_deref(),
            column.is_sortable,
            column.width.as_deref(),
            index as i32,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

/// Replaces a list's filters wholesale, for the reason `replace_columns` gives.
pub async fn replace_filters(
    transaction: &mut sqlx::PgTransaction<'_>,
    tenant_id: Uuid,
    list_id: Uuid,
    filters: &[ListFilterInput],
    actor: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM rad_list_filters WHERE tenant_id = $1 AND list_id = $2",
        tenant_id,
        list_id
    )
    .execute(&mut **transaction)
    .await?;

    for (index, filter) in filters.iter().enumerate() {
        sqlx::query!(
            r#"
            INSERT INTO rad_list_filters
                (id, tenant_id, list_id, filter_key, label, filter_type,
                 options_json, is_default, sort_order, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            Uuid::now_v7(),
            tenant_id,
            list_id,
            filter.filter_key.trim(),
            filter.label.trim(),
            filter.filter_type.as_db(),
            filter.options_json,
            filter.is_default,
            index as i32,
            actor,
        )
        .execute(&mut **transaction)
        .await?;
    }

    Ok(())
}

/// Retires a list by soft delete.
///
/// Its columns and filters are left alone. They are unreachable through the
/// API once the list is gone — every read of them is keyed on a live list — and
/// deleting them would make an undelete produce an empty table rather than the
/// list somebody retired.
pub async fn soft_delete<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    deleted_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    sqlx::query!(
        r#"
        UPDATE rad_lists
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
