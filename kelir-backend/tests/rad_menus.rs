//! The configured navigation (FR-RAD-004, [#341]).
//!
//! **`rad_menus` has been in the schema since Sprint 7 and had no reader.**
//! These are the tests for the writer and the reader, and the ones that matter
//! most are about the *tree*: `ck_rad_menus_not_its_own_parent` catches a row
//! naming itself and nothing else, so a ring of three walks straight past the
//! database and into whatever draws the sidebar.
//!
//! That is [#191](https://github.com/sujanto-gaws/kelir/issues/191), which said
//! the ancestor walk belongs *in the service that writes the column*. It does
//! now, and this file is what holds it.
//!
//! ## Mutation campaign — 2026-09-06, thirteen mutations, twelve red
//!
//! Coding standard §2.9. Each predicate #341 added, with the test that saw it.
//! Two of the thirteen changed the code under test rather than only the tests.
//!
//! | | Mutation | Verdict |
//! |---|---|---|
//! | M1 | `service::menu` — `ancestry.ids.contains(&id)` → `false` | **red**, `a_ring_of_three_is_refused_where_the_check_constraint_cannot_see_it` |
//! | M2 | `service::menu` — `ancestry.truncated` → `false` | **red**, `an_ancestry_that_cannot_be_read_to_the_root_is_refused_rather_than_guessed` |
//! | M3 | `repository::menu` — truncation drops `&& parent_menu_id.is_some()` | **green first.** Nothing here stood on the bound, so `depth >= max_depth` was never true and the second half of the predicate was unmeasured; `an_ancestry_that_reaches_the_root_on_the_last_step_is_not_truncated` builds a chain of exactly `MAX_MENU_DEPTH` and it is **red** |
//! | M4 | `service::menu` — `require_parent` returns `Ok` unconditionally | **red**, `a_parent_this_tenant_does_not_have_is_refused_by_name` |
//! | M5 | `repository::menu` — `insert_menu` writes `'CORE'` | **red**, `an_entry_is_created_read_back_and_listed_in_render_order` |
//! | M6 | `repository::menu` — the delete's re-parent matches nothing | **red**, `deleting_a_heading_moves_its_children_up_rather_than_stranding_them` |
//! | M7 | `repository::menu` — children rise to the root, not the grandparent | **red**, same test |
//! | M8 | `repository::menu` — `list_menus` drops its tenant scope | **red**, `a_menu_in_another_tenant_is_not_listed_or_readable_here` |
//! | M9 | `repository::menu` — `find_menu` drops its tenant scope | **red**, same test |
//! | M10 | `service::menu` — `key_exists` → `false` | **red**, `a_duplicate_key_is_refused_by_name_rather_than_by_the_index` |
//! | M11 | `service::menu` — `updated == 0` → `false` | **green, and kept.** `load` has already answered 404 for an id this tenant does not have, so that line is reachable only by a delete landing between the read and the write — no test reaches it without two concurrent callers. The reason is stated at the line |
//! | M12 | `domain::menu` — the leading-`/` check | **red**, `refuses_a_route_that_leaves_the_application` |
//! | M13 | `domain::menu` — the `//` check, so `//evil.test` passes | **red**, same test |
//!
//! **M8 and M9 were rewritten before they measured anything.** Dropping `$1`
//! from the SQL makes `sqlx::query!` refuse to compile, which measures the
//! macro rather than the predicate; `(tenant_id = $1 OR true)` keeps the
//! parameter bound and removes only the scoping.
//!
//! [#341]: https://github.com/sujanto-gaws/kelir/issues/341

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const PASSWORD: &str = "menu-builder-user-password";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

fn codes(body: &Value) -> Vec<String> {
    body["error"]["details"]
        .as_array()
        .map(|details| {
            details
                .iter()
                .map(|detail| detail["code"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

async fn create(app: &TestApp, token: &str, body: Value) -> common::TestResponse {
    app.post("/api/v1/rad/menus", Some(token), body).await
}

/// One entry, created and its id returned.
async fn entry(app: &TestApp, token: &str, key: &str, parent: Option<Uuid>) -> Uuid {
    let mut body = json!({ "menuKey": key, "label": key, "routePath": "/documents" });

    if let Some(parent) = parent {
        body["parentMenuId"] = json!(parent);
    }

    let created = create(app, token, body).await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn update(app: &TestApp, token: &str, id: Uuid, body: Value) -> common::TestResponse {
    app.put(&format!("/api/v1/rad/menus/{id}"), Some(token), body)
        .await
}

async fn list(app: &TestApp, token: &str) -> common::TestResponse {
    app.send(Method::GET, "/api/v1/rad/menus", Some(token), None)
        .await
}

fn keys(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["menuKey"].as_str().unwrap_or_default().to_owned())
        .collect()
}

async fn caller_holding(app: &TestApp, permissions: &[&str], nonce: usize) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-MENU-{nonce}"),
        permissions,
    )
    .await;
    let username = format!("user.menu{nonce}");

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("menu{nonce}@kelir.test"),
        PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

// ---------------------------------------------------------------------------
// The ordinary shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_entry_is_created_read_back_and_listed_in_render_order() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create(
        &app,
        &token,
        json!({
            "menuKey": "purchasing", "label": "Purchasing", "icon": "shopping-cart",
            "routePath": "/lists/purchase_requisitions",
            "requiredPermission": "document:read", "sortOrder": 20,
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let entry = &created.body["data"];

    assert_eq!(entry["menuKey"], "purchasing");
    assert_eq!(entry["routePath"], "/lists/purchase_requisitions");
    assert_eq!(entry["requiredPermission"], "document:read");
    // **Written as CONFIG whatever the caller says**: `CORE` is the navigation
    // compiled into the frontend, and a row claiming it would claim a
    // provenance it does not have.
    assert_eq!(entry["source"], "CONFIG");
    assert_eq!(entry["isEnabled"], true);

    // A second entry with a lower order, created later, still renders first.
    entry_with_order(&app, &token, "reports", 10).await;

    assert_eq!(
        keys(&list(&app, &token).await.body),
        ["reports", "purchasing"]
    );
}

async fn entry_with_order(app: &TestApp, token: &str, key: &str, order: i32) -> Uuid {
    let created = create(
        app,
        token,
        json!({ "menuKey": key, "label": key, "routePath": "/documents", "sortOrder": order }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

#[tokio::test]
async fn a_duplicate_key_is_refused_by_name_rather_than_by_the_index() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    entry(&app, &token, "purchasing", None).await;

    let again = create(
        &app,
        &token,
        json!({ "menuKey": "purchasing", "label": "Again", "routePath": "/documents" }),
    )
    .await;

    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(codes(&again.body).contains(&"DUPLICATE".to_owned()));
}

/// A menu entry links inside this application. An absolute URL would either go
/// somewhere the router cannot, or become a stored script the navigation draws
/// for everybody.
#[tokio::test]
async fn a_route_that_leaves_the_application_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for route in ["https://example.test/steal", "javascript:alert(1)"] {
        let created = create(
            &app,
            &token,
            json!({ "menuKey": "bad", "label": "Bad", "routePath": route }),
        )
        .await;

        assert_eq!(
            created.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "`{route}` was accepted"
        );
        assert!(
            codes(&created.body).contains(&"ROUTE_NOT_RENDERABLE".to_owned())
                || codes(&created.body).contains(&"ROUTE_NOT_RELATIVE".to_owned())
        );
    }
}

// ---------------------------------------------------------------------------
// The tree, and #191
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_entry_nests_under_a_parent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parent = entry(&app, &token, "operations", None).await;
    let child = entry(&app, &token, "purchasing", Some(parent)).await;

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/menus/{child}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(read.body["data"]["parentMenuId"], parent.to_string());
}

/// **The ring of three the `CHECK` cannot see** ([#191]).
///
/// `a → b → c`, then asking `a` to sit under `c`. Every hop is legal on its own
/// and the database's one-hop constraint is satisfied at every step.
///
/// **Seen red, 2026-09-08**: removing the `refuse_a_cycle` call in
/// `service::menu::update_menu`.
#[tokio::test]
async fn a_ring_of_three_is_refused_where_the_check_constraint_cannot_see_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let a = entry(&app, &token, "a", None).await;
    let b = entry(&app, &token, "b", Some(a)).await;
    let c = entry(&app, &token, "c", Some(b)).await;

    let closed = update(&app, &token, a, json!({ "parentMenuId": c })).await;

    assert_eq!(
        closed.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a ring of three was accepted: {}",
        closed.body
    );
    assert!(codes(&closed.body).contains(&"MENU_WOULD_CYCLE".to_owned()));

    // And nothing moved: `a` is still a root.
    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/menus/{a}"),
            Some(&token),
            None,
        )
        .await;

    assert!(read.body["data"]["parentMenuId"].is_null(), "{}", read.body);
}

/// **A walk that cannot see the whole path refuses, rather than passing on the
/// part it saw.**
///
/// The bound exists so the recursive term terminates on a tree that is already
/// cyclic — and on such a tree the walk returns a *partial* ancestry. A partial
/// ancestry that happens not to contain `id` would let the check that exists to
/// refuse a cycle allow one, which is the bound creating the corruption it was
/// added to survive.
///
/// **The ring is written directly, because the service refuses to make one.**
/// That is the point: the guard has to hold for a tree the service did not
/// write — a plugin, a restore, a migration, or the state that existed before
/// this walk did.
#[tokio::test]
async fn an_ancestry_that_cannot_be_read_to_the_root_is_refused_rather_than_guessed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let a = entry(&app, &token, "a", None).await;
    let b = entry(&app, &token, "b", Some(a)).await;
    let loose = entry(&app, &token, "loose", None).await;

    // Close `a` → `b` → `a` behind the service's back. The `CHECK` permits it:
    // neither row names itself.
    sqlx::query("UPDATE rad_menus SET parent_menu_id = $1 WHERE id = $2")
        .bind(b)
        .bind(a)
        .execute(&app.pool)
        .await
        .expect("the ring is closed directly");

    // `loose` is nowhere near the ring, and moving it under `b` closes no loop.
    // It is refused anyway, because the walk up from `b` never reaches a root
    // and the service will not decide on a path it could not finish reading.
    let refused = update(&app, &token, loose, json!({ "parentMenuId": b })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a re-parent under an unreadable ancestry was accepted: {}",
        refused.body
    );
    assert!(
        codes(&refused.body).contains(&"MENU_TREE_TOO_DEEP".to_owned()),
        "{}",
        refused.body
    );
}

/// **The paired assertion**: an ancestry that *is* readable to the root does not
/// trip the bound. Without it, a walk that reported every tree as truncated
/// would pass the test above.
///
/// **The chain is exactly `MAX_MENU_DEPTH` long, and that is the whole point.**
/// A shallow tree cannot tell *the walk ran out of budget* from *the walk
/// reached the bound and finished there*, because `depth >= max_depth` is never
/// true in it. Truncation is `depth >= max_depth` **and** the deepest row still
/// naming a parent, and only a chain standing on the bound exercises the second
/// half — a mutation campaign found this one green against a chain of eight.
#[tokio::test]
async fn an_ancestry_that_reaches_the_root_on_the_last_step_is_not_truncated() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut parent = None;

    // `menu_ancestors` counts the proposed parent as depth 1, so a chain of
    // MAX_MENU_DEPTH puts the root on the last row the walk is allowed to read.
    for level in 0..32 {
        parent = Some(entry(&app, &token, &format!("n{level}"), parent).await);
    }

    let loose = entry(&app, &token, "loose", None).await;
    let moved = update(&app, &token, loose, json!({ "parentMenuId": parent })).await;

    assert_eq!(
        moved.status,
        StatusCode::OK,
        "an ancestry that ends at a root on its last readable step was refused: {}",
        moved.body
    );
}

/// The one-hop case, which the database also refuses — asserted here because a
/// constraint violation would surface as a 500 and this is a 422 naming the
/// field.
#[tokio::test]
async fn an_entry_cannot_be_its_own_parent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let a = entry(&app, &token, "a", None).await;
    let refused = update(&app, &token, a, json!({ "parentMenuId": a })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(codes(&refused.body).contains(&"MENU_WOULD_CYCLE".to_owned()));
}

/// **The move that is legal**, which is what makes the two above about cycles
/// rather than about refusing every reparent.
#[tokio::test]
async fn a_reparent_that_closes_no_loop_is_allowed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let a = entry(&app, &token, "a", None).await;
    let b = entry(&app, &token, "b", None).await;
    let c = entry(&app, &token, "c", Some(a)).await;

    // `c` moves from under `a` to under `b`. Neither is below the other.
    let moved = update(&app, &token, c, json!({ "parentMenuId": b })).await;

    assert_eq!(moved.status, StatusCode::OK, "{}", moved.body);
    assert_eq!(moved.body["data"]["parentMenuId"], b.to_string());
}

#[tokio::test]
async fn clearing_a_parent_makes_an_entry_a_root() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let parent = entry(&app, &token, "operations", None).await;
    let child = entry(&app, &token, "purchasing", Some(parent)).await;

    let cleared = update(&app, &token, child, json!({ "parentMenuId": null })).await;

    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert!(cleared.body["data"]["parentMenuId"].is_null());
}

#[tokio::test]
async fn a_parent_this_tenant_does_not_have_is_refused_by_name() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create(
        &app,
        &token,
        json!({
            "menuKey": "orphan", "label": "Orphan", "routePath": "/documents",
            "parentMenuId": Uuid::now_v7(),
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(codes(&created.body).contains(&"PARENT_NOT_FOUND".to_owned()));
}

/// **A deleted heading lifts its children rather than orphaning them.** A child
/// whose parent is soft-deleted is live and unreachable — a menu entry that
/// exists and cannot be found.
#[tokio::test]
async fn deleting_a_heading_moves_its_children_up_rather_than_stranding_them() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let top = entry(&app, &token, "top", None).await;
    let middle = entry(&app, &token, "middle", Some(top)).await;
    let leaf = entry(&app, &token, "leaf", Some(middle)).await;

    let removed = app
        .send(
            Method::DELETE,
            &format!("/api/v1/rad/menus/{middle}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);

    let read = app
        .send(
            Method::GET,
            &format!("/api/v1/rad/menus/{leaf}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        read.body["data"]["parentMenuId"],
        top.to_string(),
        "the child should have moved up to its grandparent: {}",
        read.body
    );
    assert_eq!(keys(&list(&app, &token).await.body).len(), 2);
}

// ---------------------------------------------------------------------------
// The permissions, and the tenant
// ---------------------------------------------------------------------------

#[tokio::test]
async fn each_menu_route_requires_its_own_permission() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let target = entry(&app, &admin, "target", None).await;

    // A caller holding read and nothing else.
    let reader = caller_holding(&app, &["rad:menu:read"], 1).await;

    assert_eq!(list(&app, &reader).await.status, StatusCode::OK);
    assert_eq!(
        create(
            &app,
            &reader,
            json!({ "menuKey": "nope", "label": "Nope", "routePath": "/documents" })
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        update(&app, &reader, target, json!({ "label": "Renamed" }))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.send(
            Method::DELETE,
            &format!("/api/v1/rad/menus/{target}"),
            Some(&reader),
            None
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // And a caller with none of them cannot even read — without this half, a
    // route that checked nothing would pass every assertion above.
    let outsider = caller_holding(&app, &["document:read"], 2).await;

    assert_eq!(list(&app, &outsider).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_menu_in_another_tenant_is_not_listed_or_readable_here() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    entry(&app, &token, "mine", None).await;

    // The second subject: one tenant cannot tell *scoped by tenant* from
    // *unscoped* (coding standard §2.9). Seeded directly, because the harness
    // runs single-tenant and a foreign user could not sign in.
    let other = fixtures::create_tenant(&app.pool, "TNT-MENUS", "Another Customer").await;
    let theirs = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO rad_menus (id, tenant_id, menu_key, label, route_path, source,
                                sort_order, is_enabled)
         VALUES ($1, $2, 'theirs', 'Theirs', '/documents', 'CONFIG', 0, true)",
    )
    .bind(theirs)
    .bind(other)
    .execute(&app.pool)
    .await
    .expect("the other tenant's entry is seeded");

    assert_eq!(keys(&list(&app, &token).await.body), ["mine"]);
    assert_eq!(
        app.send(
            Method::GET,
            &format!("/api/v1/rad/menus/{theirs}"),
            Some(&token),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}

/// The builder's read shows what the sidebar would hide, which is the whole
/// difference between the two: an administrator has to see the entry they
/// switched off.
#[tokio::test]
async fn the_builders_read_shows_a_disabled_entry() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = entry(&app, &token, "hidden", None).await;

    assert_eq!(
        update(&app, &token, id, json!({ "isEnabled": false }))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(keys(&list(&app, &token).await.body), ["hidden"]);
}
