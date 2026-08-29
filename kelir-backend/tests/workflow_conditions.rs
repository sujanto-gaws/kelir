//! Conditional routing (FR-WF-015; [#186]).
//!
//! **Most of this item was already built, and the tests say which parts.** The
//! evaluator is `rad::evaluator` and has been since **D-10** (#154); the
//! operator bound is `CONDITIONAL_OPERATORS`, checked at save by
//! `workflow::domain::jwss` since #174; and `engine::fire` has selected an edge
//! by condition since #175, with S7's fallback-last ordering in
//! `Graph::candidates`. So AC1 and AC2 are **regression** assertions here —
//! they pin behaviour that exists, because an item that claims them should say
//! how it knows.
//!
//! What #186 changed is AC3's second half, AC4's message and AC5:
//!
//! * an expression that fails at run time now **stops the transition** instead
//!   of reading as `false` and falling through to the fallback;
//! * a state whose conditions were all false and which declares no fallback now
//!   says so, rather than reading like an action that does not apply;
//! * the evaluation is recorded, so the history can answer *why this branch*.
//!
//! Every test that names a control has been seen to fail against a build with
//! that control removed (coding standard §2.9); each says what the mutation was.
//!
//! [#186]: https://github.com/sujanto-gaws/kelir/issues/186

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const APPROVER_ROLE: &str = "COND-APPROVER";

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

/// A workflow that branches on the amount: over the threshold goes to the
/// director, under it completes.
///
/// The shape FR-WF-015 exists for, and JWSS §10's own worked example: the
/// approval that goes to a senior approver above a threshold.
fn branching_workflow(key: &str, threshold: i64, fallback: bool) -> Value {
    let mut transitions = vec![json!({
        "from": "MANAGER_APPROVAL", "to": "DIRECTOR_APPROVAL", "action": "APPROVE",
        "allowedBy": format!("ROLE:{APPROVER_ROLE}"),
        "condition": { ">": [{ "var": "variables.amount" }, threshold] }
    })];

    if fallback {
        // The unconditioned edge. S7 evaluates it last whatever its document
        // position, which is why it is written first here.
        transitions.push(json!({
            "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
            "allowedBy": format!("ROLE:{APPROVER_ROLE}")
        }));
    }

    transitions.push(json!({
        "from": "DIRECTOR_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
        "allowedBy": format!("ROLE:{APPROVER_ROLE}")
    }));

    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Threshold approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "DIRECTOR_APPROVAL", "name": "Director approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "director_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": transitions,
        "variables": [
            { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
        ]
    })
}

async fn create_definition(app: &TestApp, token: &str, definition: Value) -> common::TestResponse {
    let key = definition["workflowKey"]
        .as_str()
        .expect("a key")
        .to_owned();

    app.post(
        "/api/v1/workflow/definitions",
        Some(token),
        json!({ "workflowKey": key, "name": "Threshold approval", "definition": definition }),
    )
    .await
}

async fn publish_workflow(app: &TestApp, token: &str, definition: Value) -> Uuid {
    let created = create_definition(app, token, definition).await;
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

async fn draft(app: &TestApp, token: &str, type_id: Uuid, amount: i64) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": amount },
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
                ],
            )
            .await
        }
    }
}

async fn approver(app: &TestApp, username: &str) -> String {
    let role = approver_role(app).await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

async fn open_task_of(app: &TestApp, document_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("read the open task")
}

async fn state_of(app: &TestApp, document_id: Uuid) -> String {
    sqlx::query_scalar("SELECT current_state FROM workflow_instances WHERE document_id = $1")
        .bind(document_id)
        .fetch_one(&app.pool)
        .await
        .expect("read the instance state")
}

async fn decide(app: &TestApp, token: &str, task: Uuid) -> common::TestResponse {
    app.post(
        &format!("/api/v1/workflow/tasks/{task}/decision"),
        Some(token),
        json!({ "action": "APPROVE" }),
    )
    .await
}

/// The routing trail recorded against the transition that moved this document.
async fn routing_of(app: &TestApp, document_id: Uuid) -> Option<Value> {
    sqlx::query_scalar(
        "SELECT routing_json FROM workflow_history \
         WHERE document_id = $1 AND action = 'APPROVE'",
    )
    .bind(document_id)
    .fetch_one(&app.pool)
    .await
    .expect("the approval's history row")
}

// ---------------------------------------------------------------------------
// AC1, AC6 — one evaluator, and a condition selects exactly one transition
// ---------------------------------------------------------------------------

/// **The branch the requirement exists for**, both ways, against one definition.
///
/// A regression assertion rather than new behaviour: `engine::fire` has chosen
/// by condition since #175. It is here because #186 claims AC1 and AC6, and an
/// item that claims them should say how it knows.
///
/// **One transition, not two** (AC6). The document above the threshold lands in
/// `DIRECTOR_APPROVAL` and nowhere else; sequential approval is
/// `uq_workflow_tasks_open_per_instance` in the database, and the one open task
/// this asserts is that index being true rather than intended.
#[tokio::test]
async fn a_condition_selects_the_branch_and_only_one_of_them() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-branch-holder").await;

    let workflow =
        publish_workflow(&app, &token, branching_workflow("wf_cond_branch", 10, true)).await;
    let type_id = document_type(&app, &token, "PR_COND_BRANCH", workflow).await;

    // Over the threshold: the condition holds, so the director is asked.
    let large = draft(&app, &token, type_id, 5_000).await;
    assert_eq!(submit(&app, &token, large).await.status, StatusCode::OK);
    let decided = decide(&app, &holder, open_task_of(&app, large).await).await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, large).await, "DIRECTOR_APPROVAL");

    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_tasks \
         WHERE document_id = $1 AND status IN ('CREATED','ASSIGNED','IN_PROGRESS')",
    )
    .bind(large)
    .fetch_one(&app.pool)
    .await
    .expect("count the open tasks");
    assert_eq!(open, 1, "a condition selects one transition, not two");

    // Under it: the condition is false and the fallback S7 put last is taken.
    let small = draft(&app, &token, type_id, 1).await;
    assert_eq!(submit(&app, &token, small).await.status, StatusCode::OK);
    let decided = decide(&app, &holder, open_task_of(&app, small).await).await;
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);
    assert_eq!(state_of(&app, small).await, "COMPLETED");
}

// ---------------------------------------------------------------------------
// AC2, AC3 (save time) — the operator bound
// ---------------------------------------------------------------------------

/// **An operator the registry does not approve is refused at save.**
///
/// A regression assertion: `jwss::validate_definition` has checked this since
/// #174, against the same `CONDITIONAL_OPERATORS` `jfss` uses. It matters more
/// on this surface than on a form's, which is #186 AC2's own point —
/// `datalogic-rs` carries `datetime`, `ext-string`, `ext-array`, `ext-math` and
/// `flagd` families behind feature flags that no registry governs, and two
/// runtimes agreeing on an operator nobody approved is two runtimes agreeing on
/// something the registry calls FORBIDDEN.
#[tokio::test]
async fn an_operator_outside_the_registry_is_refused_before_it_is_stored() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = branching_workflow("wf_cond_operator", 10, true);
    // `cat` is real in `datalogic-rs` and absent from the registry, which is
    // exactly the gap the bound exists to close.
    definition["transitions"][0]["condition"] = json!({ "cat": ["a", "b"] });

    let refused = create_definition(&app, &token, definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "OPERATOR_NOT_REGISTERED",
        "{}",
        refused.body
    );

    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_definitions WHERE workflow_key = $1")
            .bind("wf_cond_operator")
            .fetch_one(&app.pool)
            .await
            .expect("count the definitions");
    assert_eq!(stored, 0, "refused at save means not stored");
}

// ---------------------------------------------------------------------------
// AC3 (run time) — a broken expression stops the transition
// ---------------------------------------------------------------------------

/// **The behaviour #186 reversed.**
///
/// `engine::holds` used to read an evaluation failure as `false`, so a broken
/// condition fell through to the fallback and the process carried on down a
/// branch its routing rule had never chosen — with nothing anywhere recording
/// that the rule never ran. AC3: *a workflow that routes wrongly on a bad
/// expression is worse than one that refuses to move.*
///
/// **The fixture is a division by zero, and the choice matters.** Every operator
/// in it is registry-approved and the definition publishes cleanly; what breaks
/// is the *data* — the divisor is zero for this document and not for the next
/// one. That is why AC3's second half exists at all: the save-time bound cannot
/// see it, because there is nothing wrong with the expression.
///
/// It also depends on **D-24**: registry v1.6.0 §3.1 says a division by zero
/// produces no value, and both sides configure `ThrowError`. Before that
/// decision `10 / 0` threw while `10.5 / 0` did not, and this test would have
/// passed or failed on whether the amount happened to be fractional.
///
/// **Seen red** against `evaluate_condition` restored to `.unwrap_or(false)`:
/// the decision succeeds, the document lands in `COMPLETED` through the
/// fallback, and the history records a clean approval.
#[tokio::test]
async fn a_condition_that_cannot_be_evaluated_stops_the_transition() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-broken-holder").await;

    let mut definition = branching_workflow("wf_cond_broken", 10, true);
    // "Route upward when the unit cost is over ten" — with the quantity, here
    // the amount, as the divisor. Perfectly ordinary until a document arrives
    // with zero in it.
    definition["transitions"][0]["condition"] =
        json!({ ">": [{ "/": [10_000, { "var": "variables.amount" }] }, 10] });

    let workflow = publish_workflow(&app, &token, definition).await;
    let type_id = document_type(&app, &token, "PR_COND_BROKEN", workflow).await;
    let document = draft(&app, &token, type_id, 0).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let task = open_task_of(&app, document).await;
    let refused = decide(&app, &holder, task).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "CONDITION_UNEVALUABLE",
        "{}",
        refused.body
    );

    // The whole point: it did **not** take the fallback.
    assert_eq!(
        state_of(&app, document).await,
        "MANAGER_APPROVAL",
        "the process is left where it was rather than sent down a branch its \
         routing rule never chose"
    );

    // And nothing was recorded as having happened.
    let moves: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_history WHERE document_id = $1 AND action IS NOT NULL",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("count the transitions");
    assert_eq!(moves, 0, "a refused transition records nothing");
}

// ---------------------------------------------------------------------------
// AC4 — every condition false, no fallback
// ---------------------------------------------------------------------------

/// **The gap conditional routing creates, said out loud.**
///
/// A definition whose only `APPROVE` edge is conditioned, and a document the
/// condition is false for. Nothing applies, nothing moves — and the refusal has
/// to say *which* nothing, because "there is no APPROVE transition" is a
/// different problem for an administrator than "there is one and it did not
/// apply to this document".
///
/// **Seen red** against `no_transition` with its third arm removed: the message
/// says the state has no such transition, which is false — it has one, and the
/// definition is silent about what should happen when it does not hold.
#[tokio::test]
async fn every_condition_false_and_no_fallback_stalls_visibly() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-nomatch-holder").await;

    // No fallback: the only APPROVE out of MANAGER_APPROVAL is conditioned.
    let workflow = publish_workflow(
        &app,
        &token,
        branching_workflow("wf_cond_nomatch", 10, false),
    )
    .await;
    let type_id = document_type(&app, &token, "PR_COND_NOMATCH", workflow).await;

    // Under the threshold, so the one condition is false.
    let document = draft(&app, &token, type_id, 1).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    let refused = decide(&app, &holder, open_task_of(&app, document).await).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "NO_SUCH_TRANSITION",
        "{}",
        refused.body
    );

    let message = refused.body["error"]["details"][0]["message"]
        .as_str()
        .expect("a message");

    assert!(
        message.contains("condition on every one of them was false"),
        "the refusal must distinguish an action that does not apply from one \
         whose conditions all failed, got: {message}"
    );
    assert!(
        message.contains("no fallback"),
        "and it must name what the definition is missing, got: {message}"
    );

    assert_eq!(state_of(&app, document).await, "MANAGER_APPROVAL");
}

// ---------------------------------------------------------------------------
// AC5 — the history answers why this branch
// ---------------------------------------------------------------------------

/// **"Why did this go to her and not to him"** (#186 AC5).
///
/// The trail records every condition the engine actually evaluated, in S7's
/// order, each with its outcome — not just the one that won, which on a history
/// row is a tautology.
///
/// **Seen red** against `engine::fire` binding `routing: None` on the history
/// write: the row records the move and says nothing about why that branch.
#[tokio::test]
async fn the_history_records_which_conditions_were_evaluated_and_what_they_said() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-why-holder").await;

    let workflow =
        publish_workflow(&app, &token, branching_workflow("wf_cond_why", 10, true)).await;
    let type_id = document_type(&app, &token, "PR_COND_WHY", workflow).await;

    // Above the threshold: the condition holds and is the reason for the branch.
    let large = draft(&app, &token, type_id, 5_000).await;
    assert_eq!(submit(&app, &token, large).await.status, StatusCode::OK);
    assert_eq!(
        decide(&app, &holder, open_task_of(&app, large).await)
            .await
            .status,
        StatusCode::OK
    );

    let trail = routing_of(&app, large).await.expect("a routing trail");
    let entries = trail.as_array().expect("an array");

    assert_eq!(entries.len(), 1, "{trail}");
    assert_eq!(entries[0]["to"], "DIRECTOR_APPROVAL");
    assert_eq!(entries[0]["outcome"], true);
    assert!(
        entries[0]["condition"].is_object(),
        "the expression travels, not only its verdict: {trail}"
    );

    // Below it: the same condition was evaluated and said no, and the fallback
    // ran. The `false` is the half of the answer that says why *not* the other
    // branch — and the fallback is absent from the trail because it has no
    // condition to evaluate.
    let small = draft(&app, &token, type_id, 1).await;
    assert_eq!(submit(&app, &token, small).await.status, StatusCode::OK);
    assert_eq!(
        decide(&app, &holder, open_task_of(&app, small).await)
            .await
            .status,
        StatusCode::OK
    );

    let trail = routing_of(&app, small).await.expect("a routing trail");
    let entries = trail.as_array().expect("an array");

    assert_eq!(entries.len(), 1, "{trail}");
    assert_eq!(entries[0]["to"], "DIRECTOR_APPROVAL");
    assert_eq!(entries[0]["outcome"], false);
}

/// A transition nothing had to choose between records no deliberation.
///
/// An empty array would say the engine evaluated nothing on a path where it had
/// nothing to evaluate — true, and indistinguishable from a trail that was lost.
#[tokio::test]
async fn a_transition_with_nothing_to_choose_records_no_trail() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-plain-holder").await;

    // The DIRECTOR_APPROVAL → COMPLETED edge is unconditioned, and so is the
    // instance's first state.
    let workflow =
        publish_workflow(&app, &token, branching_workflow("wf_cond_plain", 10, true)).await;
    let type_id = document_type(&app, &token, "PR_COND_PLAIN", workflow).await;
    let document = draft(&app, &token, type_id, 5_000).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);

    // The instance's first row: reached without a transition at all.
    let first: Option<Value> = sqlx::query_scalar(
        "SELECT routing_json FROM workflow_history WHERE document_id = $1 AND action IS NULL",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the first history row");
    assert!(first.is_none(), "{first:?}");

    // Into DIRECTOR_APPROVAL, then out of it on the one unconditioned edge.
    assert_eq!(
        decide(&app, &holder, open_task_of(&app, document).await)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        decide(&app, &holder, open_task_of(&app, document).await)
            .await
            .status,
        StatusCode::OK
    );

    let trails: Vec<Option<Value>> = sqlx::query_scalar(
        "SELECT routing_json FROM workflow_history \
         WHERE document_id = $1 AND action = 'APPROVE' ORDER BY created_at, id",
    )
    .bind(document)
    .fetch_all(&app.pool)
    .await
    .expect("the approvals");

    assert_eq!(trails.len(), 2);
    assert!(trails[0].is_some(), "the branch was chosen by a condition");
    assert!(
        trails[1].is_none(),
        "and the edge out of DIRECTOR_APPROVAL had nothing to choose between"
    );
}

/// The trail reaches the workspace, not only the database.
///
/// A history that answers *why* only for people with database access is a
/// history that does not answer it.
#[tokio::test]
async fn the_trail_is_on_the_wire_for_the_screen_that_shows_the_history() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let holder = approver(&app, "cond-wire-holder").await;

    let workflow =
        publish_workflow(&app, &token, branching_workflow("wf_cond_wire", 10, true)).await;
    let type_id = document_type(&app, &token, "PR_COND_WIRE", workflow).await;
    let document = draft(&app, &token, type_id, 5_000).await;
    assert_eq!(submit(&app, &token, document).await.status, StatusCode::OK);
    assert_eq!(
        decide(&app, &holder, open_task_of(&app, document).await)
            .await
            .status,
        StatusCode::OK
    );

    let history = app
        .get(
            &format!("/api/v1/documents/{document}/workflow/history"),
            Some(&holder),
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

    assert_eq!(approval["routing"][0]["to"], "DIRECTOR_APPROVAL");
    assert_eq!(approval["routing"][0]["outcome"], true);

    // And the row that started the instance carries none.
    let start = history.body["data"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["action"].is_null())
        .expect("the start")
        .clone();

    assert!(start["routing"].is_null(), "{start}");
}
