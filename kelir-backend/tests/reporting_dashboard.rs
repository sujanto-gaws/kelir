//! The dashboard summary: what is waiting for the person looking at it
//! (FR-RPT-001, [#431]).
//!
//! # Every assertion here reaches the rows
//!
//! [#431] AC3 asks for the viewer's scoping to be **in the statement rather than
//! in the handler**, which is the [#106]/[#121] lesson this project paid three
//! sprints of coverage findings for: *a test that asserts around a query proves
//! something about the handler and nothing about the rows.* So every scope test
//! below puts the row that must **not** be counted into the database and reads
//! the number back — a second user's draft, a second tenant's draft, a
//! submitted document, a soft-deleted one, another role's task.
//!
//! **One subject cannot tell scoped from unscoped**, and the assertion reads
//! identically either way (coding standard §2.9, [#218]'s single root cause), so
//! the fixture carries a second user and a second role wherever it asserts a
//! scope.
//!
//! # The ones that are not about this module
//!
//! [`the_summary_and_the_inbox_report_the_same_number`] and
//! [`the_widget_lists_the_rows_the_inbox_lists`] are the tests that would catch
//! the failure this module's shape exists to prevent — the first on the count,
//! the second on the rows. **Both read the inbox in the same test and assert
//! the dashboard against what it answered**, rather than against a number the
//! fixture wrote down, which is what [#432] AC2 means by a *divergence* test: a
//! task one surface lists and the other does not is the defect, whichever way
//! round it falls. A test that merely asserted the widget returned some rows
//! would pass for a widget that had grown a visibility rule of its own.
//!
//! [#432]: https://github.com/sujanto-gaws/kelir/issues/432
//!
//! The dashboard neither counts nor lists tasks its own way — both come through
//! `workflow::repository::inbox`'s statement, the one the inbox pages on — and
//! [#279](https://github.com/sujanto-gaws/kelir/issues/279) is this project's
//! record of what the alternative costs: the one duplicated predicate in that
//! file drifted, and an inbox said 23 and ended at 19. **These tests fail if
//! somebody gives the dashboard a query of its own**, which is the only way the
//! two surfaces can ever disagree.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! **Seven mutations, run 2026-09-12, each red and each reddening its own test
//! and no other.** Stated as what was run rather than as what would be run —
//! the distinction [#404](https://github.com/sujanto-gaws/kelir/issues/404) is
//! still open about. Baseline first: **11 passed, nothing mutated.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — `service::dashboard_summary`'s `caller.require(DASHBOARD_READ)` deleted | *the summary refuses a caller without the grant* |
//! | **M2** — `count_own_drafts` stops scoping to the author | *drafts are the caller's own, not the tenant's* |
//! | **M3** — `count_own_drafts` stops scoping to the tenant | *a second tenant's draft is not on this caller's dashboard* |
//! | **M4** — `count_own_drafts` stops filtering to `DRAFT` | *a submitted document has left the drafts count* |
//! | **M5** — `count_own_drafts` stops honouring the soft delete | *a deleted draft is not counted* |
//! | **M6** — `count_waiting`'s `InboxScope::Open` changed to `InboxScope::All` | *a finished task has left the waiting count* |
//! | **M7** — `count_waiting`'s overdue read taken with `InboxScope::Open` | *overdue counts only what is late* |
//!
//! **M6 survived the first run, and that is the finding worth recording.** On
//! the first pass this file had ten tests and none of them had a *finished*
//! task in its fixture, so `open` and `all` were the same set and the scope was
//! not under test at all: every assertion about the waiting count was really an
//! assertion about *whose* task it is, and the *is it still waiting* half was
//! uncovered. [`a_finished_task_has_left_the_waiting_count`] was written to
//! close it and M6 then reddened exactly that test.
//!
//! **A run that had only asked "did something go red" would have called the
//! first pass a clean sweep** — six of seven red looks like a good result and
//! the seventh was the one that mattered. That is why §2.9 asks for the
//! reddened test to be *named*.
//!
//! ## FR-RPT-002, the pending-task widget ([#432])
//!
//! **Five mutations, run 2026-09-12, each red and each reddening exactly one
//! test.** Baseline first: **16 passed, nothing mutated.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | **M1** — the waiting count taken from the page's length instead of the statement's `count(*) OVER ()` | [`the_card_shows_the_top_of_the_queue_and_counts_the_whole_of_it`] |
//! | **M2** — the paged statement's candidate arm stops scoping to `candidate_department_id` | [`a_department_scoped_task_reaches_only_that_departments_widget`] |
//! | **M3** — `waiting_work` reads `InboxScope::All` rather than `Open` | [`a_finished_task_has_left_the_waiting_count`] |
//! | **M4** — the overdue window counts every matched row rather than the late ones | [`overdue_counts_only_what_is_late`] |
//! | **M5** — the paged statement stops admitting a task by its `assignee_user_id` | [`a_delegated_task_is_on_the_delegates_widget_and_not_the_delegators`] |
//!
//! **M2 and M5 were rejected on the first attempt, and it is the anchor rule
//! working rather than a nuisance.** The visibility predicate is written three
//! times in `repository::inbox` — the page, the count and the detail gate — so
//! both mutations matched in more than one place and the runner refused them
//! instead of picking one. §2.9 names that exact failure: *`if !verified {`
//! occurring twice in one file is how a mutation lands on the wrong branch,
//! leaving the test green while nothing beneath it changed.* Here it would have
//! been worse than green — a mutation landing on `count_for_caller` would have
//! reddened a **different** test and been recorded as covering the widget's
//! rows. Both were re-anchored from the `LEFT JOIN users f` line, which only the
//! paged statement has, and then reddened the two tests above.
//!
//! **M1 is the one worth keeping in view.** The count and the rows now come from
//! one statement, so the way they can diverge is no longer a second query — it
//! is somebody deriving the count from the list, which reads as a
//! simplification and is wrong by exactly the number of rows the card is
//! hiding.
//!
//! [#106]: https://github.com/sujanto-gaws/kelir/issues/106
//! [#121]: https://github.com/sujanto-gaws/kelir/issues/121
//! [#218]: https://github.com/sujanto-gaws/kelir/issues/218
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

mod common;

use axum::http::{Method, StatusCode};
use chrono::{Duration, Utc};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const SUMMARY: &str = "/api/v1/dashboard/summary";
const TASKS: &str = "/api/v1/tasks";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// A workflow whose one task is offered to the role named.
fn workflow_for(key: &str, role_code: &str) -> Value {
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
                        "assignment": { "assigneeType": "ROLE", "roleCode": role_code } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{role_code}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{role_code}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, key: &str, role_code: &str) -> Uuid {
    publish_workflow_definition(app, token, key, workflow_for(key, role_code)).await
}

/// The same, for the two tests that need an assignment `workflow_for` does not
/// write — a department-scoped role, and one named person.
async fn publish_workflow_definition(
    app: &TestApp,
    token: &str,
    key: &str,
    definition: Value,
) -> Uuid {
    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({
                "workflowKey": key,
                "name": "Standard approval",
                "definition": definition,
            }),
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

/// A document type bound to `workflow`, with a numbering template of its own.
///
/// The template carries the type code because
/// `uq_documents_tenant_id_document_number` is tenant-wide while a numbering
/// bucket is per type — the finding `workflow_engine.rs` and `task_inbox.rs`
/// both record.
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

/// Creates a document and leaves it in `DRAFT`.
async fn draft_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": title,
                "formData": { "amount": 1_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

/// Creates a document and submits it, so it leaves `DRAFT` and raises a task.
async fn submitted_document(app: &TestApp, token: &str, type_id: Uuid, title: &str) -> Uuid {
    let id = draft_document(app, token, type_id, title).await;

    let submitted = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{id}/submission"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    id
}

/// What a dashboard caller holds, before any test adds to it.
const HOLDER_PERMISSIONS: &[&str] = &[
    "reporting:dashboard:read",
    "workflow:task:read",
    "workflow:instance:read",
    "document:read",
    "document:create",
    // So a fixture author can send their own draft, which is what
    // `a_submitted_document_has_left_the_drafts_count` needs in order to move a
    // row out of the count rather than merely never add it.
    "document:submit",
    // And so an approver can finish one, which is what
    // `a_finished_task_has_left_the_waiting_count` needs for the same reason —
    // see the note on that test.
    "workflow:task:execute",
];

/// A role holding what the dashboard and the surfaces behind it need, and a
/// user holding that role.
async fn holder(app: &TestApp, role_code: &str, username: &str) -> String {
    holder_party(app, role_code, username, &[]).await.1
}

/// The same, with the holder's **id** as well as their token, and room for a
/// permission the ordinary caller has no use for.
///
/// The delegation test needs the id because a `USER` assignment names a person
/// in the workflow definition itself, and it needs `identity:delegation:create`
/// because a window is opened by the person handing their work over.
async fn holder_party(
    app: &TestApp,
    role_code: &str,
    username: &str,
    extra: &[&str],
) -> (Uuid, String) {
    let mut permissions = HOLDER_PERMISSIONS.to_vec();
    permissions.extend_from_slice(extra);

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        role_code,
        &permissions,
    )
    .await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    (id, app.sign_in(username, common::ADMIN_PASSWORD).await)
}

async fn user_with_roles(app: &TestApp, username: &str, roles: &[Uuid]) -> String {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        roles,
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

async fn summary_of(app: &TestApp, token: &str) -> Value {
    let response = app.get(SUMMARY, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body["data"].clone()
}

/// The ids of the tasks the widget listed, in the order it listed them.
fn pending_ids(summary: &Value) -> Vec<String> {
    summary["pendingTasks"]
        .as_array()
        .unwrap_or_else(|| panic!("the summary carries a pendingTasks array: {summary}"))
        .iter()
        .map(|task| {
            task["id"]
                .as_str()
                .unwrap_or_else(|| panic!("a pending task has an id: {task}"))
                .to_owned()
        })
        .collect()
}

/// The ids of the tasks the **inbox** listed, in the order it listed them.
///
/// The widget's rows are asserted against these rather than against a fixture's
/// expectations, which is what makes the tests below divergence tests rather
/// than presence tests ([#432] AC2): a task one surface lists and the other
/// does not is the defect, whichever way round it falls.
async fn inbox_ids(app: &TestApp, token: &str) -> Vec<String> {
    let response = app.get(TASKS, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    response.body["data"]
        .as_array()
        .expect("the inbox is a page")
        .iter()
        .map(|task| task["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// A workflow whose one task is offered to a role **within one department**.
fn department_workflow(key: &str, role_code: &str, department_code: &str) -> Value {
    let mut definition = workflow_for(key, role_code);

    definition["states"][0]["task"]["assignment"] = json!({
        "assigneeType": "DEPARTMENT_ROLE",
        "roleCode": role_code,
        "departmentScope": department_code,
    });

    definition
}

/// A workflow whose one task is assigned to one named person.
///
/// A window redirects work that resolves to somebody, and a role task has no
/// assignee to redirect — `workflow::service::assignment`'s header says why — so
/// the delegated case has to be a `USER` assignment.
fn user_workflow(key: &str, assignee: Uuid) -> Value {
    let mut definition = workflow_for(key, "UNUSED");

    definition["states"][0]["task"]["assignment"] =
        json!({ "assigneeType": "USER", "userId": assignee.to_string() });

    for transition in definition["transitions"]
        .as_array_mut()
        .expect("the transitions")
    {
        transition["allowedBy"] = json!({ "assigneeType": "USER", "userId": assignee.to_string() });
    }

    definition
}

/// A department, by code.
async fn department(app: &TestApp, code: &str, name: &str) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO departments (id, tenant_id, department_code, name) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(code)
    .bind(name)
    .execute(&app.pool)
    .await
    .expect("insert the department");

    id
}

/// Scopes a user's grant to one department.
///
/// `user_roles.department_id` is the optional department-scoped grant `0002`
/// created and the column `DEPARTMENT_ROLE` resolves against.
async fn scope_grant_to(app: &TestApp, username: &str, department: Uuid) {
    let updated = sqlx::query(
        "UPDATE user_roles SET department_id = $1 \
         WHERE user_id = (SELECT id FROM users WHERE username = $2)",
    )
    .bind(department)
    .bind(username)
    .execute(&app.pool)
    .await
    .expect("scope the grant");

    assert_eq!(
        updated.rows_affected(),
        1,
        "the fixture scoped no grant, so the department clause would be untested"
    );
}

// ---------------------------------------------------------------------------
// AC2 — the permission, and the only one
// ---------------------------------------------------------------------------

/// **`reporting:dashboard:read` gates the surface.**
///
/// The caller below holds `document:read` and `workflow:task:read` — every
/// permission over the records the summary counts — and is still refused,
/// because the dashboard is a surface of its own rather than a projection of
/// those two.
#[tokio::test]
async fn the_summary_refuses_a_caller_without_the_grant() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-NO-DASH",
        &["document:read", "workflow:task:read"],
    )
    .await;
    let token = user_with_roles(&app, "rd.nodash", &[role]).await;

    let response = app.get(SUMMARY, Some(&token)).await;

    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "a caller without reporting:dashboard:read was served the dashboard: {}",
        response.body
    );
}

/// **And it is the only one it asks for** (ADR-0039, and the D-45 / D-47 shape
/// one module over).
///
/// The caller below holds `reporting:dashboard:read` and **nothing else** — no
/// `document:read`, no `workflow:task:read` — and is served. That is the
/// invariant the module rests on stated as a test: every number on the
/// dashboard is the caller's own work, so there is no second surface's
/// permission to ask for. A summary that required the underlying reads would
/// refuse this caller a count of their own drafts, which is
/// `0041_activity_read_dropped.sql`'s mistake arriving from the other
/// direction.
#[tokio::test]
async fn the_summary_asks_for_no_permission_but_its_own() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-ONLY-DASH",
        &["reporting:dashboard:read"],
    )
    .await;
    let token = user_with_roles(&app, "rd.onlydash", &[role]).await;

    let response = app.get(SUMMARY, Some(&token)).await;

    assert_eq!(
        response.status,
        StatusCode::OK,
        "a caller holding only reporting:dashboard:read was refused: {}",
        response.body
    );
    assert_eq!(response.body["data"]["tasksWaiting"], 0);
    assert_eq!(response.body["data"]["draftDocuments"], 0);
}

#[tokio::test]
async fn the_summary_needs_a_session() {
    let app = TestApp::spawn().await;

    let response = app.get(SUMMARY, None).await;

    assert_eq!(
        response.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        response.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — the viewer's scoping, asserted on rows
// ---------------------------------------------------------------------------

/// **A caller's waiting work is their own, and nobody else's.**
///
/// Two roles, two holders, two document types, two submitted documents — so
/// there are two open tasks in the tenant and each holder must see exactly one.
/// A single subject would read `1` under a correct rule and under no rule at
/// all.
#[tokio::test]
async fn the_summary_counts_the_callers_own_waiting_work_and_nobody_elses() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = holder(&app, "RD-FINANCE", "rd.finance").await;
    let legal = holder(&app, "RD-LEGAL", "rd.legal").await;

    let finance_workflow = publish_workflow(&app, &token, "rd_finance", "RD-FINANCE").await;
    let legal_workflow = publish_workflow(&app, &token, "rd_legal", "RD-LEGAL").await;

    let finance_type = document_type(&app, &token, "RD_FIN", finance_workflow).await;
    let legal_type = document_type(&app, &token, "RD_LEG", legal_workflow).await;

    submitted_document(&app, &token, finance_type, "A finance request").await;
    submitted_document(&app, &token, legal_type, "A legal request").await;

    assert_eq!(
        summary_of(&app, &finance).await["tasksWaiting"],
        1,
        "the finance approver's count included a task that is not theirs"
    );
    assert_eq!(
        summary_of(&app, &legal).await["tasksWaiting"],
        1,
        "the legal approver's count included a task that is not theirs"
    );
}

/// **The number on the dashboard is the number in the inbox**, because there is
/// one of them.
///
/// This is the test that catches the failure this module's shape exists to
/// prevent. `count_waiting` goes through `workflow::repository::inbox`'s own
/// statement; a dashboard that counted its own way would be a second answer to
/// *whose task is this*, and the two would drift exactly as
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279) records them
/// drifting inside that file — an inbox that said 23 and ended at 19.
///
/// Asserted against `meta.total` **and** the page length, so a count that
/// agreed with a wrong total would still be caught.
#[tokio::test]
async fn the_summary_and_the_inbox_report_the_same_number() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = holder(&app, "RD-SAME", "rd.same").await;
    let other = holder(&app, "RD-OTHER", "rd.other").await;

    let workflow = publish_workflow(&app, &token, "rd_same", "RD-SAME").await;
    let other_workflow = publish_workflow(&app, &token, "rd_other", "RD-OTHER").await;

    let type_id = document_type(&app, &token, "RD_SAME", workflow).await;
    let other_type = document_type(&app, &token, "RD_OTH", other_workflow).await;

    submitted_document(&app, &token, type_id, "One").await;
    submitted_document(&app, &token, type_id, "Two").await;
    // Somebody else's, so a rule that counted everything would disagree with a
    // page that does not.
    submitted_document(&app, &token, other_type, "Not theirs").await;

    let inbox = app.get(TASKS, Some(&finance)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    let rows = inbox.body["data"].as_array().expect("a page").len();
    let total = &inbox.body["meta"]["total"];
    let waiting = summary_of(&app, &finance).await["tasksWaiting"].clone();

    assert_eq!(rows, 2, "the fixture did not raise the tasks it meant to");
    assert_eq!(
        &waiting, total,
        "the dashboard says {waiting} and the inbox's total says {total} — \
         two answers to whose task is this"
    );
    assert_eq!(
        waiting.as_i64(),
        Some(rows as i64),
        "the dashboard's count and the inbox's page disagree"
    );

    // And the other holder's, so the assertion above is not passing because
    // every number in this test happens to be two.
    assert_eq!(summary_of(&app, &other).await["tasksWaiting"], 1);
}

/// **A finished task has left the waiting count.**
///
/// # This test exists because a mutation survived
///
/// `count_waiting` reads `InboxScope::Open`, and changing it to
/// `InboxScope::All` **passed every other test in this file** on 2026-09-12.
/// The reason was the fixtures: not one of them had a task that had been
/// *finished*, so `open` and `all` were the same set and the scope was not
/// under test at all. Every assertion about the waiting count was really an
/// assertion about *whose* task it is, and the *is it still waiting* half was
/// uncovered.
///
/// **So the mutation is the author of this test**, which is the whole of what
/// coding standard §2.9 asks a mutation run to be for — a survivor is a gap
/// found, not a run that went well. The task below is finished through the API
/// rather than by an `UPDATE`, so the row reaches the state the product
/// actually produces.
#[tokio::test]
async fn a_finished_task_has_left_the_waiting_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-DONE", "rd.done").await;
    let workflow = publish_workflow(&app, &token, "rd_done", "RD-DONE").await;
    let type_id = document_type(&app, &token, "RD_DONE", workflow).await;

    let finished = submitted_document(&app, &token, type_id, "The one they decide").await;
    submitted_document(&app, &token, type_id, "The one still waiting").await;

    assert_eq!(
        summary_of(&app, &approver).await["tasksWaiting"],
        2,
        "the fixture did not raise the two tasks it meant to"
    );

    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks WHERE document_id = $1 \
         AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(finished)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&approver),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let summary = summary_of(&app, &approver).await;

    assert_eq!(
        summary["tasksWaiting"], 1,
        "a task this caller has already decided was still counted as waiting"
    );
    // **And it has left the rows too.** The count and the list come from one
    // statement now (#432 AC5), so asserting only the number would leave the
    // scope covered on one of the two things that statement answers.
    assert_eq!(
        pending_ids(&summary).len(),
        1,
        "a decided task was still listed on the dashboard: {summary}"
    );
}

/// **Overdue counts only what is late**, and it is a subset of what is waiting.
///
/// The due date is set on one of the two tasks directly, because what is under
/// test is the count rather than the engine's own due-date derivation — which
/// `workflow_due_dates.rs` owns.
#[tokio::test]
async fn overdue_counts_only_what_is_late() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-LATE", "rd.late").await;
    let workflow = publish_workflow(&app, &token, "rd_late", "RD-LATE").await;
    let type_id = document_type(&app, &token, "RD_LATE", workflow).await;

    let late = submitted_document(&app, &token, type_id, "The late one").await;
    submitted_document(&app, &token, type_id, "The timely one").await;

    sqlx::query(
        "UPDATE workflow_tasks SET due_at = now() - interval '1 day' WHERE document_id = $1",
    )
    .bind(late)
    .execute(&app.pool)
    .await
    .expect("set a due date in the past");

    let summary = summary_of(&app, &approver).await;

    assert_eq!(
        summary["tasksWaiting"], 2,
        "a task that is late is still waiting: {summary}"
    );
    assert_eq!(
        summary["tasksOverdue"], 1,
        "the undated task was reported as late, or the late one was not: {summary}"
    );
}

// ---------------------------------------------------------------------------
// FR-RPT-002 — the pending-task widget ([#432])
// ---------------------------------------------------------------------------

/// **The widget lists the rows the inbox lists.**
///
/// # A divergence test, not a presence test
///
/// [#432] AC2 asks for exactly this and says why: *a task the inbox lists and
/// the widget does not, or the reverse, is the defect.* A test that merely
/// asserted the widget returned some rows would pass for a widget that had
/// grown a visibility rule of its own — which is the one failure the whole
/// shape of this module exists to prevent, and the one
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279) records the cost of.
///
/// So the expected value is **what the inbox answered**, read from the inbox in
/// the same test, rather than a list the fixture wrote down. The second holder
/// is here for the reason coding standard §2.9 gives: one subject cannot tell
/// *scoped to this caller* from *not scoped at all*, because a widget that
/// listed the tenant's tasks would read identically.
#[tokio::test]
async fn the_widget_lists_the_rows_the_inbox_lists() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let theirs = holder(&app, "RD-W-SAME", "rd.w.same").await;
    let other = holder(&app, "RD-W-OTHER", "rd.w.other").await;

    let workflow = publish_workflow(&app, &token, "rd_w_same", "RD-W-SAME").await;
    let other_workflow = publish_workflow(&app, &token, "rd_w_other", "RD-W-OTHER").await;
    let type_id = document_type(&app, &token, "RD_W_SAME", workflow).await;
    let other_type = document_type(&app, &token, "RD_W_OTH", other_workflow).await;

    submitted_document(&app, &token, type_id, "One").await;
    submitted_document(&app, &token, type_id, "Two").await;
    submitted_document(&app, &token, other_type, "Not theirs").await;

    let inbox = inbox_ids(&app, &theirs).await;
    let summary = summary_of(&app, &theirs).await;

    assert_eq!(
        inbox.len(),
        2,
        "the fixture did not raise the tasks it meant to"
    );
    assert_eq!(
        pending_ids(&summary),
        inbox,
        "the widget and the inbox disagree about which tasks are waiting — \
         two answers to whose task is this: {summary}"
    );

    // The other holder's, so the assertion above is not passing because every
    // row in this test belongs to everybody.
    let other_inbox = inbox_ids(&app, &other).await;
    let other_summary = summary_of(&app, &other).await;

    assert_eq!(other_inbox.len(), 1);
    assert_eq!(
        pending_ids(&other_summary),
        other_inbox,
        "the second holder's widget does not match their own inbox: {other_summary}"
    );
    assert!(
        pending_ids(&summary)
            .iter()
            .all(|id| !other_inbox.contains(id)),
        "one person's waiting work appeared on another person's dashboard"
    );
}

/// **The card shows the top of the queue, and the count says how deep it is.**
///
/// Six tasks, five rows, and `tasksWaiting` reads six ([#432] AC1, AC5). The
/// count is not the length of the list and must not be derived from it — which
/// is what makes *3 of 12* sayable, and what this fixture is sized to catch: a
/// widget whose count came from `pendingTasks.length` would read five here and
/// be wrong by exactly the number of tasks the person cannot see.
///
/// The order is the inbox's own, so the rows are a **prefix** of it rather than
/// a selection out of it.
#[tokio::test]
async fn the_card_shows_the_top_of_the_queue_and_counts_the_whole_of_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = holder(&app, "RD-W-DEEP", "rd.w.deep").await;
    let workflow = publish_workflow(&app, &token, "rd_w_deep", "RD-W-DEEP").await;
    let type_id = document_type(&app, &token, "RD_W_DEEP", workflow).await;

    for n in 1..=6 {
        submitted_document(&app, &token, type_id, &format!("Requisition {n}")).await;
    }

    let summary = summary_of(&app, &approver).await;
    let listed = pending_ids(&summary);

    assert_eq!(
        summary["tasksWaiting"], 6,
        "the count describes the queue, not the card: {summary}"
    );
    assert_eq!(
        listed.len(),
        5,
        "the card carries reporting::PENDING_TASKS_SHOWN rows: {summary}"
    );

    let inbox = inbox_ids(&app, &approver).await;
    assert_eq!(inbox.len(), 6, "the fixture did not raise six tasks");
    assert_eq!(
        listed,
        inbox[..5].to_vec(),
        "the widget is not the top of the inbox — it has an order of its own"
    );
}

/// **Nothing waiting is an empty list beside a zero, and never a missing
/// field** ([#432] AC4).
///
/// The screen has to be able to say *nothing is waiting for you* in words,
/// because an empty card is indistinguishable from one that failed to load.
/// What the contract owes it is the difference between *no rows* and *no
/// answer*, and `pendingTasks` is an array either way.
#[tokio::test]
async fn a_viewer_with_nothing_waiting_gets_an_empty_list_and_a_zero() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let idle = holder(&app, "RD-W-IDLE", "rd.w.idle").await;
    let busy = holder(&app, "RD-W-BUSY", "rd.w.busy").await;

    // Somebody else's task exists, so this asserts *empty for this caller*
    // rather than *empty because the tenant has no tasks at all*.
    let workflow = publish_workflow(&app, &token, "rd_w_busy", "RD-W-BUSY").await;
    let type_id = document_type(&app, &token, "RD_W_BUSY", workflow).await;
    submitted_document(&app, &token, type_id, "Somebody else's").await;

    let summary = summary_of(&app, &idle).await;

    assert!(
        summary["pendingTasks"].is_array(),
        "pendingTasks is an array even when it is empty: {summary}"
    );
    assert_eq!(pending_ids(&summary), Vec::<String>::new());
    assert_eq!(summary["tasksWaiting"], 0);

    assert_eq!(
        pending_ids(&summary_of(&app, &busy).await).len(),
        1,
        "the fixture's own task was not raised, so the empty above means nothing"
    );
}

/// **A department-scoped task is on that department's widget and no other**
/// ([#432] AC6).
///
/// # One of the two cases that has already cost this project a fix
///
/// The `candidate_department_id` clause arrived with
/// [#225](https://github.com/sujanto-gaws/kelir/issues/225), which closed the
/// half of `DEPARTMENT_ROLE` that was resolved, stored and then read by
/// nothing. It is the clause a fresh implementation of *whose task is this*
/// leaves out first, and the widget inherits it only because it inherits the
/// whole statement.
///
/// Both approvers hold the **same role** and differ only in the department
/// their grant names, which is the fixture that makes the clause observable: a
/// predicate that dropped it lists Finance's task on Procurement's dashboard,
/// and every other assertion in this file would still pass.
#[tokio::test]
async fn a_department_scoped_task_reaches_only_that_departments_widget() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let finance = department(&app, "RD-DEPT-FIN", "Finance").await;
    let procurement = department(&app, "RD-DEPT-PROC", "Procurement").await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "RD-W-DEPT",
        HOLDER_PERMISSIONS,
    )
    .await;

    let insider = user_with_roles(&app, "rd.w.fin", &[role]).await;
    scope_grant_to(&app, "rd.w.fin", finance).await;
    let outsider = user_with_roles(&app, "rd.w.proc", &[role]).await;
    scope_grant_to(&app, "rd.w.proc", procurement).await;

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_w_dept",
        department_workflow("rd_w_dept", "RD-W-DEPT", "RD-DEPT-FIN"),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_W_DEPT", workflow).await;

    submitted_document(&app, &token, type_id, "Finance's requisition").await;

    let theirs = summary_of(&app, &insider).await;
    let not_theirs = summary_of(&app, &outsider).await;

    assert_eq!(
        pending_ids(&theirs),
        inbox_ids(&app, &insider).await,
        "the department's own approver sees different work on the two surfaces: {theirs}"
    );
    assert_eq!(
        pending_ids(&theirs).len(),
        1,
        "the department's own approver was not offered their task: {theirs}"
    );
    assert_eq!(
        pending_ids(&not_theirs),
        Vec::<String>::new(),
        "Procurement's approver was offered Finance's task on the dashboard: {not_theirs}"
    );
    assert_eq!(
        not_theirs["tasksWaiting"], 0,
        "the count crossed the department boundary even though the rows did not: {not_theirs}"
    );
}

/// **A delegated task is on the delegate's widget, and off the delegator's**
/// ([#432] AC6).
///
/// # The other case, and the one whose failure is silent
///
/// [#234](https://github.com/sujanto-gaws/kelir/pull/234) built delegation end
/// to end, and a widget that forgot it **shows a delegate an empty dashboard
/// beside a full inbox** — a person told there is nothing waiting for them
/// while there is. That is worse than a wrong number, because nobody reports
/// the absence of work.
///
/// The window is opened through the API by the delegator in their own name,
/// which is the only way one can be opened, so the row reaches the state the
/// product actually produces rather than one an `UPDATE` invented.
#[tokio::test]
async fn a_delegated_task_is_on_the_delegates_widget_and_not_the_delegators() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let (delegator_id, delegator) = holder_party(
        &app,
        "RD-W-DELEGATOR",
        "rd.w.ani",
        &["identity:delegation:create"],
    )
    .await;
    let (delegate_id, delegate) = holder_party(&app, "RD-W-DELEGATE", "rd.w.budi", &[]).await;

    let opened = app
        .post(
            "/api/v1/identity/delegations",
            Some(&delegator),
            json!({
                "delegateUserId": delegate_id,
                "startsAt": (Utc::now() - Duration::hours(1)).to_rfc3339(),
                "endsAt": (Utc::now() + Duration::hours(1)).to_rfc3339(),
            }),
        )
        .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let workflow = publish_workflow_definition(
        &app,
        &token,
        "rd_w_deleg",
        user_workflow("rd_w_deleg", delegator_id),
    )
    .await;
    let type_id = document_type(&app, &token, "RD_W_DELEG", workflow).await;

    submitted_document(&app, &token, type_id, "Approved on Ani's behalf").await;

    let theirs = summary_of(&app, &delegate).await;
    let handed_over = summary_of(&app, &delegator).await;

    assert_eq!(
        pending_ids(&theirs),
        inbox_ids(&app, &delegate).await,
        "the delegate's dashboard and inbox disagree: {theirs}"
    );
    assert_eq!(
        pending_ids(&theirs).len(),
        1,
        "the delegate has an empty dashboard beside a full inbox — the widget \
         forgot delegation: {theirs}"
    );
    assert_eq!(
        theirs["pendingTasks"][0]["delegatedFromUserId"],
        json!(delegator_id.to_string()),
        "the row does not say whose approval it is, so the card cannot write \
         \"on Ani's behalf\": {theirs}"
    );
    assert_eq!(
        pending_ids(&handed_over),
        Vec::<String>::new(),
        "the task is still on the dashboard of the person who handed it over: {handed_over}"
    );
}

// ---------------------------------------------------------------------------
// The drafts half — the caller's own, in their own tenant, still unsent
// ---------------------------------------------------------------------------

/// **Drafts are the caller's own, not the tenant's.**
///
/// Two authors, two drafts. A predicate that dropped `created_by` reads `2` for
/// both, which is what this fixture exists to make visible.
#[tokio::test]
async fn drafts_are_the_callers_own_not_the_tenants() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_drafts", "RD-DRAFTS").await;
    let type_id = document_type(&app, &token, "RD_DRAFT", workflow).await;

    let ani = holder(&app, "RD-DRAFTS", "rd.ani").await;
    let budi = holder(&app, "RD-DRAFTS-2", "rd.budi").await;

    draft_document(&app, &ani, type_id, "Ani's draft").await;
    draft_document(&app, &budi, type_id, "Budi's draft").await;
    draft_document(&app, &budi, type_id, "Budi's second draft").await;

    assert_eq!(
        summary_of(&app, &ani).await["draftDocuments"],
        1,
        "Ani's dashboard counted somebody else's draft"
    );
    assert_eq!(
        summary_of(&app, &budi).await["draftDocuments"],
        2,
        "Budi's dashboard did not count both of his own"
    );
}

/// **A submitted document has left the drafts count.**
///
/// *What you have not sent yet* is the question, and a document in the middle
/// of an approval is not it.
#[tokio::test]
async fn a_submitted_document_has_left_the_drafts_count() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_sent", "RD-SENT").await;
    let type_id = document_type(&app, &token, "RD_SENT", workflow).await;

    let author = holder(&app, "RD-SENT", "rd.author").await;

    draft_document(&app, &author, type_id, "Still a draft").await;
    submitted_document(&app, &author, type_id, "Already sent").await;

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a submitted document was still counted as a draft"
    );
}

/// **A deleted draft is not counted.**
///
/// The soft delete is what the rest of the document surface honours, and a
/// count that ignored `deleted_at` would report work the author has thrown
/// away — and could never be made to go down.
#[tokio::test]
async fn a_deleted_draft_is_not_counted() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_gone", "RD-GONE").await;
    let type_id = document_type(&app, &token, "RD_GONE", workflow).await;

    let author = holder(&app, "RD-GONE", "rd.gone").await;

    let kept = draft_document(&app, &author, type_id, "Kept").await;
    let discarded = draft_document(&app, &author, type_id, "Discarded").await;

    sqlx::query("UPDATE documents SET deleted_at = now() WHERE id = $1")
        .bind(discarded)
        .execute(&app.pool)
        .await
        .expect("soft-delete a draft");

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a soft-deleted draft was counted"
    );

    // The kept one is still there, so the assertion above is not passing
    // because the count collapsed to zero.
    let still_there: i64 =
        sqlx::query_scalar("SELECT count(*) FROM documents WHERE id = $1 AND deleted_at IS NULL")
            .bind(kept)
            .fetch_one(&app.pool)
            .await
            .expect("read the kept draft");
    assert_eq!(still_there, 1);
}

/// **A second tenant's draft is not on this caller's dashboard.**
///
/// The row is written directly, with this caller as its author, because that is
/// the shape a dropped `tenant_id` predicate would expose and no API call can
/// produce it: the document endpoints stamp the caller's own tenant, so a test
/// that went through them could never tell a tenant-scoped count from an
/// unscoped one.
#[tokio::test]
async fn a_second_tenants_draft_is_not_on_this_callers_dashboard() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let workflow = publish_workflow(&app, &token, "rd_tenant", "RD-TENANT").await;
    let type_id = document_type(&app, &token, "RD_TEN", workflow).await;

    let author = holder(&app, "RD-TENANT", "rd.tenant").await;
    let author_id: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE username = $1 AND tenant_id = $2")
            .bind("rd.tenant")
            .bind(fixtures::SYSTEM_TENANT_ID)
            .fetch_one(&app.pool)
            .await
            .expect("read the fixture user");

    draft_document(&app, &author, type_id, "Theirs, in their tenant").await;

    let other_tenant = fixtures::create_tenant(&app.pool, "RD-OTHER-TENANT", "Other tenant").await;

    sqlx::query(
        "INSERT INTO documents (id, tenant_id, created_by, document_ref, document_type_id, \
         title, status) VALUES ($1, $2, $3, $4, $5, $6, 'DRAFT')",
    )
    .bind(Uuid::now_v7())
    .bind(other_tenant)
    .bind(author_id)
    .bind("RD-OTHER-0001")
    .bind(type_id)
    .bind("A draft filed under another tenant")
    .execute(&app.pool)
    .await
    .expect("insert a second tenant's draft");

    assert_eq!(
        summary_of(&app, &author).await["draftDocuments"],
        1,
        "a draft in another tenant was counted on this caller's dashboard"
    );
}
