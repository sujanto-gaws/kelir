//! A document's link to a master-data entity (#170).
//!
//! **The question is #161's and so is the answer.** A lookup field and a
//! document link are the same question wearing different clothes: *can a caller
//! who may read documents thereby read master data they could not read
//! directly?* #161 answered by holding no permission logic at all — it asks the
//! master-data module for the same record its own endpoint serves, and that
//! module refuses first. The resolution here does the same, and the test that
//! matters is the one that would go green if it stopped.

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A document type with no form, which is all this file needs: the link is a
/// property of the document rather than of its payload.
async fn plain_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// A party, made the way a person would.
async fn party(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/master-data/parties",
            Some(token),
            Some(json!({
                "partyId": code,
                "partyTypeId": "PARTY_GROUP",
                "partyGroup": { "groupName": format!("{code} Supplies") },
            })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn create(app: &TestApp, token: &str, body: Value) -> common::TestResponse {
    app.send(Method::POST, "/api/v1/documents", Some(token), Some(body))
        .await
}

async fn linked_entity(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.send(
        Method::GET,
        &format!("/api/v1/documents/{id}/linked-entity"),
        Some(token),
        None,
    )
    .await
}

// ---------------------------------------------------------------------------
// AC1 — the pair, recorded rather than inferred
// ---------------------------------------------------------------------------

/// **A link is both halves or neither.**
///
/// A bare id that could mean a party or a facility is a bug waiting for the
/// second entity type, and Kelir already has the second entity type.
#[tokio::test]
async fn a_link_needs_both_of_its_halves() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_PAIR").await;
    let supplier = party(&app, &token, "LINK-PAIR-1").await;

    for body in [
        json!({ "documentTypeId": type_id, "title": "Half a link", "entityType": "PARTY" }),
        json!({ "documentTypeId": type_id, "title": "Half a link", "entityId": supplier }),
    ] {
        let refused = create(&app, &token, body).await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "half a link was accepted: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["code"], "INCOMPLETE_ENTITY_LINK",
            "{}",
            refused.body
        );
    }

    // And the whole thing is accepted, so the assertions above are not green
    // because the endpoint refuses every link.
    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "A whole link",
            "entityType": "PARTY",
            "entityId": supplier,
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["entityType"], "PARTY");
    assert_eq!(created.body["data"]["entityId"], json!(supplier));
}

// ---------------------------------------------------------------------------
// AC2, AC3 — the permission, delegated
// ---------------------------------------------------------------------------

/// **Reading a document hands back no master-data field, and resolving the link
/// requires the entity's own read permission** (AC2, AC3).
///
/// The caller below holds every document permission and **not**
/// `master-data:party:read`. They get the document, they see that it concerns a
/// party, and they get 403 from the resolution — which is #161's choice between
/// refusing and answering empty, taken the same way and for its second reason:
/// refusing leaks nothing they do not already hold, because they read the
/// identifier off the document itself.
///
/// **Seen red** (coding standard §2.9) against a build where
/// `service::link::resolve_link` reads the party through
/// `master_data::repository` instead of through `master_data::service`: the
/// caller is handed the supplier's name and code.
#[tokio::test]
async fn a_document_does_not_open_what_the_master_data_surface_does_not() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_PERMISSION").await;
    let supplier = party(&app, &token, "LINK-PERM-1").await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Concerns a supplier",
            "entityType": "PARTY",
            "entityId": supplier,
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    // Every document permission, and nothing over master data. The document
    // permissions matter: without them the refusal would come from the
    // *document* check and the mutation beneath it would report coverage that
    // does not exist — the gate §2.9 describes.
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "DOC-NO-MASTER-DATA",
        &["document:read", "document:create", "document:update"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "doc.no.masterdata",
        "doc.no.masterdata@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let caller = app
        .sign_in("doc.no.masterdata", common::ADMIN_PASSWORD)
        .await;

    // The document is readable, and carries the identifiers and nothing else.
    let document = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}"),
            Some(&caller),
            None,
        )
        .await;

    assert_eq!(document.status, StatusCode::OK, "{}", document.body);
    assert_eq!(document.body["data"]["entityId"], json!(supplier));

    let serialized = document.body.to_string();
    assert!(
        !serialized.contains("LINK-PERM-1") && !serialized.contains("Supplies"),
        "a document handed back master-data fields: {serialized}"
    );

    // And the resolution refuses, in the master-data module's own words.
    let refused = linked_entity(&app, &caller, id).await;

    assert_eq!(
        refused.status,
        StatusCode::FORBIDDEN,
        "a caller without master-data:party:read resolved a supplier through a \
         document: {}",
        refused.body
    );
}

/// A caller who *does* hold the entity's read permission gets the record.
///
/// The other side of the assertion above, and what stops it being green because
/// the endpoint refuses everybody.
#[tokio::test]
async fn a_caller_who_may_read_the_party_resolves_the_link() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_RESOLVE").await;
    let supplier = party(&app, &token, "LINK-RESOLVE-1").await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Concerns a supplier",
            "entityType": "PARTY",
            "entityId": supplier,
        }),
    )
    .await;
    let id = id_of(&created.body["data"]);

    let resolved = linked_entity(&app, &token, id).await;

    assert_eq!(resolved.status, StatusCode::OK, "{}", resolved.body);
    assert_eq!(resolved.body["data"]["entityType"], "PARTY");
    assert_eq!(resolved.body["data"]["code"], "LINK-RESOLVE-1");
    assert_eq!(resolved.body["data"]["name"], "LINK-RESOLVE-1 Supplies");
}

// ---------------------------------------------------------------------------
// AC4 — the entity has to exist when the link is written
// ---------------------------------------------------------------------------

/// **Linking to a record that does not exist, or is soft-deleted, is refused.**
///
/// `documents.entity_id` carries no foreign key — the thing it points at is
/// polymorphic — so this check *is* the constraint, which is why it runs in the
/// write's transaction under a lock on the row it read.
///
/// **Seen red** against `repository::link::lock_linked_entity`'s
/// `deleted_at IS NULL` weakened to `(deleted_at IS NULL OR TRUE)`: the document
/// is created against a retired supplier.
#[tokio::test]
async fn a_link_to_a_record_that_is_gone_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_MISSING").await;

    // Never existed.
    let refused = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Concerns nothing",
            "entityType": "PARTY",
            "entityId": Uuid::now_v7(),
        }),
    )
    .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "entityId");

    // Existed and was retired.
    let retired = party(&app, &token, "LINK-RETIRED-1").await;
    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/master-data/parties/{retired}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let refused = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Concerns a retired supplier",
            "entityType": "PARTY",
            "entityId": retired,
        }),
    )
    .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a document was linked to a retired supplier: {}",
        refused.body
    );
}

/// **A document linked to a record that is later retired stays readable**
/// (AC5).
///
/// The decision the AC asks for rather than a default: nothing cascades and
/// nothing is nulled. A purchase order that concerned supplier X still concerned
/// supplier X, and rewriting a historical record because a supplier was retired
/// years later would be worse than a link that points at something retired.
/// What the caller sees is a 404 naming the **entity**, which is a true
/// statement — the document is fine and the thing it points at is gone.
#[tokio::test]
async fn retiring_the_linked_record_leaves_the_document_readable() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_ORPHAN").await;
    let supplier = party(&app, &token, "LINK-ORPHAN-1").await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Concerns a supplier that will be retired",
            "entityType": "PARTY",
            "entityId": supplier,
        }),
    )
    .await;
    let id = id_of(&created.body["data"]);

    let deleted = app
        .send(
            Method::DELETE,
            &format!("/api/v1/master-data/parties/{supplier}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let document = app
        .send(
            Method::GET,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            None,
        )
        .await;

    assert_eq!(
        document.status,
        StatusCode::OK,
        "retiring a supplier made a document unreadable: {}",
        document.body
    );
    assert_eq!(
        document.body["data"]["entityId"],
        json!(supplier),
        "retiring a supplier rewrote a document's history"
    );

    let resolved = linked_entity(&app, &token, id).await;

    assert_eq!(
        resolved.status,
        StatusCode::NOT_FOUND,
        "a retired record resolved: {}",
        resolved.body
    );
}

/// A document with no link answers 404 from the resolution, about the link.
#[tokio::test]
async fn a_document_with_no_link_has_nothing_to_resolve() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_NONE").await;

    let created = create(
        &app,
        &token,
        json!({ "documentTypeId": type_id, "title": "Concerns nobody" }),
    )
    .await;
    let id = id_of(&created.body["data"]);

    let resolved = linked_entity(&app, &token, id).await;

    assert_eq!(resolved.status, StatusCode::NOT_FOUND, "{}", resolved.body);
}

/// **The link is cleared by sending both halves as null, and left alone by
/// sending neither.**
///
/// The distinction `present_or_absent` exists for: `entityId: null` and no
/// `entityId` at all are different instructions, and a request shape that could
/// not tell them apart would make "remove this link" unexpressible.
#[tokio::test]
async fn a_link_is_cleared_by_sending_both_halves_as_null() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = plain_type(&app, &token, "PR_LINK_CLEAR").await;
    let supplier = party(&app, &token, "LINK-CLEAR-1").await;

    let created = create(
        &app,
        &token,
        json!({
            "documentTypeId": type_id,
            "title": "Linked for now",
            "entityType": "PARTY",
            "entityId": supplier,
        }),
    )
    .await;
    let id = id_of(&created.body["data"]);

    // Absent: untouched.
    let renamed = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "title": "Renamed" })),
        )
        .await;

    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(renamed.body["data"]["entityId"], json!(supplier));

    // Both null: cleared.
    let cleared = app
        .send(
            Method::PUT,
            &format!("/api/v1/documents/{id}"),
            Some(&token),
            Some(json!({ "entityType": null, "entityId": null })),
        )
        .await;

    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert_eq!(cleared.body["data"]["entityType"], Value::Null);
    assert_eq!(cleared.body["data"]["entityId"], Value::Null);
}
