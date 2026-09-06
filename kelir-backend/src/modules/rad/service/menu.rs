//! Menu definition use cases (FR-RAD-004, [#341]).
//!
//! **The cycle check is the reason this file is longer than the CRUD it
//! wraps.** `rad_menus.parent_menu_id` makes a tree, and
//! `ck_rad_menus_not_its_own_parent` catches exactly one shape of the problem —
//! a row naming itself. A ring of three walks straight past it, and the thing
//! that renders the tree walks straight into it.
//! [#191](https://github.com/sujanto-gaws/kelir/issues/191) named the fix and
//! where it goes: *the ancestor walk #141 built for facilities, in the service
//! that writes the column*. This is that service.
//!
//! [#341]: https://github.com/sujanto-gaws/kelir/issues/341

use serde_json::json;
use uuid::Uuid;

use super::super::domain::menu::{
    validate_create_menu, validate_update_menu, CreateMenuRequest, MenuEntry, MenuSource,
    UpdateMenuRequest, MAX_MENU_DEPTH,
};
use super::super::repository::menu::{self as repo, MenuFields, NewMenu};
use super::super::{MENU_CREATE, MENU_DELETE, MENU_READ, MENU_UPDATE};
use crate::error::{AppError, ValidationDetail};
use crate::middleware::auth::Authenticated;
use crate::modules::audit::{self, domain::ObjectType, AuditEntry, ChangeSet};
use crate::state::AppState;

/// The whole configured navigation.
///
/// **Every entry, enabled or not, and whatever the caller's permissions.** This
/// is the *builder's* read: an administrator editing the navigation has to see
/// the entry they switched off, and one they cannot personally open. The
/// sidebar's own filtering — `isEnabled`, and `requiredPermission` against the
/// person looking — happens where the sidebar is drawn, which is the client,
/// and is cosmetic there by design (§5.9's column comment).
pub async fn list_menus(
    state: &AppState,
    caller: &Authenticated,
) -> Result<Vec<MenuEntry>, AppError> {
    caller.require(MENU_READ)?;

    Ok(repo::list_menus(&state.pool, caller.tenant_id()).await?)
}

pub async fn get_menu(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<MenuEntry, AppError> {
    caller.require(MENU_READ)?;

    repo::find_menu(&state.pool, caller.tenant_id(), id)
        .await?
        .ok_or_else(|| AppError::not_found("Menu entry"))
}

pub async fn create_menu(
    state: &AppState,
    caller: &Authenticated,
    request: &CreateMenuRequest,
) -> Result<MenuEntry, AppError> {
    caller.require(MENU_CREATE)?;
    validate_create_menu(request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let menu_key = request.menu_key.trim();

    if repo::key_exists(&state.pool, tenant_id, menu_key).await? {
        return Err(duplicate_key(menu_key));
    }

    // A new entry has no children, so it cannot close a loop — but its parent
    // must exist and be this tenant's, which the foreign key would answer as a
    // 500 rather than as a refusal naming the field.
    if let Some(parent) = request.parent_menu_id {
        require_parent(state, tenant_id, parent).await?;
    }

    let id = Uuid::now_v7();

    repo::insert_menu(
        &state.pool,
        &NewMenu {
            id,
            tenant_id,
            menu_key,
            label: request.label.trim(),
            icon: trimmed(request.icon.as_deref()),
            parent_menu_id: request.parent_menu_id,
            route_path: trimmed(request.route_path.as_deref()),
            required_permission: trimmed(request.required_permission.as_deref()),
            sort_order: request.sort_order.unwrap_or(0),
            is_enabled: request.is_enabled.unwrap_or(true),
            created_by: actor,
        },
    )
    .await?;

    // Read back before the record is written, so the record says what the row
    // holds rather than what the request asked for (#135) — keys are trimmed
    // and defaults fill fields the caller omitted, so the two differ here.
    let created = load(state, tenant_id, id).await?;

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadMenu.Created",
            action: "CREATE",
            object_type: ObjectType::RadMenu,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: None,
            new_value: Some(json!({
                "menuKey": created.menu_key,
                "label": created.label,
                "routePath": created.route_path,
                "parentMenuId": created.parent_menu_id,
            })),
        },
    )
    .await;

    Ok(created)
}

pub async fn update_menu(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
    request: &UpdateMenuRequest,
) -> Result<MenuEntry, AppError> {
    caller.require(MENU_UPDATE)?;
    validate_update_menu(request)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let before = load(state, tenant_id, id).await?;

    // **The cycle check, and it runs only when the parent moves.** Re-walking a
    // tree that has not changed would put a recursive query on every label
    // edit, and the shape it guards against cannot arise from one.
    //
    // The doubly-nested `Some` is the field's own shape: the outer one is *the
    // request carried this field*, the inner one *and set it to a parent*.
    // Clearing it — `Some(None)` — makes the entry a root, which can close no
    // loop and needs no walk.
    if let Some(Some(parent)) = request.parent_menu_id {
        require_parent(state, tenant_id, parent).await?;
        refuse_a_cycle(state, tenant_id, id, parent).await?;
    }

    let updated = repo::update_menu(
        &state.pool,
        tenant_id,
        id,
        &MenuFields {
            label: request.label.as_deref().map(str::trim),
            icon: request.icon.as_ref().map(|icon| trimmed(icon.as_deref())),
            parent_menu_id: request.parent_menu_id,
            route_path: request
                .route_path
                .as_ref()
                .map(|route| trimmed(route.as_deref())),
            required_permission: request
                .required_permission
                .as_ref()
                .map(|permission| trimmed(permission.as_deref())),
            sort_order: request.sort_order,
            is_enabled: request.is_enabled,
        },
        actor,
    )
    .await?;

    if updated == 0 {
        // The row was removed between the read and this statement.
        //
        // **A mutation of this line comes back green, and is expected to.**
        // `load` above has already answered 404 for an id this tenant does not
        // have, so the only way here is a delete landing between the two
        // statements — which no test can reach without two concurrent callers.
        // It is kept because the alternative is a 200 describing a row that is
        // gone (coding standard §2.9's *green with a stated reason*).
        return Err(AppError::not_found("Menu entry"));
    }

    let after = load(state, tenant_id, id).await?;
    let (old, new) = changes(&before, &after).halves();

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadMenu.Updated",
            action: "UPDATE",
            object_type: ObjectType::RadMenu,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(old),
            new_value: Some(new),
        },
    )
    .await;

    Ok(after)
}

pub async fn delete_menu(
    state: &AppState,
    caller: &Authenticated,
    id: Uuid,
) -> Result<(), AppError> {
    caller.require(MENU_DELETE)?;

    let tenant_id = caller.tenant_id();
    let actor = Some(caller.user_id());
    let before = load(state, tenant_id, id).await?;

    if repo::soft_delete(&state.pool, tenant_id, id, actor).await? == 0 {
        return Err(AppError::not_found("Menu entry"));
    }

    audit::record_or_warn(
        &state.pool,
        AuditEntry {
            tenant_id,
            event_type: "RadMenu.Deleted",
            action: "DELETE",
            object_type: ObjectType::RadMenu,
            object_id: id,
            actor_user_id: actor,
            ip_address: caller.ip_address(),
            reason: None,
            old_value: Some(json!({ "menuKey": before.menu_key, "label": before.label })),
            new_value: None,
        },
    )
    .await;

    Ok(())
}

/// Refuses a parent that is not this tenant's live tree.
async fn require_parent(state: &AppState, tenant_id: Uuid, parent: Uuid) -> Result<(), AppError> {
    if repo::find_menu(&state.pool, tenant_id, parent)
        .await?
        .is_some()
    {
        return Ok(());
    }

    Err(AppError::validation(vec![ValidationDetail::new(
        "parentMenuId",
        "reference",
        "PARENT_NOT_FOUND",
        "this tenant has no menu entry with that id, so nothing can be nested under it",
    )]))
}

/// Refuses a parent that is already a descendant ([#191]).
///
/// **Walking up from the proposed parent is what makes one query enough.** If
/// `id` appears anywhere on the path from `parent` to the root, then attaching
/// `id` above `parent` closes a ring — the `CHECK` catches the one-hop case
/// where `parent == id`, and this catches every longer one.
///
/// **A truncated walk is a refusal, not a pass.** `menu_ancestors` returns the
/// flag because a bounded walk over a tree that is already cyclic returns a
/// *partial* path, and a partial path that happens not to contain `id` would
/// let the check that exists to refuse a cycle allow one — the bound creating
/// the corruption it was there to survive. A caller that cannot see a complete
/// path refuses.
///
/// [#191]: https://github.com/sujanto-gaws/kelir/issues/191
async fn refuse_a_cycle(
    state: &AppState,
    tenant_id: Uuid,
    id: Uuid,
    parent: Uuid,
) -> Result<(), AppError> {
    let ancestry = repo::menu_ancestors(&state.pool, tenant_id, parent, MAX_MENU_DEPTH).await?;

    if ancestry.truncated {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentMenuId",
            "cycle",
            "MENU_TREE_TOO_DEEP",
            format!(
                "this menu's ancestry could not be read within {MAX_MENU_DEPTH} levels, so \
                 whether the move would create a loop cannot be decided — it is refused \
                 rather than guessed"
            ),
        )]));
    }

    if ancestry.ids.contains(&id) {
        return Err(AppError::validation(vec![ValidationDetail::new(
            "parentMenuId",
            "cycle",
            "MENU_WOULD_CYCLE",
            "that entry is already below this one, so nesting this one under it would make a \
             loop the navigation could not draw",
        )]));
    }

    Ok(())
}

async fn load(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<MenuEntry, AppError> {
    repo::find_menu(&state.pool, tenant_id, id)
        .await?
        .ok_or_else(|| AppError::not_found("Menu entry"))
}

fn duplicate_key(menu_key: &str) -> AppError {
    AppError::validation(vec![ValidationDetail::new(
        "menuKey",
        "unique",
        "DUPLICATE",
        format!("a menu entry called `{menu_key}` already exists in this tenant"),
    )])
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// What actually moved, for the audit record (#135).
///
/// Every field by the name the API publishes it under, which is what
/// [`ChangeSet::field`] is for: a record reads in the caller's vocabulary
/// rather than the column's, and a field that did not move is absent rather
/// than recorded as unchanged.
fn changes(before: &MenuEntry, after: &MenuEntry) -> ChangeSet {
    let mut changes = ChangeSet::new();

    changes.field("label", &before.label, &after.label);
    changes.field("icon", &before.icon, &after.icon);
    changes.field(
        "parentMenuId",
        &before.parent_menu_id,
        &after.parent_menu_id,
    );
    changes.field("routePath", &before.route_path, &after.route_path);
    changes.field(
        "requiredPermission",
        &before.required_permission,
        &after.required_permission,
    );
    changes.field("sortOrder", &before.sort_order, &after.sort_order);
    changes.field("isEnabled", &before.is_enabled, &after.is_enabled);
    changes
}

/// The source this API writes, stated so a reader does not have to find it in
/// the `INSERT`.
pub const WRITES: MenuSource = MenuSource::Config;
