//! A row belonging to another tenant does not render, however it got there (#108).
//!
//! `repository/mod.rs` opened with "every query filters by `tenant_id` and
//! excludes soft-deleted rows". A reader auditing the module believed it. It was
//! true of the base tables and false of the **joins**: `list_relationships` and
//! `list_party_roles` filtered `mdm_party_relationships` / `mdm_party_roles` and
//! then joined `mdm_parties` and `mdm_role_types` with no tenant predicate, so a
//! cross-tenant row present in storage would render another tenant's
//! `party_code` inside `GET /parties/{mine}`. `list_statuses`,
//! `list_contact_mechs`, `list_parties` and the three profile joins had the same
//! shape.
//!
//! **Every fixture here is seeded with direct SQL, and that is the point rather
//! than a shortcut.** The API cannot produce these rows: `resolve_relationships`
//! and `resolve_party_reference` both resolve through the tenant-scoped
//! `find_party_id_by_code`, so there is no request that creates one. That is
//! what made the defect latent — and what makes it untestable through the
//! routes. One bulk import or one admin script is the whole distance between
//! latent and live, so the predicate is the fix and this is how it gets covered.
//!
//! Each test names one join. A single test over one route would be the gate
//! §2.9 warns about: the first predicate to be restored would absorb every
//! mutation below it and the rest would look covered.

mod common;

use axum::http::StatusCode;
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

const PARTIES: &str = "/api/v1/master-data/parties";

/// A code no legitimate fixture uses, so finding it anywhere in a response is
/// unambiguous.
const FOREIGN: &str = "SECRET-COMPETITOR";

async fn create_party(app: &TestApp, token: &str, body: Value) -> Uuid {
    let response = app.post(PARTIES, Some(token), body).await;

    assert_eq!(
        response.status,
        StatusCode::CREATED,
        "create refused: {}",
        response.body
    );

    response.data()["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("the created party carries an id")
}

fn group(party_code: &str, name: &str) -> Value {
    json!({
        "partyId": party_code,
        "partyTypeId": "PARTY_GROUP",
        "partyGroup": { "groupName": name },
    })
}

/// Another tenant, and a party inside it carrying [`FOREIGN`] as its code.
async fn foreign_tenant_with_a_party(pool: &PgPool, code: &str) -> (Uuid, Uuid) {
    let tenant = fixtures::create_tenant(pool, code, "Other").await;
    let party = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_parties (id, tenant_id, party_code, party_type)
         VALUES ($1, $2, $3, 'PARTY_GROUP')",
    )
    .bind(party)
    .bind(tenant)
    .bind(FOREIGN)
    .execute(pool)
    .await
    .expect("insert the other tenant's party");

    (tenant, party)
}

/// The whole aggregate as the API renders it, as a string to search.
async fn rendered(app: &TestApp, token: &str, party: Uuid) -> String {
    let response = app.get(&format!("{PARTIES}/{party}"), Some(token)).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body.to_string()
}

#[tokio::test]
async fn a_relationship_to_another_tenants_party_does_not_render_its_code() {
    // `list_relationships` — the sharpest of the seven, because what leaks is a
    // business identifier rendered inside the caller's own aggregate.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(&app, &token, group("PARTY-MINE", "Mine")).await;
    let (_, theirs) = foreign_tenant_with_a_party(&app.pool, "TNT-REL").await;

    sqlx::query(
        "INSERT INTO mdm_party_relationships
             (id, tenant_id, from_party_id, to_party_id, relationship_type, starts_at)
         VALUES ($1, (SELECT tenant_id FROM mdm_parties WHERE id = $2), $2, $3,
                 'ORGANIZATION_ROLLUP', now())",
    )
    .bind(Uuid::now_v7())
    .bind(mine)
    .bind(theirs)
    .execute(&app.pool)
    .await
    .expect("seed the cross-tenant relationship");

    let body = rendered(&app, &token, mine).await;

    assert!(
        !body.contains(FOREIGN),
        "GET /parties/{{mine}} rendered another tenant's party code: {body}"
    );
}

#[tokio::test]
async fn a_role_typed_by_another_tenant_does_not_render_its_code() {
    // `list_party_roles`. The leak is a `role_type_code`, which is a smaller
    // secret than a party code and reaches the same aggregate.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(&app, &token, group("PARTY-MINE", "Mine")).await;
    let tenant = fixtures::create_tenant(&app.pool, "TNT-ROLE", "Other").await;
    let role_type = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_role_types (id, tenant_id, role_type_code, name, is_system)
         VALUES ($1, $2, $3, $3, false)",
    )
    .bind(role_type)
    .bind(tenant)
    .bind(FOREIGN)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's role type");

    sqlx::query(
        "INSERT INTO mdm_party_roles (id, tenant_id, party_id, role_type_id, starts_at)
         VALUES ($1, (SELECT tenant_id FROM mdm_parties WHERE id = $2), $2, $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(mine)
    .bind(role_type)
    .execute(&app.pool)
    .await
    .expect("seed the role typed by another tenant");

    let response = app
        .get(&format!("{PARTIES}/{mine}/roles"), Some(&token))
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert!(
        !response.body.to_string().contains(FOREIGN),
        "GET /parties/{{mine}}/roles rendered another tenant's role type: {}",
        response.body
    );
}

#[tokio::test]
async fn a_status_changed_by_another_tenants_user_does_not_render_their_username() {
    // `list_statuses`' `LEFT JOIN users`. A username is an identity, and this is
    // the only join in the module that reaches outside master data.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(&app, &token, group("PARTY-MINE", "Mine")).await;
    let tenant = fixtures::create_tenant(&app.pool, "TNT-USER", "Other").await;

    let user = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (id, tenant_id, username, email, password_hash, display_name, status)
         VALUES ($1, $2, $3, 'foreign@example.test', 'x', 'Foreign', 'ACTIVE')
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(FOREIGN)
    .fetch_one(&app.pool)
    .await
    .expect("insert the other tenant's user");

    sqlx::query(
        "INSERT INTO mdm_party_statuses (id, tenant_id, party_id, status, changed_by, status_at)
         VALUES ($1, (SELECT tenant_id FROM mdm_parties WHERE id = $2), $2, 'PARTY_DISABLED',
                 $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(mine)
    .bind(user)
    .execute(&app.pool)
    .await
    .expect("seed the status changed by another tenant's user");

    let body = rendered(&app, &token, mine).await;

    assert!(
        !body.contains(FOREIGN),
        "GET /parties/{{mine}} rendered another tenant's username: {body}"
    );
}

#[tokio::test]
async fn a_contact_mechanism_owned_by_another_tenant_does_not_render() {
    // `list_contact_mechs`. The link is the caller's; the mechanism behind it is
    // not, and `detail_json` is where the address or the number lives.
    //
    // **The marker goes in `detail_json`, not in `display_value`.** A first
    // version of this test put it in `display_value` and came back green under
    // the mutation — `list_contact_mechs` does not select that column, so
    // nothing distinctive was ever on the wire and the test was reporting on
    // nothing (§2.9). Recorded because the next reader will reach for the
    // obvious column too.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(&app, &token, group("PARTY-MINE", "Mine")).await;
    let tenant = fixtures::create_tenant(&app.pool, "TNT-MECH", "Other").await;
    let mechanism = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO mdm_contact_mechs (id, tenant_id, contact_mech_type, display_value, detail_json)
         VALUES ($1, $2, 'EMAIL_ADDRESS', $3, jsonb_build_object('emailAddress', $3::text))",
    )
    .bind(mechanism)
    .bind(tenant)
    .bind(FOREIGN)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's contact mechanism");

    sqlx::query(
        "INSERT INTO mdm_party_contact_mechs (id, tenant_id, party_id, contact_mech_id, starts_at)
         VALUES ($1, (SELECT tenant_id FROM mdm_parties WHERE id = $2), $2, $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(mine)
    .bind(mechanism)
    .execute(&app.pool)
    .await
    .expect("seed the link to another tenant's mechanism");

    let body = rendered(&app, &token, mine).await;

    assert!(
        !body.contains(FOREIGN),
        "GET /parties/{{mine}} rendered another tenant's contact mechanism: {body}"
    );
}

#[tokio::test]
async fn a_billing_party_in_another_tenant_does_not_render_its_code() {
    // `find_customer_profile`'s `LEFT JOIN mdm_parties b`, and by construction
    // the same shape as `find_employee_profile`'s manager and
    // `find_contact_profile`'s assistant. One is asserted rather than three
    // because the three statements are identical apart from the column name —
    // and the boundary is stated here rather than implied.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(&app, &token, group("PARTY-MINE", "Mine")).await;
    let (_, theirs) = foreign_tenant_with_a_party(&app.pool, "TNT-BILL").await;

    let assigned = app
        .put(
            &format!("{PARTIES}/{mine}/roles/CUSTOMER"),
            Some(&token),
            json!({
                "fromDate": "2026-01-01T00:00:00Z",
                "profile": { "customer": { "customerNumber": "CUS-0001" } },
            }),
        )
        .await;
    assert_eq!(assigned.status, StatusCode::CREATED, "{}", assigned.body);

    // The API refuses to point this at another tenant's party, which is why it
    // is written directly.
    sqlx::query("UPDATE mdm_customer_profiles SET billing_party_id = $2 WHERE party_id = $1")
        .bind(mine)
        .bind(theirs)
        .execute(&app.pool)
        .await
        .expect("seed the cross-tenant billing reference");

    let response = app
        .get(&format!("{PARTIES}/{mine}/roles"), Some(&token))
        .await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert!(
        !response.body.to_string().contains(FOREIGN),
        "the customer profile rendered another tenant's party code: {}",
        response.body
    );
    assert_eq!(
        response.data()["profiles"]["customer"]["billingPartyId"],
        Value::Null,
        "a reference that cannot be resolved in this tenant should read as absent: {}",
        response.body
    );
}

#[tokio::test]
async fn an_extension_row_owned_by_another_tenant_does_not_name_the_party_in_a_list() {
    // `list_parties`' two extension joins. A party's name in a list is derived
    // from `mdm_party_groups` / `mdm_persons`, so an extension row another
    // tenant owns renames the caller's party in their own list.
    //
    // **A group row over a PERSON party, and that pairing is load-bearing.** The
    // name is `COALESCE(g.group_name, <the person's names>, p.party_code)`, so a
    // foreign *person* row on a party that already has a group name never wins
    // the COALESCE and the mutation came back green — the fixture, not the
    // predicate, was what the first version of this test was measuring (§2.9).
    // Taking the branch that is preferred is what makes the leak visible.
    //
    // **`mdm_persons`' own predicate is not reachable, and is kept anyway.**
    // `uq_mdm_persons_party_id` and `uq_mdm_party_groups_party_id` are unique on
    // `party_id` alone, across tenants — so a foreign extension row can exist
    // only where the party has none of its own, and a party that has no person
    // row is a `PARTY_GROUP`, whose `group_name` wins the COALESCE ahead of any
    // person. There is no fixture that makes dropping `pe.tenant_id` change an
    // answer. The predicate stays because the two joins are one statement and a
    // reader should not have to work out which half of it is load-bearing; this
    // note is the boundary the sweep is required to state (§2.9).
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mine = create_party(
        &app,
        &token,
        json!({
            "partyId": "PARTY-MINE",
            "partyTypeId": "PERSON",
            "person": { "firstName": "Ana", "lastName": "Prawira" },
        }),
    )
    .await;
    let tenant = fixtures::create_tenant(&app.pool, "TNT-PERS", "Other").await;

    sqlx::query(
        "INSERT INTO mdm_party_groups (id, tenant_id, party_id, group_name)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(tenant)
    .bind(mine)
    .bind(FOREIGN)
    .execute(&app.pool)
    .await
    .expect("insert the other tenant's group row");

    let response = app.get(PARTIES, Some(&token)).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert!(
        !response.body.to_string().contains(FOREIGN),
        "the party list took its name from another tenant's row: {}",
        response.body
    );
}
