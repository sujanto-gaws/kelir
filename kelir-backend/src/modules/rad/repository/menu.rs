//! Queries over `rad_menus` (§5.9, [#341]).
//!
//! [`super`]'s two conventions hold: `tenant_id` comes from the caller's
//! claims, and every read filters `deleted_at IS NULL`.
//!
//! [#341]: https://github.com/sujanto-gaws/kelir/issues/341

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use super::super::domain::menu::{MenuEntry, MenuSource};

pub struct NewMenu<'a> {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub menu_key: &'a str,
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub parent_menu_id: Option<Uuid>,
    pub route_path: Option<&'a str>,
    pub required_permission: Option<&'a str>,
    pub sort_order: i32,
    pub is_enabled: bool,
    pub created_by: Option<Uuid>,
}

/// The fields an update may change. `None` leaves a column alone; the outer
/// `Some(None)` on a nullable column clears it.
pub struct MenuFields<'a> {
    pub label: Option<&'a str>,
    pub icon: Option<Option<&'a str>>,
    pub parent_menu_id: Option<Option<Uuid>>,
    pub route_path: Option<Option<&'a str>>,
    pub required_permission: Option<Option<&'a str>>,
    pub sort_order: Option<i32>,
    pub is_enabled: Option<bool>,
}

/// Every menu entry a tenant has configured, in the order a sidebar renders.
///
/// **Not paged**, unlike every other list in this module. A navigation is read
/// whole on every page load and is bounded by what a person can usefully
/// navigate; paging it would make the sidebar's completeness depend on a page
/// size, and a second page of navigation is a navigation nobody found.
///
/// Ordered by `sort_order` then `menu_key`: the column defaults to 0, so a
/// deployment that sets no order gets its entries alphabetically rather than in
/// whatever order the rows come back — the pairing `columns_of` uses one file
/// over, and the difference between a stable sidebar and one that reshuffles.
pub async fn list_menus<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
) -> Result<Vec<MenuEntry>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, menu_key, label, icon, parent_menu_id, route_path,
               required_permission, source, sort_order, is_enabled,
               created_at, updated_at
        FROM rad_menus
        WHERE tenant_id = $1 AND deleted_at IS NULL
        ORDER BY sort_order, menu_key
        "#,
        tenant_id,
    )
    .fetch_all(executor)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| MenuEntry {
            id: row.id,
            menu_key: row.menu_key,
            label: row.label,
            icon: row.icon,
            parent_menu_id: row.parent_menu_id,
            route_path: row.route_path,
            required_permission: row.required_permission,
            source: MenuSource::from_db(&row.source),
            sort_order: row.sort_order,
            is_enabled: row.is_enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

pub async fn find_menu<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<MenuEntry>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT id, menu_key, label, icon, parent_menu_id, route_path,
               required_permission, source, sort_order, is_enabled,
               created_at, updated_at
        FROM rad_menus
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(row.map(|row| MenuEntry {
        id: row.id,
        menu_key: row.menu_key,
        label: row.label,
        icon: row.icon,
        parent_menu_id: row.parent_menu_id,
        route_path: row.route_path,
        required_permission: row.required_permission,
        source: MenuSource::from_db(&row.source),
        sort_order: row.sort_order,
        is_enabled: row.is_enabled,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

/// The ids on the path from `start` up to the root, and whether the walk ran
/// out of budget.
///
/// **This is [#191](https://github.com/sujanto-gaws/kelir/issues/191)'s
/// ancestor walk**, which the Database Schema §5.9 note says belongs *in the
/// service that writes the column* — `ck_rad_menus_not_its_own_parent` catches
/// the one-hop case and a ring of three walks straight past it.
///
/// **`truncated` travels with the ids, and the caller must refuse on it.**
/// `facility_ancestors` argues this at length and the argument is identical
/// here: a walk over a tree that is *already* cyclic never terminates without a
/// bound, and a bound that silently returns a partial path turns the check that
/// exists to refuse a cycle into one that allows it. A caller that cannot see a
/// complete path refuses rather than assumes.
///
/// A walk stopping early for any other reason — a parent that is soft-deleted
/// or in another tenant — is not truncation: such a parent is not in the live
/// tree, so no live traversal reaches through it.
pub struct MenuAncestry {
    pub ids: Vec<Uuid>,
    pub truncated: bool,
}

pub async fn menu_ancestors<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    start: Uuid,
    max_depth: i32,
) -> Result<MenuAncestry, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        WITH RECURSIVE up AS (
            SELECT id, parent_menu_id, 1 AS depth
            FROM rad_menus
            WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            UNION ALL
            SELECT m.id, m.parent_menu_id, up.depth + 1
            FROM rad_menus m
            JOIN up ON m.id = up.parent_menu_id
            WHERE m.tenant_id = $1 AND m.deleted_at IS NULL AND up.depth < $3
        )
        SELECT id AS "id!", parent_menu_id, depth AS "depth!" FROM up
        "#,
        tenant_id,
        start,
        max_depth,
    )
    .fetch_all(executor)
    .await?;

    // The deepest row still naming a parent is the walk running out of budget:
    // the recursive term declines to follow it precisely because `depth` has
    // reached the bound.
    let truncated = rows
        .iter()
        .any(|row| row.depth >= max_depth && row.parent_menu_id.is_some());

    Ok(MenuAncestry {
        ids: rows.into_iter().map(|row| row.id).collect(),
        truncated,
    })
}

pub async fn insert_menu<'e, E: PgExecutor<'e>>(
    executor: E,
    menu: &NewMenu<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO rad_menus
            (id, tenant_id, menu_key, label, icon, parent_menu_id, route_path,
             required_permission, source, sort_order, is_enabled, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'CONFIG', $9, $10, $11, $11)
        "#,
        menu.id,
        menu.tenant_id,
        menu.menu_key,
        menu.label,
        menu.icon,
        menu.parent_menu_id,
        menu.route_path,
        menu.required_permission,
        menu.sort_order,
        menu.is_enabled,
        menu.created_by,
    )
    .execute(executor)
    .await
    .map(|_| ())
}

/// Updates one entry, leaving the columns the request did not carry.
///
/// The `CASE WHEN $n THEN` pairs are how a nullable column tells *leave alone*
/// from *set to null* in one static statement — the shape `update_draft` uses
/// for a form's `entity_id`, for the same reason: two statements would be two
/// places to add the next column to.
pub async fn update_menu<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    id: Uuid,
    fields: &MenuFields<'_>,
    updated_by: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let (icon_set, icon) = split(fields.icon);
    let (parent_set, parent) = split(fields.parent_menu_id);
    let (route_set, route) = split(fields.route_path);
    let (permission_set, permission) = split(fields.required_permission);

    sqlx::query!(
        r#"
        UPDATE rad_menus
        SET label = COALESCE($3, label),
            icon = CASE WHEN $4 THEN $5 ELSE icon END,
            parent_menu_id = CASE WHEN $6 THEN $7 ELSE parent_menu_id END,
            route_path = CASE WHEN $8 THEN $9 ELSE route_path END,
            required_permission = CASE WHEN $10 THEN $11 ELSE required_permission END,
            sort_order = COALESCE($12, sort_order),
            is_enabled = COALESCE($13, is_enabled),
            updated_by = $14,
            updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        fields.label,
        icon_set,
        icon,
        parent_set,
        parent,
        route_set,
        route,
        permission_set,
        permission,
        fields.sort_order,
        fields.is_enabled,
        updated_by,
    )
    .execute(executor)
    .await
    .map(|result| result.rows_affected())
}

fn split<T>(field: Option<Option<T>>) -> (bool, Option<T>) {
    match field {
        None => (false, None),
        Some(value) => (true, value),
    }
}

/// Soft-deletes one entry.
///
/// **Its children are re-parented to its own parent rather than orphaned.** A
/// child whose parent is soft-deleted is unreachable in the tree the sidebar
/// walks — the row is live and nothing renders it — which is a menu entry that
/// exists and cannot be found. Lifting the children one level is what a person
/// deleting a heading means, and it keeps the tree connected.
pub async fn soft_delete(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    actor: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    let mut transaction = pool.begin().await?;

    sqlx::query!(
        r#"
        UPDATE rad_menus
        SET parent_menu_id = (
                SELECT parent_menu_id FROM rad_menus
                WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
            ),
            updated_by = $3,
            updated_at = now()
        WHERE tenant_id = $1 AND parent_menu_id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        actor,
    )
    .execute(&mut *transaction)
    .await?;

    let removed = sqlx::query!(
        r#"
        UPDATE rad_menus
        SET deleted_at = now(), updated_by = $3, updated_at = now()
        WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
        "#,
        tenant_id,
        id,
        actor,
    )
    .execute(&mut *transaction)
    .await?
    .rows_affected();

    transaction.commit().await?;

    Ok(removed)
}

/// Whether a key is already taken in this tenant.
///
/// Asked before the insert so the caller gets a 422 naming the field rather
/// than a unique-index violation surfacing as a 500 — the same reasoning
/// `check_columns` gives about a duplicate column key.
pub async fn key_exists<'e, E: PgExecutor<'e>>(
    executor: E,
    tenant_id: Uuid,
    menu_key: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM rad_menus
            WHERE tenant_id = $1 AND menu_key = $2 AND deleted_at IS NULL
        ) AS "taken!"
        "#,
        tenant_id,
        menu_key,
    )
    .fetch_one(executor)
    .await
}
