//! Delegation, end to end (FR-IDM-006, FR-WF-009, FR-TASK-008; [#184]).
//!
//! The three surfaces are one item by decision **D-17**, and this file is
//! arranged around the reason: a window with nothing reading it is the state
//! **D-13** unscheduled [#24] over. So the tests here are mostly *about the
//! reader* — a window is opened through the API, a document is submitted, and
//! the assertion is on the task row the engine wrote.
//!
//! Every test that names a control has been seen to fail against a build with
//! that control removed (coding standard §2.9); each says what the mutation was
//! and what it produced.
//!
//! [#24]: https://github.com/sujanto-gaws/kelir/issues/24
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184

mod common;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const APPROVER_ROLE: &str = "DLG-APPROVER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A workflow whose one task is assigned to **a named person**.
///
/// A window redirects work that resolves to somebody; a role task has no
/// assignee to redirect (`workflow::service::assignment`'s header says why), so
/// the subject of almost every test here has to be a `USER` assignment.
///
/// `allowedBy` names the same person, which is the shape that makes AC5
/// observable: the delegate does not satisfy this edge in their own right, and
/// what lets them take it is the task recording whose authority they hold.
fn user_workflow(key: &str, assignee: Uuid) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Named approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "USER", "userId": assignee.to_string() } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": { "assigneeType": "USER", "userId": assignee.to_string() } },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "USER", "userId": assignee.to_string() } }
        ]
    })
}

/// The same shape, with the task offered to a role instead.
///
/// Used by the test that pins the rule a window does **not** cross.
fn role_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Queue approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]
    })
}

async fn publish_workflow(app: &TestApp, token: &str, definition: Value) -> Uuid {
    let key = definition["workflowKey"]
        .as_str()
        .expect("a key")
        .to_owned();

    let created = app
        .post(
            "/api/v1/workflow/definitions",
            Some(token),
            json!({ "workflowKey": key, "name": "Delegation subject", "definition": definition }),
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

async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Option<Uuid>) -> Uuid {
    let form = published_form(app, token, &code.to_lowercase().replace('_', "-")).await;
    let mut body = json!({ "typeCode": code, "name": code, "formId": form });

    if let Some(workflow) = workflow {
        body["workflows"] = json!([{ "workflowDefinitionId": workflow }]);
    }

    let created = app.post("/api/v1/document-types", Some(token), body).await;
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

async fn draft(app: &TestApp, token: &str, type_id: Uuid) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": 45_000_000 },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn submit(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.send(
        Method::POST,
        &format!("/api/v1/documents/{id}/submission"),
        Some(token),
        None,
    )
    .await
}

/// The role every party in these tests holds: enough to work a task, read a
/// document, and open a window of their own.
async fn party_role(app: &TestApp) -> Uuid {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM roles WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(APPROVER_ROLE)
    .fetch_optional(&app.pool)
    .await
    .expect("look the role up");

    match existing {
        Some(id) => id,
        None => {
            fixtures::create_role_with_permissions(
                &app.pool,
                fixtures::SYSTEM_TENANT_ID,
                APPROVER_ROLE,
                &[
                    "workflow:task:read",
                    "workflow:task:execute",
                    "workflow:instance:read",
                    "document:read",
                    "identity:delegation:create",
                    "identity:delegation:read",
                    "identity:delegation:delete",
                ],
            )
            .await
        }
    }
}

/// One party to a delegation: their id, and a token to act with.
struct Party {
    id: Uuid,
    token: String,
}

async fn party(app: &TestApp, username: &str) -> Party {
    let role = party_role(app).await;

    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    Party {
        id,
        token: app.sign_in(username, common::ADMIN_PASSWORD).await,
    }
}

/// A user with no permissions at all, and a token for them.
async fn outsider(app: &TestApp, username: &str) -> Party {
    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[],
    )
    .await;

    Party {
        id,
        token: app.sign_in(username, common::ADMIN_PASSWORD).await,
    }
}

fn window(delegate: Uuid, starts_at: DateTime<Utc>, ends_at: DateTime<Utc>) -> Value {
    json!({
        "delegateUserId": delegate,
        "startsAt": starts_at.to_rfc3339(),
        "endsAt": ends_at.to_rfc3339(),
    })
}

async fn open_window(app: &TestApp, token: &str, body: Value) -> common::TestResponse {
    app.post("/api/v1/identity/delegations", Some(token), body)
        .await
}

/// The open task of a document, with the two columns this item is about.
async fn open_task_of(
    app: &TestApp,
    document_id: Uuid,
) -> (Uuid, Option<Uuid>, Option<Uuid>, String) {
    sqlx::query_as(
        "SELECT id, assignee_user_id, delegated_from_user_id, status \
         FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

// ---------------------------------------------------------------------------
// AC1 — a user delegates to another user for a window
// ---------------------------------------------------------------------------

/// The request type has no `delegatorUserId` at all, which is the escalation
/// this prevents rather than merely declines: a holder of
/// `identity:delegation:create` who could name somebody else would be able to
/// point the finance director's approvals at themselves, and the row would look
/// exactly like a legitimate window.
///
/// **Seen red** against `delegation_service::create_delegation` writing the
/// delegate into `delegator_user_id`: the window comes back naming the wrong
/// party, and `ck_delegations_not_self` — which has guarded this table since
/// `0002` with nothing writing to it — refuses the row.
#[tokio::test]
async fn a_window_is_opened_in_the_callers_own_name() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-ac1-ani").await;
    let budi = party(&app, "dlg-ac1-budi").await;

    let now = Utc::now();
    let created = open_window(
        &app,
        &ani.token,
        json!({
            "delegateUserId": budi.id,
            "startsAt": (now - TimeDelta::hours(1)).to_rfc3339(),
            "endsAt": (now + TimeDelta::days(7)).to_rfc3339(),
            "reason": "Annual leave",
        }),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let window = &created.body["data"];
    assert_eq!(
        window["delegatorUserId"].as_str().expect("a delegator"),
        ani.id.to_string(),
        "the caller is the delegator; nobody hands over another person's authority"
    );
    assert_eq!(
        window["delegateUserId"].as_str().expect("a delegate"),
        budi.id.to_string()
    );
    assert_eq!(window["scope"], "ALL");
    assert_eq!(window["isActive"], true);
    assert_eq!(
        window["isRouting"], true,
        "it started an hour ago and has not ended, so it is routing now"
    );
    assert_eq!(window["reason"], "Annual leave");
}

/// The two constraints `0002_identity.sql` has enforced since before anything
/// wrote to this table, arriving as messages that name a field.
#[tokio::test]
async fn a_window_to_yourself_or_one_that_ends_before_it_starts_is_refused() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-ac1b-ani").await;
    let budi = party(&app, "dlg-ac1b-budi").await;

    let now = Utc::now();

    let to_self = open_window(
        &app,
        &ani.token,
        window(ani.id, now, now + TimeDelta::days(1)),
    )
    .await;
    assert_eq!(
        to_self.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        to_self.body
    );
    assert_eq!(
        to_self.body["error"]["details"][0]["code"], "DELEGATE_IS_DELEGATOR",
        "{}",
        to_self.body
    );

    let inverted = open_window(
        &app,
        &ani.token,
        window(budi.id, now + TimeDelta::days(1), now),
    )
    .await;
    assert_eq!(
        inverted.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        inverted.body
    );
    assert_eq!(
        inverted.body["error"]["details"][0]["code"], "WINDOW_INVERTED",
        "{}",
        inverted.body
    );
}

/// A window pointing at somebody who cannot sign in would produce tasks that
/// look assigned and that nobody can reach.
#[tokio::test]
async fn a_window_to_an_account_that_cannot_act_is_refused() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-ac1c-ani").await;
    let budi = party(&app, "dlg-ac1c-budi").await;

    sqlx::query("UPDATE users SET status = 'INACTIVE' WHERE id = $1")
        .bind(budi.id)
        .execute(&app.pool)
        .await
        .expect("deactivate the delegate");

    let now = Utc::now();
    let refused = open_window(
        &app,
        &ani.token,
        window(budi.id, now, now + TimeDelta::days(1)),
    )
    .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "NOT_AVAILABLE",
        "{}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC2 — task assignment consults active delegations at the seam
// ---------------------------------------------------------------------------

/// **The item's whole point in one test.** A window is open; a document is
/// submitted; the task the engine writes is the delegate's, and it records whose
/// work it is.
///
/// The assertion is on the **row**, not on a response, because what the window
/// changes is durable: a task that says it is Budi's while the inbox thinks
/// otherwise is the disagreement this seam exists to prevent.
///
/// **Seen red** against `assignment::resolve` with the `redirect` call removed:
/// the task is written to Ani with `delegated_from_user_id` null, which is
/// `delegations` back to being a table with a writer and no reader.
#[tokio::test]
async fn a_window_routes_the_task_to_the_delegate_and_records_whose_it_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac2-ani").await;
    let budi = party(&app, "dlg-ac2-budi").await;

    let now = Utc::now();
    let opened = open_window(
        &app,
        &ani.token,
        window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac2", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC2", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    let submitted = submit(&app, &token, document).await;
    assert_eq!(submitted.status, StatusCode::OK, "{}", submitted.body);

    let (_, assignee, delegated_from, status) = open_task_of(&app, document).await;

    assert_eq!(
        assignee,
        Some(budi.id),
        "the definition names Ani and the window redirects to Budi"
    );
    assert_eq!(
        delegated_from,
        Some(ani.id),
        "and the row says whose work it is, or the decision cannot record both parties"
    );
    assert_eq!(
        status, "ASSIGNED",
        "a delegated task is an open task somebody else now holds; `DELEGATED` \
         would take it out of the open-task index and out of the inbox"
    );
}

/// A window narrowed to one document type does not touch another type's work.
///
/// **Seen red** against `active_delegate_of` with the `scope` predicate removed:
/// the second type's task goes to Budi as well, which makes `scope` a column the
/// API writes and nothing reads.
#[tokio::test]
async fn a_document_type_window_covers_only_that_type() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-scope-ani").await;
    let budi = party(&app, "dlg-scope-budi").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_scope", ani.id)).await;
    let covered = document_type(&app, &token, "PR_DLG_IN", Some(workflow)).await;
    let uncovered = document_type(&app, &token, "PR_DLG_OUT", Some(workflow)).await;

    let now = Utc::now();
    let opened = open_window(
        &app,
        &ani.token,
        json!({
            "delegateUserId": budi.id,
            "startsAt": (now - TimeDelta::hours(1)).to_rfc3339(),
            "endsAt": (now + TimeDelta::days(7)).to_rfc3339(),
            "scope": "DOCUMENT_TYPE",
            "documentTypeId": covered,
        }),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let inside = draft(&app, &token, covered).await;
    assert_eq!(submit(&app, &token, inside).await.status, StatusCode::OK);

    let outside = draft(&app, &token, uncovered).await;
    assert_eq!(submit(&app, &token, outside).await.status, StatusCode::OK);

    let (_, inside_assignee, _, _) = open_task_of(&app, inside).await;
    let (_, outside_assignee, outside_from, _) = open_task_of(&app, outside).await;

    assert_eq!(inside_assignee, Some(budi.id));
    assert_eq!(
        outside_assignee,
        Some(ani.id),
        "the other type is outside the window and stays with the person the \
         definition named"
    );
    assert_eq!(outside_from, None);
}

/// A window does not redirect a task offered to a role.
///
/// There is no one person's work in a role task to hand over: it has no
/// assignee, and every other holder of the role is still being offered it.
///
/// **This test pins an absence, so there is no control in it to remove.** The
/// mutation that covers the same line is the one on
/// `a_window_routes_the_task_to_the_delegate_and_records_whose_it_is`: with
/// `redirect` gone, that test fails and this one still passes, which is what
/// says the guard rather than the caller is what keeps role tasks out.
#[tokio::test]
async fn a_window_does_not_redirect_a_task_offered_to_a_role() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-role-ani").await;
    let budi = party(&app, "dlg-role-budi").await;

    let now = Utc::now();
    let opened = open_window(
        &app,
        &ani.token,
        window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let workflow = publish_workflow(&app, &token, role_workflow("wf_dlg_role")).await;
    let type_id = document_type(&app, &token, "PR_DLG_ROLE", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (_, assignee, delegated_from, status) = open_task_of(&app, document).await;

    assert_eq!(
        assignee, None,
        "a role task has no assignee until it is claimed"
    );
    assert_eq!(delegated_from, None);
    assert_eq!(status, "CREATED");
}

/// A `ROLE`-scoped window is refused at the API, in its own words, rather than
/// stored as a row that would route nothing.
#[tokio::test]
async fn a_role_scoped_window_is_refused_with_the_reason() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-rolescope-ani").await;
    let budi = party(&app, "dlg-rolescope-budi").await;

    let now = Utc::now();
    let refused = open_window(
        &app,
        &ani.token,
        json!({
            "delegateUserId": budi.id,
            "startsAt": now.to_rfc3339(),
            "endsAt": (now + TimeDelta::days(1)).to_rfc3339(),
            "scope": "ROLE",
        }),
    )
    .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "SCOPE_UNSUPPORTED",
        "{}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — what happens to tasks already assigned when a window opens
// ---------------------------------------------------------------------------

/// **They do not move, and that is the decision rather than the default.**
///
/// A window is prospective: it redirects work that has not arrived. Reaching
/// back would move approvals out from under somebody mid-decision, on a schedule
/// nobody triggered. The complement is the hand-off route, which the second half
/// of this test exercises — together they are what makes the decision a design
/// rather than a gap.
///
/// The first half pins an absence and has no control to remove. The second half
/// does: **seen red** against `record_task_history` binding `None` for the
/// comment, which loses the only record of why a task changed hands — the
/// decision's three comment columns are untouched by a hand-off, because nothing
/// was decided.
#[tokio::test]
async fn a_task_already_assigned_stays_put_when_a_window_opens_and_is_handed_over_explicitly() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac3-ani").await;
    let budi = party(&app, "dlg-ac3-budi").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac3", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC3", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, before, _, _) = open_task_of(&app, document).await;
    assert_eq!(before, Some(ani.id));

    let now = Utc::now();
    let opened = open_window(
        &app,
        &ani.token,
        window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{}", opened.body);

    let (_, after, after_from, _) = open_task_of(&app, document).await;
    assert_eq!(
        after,
        Some(ani.id),
        "the window opened after this task existed; it does not reach back"
    );
    assert_eq!(after_from, None);

    // The other half of the decision: Ani hands it over herself.
    let handed = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/delegation"),
            Some(&ani.token),
            json!({ "delegateUserId": budi.id, "comment": "Off from tomorrow" }),
        )
        .await;
    assert_eq!(handed.status, StatusCode::OK, "{}", handed.body);

    let (_, handed_to, handed_from, status) = open_task_of(&app, document).await;
    assert_eq!(handed_to, Some(budi.id));
    assert_eq!(handed_from, Some(ani.id));
    assert_eq!(
        status, "ASSIGNED",
        "the process has not moved and the task is still open"
    );

    let recorded: (String, Option<String>, Option<Uuid>) = sqlx::query_as(
        "SELECT action, comment, actor_user_id FROM workflow_task_history \
         WHERE task_id = $1 AND action = 'DELEGATE'",
    )
    .bind(task_id)
    .fetch_one(&app.pool)
    .await
    .expect("the hand-off's own history row");

    assert_eq!(recorded.0, "DELEGATE");
    assert_eq!(recorded.1.as_deref(), Some("Off from tomorrow"));
    assert_eq!(recorded.2, Some(ani.id));

    // And nothing was written to the *document's* history: the process did not
    // move. **This assertion got stronger when #259 dropped
    // `ck_workflow_history_moved`.** It used to be guarded by the database as
    // well as by the code, so it could not have failed; now only `delegate` not
    // calling `fire` keeps it true, and this is what says so.
    let moves: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_history WHERE document_id = $1 AND action = 'DELEGATE'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("count the transitions");

    assert_eq!(moves, 0, "handing a task on is not a transition");
}

/// An unclaimed role task has no holder, so there is nothing yet to hand over.
///
/// Giving it to one named person would be a claim and a delegation at once,
/// taking it out of every other holder's queue without anybody asking.
#[tokio::test]
async fn an_unclaimed_role_task_cannot_be_handed_over() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-unclaimed-ani").await;
    let budi = party(&app, "dlg-unclaimed-budi").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_dlg_unclaimed")).await;
    let type_id = document_type(&app, &token, "PR_DLG_UNCL", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/delegation"),
            Some(&ani.token),
            json!({ "delegateUserId": budi.id }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// AC4 — a delegated decision records both parties
// ---------------------------------------------------------------------------

/// **Who decided, and on whose behalf**, on the row a person reads.
///
/// The history is where the pair goes rather than `approval_decisions`: that
/// record's approver is the signature, and this one answers *how did this
/// document get here* — which is the question accountability is asked in.
///
/// **Seen red** against `engine::fire` with `on_behalf_of_user_id` bound to
/// `None` in the history write: the row shows Budi approving, alone, and Ani's
/// name is nowhere in the document's account of its own approval.
#[tokio::test]
async fn a_delegated_decision_records_both_parties_in_the_history() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac4-ani").await;
    let budi = party(&app, "dlg-ac4-budi").await;

    let now = Utc::now();
    assert_eq!(
        open_window(
            &app,
            &ani.token,
            window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
        )
        .await
        .status,
        StatusCode::CREATED
    );

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac4", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC4", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&budi.token),
            json!({ "action": "APPROVE", "comment": "Approved while Ani is away" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let (actor, on_behalf_of): (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT actor_user_id, on_behalf_of_user_id FROM workflow_history \
         WHERE document_id = $1 AND action = 'APPROVE'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the approval's history row");

    assert_eq!(actor, Some(budi.id), "Budi decided it");
    assert_eq!(
        on_behalf_of,
        Some(ani.id),
        "and it was Ani's approval to give — a history showing only the \
         delegate loses the accountability delegation was supposed to preserve"
    );

    // The API says the same thing, with both names, to the approver reading the
    // workspace.
    let history = app
        .get(
            &format!("/api/v1/documents/{document}/workflow/history"),
            Some(&ani.token),
        )
        .await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);

    let approval = history.body["data"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["action"] == "APPROVE")
        .expect("the approval")
        .clone();

    assert_eq!(approval["actorUsername"], "dlg-ac4-budi");
    assert_eq!(approval["onBehalfOfUsername"], "dlg-ac4-ani");
}

/// A decision nobody was standing in for records no second party.
///
/// The other half of the pair: writing the actor into both columns to avoid a
/// null would make *acting for myself* and *acting for somebody who happens to
/// be me* the same row.
#[tokio::test]
async fn an_ordinary_decision_names_nobody_it_was_taken_for() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-plain-ani").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_plain", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_PLAIN", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&ani.token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let on_behalf_of: Option<Uuid> = sqlx::query_scalar(
        "SELECT on_behalf_of_user_id FROM workflow_history \
         WHERE document_id = $1 AND action = 'APPROVE'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the approval's history row");

    assert_eq!(on_behalf_of, None);
}

// ---------------------------------------------------------------------------
// AC5 — delegation does not escalate permission
// ---------------------------------------------------------------------------

/// **The delegate acts with their own permissions.** A window routes work; it
/// grants nothing.
///
/// Budi is delegated to and holds no permission at all, so the decision is
/// refused at the permission gate — before the task is even looked at.
///
/// **Seen red** against `task::decide` with its `caller.require(TASK_EXECUTE)`
/// removed: the decision succeeds, which would make a window a way to hand
/// somebody an authority they were never granted.
#[tokio::test]
async fn a_delegate_still_needs_the_permission_to_work_tasks() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac5-ani").await;
    let budi = outsider(&app, "dlg-ac5-budi").await;

    let now = Utc::now();
    assert_eq!(
        open_window(
            &app,
            &ani.token,
            window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
        )
        .await
        .status,
        StatusCode::CREATED
    );

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac5", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC5", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, assignee, _, _) = open_task_of(&app, document).await;
    assert_eq!(assignee, Some(budi.id), "the window did route it to Budi");

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&budi.token),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// A person who is nobody's delegate cannot decide somebody else's task by
/// asserting that they are.
///
/// There is no field in the request that could say so — the second party comes
/// off the task row, which the server wrote — and this is what that costs an
/// attacker: nothing they can send.
#[tokio::test]
async fn a_third_party_cannot_decide_a_delegated_task() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac5b-ani").await;
    let budi = party(&app, "dlg-ac5b-budi").await;
    let citra = party(&app, "dlg-ac5b-citra").await;

    let now = Utc::now();
    assert_eq!(
        open_window(
            &app,
            &ani.token,
            window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
        )
        .await
        .status,
        StatusCode::CREATED
    );

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac5b", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC5B", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    let refused = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&citra.token),
            json!({ "action": "APPROVE" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// The delegate satisfies the edge **as the delegator**, not in their own right.
///
/// `allowedBy` on this workflow names Ani. Budi is not Ani, so what lets the
/// approval through is the task recording whose authority he holds — and that is
/// exactly the bound AC5 asks for: what the delegator could do, and nothing the
/// delegator could not.
///
/// **Seen red** against `assignment::permits` with the `on_behalf_of` candidate
/// dropped from the loop: the delegate holds a task they cannot decide, and the
/// approval stops with a 403 nobody can clear.
#[tokio::test]
async fn a_delegate_takes_the_edge_the_delegator_was_allowed_to_take() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-edge-ani").await;
    let budi = party(&app, "dlg-edge-budi").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_edge", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_EDGE", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    // Handed over rather than routed, so the definition's `allowedBy` is
    // unambiguously about Ani and the task is unambiguously Budi's.
    let handed = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/delegation"),
            Some(&ani.token),
            json!({ "delegateUserId": budi.id }),
        )
        .await;
    assert_eq!(handed.status, StatusCode::OK, "{}", handed.body);

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&budi.token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(decided.body["data"]["currentState"], "COMPLETED");
}

// ---------------------------------------------------------------------------
// AC6 — an expired or revoked delegation stops routing immediately
// ---------------------------------------------------------------------------

/// **Tested at the boundary rather than assumed**, which is AC6's own wording.
///
/// Three windows over the same pair, each submitted against: one that has not
/// opened, one whose end has just passed, and one that was switched off. None of
/// them routes, and the difference between them is only in the columns.
///
/// **Seen red** against `active_delegate_of` with `now() < d.ends_at` removed:
/// the expired window keeps routing, and cover that ended last month is still
/// sending somebody else's approvals to a stranger.
#[tokio::test]
async fn a_window_that_has_not_opened_has_ended_or_was_switched_off_routes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-ac6-ani").await;
    let budi = party(&app, "dlg-ac6-budi").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_ac6", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_AC6", Some(workflow)).await;

    let now = Utc::now();

    // 1. Not yet open.
    let future = open_window(
        &app,
        &ani.token,
        window(budi.id, now + TimeDelta::days(3), now + TimeDelta::days(10)),
    )
    .await;
    assert_eq!(future.status, StatusCode::CREATED, "{}", future.body);
    assert_eq!(
        future.body["data"]["isRouting"], false,
        "it is active and not yet routing, which are two different facts"
    );

    let before = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, before).await.status, StatusCode::OK);
    assert_eq!(open_task_of(&app, before).await.1, Some(ani.id));

    // 2. Ended. Written directly, because the API refuses to *open* a window
    //    that is already over — which is a different rule and has its own test.
    sqlx::query("UPDATE delegations SET starts_at = $2, ends_at = $3 WHERE delegator_user_id = $1")
        .bind(ani.id)
        .bind(now - TimeDelta::days(10))
        .bind(now - TimeDelta::seconds(1))
        .execute(&app.pool)
        .await
        .expect("age the window");

    let expired = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, expired).await.status, StatusCode::OK);
    assert_eq!(
        open_task_of(&app, expired).await.1,
        Some(ani.id),
        "a window whose end has passed stops routing on the next transition, \
         not on the next sweep"
    );

    // 3. Open in time, and switched off through the API.
    let live = open_window(
        &app,
        &ani.token,
        window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
    )
    .await;
    assert_eq!(live.status, StatusCode::CREATED, "{}", live.body);
    let window_id = id_of(&live.body["data"]);

    let routed = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, routed).await.status, StatusCode::OK);
    assert_eq!(
        open_task_of(&app, routed).await.1,
        Some(budi.id),
        "it routes while it is open, or the next assertion proves nothing"
    );

    let ended = app
        .delete(
            &format!("/api/v1/identity/delegations/{window_id}"),
            Some(&ani.token),
        )
        .await;
    assert_eq!(ended.status, StatusCode::NO_CONTENT, "{}", ended.body);

    let after = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, after).await.status, StatusCode::OK);
    assert_eq!(
        open_task_of(&app, after).await.1,
        Some(ani.id),
        "ending a window stops it immediately"
    );
}

/// A window whose delegate has since been deactivated routes nothing, and
/// routing falls back to the delegator.
///
/// The one outcome worse than not delegating is delegating to an account nobody
/// can sign in to: the task looks assigned the whole time.
///
/// **Seen red** against `active_delegate_of` with the `users` join removed: the
/// task is written to a deactivated account and the approval stops with nothing
/// to say so.
#[tokio::test]
async fn a_window_whose_delegate_has_been_deactivated_stops_routing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-gone-ani").await;
    let budi = party(&app, "dlg-gone-budi").await;

    let now = Utc::now();
    assert_eq!(
        open_window(
            &app,
            &ani.token,
            window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
        )
        .await
        .status,
        StatusCode::CREATED
    );

    sqlx::query("UPDATE users SET status = 'INACTIVE' WHERE id = $1")
        .bind(budi.id)
        .execute(&app.pool)
        .await
        .expect("deactivate the delegate");

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_gone", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_GONE", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (_, assignee, delegated_from, _) = open_task_of(&app, document).await;
    assert_eq!(assignee, Some(ani.id));
    assert_eq!(delegated_from, None);
}

// ---------------------------------------------------------------------------
// The surfaces the three requirements are read through
// ---------------------------------------------------------------------------

/// The inbox says whose work a delegated task is, so the screen can write
/// "on Ani's behalf" rather than leaving the holder to guess.
#[tokio::test]
async fn the_inbox_names_the_person_whose_work_it_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-inbox-ani").await;
    let budi = party(&app, "dlg-inbox-budi").await;

    let now = Utc::now();
    assert_eq!(
        open_window(
            &app,
            &ani.token,
            window(budi.id, now - TimeDelta::hours(1), now + TimeDelta::days(7)),
        )
        .await
        .status,
        StatusCode::CREATED
    );

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_inbox", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_INBOX", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let inbox = app.get("/api/v1/tasks", Some(&budi.token)).await;
    assert_eq!(inbox.status, StatusCode::OK, "{}", inbox.body);

    let row = inbox.body["data"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|task| task["documentId"] == document.to_string())
        .expect("Budi's delegated task")
        .clone();

    assert_eq!(
        row["assignment"], "MINE",
        "a delegated task is unambiguously the holder's; whose approval it is \
         is a second sentence, not a different answer to that question"
    );
    assert_eq!(row["delegatedFromUserId"], ani.id.to_string());
    assert_eq!(row["delegatedFromDisplayName"], "dlg-inbox-ani");

    // And it has left the delegator's queue: the work is Budi's now.
    let ani_inbox = app.get("/api/v1/tasks", Some(&ani.token)).await;
    assert_eq!(ani_inbox.status, StatusCode::OK, "{}", ani_inbox.body);
    assert!(
        !ani_inbox.body["data"]
            .as_array()
            .expect("rows")
            .iter()
            .any(|task| task["documentId"] == document.to_string()),
        "{}",
        ani_inbox.body
    );
}

/// Each of the three permissions gates its own surface, and none of them is the
/// others.
#[tokio::test]
async fn the_delegation_surfaces_each_require_their_own_permission() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-perm-ani").await;
    let budi = party(&app, "dlg-perm-budi").await;
    let nobody = outsider(&app, "dlg-perm-nobody").await;

    let now = Utc::now();
    let created = open_window(
        &app,
        &ani.token,
        window(budi.id, now, now + TimeDelta::days(1)),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let window_id = id_of(&created.body["data"]);

    assert_eq!(
        open_window(
            &app,
            &nobody.token,
            window(budi.id, now, now + TimeDelta::days(1)),
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.get("/api/v1/identity/delegations", Some(&nobody.token))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.delete(
            &format!("/api/v1/identity/delegations/{window_id}"),
            Some(&nobody.token),
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let listed = app
        .get("/api/v1/identity/delegations", Some(&ani.token))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(listed.body["data"]
        .as_array()
        .expect("rows")
        .iter()
        .any(|row| row["id"] == window_id.to_string()));
}

/// A window belonging to another tenant is invisible and cannot be ended.
///
/// The tenant predicate is on every statement in the slice; this is the test
/// that reaches the rows rather than the handler (#106/#121's lesson).
#[tokio::test]
async fn a_window_in_another_tenant_is_out_of_reach() {
    let app = TestApp::spawn().await;
    let ani = party(&app, "dlg-tenant-ani").await;
    let budi = party(&app, "dlg-tenant-budi").await;

    let now = Utc::now();
    let created = open_window(
        &app,
        &ani.token,
        window(budi.id, now, now + TimeDelta::days(1)),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let window_id = id_of(&created.body["data"]);

    let other = fixtures::create_tenant(&app.pool, "OTHERDLG", "Other").await;
    sqlx::query("UPDATE delegations SET tenant_id = $2 WHERE id = $1")
        .bind(window_id)
        .bind(other)
        .execute(&app.pool)
        .await
        .expect("move the window to another tenant");

    let listed = app
        .get("/api/v1/identity/delegations", Some(&ani.token))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        !listed.body["data"]
            .as_array()
            .expect("rows")
            .iter()
            .any(|row| row["id"] == window_id.to_string()),
        "{}",
        listed.body
    );

    assert_eq!(
        app.delete(
            &format!("/api/v1/identity/delegations/{window_id}"),
            Some(&ani.token),
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}

/// A second hand-off still names the person whose **authority** it is.
///
/// Ani's task reaches Budi, and Budi passes it to Citra. The column does not
/// become Budi: what it answers is *whose approval is being given*, and that is
/// still Ani's — which is what the `allowedBy` check reads, and what the
/// history has to record. Overwriting it would make an edge Ani was allowed to
/// take unreachable by the person now holding her task, and it would drop her
/// name from a decision made on her behalf.
///
/// The chain itself is not lost: each hand-off writes its own
/// `workflow_task_history` row naming the person who made it.
///
/// **Seen red** against `repository::task::delegate` with the `COALESCE`
/// replaced by a plain assignment: the history names Budi, and the approval is
/// refused with a 403 nobody holding the task can clear.
#[tokio::test]
async fn a_second_hand_off_still_names_the_person_whose_authority_it_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = party(&app, "dlg-chain-ani").await;
    let budi = party(&app, "dlg-chain-budi").await;
    let citra = party(&app, "dlg-chain-citra").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_dlg_chain", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_DLG_CHAIN", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let (task_id, _, _, _) = open_task_of(&app, document).await;

    for (from, to) in [(&ani, &budi), (&budi, &citra)] {
        let handed = app
            .post(
                &format!("/api/v1/workflow/tasks/{task_id}/delegation"),
                Some(&from.token),
                json!({ "delegateUserId": to.id }),
            )
            .await;
        assert_eq!(handed.status, StatusCode::OK, "{}", handed.body);
    }

    let (_, assignee, delegated_from, _) = open_task_of(&app, document).await;
    assert_eq!(assignee, Some(citra.id));
    assert_eq!(
        delegated_from,
        Some(ani.id),
        "the authority being exercised is still Ani's, however many hands it \
         has passed through"
    );

    let hand_offs: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT actor_user_id FROM workflow_task_history \
         WHERE task_id = $1 AND action = 'DELEGATE' ORDER BY created_at, id",
    )
    .bind(task_id)
    .fetch_all(&app.pool)
    .await
    .expect("the hand-offs");

    assert_eq!(
        hand_offs,
        vec![Some(ani.id), Some(budi.id)],
        "and the chain of who passed it on is in the task's own history"
    );

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task_id}/decision"),
            Some(&citra.token),
            json!({ "action": "APPROVE" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let on_behalf_of: Option<Uuid> = sqlx::query_scalar(
        "SELECT on_behalf_of_user_id FROM workflow_history \
         WHERE document_id = $1 AND action = 'APPROVE'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the approval's history row");

    assert_eq!(on_behalf_of, Some(ani.id));
}
