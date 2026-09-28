//! A role whose last holder leaves keeps its open tasks, and the Roles screen
//! says so ([#508], **D-91** (2)).
//!
//! Three things take a role's last holder away, and none of them is refused:
//! the grant is removed (`PUT /api/v1/identity/users/{id}` with `roleIds`), the
//! grant's `valid_to` passes, or the holder is deactivated. D-89's refusal is
//! for a role *delete*, and the role here stays live. So the tasks that need it
//! stay open, their documents keep their state, and #507's inbox filter does
//! not hide them, because that filter hides only what a *deleted* role
//! strands.
//!
//! What the product gives an administrator instead is a count. For a caller
//! holding `workflow:task:reassign`, each role on `GET /api/v1/identity/roles`
//! and `GET /api/v1/identity/roles/{id}` carries `liveHolders` and
//! `openTasks`. `0` beside a non-zero `openTasks` is a stranded role, and each
//! of its tasks is cleared with row 10's reassign.
//!
//! # The fixture
//!
//! Two roles, `R` (the one whose last holder leaves) and `Q`, and two documents:
//!
//! * **A**: offered to `R`, unclaimed, and every edge `allowedBy` `R`. It needs
//!   `R` both ways.
//! * **B**: offered to `Q`, claimed by `Q`'s holder, and every edge `allowedBy`
//!   `R`. It needs `R` through `allowedBy` only, and `Q` not at all: a claimed
//!   task does not need its candidate role (#529).
//!
//! So `R` reads two open tasks and `Q` none, and `Q`'s holder is the one person
//! whose inbox can show whether #507's filter hid B.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Mutations run 2026-09-28/29, each reverted after:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `u.status = ANY($3)` dropped from `identity::repository::count_live_holders` | *deactivating the last holder…*: the INACTIVE holder still counted |
//! | `.filter(can_sign_in)` dropped from `UserStatus::signing_in_db_values` | the same, and the domain unit test |
//! | `valid_to >= current_date` made `>` | *a last grant whose window closes…*: today's grant not counted |
//! | The `valid_from` clause dropped | *a last grant whose window closes…*: tomorrow's grant counted |
//! | `ur.deleted_at IS NULL` dropped | *a last grant whose window closes…*: the deleted grant counted |
//! | `u.deleted_at IS NULL` dropped | *a last grant whose window closes…*: the soft-deleted ACTIVE user counted |
//! | `COUNT(DISTINCT …)` made `COUNT(…)` | *each role on the page…*: one user with two grants counted twice |
//! | `caller.holds(TASK_REASSIGN)` dropped from `service::staff` | *only workflow:task:reassign reads the counts* |
//! | `with_staffing` reading the open-task map for both fields | every test here, and the domain unit test |
//! | `WHERE n.role_id = r.id` dropped from the per-role count in `open_tasks_needing_roles` | every test here |
//! | `get_role` not filling the counts | the four tests that read a role alone |
//!
//! [#508]: https://github.com/sujanto-gaws/kelir/issues/508

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

/// The inbox's permissions, and not `workflow:task:reassign`.
const WORKER: &[&str] = &[
    "workflow:task:read",
    "workflow:task:execute",
    "workflow:instance:read",
    "document:read",
];

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A workflow whose task is offered to `queue` and whose edges are `allowedBy`
/// `edge`.
fn offered_to_and_decided_by(key: &str, queue: &str, edge: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval",
                        "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": queue } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{edge}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{edge}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, key: &str, definition: Value) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({ "workflowKey": key, "name": "Standard approval", "definition": definition }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let publication = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    id
}

async fn published_form(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/rad/forms",
            Some(token),
            json!({
                "formKey": key,
                "title": "Purchase requisition",
                "definition": {
                    "formId": key,
                    "version": "2.0.1",
                    "title": "Purchase requisition",
                    "components": [{
                        "id": "amount-field", "role": "data", "type": "number",
                        "key": "amount", "label": "Amount",
                        "validation": { "type": "number", "minimum": 0 }
                    }]
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    let published = app
        .post(
            &format!("/api/v1/rad/forms/{id}/publish"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

/// A document type bound to `workflow`, with a numbering template of its own
/// (`uq_documents_tenant_id_document_number` is tenant-wide).
async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Uuid) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;

    let created = app
        .post(
            "/api/v1/document-types",
            Some(token),
            json!({
                "typeCode": code,
                "name": code,
                "formId": form,
                "workflows": [{ "workflowDefinitionId": workflow }],
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let type_id = id_of(&created.body["data"]);

    let rule = app
        .put(
            &format!("/api/v1/document-types/{type_id}/numbering-rule"),
            Some(token),
            json!({
                "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
                "sequenceScope": "YEAR",
                "gapPolicy": "GAPLESS",
            }),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);

    type_id
}

/// Submits a new document through a workflow offered to `queue` and decided
/// by `edge`, and returns the document and its open task.
async fn document_needing(
    app: &TestApp,
    token: &str,
    code: &str,
    queue: &str,
    edge: &str,
) -> (Uuid, Uuid) {
    let key = code.to_lowercase();
    let workflow = publish_workflow(
        app,
        token,
        &key,
        offered_to_and_decided_by(&key, queue, edge),
    )
    .await;
    let type_id = document_type(app, token, code, workflow).await;

    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({ "documentTypeId": type_id, "title": code, "formData": { "amount": 1_000 } }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let document = id_of(&created.body["data"]);

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{document}/submission"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let task = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task");

    (document, task)
}

async fn worker_role(app: &TestApp, code: &str) -> Uuid {
    fixtures::create_role_with_permissions(&app.pool, fixtures::SYSTEM_TENANT_ID, code, WORKER)
        .await
}

/// A user of the system tenant holding `roles`, and their token.
async fn worker(app: &TestApp, username: &str, roles: &[Uuid]) -> (Uuid, String) {
    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        roles,
    )
    .await;

    (id, app.sign_in(username, common::ADMIN_PASSWORD).await)
}

/// The fixture the module comment describes.
struct Stranding {
    token: String,
    r: Uuid,
    q: Uuid,
    /// `R`'s holder, the one each trigger takes away.
    r_holder: Uuid,
    /// `Q`'s holder, who claimed B.
    q_token: String,
    a: (Uuid, Uuid),
    b: (Uuid, Uuid),
}

async fn stranding(app: &TestApp, tag: &str) -> Stranding {
    let token = app.administrator_token().await;

    let (r_code, q_code) = (format!("RH-{tag}-R"), format!("RH-{tag}-Q"));
    let r = worker_role(app, &r_code).await;
    let q = worker_role(app, &q_code).await;
    let (r_holder, _) = worker(app, &format!("rh.{tag}.r"), &[r]).await;
    let (_, q_token) = worker(app, &format!("rh.{tag}.q"), &[q]).await;

    let a = document_needing(app, &token, &format!("RH_{tag}_A"), &r_code, &r_code).await;
    let b = document_needing(app, &token, &format!("RH_{tag}_B"), &q_code, &r_code).await;

    let claimed = app
        .post(
            &format!("/api/v1/workflow/tasks/{}/claim", b.1),
            Some(&q_token),
            json!({}),
        )
        .await;
    assert_eq!(claimed.status, StatusCode::OK, "{}", claimed.body);

    Stranding {
        token,
        r,
        q,
        r_holder,
        q_token,
        a,
        b,
    }
}

/// A role's `liveHolders` and `openTasks`, as `GET /identity/roles/{id}`
/// serves them, checked against the same role's row on the list.
async fn staffing(app: &TestApp, token: &str, role: Uuid) -> (Value, Value) {
    let read = app
        .get(&format!("/api/v1/identity/roles/{role}"), Some(token))
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let read = &read.body["data"];

    let list = app
        .get("/api/v1/identity/roles?pageSize=100", Some(token))
        .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let row = list.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .find(|row| row["id"] == json!(role))
        .unwrap_or_else(|| panic!("{role} is not on the list: {}", list.body))
        .clone();

    assert_eq!(
        (&row["liveHolders"], &row["openTasks"]),
        (&read["liveHolders"], &read["openTasks"]),
        "the list and the read disagree about {role}"
    );

    (read["liveHolders"].clone(), read["openTasks"].clone())
}

/// A task's status and its document's, straight from the rows.
async fn states_of(app: &TestApp, (document, task): (Uuid, Uuid)) -> (String, String) {
    sqlx::query_as(
        "SELECT t.status, d.status FROM workflow_tasks t
         JOIN documents d ON d.id = t.document_id
         WHERE t.id = $1 AND d.id = $2",
    )
    .bind(task)
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("read the task and its document")
}

fn inbox_holds(inbox: &common::TestResponse, task: Uuid) -> bool {
    inbox.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .any(|row| row["id"] == json!(task))
}

impl Stranding {
    /// Before a trigger: `R` has `holders` and two open tasks, `Q` one holder
    /// and none, and both tasks are open on documents pending approval.
    async fn before(&self, app: &TestApp, holders: i64) -> [(String, String); 2] {
        assert_eq!(
            staffing(app, &self.token, self.r).await,
            (json!(holders), json!(2)),
            "R before the trigger"
        );
        assert_eq!(
            staffing(app, &self.token, self.q).await,
            (json!(1), json!(0)),
            "Q before the trigger"
        );

        let states = [states_of(app, self.a).await, states_of(app, self.b).await];
        for (_, document) in &states {
            assert_eq!(document, "PENDING_APPROVAL");
        }
        states
    }

    /// After a trigger (#508 AC2): `R` reads **0 holders and 2 open tasks**;
    /// nothing about either task or document moved; B is still on its
    /// claimant's inbox, so #507's filter did not hide it; and a newcomer
    /// granted `R` finds A waiting, offered to them.
    async fn after(&self, app: &TestApp, tag: &str, before: [(String, String); 2]) {
        assert_eq!(
            staffing(app, &self.token, self.r).await,
            (json!(0), json!(2)),
            "R after its last holder left: 0 holders and both tasks"
        );
        assert_eq!(
            staffing(app, &self.token, self.q).await,
            (json!(1), json!(0)),
            "Q is not R"
        );

        let after = [states_of(app, self.a).await, states_of(app, self.b).await];
        assert_eq!(after, before, "a trigger moved a task or its document");

        let claimant = app.get("/api/v1/tasks", Some(&self.q_token)).await;
        assert_eq!(claimant.status, StatusCode::OK, "{}", claimant.body);
        assert!(
            inbox_holds(&claimant, self.b.1),
            "B left its claimant's inbox although R is live: {}",
            claimant.body
        );

        let (_, newcomer) = worker(app, &format!("rh.{tag}.new"), &[self.r]).await;
        let inbox = app.get("/api/v1/tasks", Some(&newcomer)).await;
        assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);
        assert!(
            inbox_holds(&inbox, self.a.1),
            "A was not waiting for R's next holder: {}",
            inbox.body
        );
        assert_eq!(
            staffing(app, &self.token, self.r).await,
            (json!(1), json!(2)),
            "R once it has a holder again"
        );
    }
}

/// **Trigger (a): the last grant is removed** through
/// `PUT /api/v1/identity/users/{id}` with `roleIds` ([#508] AC2, AC5). The
/// route answers 200, as it did before this issue.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
#[tokio::test]
async fn removing_the_last_grant_leaves_the_roles_tasks_open_and_counted() {
    let app = TestApp::spawn().await;
    let fixture = stranding(&app, "GRANT").await;
    let before = fixture.before(&app, 1).await;

    let removed = app
        .put(
            &format!("/api/v1/identity/users/{}", fixture.r_holder),
            Some(&fixture.token),
            json!({ "roleIds": [] }),
        )
        .await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.body);

    fixture.after(&app, "grant", before).await;
}

/// **Trigger (b): the last grant's `valid_to` passes** ([#508] AC2, AC6).
///
/// Time passing is a `valid_to` in the past, set in SQL: no route sets it.
/// Beside `R`'s holder sit three grants that never counted: one whose
/// `valid_from` is tomorrow, one soft-deleted, and a live grant held by a user
/// soft-deleted in SQL with their status left `ACTIVE`. No route leaves a user
/// in that state (a deactivation also sets `INACTIVE`), so it is the one way to
/// show that `users.deleted_at` is read at all. The boundaries are the
/// grant's: a `valid_to` of **today** still holds the role, and yesterday's
/// does not.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
#[tokio::test]
async fn a_last_grant_whose_window_closes_leaves_the_roles_tasks_open_and_counted() {
    let app = TestApp::spawn().await;
    let fixture = stranding(&app, "WINDOW").await;

    let (early, _) = worker(&app, "rh.window.early", &[fixture.r]).await;
    let (gone, _) = worker(&app, "rh.window.gone", &[fixture.r]).await;
    sqlx::query("UPDATE user_roles SET valid_from = current_date + 1 WHERE user_id = $1")
        .bind(early)
        .execute(&app.pool)
        .await
        .expect("date the grant from tomorrow");
    sqlx::query("UPDATE user_roles SET deleted_at = now() WHERE user_id = $1")
        .bind(gone)
        .execute(&app.pool)
        .await
        .expect("soft-delete the grant");
    let (deleted, _) = worker(&app, "rh.window.deleted", &[fixture.r]).await;
    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1 AND status = 'ACTIVE'")
        .bind(deleted)
        .execute(&app.pool)
        .await
        .expect("soft-delete the user, leaving them ACTIVE");

    sqlx::query("UPDATE user_roles SET valid_to = current_date WHERE user_id = $1")
        .bind(fixture.r_holder)
        .execute(&app.pool)
        .await
        .expect("end the grant today");
    let before = fixture.before(&app, 1).await;

    sqlx::query("UPDATE user_roles SET valid_to = current_date - 1 WHERE user_id = $1")
        .bind(fixture.r_holder)
        .execute(&app.pool)
        .await
        .expect("end the grant yesterday");

    fixture.after(&app, "window", before).await;
}

/// **Trigger (c): the last holder is deactivated** ([#508] AC2, AC5, AC6).
///
/// The grants stay live, so a count that copied `holds_role`'s window and
/// nothing else would still read two here. `R` has two holders, taken away the
/// two ways an account is deactivated: one set `INACTIVE` through
/// `PUT /api/v1/identity/users/{id}` (200), then the other through
/// `DELETE /api/v1/identity/users/{id}` (204). Neither is refused.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
#[tokio::test]
async fn deactivating_the_last_holder_leaves_the_roles_tasks_open_and_counted() {
    let app = TestApp::spawn().await;
    let fixture = stranding(&app, "STATUS").await;
    let (second, _) = worker(&app, "rh.status.second", &[fixture.r]).await;
    let before = fixture.before(&app, 2).await;

    let inactive = app
        .put(
            &format!("/api/v1/identity/users/{}", fixture.r_holder),
            Some(&fixture.token),
            json!({ "status": "INACTIVE" }),
        )
        .await;
    assert_eq!(inactive.status, StatusCode::OK, "{}", inactive.body);
    assert_eq!(
        staffing(&app, &fixture.token, fixture.r).await,
        (json!(1), json!(2)),
        "an INACTIVE holder is still counted"
    );

    let deleted = app
        .delete(
            &format!("/api/v1/identity/users/{second}"),
            Some(&fixture.token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let grants: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles WHERE role_id = $1 AND deleted_at IS NULL",
    )
    .bind(fixture.r)
    .fetch_one(&app.pool)
    .await
    .expect("count the grants");
    assert_eq!(
        grants, 2,
        "deactivation is expected to leave the grants live"
    );

    fixture.after(&app, "status", before).await;
}

/// **A caller without `workflow:task:reassign` reads the roles and no counts**
/// ([#508] AC3, the product owner's decision of 2026-09-28).
///
/// The caller holds every permission in the catalogue but that one, so the
/// gate is shown to be that permission and no other. They read the list and
/// the role, and neither carries `liveHolders` or `openTasks`: omitted, not
/// null and not zero. A caller holding only `identity:role:read` and
/// `workflow:task:reassign` reads both.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
#[tokio::test]
async fn only_workflow_task_reassign_reads_the_counts() {
    let app = TestApp::spawn().await;
    let fixture = stranding(&app, "GATE").await;

    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT permission_code FROM permissions
         WHERE deleted_at IS NULL AND permission_code <> 'workflow:task:reassign'",
    )
    .fetch_all(&app.pool)
    .await
    .expect("read the permission catalogue");
    assert!(codes.iter().any(|code| code == "identity:role:read"));
    assert!(codes.iter().any(|code| code == "identity:role:delete"));
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();

    let everything_else = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RH-ELSE",
        &codes,
    )
    .await;
    let (_, without) = worker(&app, "rh.gate.else", &[everything_else]).await;

    let list = app
        .get("/api/v1/identity/roles?pageSize=100", Some(&without))
        .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);
    let rows = list.body["data"].as_array().expect("a page");
    assert!(rows.iter().any(|row| row["id"] == json!(fixture.r)));
    for row in rows {
        assert!(row.get("liveHolders").is_none(), "{row}");
        assert!(row.get("openTasks").is_none(), "{row}");
    }

    let read = app
        .get(
            &format!("/api/v1/identity/roles/{}", fixture.r),
            Some(&without),
        )
        .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert!(
        read.body["data"].get("liveHolders").is_none(),
        "{}",
        read.body
    );
    assert!(
        read.body["data"].get("openTasks").is_none(),
        "{}",
        read.body
    );

    let reassigner = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RH-REASSIGN",
        &["identity:role:read", "workflow:task:reassign"],
    )
    .await;
    let (_, with) = worker(&app, "rh.gate.with", &[reassigner]).await;
    assert_eq!(
        staffing(&app, &with, fixture.r).await,
        (json!(1), json!(2)),
        "R, read with workflow:task:reassign"
    );
    assert_eq!(
        staffing(&app, &with, fixture.q).await,
        (json!(1), json!(0)),
        "Q, read with workflow:task:reassign"
    );
}

/// **Each role on a page reads its own counts** ([#508] AC3): the two counts
/// are taken for the whole page at once, and a count filed under the wrong role
/// would be a stranded role shown as staffed.
///
/// `R` and `Q` sit on one page with different answers, and a role nobody holds
/// and nothing needs reads `0` and `0`, present rather than omitted. A user
/// holding a role twice, once generally and once within a department, is one
/// holder.
///
/// [#508]: https://github.com/sujanto-gaws/kelir/issues/508
#[tokio::test]
async fn each_role_on_the_page_reads_its_own_counts() {
    let app = TestApp::spawn().await;
    let fixture = stranding(&app, "PAGE").await;
    let idle = worker_role(&app, "RH-PAGE-IDLE").await;
    let twice = worker_role(&app, "RH-PAGE-TWICE").await;
    let (holder, _) = worker(&app, "rh.page.twice", &[twice]).await;

    let department = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name) VALUES ($1, $2, $3, $3)",
    )
    .bind(department)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind("RH-PAGE-DEPT")
    .execute(&app.pool)
    .await
    .expect("insert the department");
    sqlx::query(
        "INSERT INTO user_roles (id, tenant_id, user_id, role_id, department_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(holder)
    .bind(twice)
    .bind(department)
    .execute(&app.pool)
    .await
    .expect("grant the role again, within the department");

    let list = app
        .get("/api/v1/identity/roles?pageSize=100", Some(&fixture.token))
        .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.body);

    let counts = |role: Uuid| {
        list.body["data"]
            .as_array()
            .expect("a page")
            .iter()
            .find(|row| row["id"] == json!(role))
            .map(|row| (row["liveHolders"].clone(), row["openTasks"].clone()))
            .unwrap_or_else(|| panic!("{role} is not on the list: {}", list.body))
    };

    assert_eq!(counts(fixture.r), (json!(1), json!(2)), "R");
    assert_eq!(counts(fixture.q), (json!(1), json!(0)), "Q");
    assert_eq!(counts(idle), (json!(0), json!(0)), "a role nobody holds");
    assert_eq!(counts(twice), (json!(1), json!(0)), "one user, two grants");
}
