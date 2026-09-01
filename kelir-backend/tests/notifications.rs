//! Telling somebody a thing is waiting for them (FR-NTF-001/002/003; [#251]).
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Every mutation below was run against this file and the reddened test named,
//! on 2026-09-01:
//!
//! - **M1** — `recipient_user_id = $2` dropped from `list_for_recipient`'s
//!   `WHERE`. Red: *the centre shows only the caller's own* — **after** that
//!   test was rewritten, because the first version survived it. See there.
//! - **M2** — `recipient_user_id = $2` dropped from `mark_read`'s `mine` CTE.
//!   Red: *marking somebody else's notification read is a 404*.
//! - **M3** — `read_at IS NULL` dropped from `mark_read`'s `UPDATE`. Red:
//!   *marking read twice does not move the timestamp*.
//! - **M4** — `TaskArrival.assignee_user_id` forced to `None`, so the
//!   notification stops following the task's own holder. Red: *a task reaching
//!   somebody tells them*, *a delegated task tells the delegate*.
//! - **M5** — `caller.require(NOTIFICATION_READ)?` deleted from `list_mine`.
//!   Red: *a caller without the permission has no centre*.
//! - **M6** — the `filter(|owner| *owner != user_id)` removed from `decide`.
//!   Red: *deciding your own document tells you nothing*.
//!
//! **Two of these were findings before they were evidence.** M4's first anchor
//! matched `insert_task` rather than the notification twelve lines below — two
//! call sites share the field, and §2.9's rule about anchoring on enough
//! context is what that cost. M6 came back **green**, and the cause was not the
//! anchor but the fixture: every scenario had the owner and the decider as
//! different people, so the branch the filter guards was never reached.
//! *Deciding your own document tells you nothing* is the test that finding
//! bought.
//!
//! [#251]: https://github.com/sujanto-gaws/kelir/issues/251

mod common;

use axum::http::{Method, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};
use kelir_backend::modules::notification;

const APPROVER_ROLE: &str = "NTF-APPROVER";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

// ---------------------------------------------------------------------------
// Fixtures — a workflow whose one task is offered to a role, and one to a user
// ---------------------------------------------------------------------------

fn role_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}") }
        ]
    })
}

fn user_workflow(key: &str, approver: Uuid) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "USER", "userId": approver } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": { "assigneeType": "USER", "userId": approver } },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "USER", "userId": approver } }
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
            json!({ "workflowKey": key, "name": "Approval", "definition": definition }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let published = app
        .post(
            &format!("/api/v1/workflow/definitions/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    id
}

async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Option<Uuid>) -> Uuid {
    let mut body = json!({ "typeCode": code, "name": code });

    if let Some(workflow) = workflow {
        // The role the workflow assigns to has to exist before anything submits
        // against this type, or `assignment::resolve` refuses the submit.
        approver_role(app).await;
        body["workflows"] = json!([{ "workflowDefinitionId": workflow }]);
    }

    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(body),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let type_id = id_of(&created.body["data"]);

    // **The template carries the type code** for `workflow_engine.rs`'s reason:
    // `uq_documents_tenant_id_document_number` is tenant-wide while a numbering
    // bucket is per type, so two types sharing a template collide on the second
    // submit.
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
        .send(
            Method::POST,
            "/api/v1/documents",
            Some(token),
            Some(json!({ "documentTypeId": type_id, "title": "Two standing desks" })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
}

async fn submit(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.post(
        &format!("/api/v1/documents/{id}/submission"),
        Some(token),
        json!({}),
    )
    .await
}

struct Person {
    id: Uuid,
    token: String,
}

/// A user holding the approver role plus what the workflow surface needs.
async fn approver(app: &TestApp, username: &str) -> Person {
    let role = approver_role(app).await;
    person(app, username, &[role]).await
}

async fn approver_role(app: &TestApp) -> Uuid {
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
                    "notification:read",
                    // `a_delegated_task_tells_the_delegate` opens a window in
                    // its own name, which is #184's rule: a delegation is
                    // created by the person handing their work over.
                    "identity:delegation:create",
                    // `deciding_your_own_document_tells_you_nothing` needs one
                    // person on both ends, so an approver has to be able to
                    // raise a document as well as decide one.
                    "document:create",
                    "document:submit",
                ],
            )
            .await
        }
    }
}

async fn person(app: &TestApp, username: &str, roles: &[Uuid]) -> Person {
    let id = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        roles,
    )
    .await;

    let token = app.sign_in(username, common::ADMIN_PASSWORD).await;

    Person { id, token }
}

/// A user who may read notifications and documents and nothing else.
async fn reader(app: &TestApp, username: &str) -> Person {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        &[
            "document:read",
            "document:create",
            "document:submit",
            "notification:read",
        ],
    )
    .await;

    person(app, username, &[role]).await
}

async fn notifications_of(app: &TestApp, user: Uuid) -> Vec<(String, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT notification_type, task_id FROM notifications \
         WHERE recipient_user_id = $1 ORDER BY created_at, id",
    )
    .bind(user)
    .fetch_all(&app.pool)
    .await
    .expect("read the notifications")
}

// ---------------------------------------------------------------------------
// AC2, AC4 — a task reaching somebody tells them, and delegation is honoured
// ---------------------------------------------------------------------------

/// **A task assigned to a person notifies that person** (#251 AC2).
#[tokio::test]
async fn a_task_reaching_somebody_tells_them() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = approver(&app, "ntf-user-ani").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_user", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_USER", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let told = notifications_of(&app, ani.id).await;

    assert_eq!(told.len(), 1, "the assignee was not told about their task");
    assert_eq!(told[0].0, "TASK_ASSIGNED");
    assert!(
        told[0].1.is_some(),
        "the notification does not name the task"
    );
}

/// **A role task tells every current holder** (**D-48**).
///
/// The inbox offers it to all of them and any one may claim it, so a
/// notification to one of them would be a lottery and a notification to none
/// would leave this product's commonest approval shape silent.
///
/// **Three people, and the third is the control**: somebody who holds a
/// different role is not told, so this asserts a fan-out to the role rather
/// than to everybody.
#[tokio::test]
async fn a_role_task_tells_every_holder_of_the_role() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-role-ani").await;
    let budi = approver(&app, "ntf-role-budi").await;
    let citra = reader(&app, "ntf-role-citra").await;

    let workflow = publish_workflow(&app, &token, role_workflow("wf_ntf_role")).await;
    let type_id = document_type(&app, &token, "PR_NTF_ROLE", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    assert_eq!(notifications_of(&app, ani.id).await.len(), 1);
    assert_eq!(notifications_of(&app, budi.id).await.len(), 1);
    assert_eq!(
        notifications_of(&app, citra.id).await.len(),
        0,
        "somebody who does not hold the role was told about its task"
    );
}

/// **A delegated task tells the delegate** (#251 AC4).
///
/// #184 made the window redirect the *task*; a notification resolved from the
/// definition would tell Ani about a task that is in Budi's inbox — the wrong
/// person, and the right one never hears.
///
/// **Seen red (M4)** with `notify_the_task_reached` re-resolving the
/// definition's assignment instead of reading the task's own holder: Ani is
/// told and Budi is not.
#[tokio::test]
async fn a_delegated_task_tells_the_delegate() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-dlg-ani").await;
    let budi = approver(&app, "ntf-dlg-budi").await;

    let now = Utc::now();
    let window = app
        .post(
            "/api/v1/identity/delegations",
            Some(&ani.token),
            json!({
                "delegateUserId": budi.id,
                "startsAt": (now - TimeDelta::hours(1)).to_rfc3339(),
                "endsAt": (now + TimeDelta::days(7)).to_rfc3339(),
            }),
        )
        .await;
    assert_eq!(window.status, StatusCode::CREATED, "{}", window.body);

    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_dlg", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_DLG", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    assert_eq!(
        notifications_of(&app, budi.id).await.len(),
        1,
        "the delegate holds the task and was not told about it"
    );
    assert_eq!(
        notifications_of(&app, ani.id).await.len(),
        0,
        "the delegator was told about a task that is not theirs to do"
    );
}

// ---------------------------------------------------------------------------
// AC2 — a decision tells the document's owner
// ---------------------------------------------------------------------------

/// **A decision on somebody's document tells them** (#251 AC2).
#[tokio::test]
async fn a_decision_tells_the_owner_of_the_document() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-dec-ani").await;
    let raiser = reader(&app, "ntf-dec-raiser").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_dec", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_DEC", Some(workflow)).await;

    // Raised by somebody who is not the approver.
    let document = draft(&app, &raiser.token, type_id).await;
    assert_eq!(
        submit(&app, &raiser.token, document).await.status,
        StatusCode::OK
    );

    let task: Uuid = sqlx::query_scalar("SELECT id FROM workflow_tasks WHERE document_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("the task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&ani.token),
            json!({ "action": "APPROVE", "comment": "Fine" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let told = notifications_of(&app, raiser.id).await;
    assert_eq!(
        told.iter()
            .filter(|(kind, _)| kind == "DOCUMENT_DECIDED")
            .count(),
        1,
        "the person who raised the document was not told it had been decided"
    );

    // And the approver, who is not the owner, is told nothing about the
    // decision they took — only about the task that reached them.
    let approver_told = notifications_of(&app, ani.id).await;
    assert!(
        approver_told
            .iter()
            .all(|(kind, _)| kind == "TASK_ASSIGNED"),
        "the approver was told about somebody else's document: {approver_told:?}"
    );
}

/// **Deciding your own document tells you nothing** (#251 AC2, the half that
/// takes a second person to see).
///
/// An `OWNER` task puts the same person on both ends: they raised it and they
/// are the one who acts on it. *Your document was approved* addressed to the
/// person who just pressed Approve is noise, and the notification centre is
/// worth exactly as much as the proportion of it worth reading.
///
/// # This test exists because a mutation survived without it
///
/// The version above had the raiser and the approver as different people in
/// **both** branches, so `document.created_by.filter(|owner| *owner != user_id)`
/// could lose its `filter` and nothing went red — the case the filter exists
/// for was never reached. A green mutation is a finding rather than a nuisance
/// (coding standard §2.9), and this is the finding: not a wrong assertion, a
/// missing second subject.
///
/// **Seen red (M6)**, 2026-09-01, with the `filter` removed.
#[tokio::test]
async fn deciding_your_own_document_tells_you_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // Holds the approver role *and* raises the document, so the OWNER task
    // resolves to the same person who submitted it.
    let ani = approver(&app, "ntf-self-ani").await;

    let mut definition = user_workflow("wf_ntf_self", ani.id);
    definition["states"][0]["task"]["assignment"] = json!({ "assigneeType": "OWNER" });
    definition["transitions"][0]["allowedBy"] = json!({ "assigneeType": "OWNER" });
    definition["transitions"][1]["allowedBy"] = json!({ "assigneeType": "OWNER" });

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_NTF_SELF", Some(workflow)).await;

    let document = draft(&app, &ani.token, type_id).await;
    assert_eq!(
        submit(&app, &ani.token, document).await.status,
        StatusCode::OK
    );

    let task: Uuid = sqlx::query_scalar("SELECT id FROM workflow_tasks WHERE document_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("the task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(&ani.token),
            json!({ "action": "APPROVE", "comment": "Mine, and fine" }),
        )
        .await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    let told = notifications_of(&app, ani.id).await;

    // The task reaching them is worth saying — they did not create the task,
    // the engine did. Their own decision is not.
    assert_eq!(
        told.iter()
            .filter(|(kind, _)| kind == "TASK_ASSIGNED")
            .count(),
        1,
        "the OWNER task did not reach its own owner: {told:?}"
    );
    assert_eq!(
        told.iter()
            .filter(|(kind, _)| kind == "DOCUMENT_DECIDED")
            .count(),
        0,
        "somebody was told about a decision they made themselves: {told:?}"
    );
}

// ---------------------------------------------------------------------------
// AC3 — the notification belongs to the action's transaction
// ---------------------------------------------------------------------------

/// **A submit that failed tells nobody.**
///
/// The assignment cannot resolve — the role does not exist — so the submit's
/// transaction goes back and takes the instance, the task and anything that
/// would have been said about them with it.
///
/// **The role has to be absent rather than unheld**, which cost two attempts.
/// A role nobody holds still resolves to a `candidate_role_id` and the task is
/// created unassigned, so the submit succeeds; and *unheld* is not even
/// achievable here, because this file's other tests grant `NTF-APPROVER` to
/// three people against the same database. Both wrong versions failed on their
/// own precondition rather than passing quietly, which is what the
/// `assert_ne!` is for.
#[tokio::test]
async fn a_submit_that_failed_told_nobody() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = role_workflow("wf_ntf_rollback");
    definition["states"][0]["task"]["assignment"]["roleCode"] = json!("NTF-ROLE-THAT-IS-NOT-THERE");

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_NTF_ROLLBACK", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;

    let refused = submit(&app, &token, document).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "this test needs the submit to fail: {}",
        refused.body
    );

    let any: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE document_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(
        any, 0,
        "somebody was told about a task the failed submit never created"
    );
}

/// **A notification does not outlive the action it announces** (#251 AC3).
///
/// # What this test proves, and what the signature proves
///
/// The property is held by [`notification::service::notify`] taking a
/// `&mut PgTransaction` and returning its error: **there is no way to reach the
/// insert with a pool**, so no caller can write a notification that survives
/// its own action failing. Changing that signature to `&PgPool` fails to
/// compile at both call sites, which is a stronger result than any assertion
/// here and is why it is not the mutation for this test.
///
/// What this guards is the day somebody gives the module a second way in.
/// `document_activity.rs` states the same thing about `activity::record` and
/// this is deliberately its twin — the two records make the same promise and it
/// is worth being able to see that they do.
///
/// **The half `modules::audit` does not make** is the one this module owes on
/// its own: `record_or_warn` swallows its failure so that an audit row cannot
/// take an action down with it, and a notification must not be lost that way,
/// because a person who was never told has nothing to find later.
#[tokio::test]
async fn a_notification_does_not_survive_the_action_rolling_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let ani = approver(&app, "ntf-tx-ani").await;
    let type_id = document_type(&app, &token, "PR_NTF_TX", None).await;
    let document = draft(&app, &token, type_id).await;

    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE recipient_user_id = $1")
            .bind(ani.id)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    let mut transaction = app.pool.begin().await.expect("a transaction");

    notification::service::notify(
        &mut transaction,
        &notification::service::Telling {
            tenant_id: fixtures::SYSTEM_TENANT_ID,
            recipient_user_id: ani.id,
            document_id: Some(document),
            workflow_instance_id: None,
            task_id: None,
            notification_type: notification::domain::NotificationType::TaskAssigned,
            title: "Something that did not happen",
            body: "and which nobody should be told about",
            actor: None,
        },
    )
    .await
    .expect("the notification to be written into the transaction");

    // The action fails, so the transaction goes back.
    transaction.rollback().await.expect("the rollback");

    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM notifications WHERE recipient_user_id = $1")
            .bind(ani.id)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    assert_eq!(
        after, before,
        "a notification outlived the action it announces; that is the audit \
         trail's rule, not this one"
    );
}

// ---------------------------------------------------------------------------
// AC5, AC7 — the centre, and whose notifications are in it
// ---------------------------------------------------------------------------

/// **The centre shows only the caller's own** (#251 AC7), and the predicate is
/// in the statement.
///
/// # The assertion is about which rows came back, and it took a finding to be
///
/// The first version gave two people a notification each about **one shared
/// document** and asserted `meta.total == 1` for both. That survived
/// `recipient_user_id = $2` being dropped from `list_for_recipient`, because
/// the total comes from `count_for_recipient` — a *different statement*, still
/// scoped — so the count kept saying one while the page would have handed over
/// two. §2.9's warning about an assertion that is identical either way, in the
/// shape where the two halves of one endpoint disagree.
///
/// So: **two documents, one notification each, and the assertion is the
/// `documentId` that came back.** `Notification` carries no `recipientUserId`
/// on purpose — there is no field to assert the wrong value in — which is
/// exactly why the test has to be about which rows arrived.
///
/// **Seen red (M1)**, 2026-09-01, with the predicate dropped: each centre
/// returns both rows.
#[tokio::test]
async fn the_centre_shows_only_the_callers_own() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-mine-ani").await;
    let budi = approver(&app, "ntf-mine-budi").await;

    // One workflow each, so each person's notification names a different
    // document and *whose is this* is answerable from the payload.
    let hers = one_task_for(&app, &token, "wf_ntf_mine_a", "PR_NTF_MINE_A", ani.id).await;
    let his = one_task_for(&app, &token, "wf_ntf_mine_b", "PR_NTF_MINE_B", budi.id).await;

    for (who, own, other) in [(&ani, hers, his), (&budi, his, hers)] {
        let listed = app.get("/api/v1/notifications", Some(&who.token)).await;

        assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

        let rows = listed.body["data"].as_array().expect("a page");

        assert_eq!(
            rows.len(),
            1,
            "somebody else's notification reached this centre: {}",
            listed.body
        );
        assert_eq!(
            rows[0]["documentId"],
            Value::String(own.to_string()),
            "this centre is showing a notification about the wrong document"
        );
        assert!(
            !listed.body.to_string().contains(&other.to_string()),
            "the other person's document reached this centre: {}",
            listed.body
        );
        assert_eq!(listed.body["meta"]["total"], 1);
    }
}

/// A submitted document whose one task is assigned to `approver`, returned by
/// document id — the fixture the scoping test needs two of.
async fn one_task_for(
    app: &TestApp,
    token: &str,
    workflow_key: &str,
    type_code: &str,
    approver: Uuid,
) -> Uuid {
    let workflow = publish_workflow(app, token, user_workflow(workflow_key, approver)).await;
    let type_id = document_type(app, token, type_code, Some(workflow)).await;
    let document = draft(app, token, type_id).await;

    assert_eq!(submit(app, token, document).await.status, StatusCode::OK);

    document
}

/// **Marking read is idempotent** (#251 AC5), and the second call does not move
/// the timestamp.
///
/// **Seen red (M3)** with `read_at IS NULL` dropped from the `UPDATE`: the
/// second call restamps, so *when did I read this* answers *the last time I
/// clicked it*.
#[tokio::test]
async fn marking_read_twice_does_not_move_the_timestamp() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-read-ani").await;
    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_read", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_READ", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let listed = app.get("/api/v1/notifications", Some(&ani.token)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    let id = id_of(&listed.body["data"][0]);
    assert!(listed.body["data"][0]["readAt"].is_null());

    let unread = app
        .get("/api/v1/notifications/unread-count", Some(&ani.token))
        .await;
    assert_eq!(unread.body["data"]["unread"], 1);

    let first = app
        .post(
            &format!("/api/v1/notifications/{id}/read"),
            Some(&ani.token),
            json!({}),
        )
        .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.body);

    let stamped: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT read_at FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row");
    let stamped = stamped.expect("a read timestamp");

    let second = app
        .post(
            &format!("/api/v1/notifications/{id}/read"),
            Some(&ani.token),
            json!({}),
        )
        .await;
    assert_eq!(
        second.status,
        StatusCode::NO_CONTENT,
        "a second mark-read was refused, so it is not idempotent: {}",
        second.body
    );

    let again: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT read_at FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row");

    assert_eq!(
        again,
        Some(stamped),
        "the second mark-read moved the timestamp"
    );

    let cleared = app
        .get("/api/v1/notifications/unread-count", Some(&ani.token))
        .await;
    assert_eq!(cleared.body["data"]["unread"], 0);
}

/// **Somebody else's notification is not found** (#251 AC7).
///
/// 404 rather than 403, which is `attachment::service::download`'s choice and
/// for the same reason: a refusal that separates *no such notification* from
/// *one that is not yours* is an oracle for other people's ids.
///
/// **Seen red (M2)** with `recipient_user_id = $2` dropped from the `mine` CTE:
/// Budi marks Ani's notification read.
#[tokio::test]
async fn marking_somebody_elses_notification_read_is_a_404() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-theirs-ani").await;
    let budi = approver(&app, "ntf-theirs-budi").await;

    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_theirs", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_THEIRS", Some(workflow)).await;
    let document = draft(&app, &token, type_id).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let hers: Uuid =
        sqlx::query_scalar("SELECT id FROM notifications WHERE recipient_user_id = $1")
            .bind(ani.id)
            .fetch_one(&app.pool)
            .await
            .expect("Ani's notification");

    let refused = app
        .post(
            &format!("/api/v1/notifications/{hers}/read"),
            Some(&budi.token),
            json!({}),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "somebody marked another person's notification read: {}",
        refused.body
    );

    let untouched: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT read_at FROM notifications WHERE id = $1")
            .bind(hers)
            .fetch_one(&app.pool)
            .await
            .expect("the row");

    assert!(untouched.is_none(), "the refusal wrote `read_at` anyway");
}

/// Clearing everything, and the answer is what is left rather than what went.
#[tokio::test]
async fn marking_all_read_answers_with_what_is_left() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let ani = approver(&app, "ntf-all-ani").await;
    let workflow = publish_workflow(&app, &token, user_workflow("wf_ntf_all", ani.id)).await;
    let type_id = document_type(&app, &token, "PR_NTF_ALL", Some(workflow)).await;

    for _ in 0..2 {
        let document = draft(&app, &token, type_id).await;
        assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);
    }

    let before = app
        .get("/api/v1/notifications/unread-count", Some(&ani.token))
        .await;
    assert_eq!(before.body["data"]["unread"], 2);

    let cleared = app
        .post("/api/v1/notifications/read", Some(&ani.token), json!({}))
        .await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.body);
    assert_eq!(cleared.body["data"]["unread"], 0);

    // Idempotent in the large, too.
    let again = app
        .post("/api/v1/notifications/read", Some(&ani.token), json!({}))
        .await;
    assert_eq!(again.body["data"]["unread"], 0);
}

/// **`notification:read` is what gates the centre** (#251 AC1).
///
/// **Seen red (M5)** with the `require` deleted from `list_mine`: an account
/// holding no notification permission reads a page of them.
#[tokio::test]
async fn a_caller_without_the_permission_has_no_centre() {
    let app = TestApp::spawn().await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-NTF-NONE",
        &["document:read"],
    )
    .await;

    let nobody = person(&app, "ntf-none", &[role]).await;

    for route in [
        "/api/v1/notifications",
        "/api/v1/notifications/unread-count",
    ] {
        let refused = app.get(route, Some(&nobody.token)).await;

        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN,
            "{route} answered without `notification:read`: {}",
            refused.body
        );
    }
}
