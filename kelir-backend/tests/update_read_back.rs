//! An update a delete follows still answers 200 and writes its audit row
//! ([#787]).
//!
//! Each of these updates committed and then read its row back on the pool, for
//! its answer and (in most) its audit row. A delete that committed in between
//! left that read with nothing, and the request answered 500 or 404 for a
//! change that had committed. Most wrote no audit row either, since the audit
//! came after the read. Each now reads back inside its transaction, before its
//! commit, as #786 made workflow definitions do ([#785]).
//!
//! [`ReadGate`] holds the gap open: it stops the update's read after its commit,
//! and only that read, until the delete has answered (`common::gate`). One test
//! per update, each staged the same way by [`raced`], and one of the gate itself
//! (`the_read_gate_drops_a_row_deleted_while_its_read_waited`).
//!
//! [#785]: https://github.com/sujanto-gaws/kelir/issues/785
//! [#787]: https://github.com/sujanto-gaws/kelir/issues/787

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::gate::{a_delete_follows, ReadGate, PATIENCE};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The value every update writes, and the gate's predicate looks for.
const RENAMED: &str = "Renamed before the delete";

/// One update and the delete that follows it.
struct Case<'a> {
    /// The table the row is in, which the gate stands in for.
    table: &'a str,
    /// The row.
    id: Uuid,
    /// The column the update writes [`RENAMED`] (or `written`) to.
    column: &'a str,
    /// What the update writes, when it is not [`RENAMED`].
    written: &'a str,
    update: (Method, String, Option<Value>),
    delete: String,
    /// The field of the update's `data` that shows the change.
    answered: &'a str,
    /// A child row the update writes in the same transaction, as a JSON
    /// pointer into the answer and the value it must hold there. The read back
    /// before the commit has to come after that write too: read before it, or
    /// on another connection, it answers the children as they were.
    child: Option<(&'a str, Value)>,
    /// The update's audit `event_type`.
    event: &'a str,
}

impl<'a> Case<'a> {
    fn new(table: &'a str, id: Uuid, column: &'a str) -> Self {
        Case {
            table,
            id,
            column,
            written: RENAMED,
            update: (Method::PUT, String::new(), None),
            delete: String::new(),
            answered: "",
            child: None,
            event: "",
        }
    }
}

/// Runs `case`'s race and asserts what it is for: the delete deleted, the
/// update answered 200 with what it wrote and wrote one audit row, and what it
/// wrote committed.
async fn raced(app: &Arc<TestApp>, token: &str, case: Case<'_>) {
    let reads = ReadGate::close(
        app,
        case.table,
        &format!("r.{} = '{}'", case.column, case.written),
        "r.deleted_at IS NOT NULL",
    )
    .await;

    let (method, uri, body) = case.update;
    let raced = a_delete_follows(
        app,
        reads,
        {
            let (app, token) = (Arc::clone(app), token.to_owned());
            async move { app.send(method, &uri, Some(&token), body).await }
        },
        {
            let (app, token, uri) = (Arc::clone(app), token.to_owned(), case.delete);
            async move { app.delete(&uri, Some(&token)).await }
        },
    )
    .await;

    assert_eq!(
        raced.deleted.status,
        StatusCode::NO_CONTENT,
        "{}",
        raced.deleted.body
    );
    assert_eq!(
        raced.updated.status,
        StatusCode::OK,
        "an update that committed answered as if it had not: {}",
        raced.updated.body
    );
    assert_eq!(
        raced.updated.body["data"][case.answered], case.written,
        "{}",
        raced.updated.body
    );
    if let Some((pointer, expected)) = &case.child {
        assert_eq!(
            raced.updated.body.pointer(pointer),
            Some(expected),
            "the answer's children are not the ones the update wrote: {}",
            raced.updated.body
        );
    }

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE object_id = $1 AND event_type = $2",
    )
    .bind(case.id)
    .bind(case.event)
    .fetch_one(&app.pool)
    .await
    .expect("count the audit rows");
    assert_eq!(audited, 1, "the update wrote {audited} {} rows", case.event);

    // The update committed: the deleted row carries what it wrote. Read from
    // the table, since the view stands in for it and hides a deleted row.
    let stored: String = sqlx::query_scalar(&format!(
        "SELECT {}::text FROM {}_stored WHERE id = $1 AND deleted_at IS NOT NULL",
        case.column, case.table
    ))
    .bind(case.id)
    .fetch_one(&app.pool)
    .await
    .expect("read the deleted row");
    assert_eq!(
        stored, case.written,
        "an update answered 200 and was rolled back"
    );
}

fn id_of(response: &common::TestResponse) -> Uuid {
    response.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", response.body))
}

async fn created(app: &TestApp, token: &str, uri: &str, body: Value) -> Uuid {
    let response = app.post(uri, Some(token), body).await;
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::OK,
        "creating at {uri}: {} {}",
        response.status,
        response.body
    );
    id_of(&response)
}

async fn document_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    created(
        app,
        token,
        "/api/v1/document-types",
        json!({ "typeCode": code, "name": code }),
    )
    .await
}

async fn draft(app: &TestApp, token: &str, code: &str) -> Uuid {
    let type_id = document_type(app, token, code).await;
    created(
        app,
        token,
        "/api/v1/documents",
        json!({ "documentTypeId": type_id, "title": "Two standing desks" }),
    )
    .await
}

async fn form(app: &TestApp, token: &str, key: &str) -> Uuid {
    created(
        app,
        token,
        "/api/v1/rad/forms",
        json!({
            "formKey": key,
            "title": "Purchase requisition",
            "definition": {
                "formId": key,
                "version": "2.0.1",
                "title": "Purchase requisition",
                "components": [{
                    "id": "quantity",
                    "role": "data",
                    "type": "number",
                    "key": "quantity",
                    "label": "Quantity",
                    "validation": { "type": "number" }
                }]
            },
        }),
    )
    .await
}

fn put(uri: String, body: Value) -> (Method, String, Option<Value>) {
    (Method::PUT, uri, Some(body))
}

async fn staged() -> (Arc<TestApp>, String) {
    let app = Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    (app, token)
}

#[tokio::test]
async fn a_document_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = draft(&app, &token, "doc_read_back").await;
    let uri = format!("/api/v1/documents/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "title": RENAMED })),
            delete: uri,
            answered: "title",
            event: "Document.Updated",
            ..Case::new("documents", id, "title")
        },
    )
    .await;
}

#[tokio::test]
async fn a_comment_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let document = draft(&app, &token, "comment_read_back").await;
    let id = created(
        &app,
        &token,
        &format!("/api/v1/documents/{document}/comments"),
        json!({ "body": "Please add the delivery date" }),
    )
    .await;
    let uri = format!("/api/v1/documents/{document}/comments/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "body": RENAMED })),
            delete: uri,
            answered: "body",
            event: "Comment.Edited",
            ..Case::new("comments", id, "body")
        },
    )
    .await;
}

#[tokio::test]
async fn a_document_type_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = document_type(&app, &token, "type_read_back").await;
    let uri = format!("/api/v1/document-types/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "name": RENAMED })),
            delete: uri,
            answered: "name",
            event: "DocumentType.Updated",
            ..Case::new("document_types", id, "name")
        },
    )
    .await;
}

#[tokio::test]
async fn a_department_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/organization/departments",
        json!({ "departmentId": "DEPT-READ-BACK", "name": "Procurement" }),
    )
    .await;
    let uri = format!("/api/v1/organization/departments/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "name": RENAMED })),
            delete: uri,
            answered: "name",
            event: "Department.Updated",
            ..Case::new("departments", id, "name")
        },
    )
    .await;
}

#[tokio::test]
async fn a_list_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/rad/lists",
        json!({
            "listKey": "list-read-back",
            "title": "Purchase requisitions",
            "columns": [
                {"columnKey": "document_number", "label": "Number", "dataType": "STRING"}
            ],
        }),
    )
    .await;
    let uri = format!("/api/v1/rad/lists/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "title": RENAMED })),
            delete: uri,
            answered: "title",
            event: "RadList.Updated",
            ..Case::new("rad_lists", id, "title")
        },
    )
    .await;
}

#[tokio::test]
async fn a_facility_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/master-data/facilities",
        json!({ "facilityId": "FAC-READ-BACK", "name": "North warehouse", "facilityTypeId": "BUILDING" }),
    )
    .await;
    let uri = format!("/api/v1/master-data/facilities/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "name": RENAMED })),
            delete: uri,
            answered: "name",
            event: "Facility.Updated",
            ..Case::new("mdm_facilities", id, "name")
        },
    )
    .await;
}

#[tokio::test]
async fn a_party_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/master-data/parties",
        json!({
            "partyId": "PARTY-READ-BACK",
            "partyTypeId": "PARTY_GROUP",
            "partyGroup": { "groupName": "Acme Supplies" },
        }),
    )
    .await;
    let uri = format!("/api/v1/master-data/parties/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "description": RENAMED })),
            delete: uri,
            answered: "description",
            event: "Party.Updated",
            ..Case::new("mdm_parties", id, "description")
        },
    )
    .await;
}

#[tokio::test]
async fn a_credential_edit_a_delete_follows_answers_and_audits() {
    const MOVED: &str = "vault://moved";

    let (app, token) = staged().await;
    let base = "/api/v1/integration/external-systems";
    let system = created(
        &app,
        &token,
        base,
        json!({ "systemCode": "SYS_READ_BACK", "systemName": "Read-back system" }),
    )
    .await;
    let id = created(
        &app,
        &token,
        &format!("{base}/{system}/credentials"),
        json!({ "credentialType": "API_KEY", "secretReference": "vault://in" }),
    )
    .await;
    let uri = format!("{base}/{system}/credentials/{id}");

    raced(
        &app,
        &token,
        Case {
            written: MOVED,
            update: put(uri.clone(), json!({ "secretReference": MOVED })),
            delete: uri,
            answered: "secretReference",
            event: "IntegrationCredential.Updated",
            ..Case::new("integration_credentials", id, "secret_reference")
        },
    )
    .await;
}

#[tokio::test]
async fn a_form_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = form(&app, &token, "form_edit_read_back").await;
    let uri = format!("/api/v1/rad/forms/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "title": RENAMED })),
            delete: uri,
            answered: "title",
            event: "RadForm.Updated",
            ..Case::new("rad_forms", id, "title")
        },
    )
    .await;
}

#[tokio::test]
async fn a_form_publish_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = form(&app, &token, "form_publish_read_back").await;
    let uri = format!("/api/v1/rad/forms/{id}");

    raced(
        &app,
        &token,
        Case {
            written: "PUBLISHED",
            update: (Method::POST, format!("{uri}/publish"), None),
            delete: uri,
            answered: "status",
            event: "RadForm.Published",
            ..Case::new("rad_forms", id, "status")
        },
    )
    .await;
}

#[tokio::test]
async fn a_menu_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/rad/menus",
        json!({ "menuKey": "menu-read-back", "label": "Documents", "routePath": "/documents" }),
    )
    .await;
    let uri = format!("/api/v1/rad/menus/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "label": RENAMED })),
            delete: uri,
            answered: "label",
            event: "RadMenu.Updated",
            ..Case::new("rad_menus", id, "label")
        },
    )
    .await;
}

#[tokio::test]
async fn a_tenant_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = fixtures::create_tenant(&app.pool, "READ-BACK", "Read-back tenant").await;
    let uri = format!("/api/v1/organization/tenants/{id}");

    raced(
        &app,
        &token,
        Case {
            update: put(uri.clone(), json!({ "name": RENAMED })),
            delete: uri,
            answered: "name",
            event: "Tenant.Updated",
            ..Case::new("tenants", id, "name")
        },
    )
    .await;
}

#[tokio::test]
async fn a_role_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/identity/roles",
        json!({ "roleCode": "ROLE-READ-BACK", "name": "Temporary" }),
    )
    .await;
    let uri = format!("/api/v1/identity/roles/{id}");
    let permission: Uuid = sqlx::query_scalar(
        "SELECT id FROM permissions WHERE permission_code = 'identity:user:read'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("identity:user:read is in the seeded catalogue");

    raced(
        &app,
        &token,
        Case {
            update: put(
                uri.clone(),
                json!({ "name": RENAMED, "permissionIds": [permission] }),
            ),
            delete: uri,
            answered: "name",
            // No other test reads the permissions an update answers with.
            child: Some((
                "/data/permissions/0/permissionCode",
                json!("identity:user:read"),
            )),
            event: "Role.Updated",
            ..Case::new("roles", id, "name")
        },
    )
    .await;
}

#[tokio::test]
async fn a_user_edit_a_delete_follows_answers_and_audits() {
    let (app, token) = staged().await;
    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "read.back",
        "read.back@example.test",
        "a long enough password",
        &[],
    )
    .await;
    let uri = format!("/api/v1/identity/users/{id}");
    let role = created(
        &app,
        &token,
        "/api/v1/identity/roles",
        json!({ "roleCode": "ROLE-USER-READ-BACK", "name": "Granted on the update" }),
    )
    .await;

    raced(
        &app,
        &token,
        Case {
            update: put(
                uri.clone(),
                json!({ "displayName": RENAMED, "roleIds": [role] }),
            ),
            delete: uri,
            answered: "displayName",
            // No other test reads the roles an update answers with.
            child: Some(("/data/roles/0/roleCode", json!("ROLE-USER-READ-BACK"))),
            event: "User.Updated",
            ..Case::new("users", id, "display_name")
        },
    )
    .await;
}

/// **The gate the races above stand on drops a row deleted while its read
/// waited.** Each race proves its update only while [`ReadGate`] holds the gap
/// open the way a delete landing in it would. Without the fresh look the gate
/// takes once it opens, a read stopped after the commit returns the row as its
/// snapshot saw it, so an update that reads back after its commit answers 200
/// with its audit row, and all but one of the races pass against the code
/// #787 fixed. This drives the gate directly, so that a gate left
/// toothless goes red here rather than turning those races green.
///
/// It also shows the one-shot claim: a second plain read of the row, as a
/// delete's own read is, is not stopped behind the first. Without it the
/// races still fail against the old code, but as "the delete never answered"
/// after the patience runs out, not as the 500 or 404 they are for.
#[tokio::test]
async fn the_read_gate_drops_a_row_deleted_while_its_read_waited() {
    const READ: &str = "SELECT label FROM rad_menus WHERE id = $1 AND deleted_at IS NULL";

    let (app, token) = staged().await;
    let id = created(
        &app,
        &token,
        "/api/v1/rad/menus",
        json!({ "menuKey": "menu-gate-self-test", "label": "Documents", "routePath": "/documents" }),
    )
    .await;

    let mut reads = ReadGate::close(
        &app,
        "rad_menus",
        &format!("r.label = '{RENAMED}'"),
        "r.deleted_at IS NOT NULL",
    )
    .await;

    // A write that committed, and a plain read of its row after the commit.
    sqlx::query("UPDATE rad_menus SET label = $2 WHERE id = $1")
        .bind(id)
        .bind(RENAMED)
        .execute(&app.pool)
        .await
        .expect("the write");
    let reading = {
        let pool = app.pool.clone();
        tokio::spawn(async move {
            sqlx::query_scalar::<_, String>(READ)
                .bind(id)
                .fetch_optional(&pool)
                .await
                .expect("the stopped read")
        })
    };

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !reads.holds_a_waiter(&app).await {
        assert!(
            !reading.is_finished(),
            "a plain read after the commit was not stopped"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "a plain read after the commit never reached the gate"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let second: Option<String> = tokio::time::timeout(
        PATIENCE,
        sqlx::query_scalar(READ).bind(id).fetch_optional(&app.pool),
    )
    .await
    .expect("a second read was stopped behind the first")
    .expect("the second read");
    assert_eq!(second.as_deref(), Some(RENAMED));

    tokio::time::timeout(
        PATIENCE,
        sqlx::query("UPDATE rad_menus SET deleted_at = now() WHERE id = $1")
            .bind(id)
            .execute(&app.pool),
    )
    .await
    .expect("the delete was stopped")
    .expect("the delete");
    reads.open().await;

    let stopped = tokio::time::timeout(PATIENCE, reading)
        .await
        .expect("the stopped read never answered")
        .expect("the stopped read panicked");
    assert_eq!(
        stopped, None,
        "the gate returned a row deleted while its read waited, as the read's \
         snapshot saw it"
    );
}
