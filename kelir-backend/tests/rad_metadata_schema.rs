//! Properties of the RAD metadata schema that no single query would notice
//! (#155, `0014_rad.sql`).
//!
//! These assert the schema rather than a route — there is no RAD route until
//! #156 — and each one is a property the *next* table added to this group has
//! to satisfy too. So every test here **discovers its subjects** from
//! `information_schema` rather than listing them, as sprint plan §5
//! verification rule 6 requires: an enumerating test fails the way its list
//! fails, which is silently, and a `rad_` table added in Sprint 8 without a
//! `tenant_id` is exactly the thing worth catching.

mod common;

use common::{fixtures, TestApp};

/// Database Schema §1.2. Every table carries these unless its section says
/// otherwise, and no section of §5 says otherwise — none of the RAD tables is
/// append-only.
const BASE_COLUMNS: [&str; 7] = [
    "id",
    "tenant_id",
    "created_by",
    "created_at",
    "updated_by",
    "updated_at",
    "deleted_at",
];

/// The tables `0014_rad.sql` created, as the database reports them.
async fn rad_tables(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name LIKE 'rad\\_%'
         ORDER BY table_name",
    )
    .fetch_all(&app.pool)
    .await
    .expect("information_schema is queryable")
}

#[tokio::test]
async fn the_rad_group_exists() {
    let app = TestApp::spawn().await;
    let tables = rad_tables(&app).await;

    // Twelve, per Database Schema §5.1-5.12. Asserted as a floor rather than an
    // equality so a thirteenth table does not fail a test about the twelve —
    // the properties below are what a new table has to satisfy.
    assert!(
        tables.len() >= 12,
        "0014_rad.sql creates the twelve tables of §5.1-5.12; found {}: {tables:?}",
        tables.len()
    );

    // The one this migration exists to guarantee: `document_types.form_id`
    // references `rad_forms` (§6.2), so the document migration cannot add its
    // foreign key unless this table is already here (#155 AC2).
    assert!(
        tables.iter().any(|table| table == "rad_forms"),
        "rad_forms must exist before the document migration; found {tables:?}"
    );
}

#[tokio::test]
async fn every_rad_table_carries_the_base_columns() {
    let app = TestApp::spawn().await;
    let tables = rad_tables(&app).await;

    assert!(
        !tables.is_empty(),
        "no rad_ tables — the migration did not run"
    );

    let mut missing = Vec::new();

    for table in &tables {
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns
             WHERE table_schema = 'public' AND table_name = $1",
        )
        .bind(table)
        .fetch_all(&app.pool)
        .await
        .expect("information_schema is queryable");

        for base in BASE_COLUMNS {
            if !columns.iter().any(|column| column == base) {
                missing.push(format!("  {table}.{base}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Database Schema §1.2 requires the base column set on every table in this group.\n\
         Missing:\n{}",
        missing.join("\n")
    );
}

#[tokio::test]
async fn every_rad_table_is_tenant_scoped_by_a_foreign_key() {
    // `tenant_id` existing is not the same as `tenant_id` meaning something. A
    // column with no reference admits any UUID, and a row in a tenant that does
    // not exist is invisible to every tenant-filtered query — a leak in the
    // direction of disappearance rather than disclosure, which is worse to
    // diagnose.
    let app = TestApp::spawn().await;
    let tables = rad_tables(&app).await;

    let constrained: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT source.relname::text
         FROM pg_constraint c
         JOIN pg_class source ON source.oid = c.conrelid
         JOIN pg_class target ON target.oid = c.confrelid
         JOIN pg_attribute a
           ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey)
         WHERE c.contype = 'f'
           AND target.relname = 'tenants'
           AND a.attname = 'tenant_id'
           AND source.relname LIKE 'rad\\_%'",
    )
    .fetch_all(&app.pool)
    .await
    .expect("pg_constraint is queryable");

    let unconstrained: Vec<&String> = tables
        .iter()
        .filter(|table| !constrained.contains(table))
        .collect();

    assert!(
        unconstrained.is_empty(),
        "every RAD table's tenant_id must reference tenants (id); these do not: {unconstrained:?}"
    );
}

#[tokio::test]
async fn a_published_form_must_carry_its_publication_stamp() {
    // The constraint the immutability rule keys on. A PUBLISHED row with a null
    // `published_at` would be editable by any code that asks "was this
    // published?" the obvious way, and a document pinning that revision would
    // re-render years later against a definition that had moved underneath it.
    let app = TestApp::spawn().await;

    let refused = sqlx::query(
        "INSERT INTO rad_forms
             (id, tenant_id, form_key, title, jfss_version, definition_json, status)
         VALUES (gen_random_uuid(), $1, 'unstamped', 'Unstamped', '2.0.1', '{}'::jsonb, 'PUBLISHED')",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await;

    assert!(
        refused.is_err(),
        "a PUBLISHED form with no published_at must be refused"
    );

    // And the other direction: a draft that claims a publication stamp.
    let also_refused = sqlx::query(
        "INSERT INTO rad_forms
             (id, tenant_id, form_key, title, jfss_version, definition_json, status, published_at)
         VALUES (gen_random_uuid(), $1, 'draft-stamped', 'Draft', '2.0.1', '{}'::jsonb, 'DRAFT', now())",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await;

    assert!(
        also_refused.is_err(),
        "a DRAFT form carrying a published_at must be refused"
    );
}

#[tokio::test]
async fn a_menu_cannot_be_its_own_parent() {
    // The one hop of the cycle problem a constraint can express. A ring of
    // three still needs the ancestor walk #141 built for facilities, which is
    // #191 and belongs with the surface that writes this column.
    let app = TestApp::spawn().await;

    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO rad_menus (id, tenant_id, menu_key, label)
         VALUES (gen_random_uuid(), $1, 'self-parent', 'Self') RETURNING id",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .fetch_one(&app.pool)
    .await
    .expect("a menu inserts");

    let refused = sqlx::query("UPDATE rad_menus SET parent_menu_id = id WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await;

    assert!(refused.is_err(), "a menu must not be its own parent");
}

#[tokio::test]
async fn a_list_page_size_is_bounded() {
    // Zero pages forever, and the upper bound matches the pagination cap the
    // API already enforces (FR-API-006). Both are storage-level because the
    // list definition is configuration: whoever writes it is not necessarily
    // going through a form that would have validated it.
    let app = TestApp::spawn().await;

    for page_size in [0, -1, 101] {
        let refused = sqlx::query(
            "INSERT INTO rad_lists (id, tenant_id, list_key, title, page_size)
             VALUES (gen_random_uuid(), $1, $2, 'Bounded', $3)",
        )
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(format!("bounded-{page_size}"))
        .bind(page_size)
        .execute(&app.pool)
        .await;

        assert!(refused.is_err(), "page_size {page_size} must be refused");
    }

    let accepted = sqlx::query(
        "INSERT INTO rad_lists (id, tenant_id, list_key, title, page_size)
         VALUES (gen_random_uuid(), $1, 'bounded-ok', 'Bounded', 20)",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await;

    assert!(accepted.is_ok(), "a page size of 20 must be accepted");
}
