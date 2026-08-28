//! Workflow definitions: stored, validated, published, projected (#174).
//!
//! **The validator is the item.** A workflow that can deadlock is a workflow
//! that will, and #174 AC3 asks for a definition whose transitions do not form a
//! reachable, terminating graph to be refused at **save** time rather than
//! discovered by whoever was waiting for an approval that could not move. Every
//! S-rule case below is a graph a person could plausibly write.

mod common;

use axum::http::{Method, StatusCode};
use common::TestApp;
use kelir_backend::modules::workflow::repository::definition as definition_repo;
use serde_json::{json, Value};
use uuid::Uuid;

const DEFINITIONS: &str = "/api/v1/workflow/definitions";

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

/// The workflow every other test in this suite is a mutation of: submitted,
/// approved or rejected, both ends final.
pub fn approval_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Standard approval",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "WF-APPROVER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "COMPLETED", "action": "APPROVE",
              "allowedBy": "ROLE:WF-APPROVER" },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": "ROLE:WF-APPROVER" }
        ]
    })
}

async fn create(app: &TestApp, token: &str, key: &str, definition: Value) -> common::TestResponse {
    app.post(
        DEFINITIONS,
        Some(token),
        json!({ "workflowKey": key, "name": "Standard approval", "definition": definition }),
    )
    .await
}

/// Creates and publishes a workflow, failing here rather than at the next
/// assertion if either half was refused.
pub async fn published(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = create(app, token, key, approval_workflow(key)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);

    let publication = app
        .post(
            &format!("{DEFINITIONS}/{id}/publication"),
            Some(token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    id
}

// ---------------------------------------------------------------------------
// AC1 — stored and validated against the schema on save
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_definition_is_stored_with_its_spec_version_and_initial_state() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let created = create(&app, &token, "wf_stored", approval_workflow("wf_stored")).await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["workflowKey"], "wf_stored");
    assert_eq!(
        created.body["data"]["version"], 1,
        "the definition revision"
    );
    assert_eq!(
        created.body["data"]["jwssVersion"], "1.0.0",
        "the JWSS spec version, which is a different number"
    );
    assert_eq!(created.body["data"]["initialState"], "MANAGER_APPROVAL");
    assert_eq!(created.body["data"]["status"], "DRAFT");
}

#[tokio::test]
async fn a_document_that_is_not_jwss_is_refused_at_save() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let refused = create(&app, &token, "wf_not_jwss", json!({ "states": "several" })).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body["error"]["details"]
            .as_array()
            .expect("details")
            .iter()
            .any(|detail| detail["code"] == "INVALID_DEFINITION"),
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn an_operator_no_registry_approves_is_refused_in_a_condition() {
    // JWSS §6.2, and it is the reason this check exists at all: **D-10**'s
    // engine evaluates a far wider surface than the registry approves,
    // identically on both sides. Parity is not governance.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_operator");
    definition["transitions"][0]["condition"] = json!({ "cat": ["a", "b"] });

    let refused = create(&app, &token, "wf_operator", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("OPERATOR_NOT_REGISTERED"),
        "{}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// AC3 — a graph that does not terminate is refused at save
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_workflow_that_can_deadlock_is_refused_at_save() {
    // S6's second half. `STUCK` is reachable, has no way out, and is not final:
    // a document that gets there can never finish, and nobody is told until
    // somebody asks why a requisition has been sitting for a month.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_deadlock");
    definition["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({
            "code": "STUCK", "name": "Stuck", "mapsToDocumentStatus": "IN_REVIEW"
        }));
    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "STUCK", "action": "RETURN",
            "allowedBy": "ROLE:WF-APPROVER"
        }));

    let refused = create(&app, &token, "wf_deadlock", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a workflow with a dead end was stored: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("DEAD_END_STATE"),
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn a_state_nothing_routes_to_is_refused_at_save() {
    // S6's first half, and it is the other kind of mistake: an orphan is
    // usually a typo or an edge somebody forgot, and a caller fixes it
    // differently from a dead end.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_orphan");
    definition["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({
            "code": "ARCHIVED", "name": "Archived", "mapsToDocumentStatus": "ARCHIVED",
            "isFinal": true
        }));

    let refused = create(&app, &token, "wf_orphan", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("UNREACHABLE_STATE"),
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn a_transition_out_of_a_final_state_is_refused() {
    // S4. A final state that leads somewhere is a state machine whose author
    // and whose engine disagree about when the process ends.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_after_final");
    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "COMPLETED", "to": "MANAGER_APPROVAL", "action": "RESUBMIT",
            "allowedBy": "OWNER"
        }));

    let refused = create(&app, &token, "wf_after_final", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("TRANSITION_FROM_FINAL"),
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn two_unconditioned_transitions_on_one_action_are_refused() {
    // S7. Which one fires would depend on document order, which is a routing
    // decision nobody made — `check_workflows`' duplicate-priority refusal, one
    // artefact over.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_ambiguous");
    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "APPROVE",
            "allowedBy": "ROLE:WF-APPROVER"
        }));

    let refused = create(&app, &token, "wf_ambiguous", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("AMBIGUOUS_FALLBACK"),
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn a_workflow_nothing_can_finish_is_refused() {
    // S9's second half: no state maps to COMPLETED or CANCELLED, so no document
    // this workflow drives can ever end.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let definition = json!({
        "workflowKey": "wf_endless",
        "version": "1.0.0",
        "name": "Endless",
        "initialState": "REVIEW",
        "states": [
            { "code": "REVIEW", "name": "Review", "mapsToDocumentStatus": "IN_REVIEW" },
            { "code": "APPROVED", "name": "Approved", "mapsToDocumentStatus": "APPROVED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "REVIEW", "to": "APPROVED", "action": "APPROVE", "allowedBy": "OWNER" }
        ]
    });

    let refused = create(&app, &token, "wf_endless", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("NO_TERMINAL_STATUS"),
        "{}",
        refused.body
    );
}

// ---------------------------------------------------------------------------
// JWSS §5.3 — the assignee types this implementation refuses, and why at save
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_assignment_this_engine_cannot_resolve_is_refused_at_save() {
    // Refused at save rather than at run time, which is `jfss.rs`'s discipline:
    // a workflow that publishes cleanly and then cannot assign its first task
    // is a stalled instance nobody is told about. The message has to name what
    // to use instead, because "unsupported" tells an author nothing.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_manager");
    definition["states"][0]["task"]["assignment"] = json!({ "assigneeType": "MANAGER_OF_OWNER" });

    let refused = create(&app, &token, "wf_manager", definition).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let body = refused.body.to_string();
    assert!(body.contains("ASSIGNEE_TYPE_NOT_RESOLVABLE"), "{body}");
    assert!(
        body.contains("DEPARTMENT_ROLE"),
        "the refusal must name what to use instead: {body}"
    );
}

// ---------------------------------------------------------------------------
// Publishing, its projections, and immutability
// ---------------------------------------------------------------------------

#[tokio::test]
async fn publishing_projects_the_states_and_transitions() {
    // JWSS §9, and the projection is load-bearing rather than decorative: the
    // foreign key on `workflow_instances.current_state` reads `workflow_states`,
    // so a publish that committed without them would produce a definition
    // nothing could start.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "wf_projected").await;

    let states: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_states WHERE workflow_definition_id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count the projected states");

    let transitions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_transitions WHERE workflow_definition_id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count the projected transitions");

    assert_eq!(states, 3, "three states were declared");
    assert_eq!(transitions, 2, "two transitions were declared");

    let initial: bool = sqlx::query_scalar(
        "SELECT is_initial FROM workflow_states WHERE workflow_definition_id = $1 AND state_code = 'MANAGER_APPROVAL'",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read the initial flag");

    assert!(initial, "the initial state is flagged in the projection");
}

#[tokio::test]
async fn republishing_the_next_revision_rewrites_its_own_projection_only() {
    // Delete-then-insert, scoped to the definition: the JSON is the authority,
    // so a projected row that survives a republish is a row the authority did
    // not ask for. The **third move** is what distinguishes "separate" from
    // "overwritten" (coding standard §2.9): revision 1, revision 2, and then
    // revision 1 again, still holding its own three states.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "wf_revised").await;

    let mut narrower = approval_workflow("wf_revised");
    narrower["states"]
        .as_array_mut()
        .expect("states")
        .retain(|state| state["code"] != "REJECTED");
    narrower["transitions"]
        .as_array_mut()
        .expect("transitions")
        .retain(|transition| transition["to"] != "REJECTED");

    let revision = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({ "definition": narrower }),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    assert_eq!(revision.body["data"]["version"], 2);

    let second = id_of(&revision.body["data"]);
    let publication = app
        .post(
            &format!("{DEFINITIONS}/{second}/publication"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(publication.status, StatusCode::OK, "{}", publication.body);

    let first_states: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_states WHERE workflow_definition_id = $1",
    )
    .bind(first)
    .fetch_one(&app.pool)
    .await
    .expect("count");

    let second_states: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_states WHERE workflow_definition_id = $1",
    )
    .bind(second)
    .fetch_one(&app.pool)
    .await
    .expect("count");

    assert_eq!(first_states, 3, "revision 1 kept its own projection");
    assert_eq!(
        second_states, 2,
        "revision 2 projected only what it declares"
    );
}

#[tokio::test]
async fn a_published_revision_cannot_be_edited() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "wf_immutable").await;

    let refused = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "name": "Renamed" }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a published revision was edited: {}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "NOT_A_DRAFT");
}

/// **The publish-time check, reached** (coding standard §2.5).
///
/// The save path validates, so this rule fires only for a row that reached the
/// database another way — a migration, a restore, a hand-written `INSERT`. That
/// makes it a second line of defence, and §2.5 does not accept a paragraph
/// explaining why one exists in place of a test that reaches it. This writes the
/// invalid definition through the pool, exactly as the excluded paths would, and
/// publishes it through the API.
///
/// **Seen red** against `publish_definition` with its `jwss::validate_definition`
/// call removed: the definition reaches `ACTIVE`, which JWSS §1.3 clause 2
/// forbids, and its projection then contains a dead end that an instance can
/// walk into.
#[tokio::test]
async fn a_definition_that_reached_the_database_another_way_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_smuggled");
    definition["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({
            "code": "STUCK", "name": "Stuck", "mapsToDocumentStatus": "IN_REVIEW"
        }));
    definition["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "STUCK", "action": "RETURN",
            "allowedBy": "ROLE:WF-APPROVER"
        }));

    let id = Uuid::now_v7();

    sqlx::query(
        r#"
        INSERT INTO workflow_definitions
            (id, tenant_id, workflow_key, name, version, jwss_version, definition_json,
             initial_state, status)
        VALUES ($1, $2, 'wf_smuggled', 'Smuggled', 1, '1.0.0', $3, 'MANAGER_APPROVAL', 'DRAFT')
        "#,
    )
    .bind(id)
    .bind(common::fixtures::SYSTEM_TENANT_ID)
    .bind(&definition)
    .execute(&app.pool)
    .await
    .expect("write a definition the API would have refused");

    let refused = app
        .post(
            &format!("{DEFINITIONS}/{id}/publication"),
            Some(&token),
            json!({}),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a definition with a dead end reached ACTIVE: {}",
        refused.body
    );
    assert!(
        refused.body.to_string().contains("DEAD_END_STATE"),
        "{}",
        refused.body
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM workflow_definitions WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("read the status back");

    assert_eq!(
        status, "DRAFT",
        "a definition failing an S-rule must stay DRAFT"
    );
}

// ---------------------------------------------------------------------------
// Permissions and scope
// ---------------------------------------------------------------------------

/// **Seen red** against a build where `create_definition`'s
/// `caller.require(DEFINITION_CREATE)` is replaced by `DEFINITION_READ`: the
/// caller below creates a workflow with a read-only grant.
#[tokio::test]
async fn creating_a_workflow_needs_the_create_permission() {
    let app = TestApp::spawn().await;

    let role = common::fixtures::create_role_with_permissions(
        &app.pool,
        common::fixtures::SYSTEM_TENANT_ID,
        "WF-READER",
        &["workflow:definition:read"],
    )
    .await;

    common::fixtures::create_user(
        &app.pool,
        common::fixtures::SYSTEM_TENANT_ID,
        "wf.reader",
        "wf.reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("wf.reader", common::ADMIN_PASSWORD).await;

    let refused = create(
        &app,
        &reader,
        "wf_permission",
        approval_workflow("wf_permission"),
    )
    .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    // And the read the role *does* grant works, so the assertion above is not
    // green because the caller can reach nothing at all — the gate §2.9 warns
    // about.
    let listed = app.get(DEFINITIONS, Some(&reader)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
}

/// **Seen red** against `repository::definition::find_definition` with its
/// `tenant_id = $1` predicate removed: the second tenant's administrator reads
/// the first tenant's approval chain.
#[tokio::test]
async fn a_workflow_is_not_visible_from_another_tenant() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    // `administrator_token` signs in without a tenant code, which this
    // deployment refuses; the mode is on precisely so a second tenant exists to
    // be scoped against.
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let id = published(&app, &token, "wf_scoped").await;

    let other = common::fixtures::create_tenant(&app.pool, "OTHER", "Other tenant").await;
    let role = common::fixtures::create_role_with_permissions(
        &app.pool,
        other,
        "WF-ADMIN",
        &["workflow:definition:read"],
    )
    .await;
    common::fixtures::create_user(
        &app.pool,
        other,
        "other.admin",
        "other.admin@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let outsider = app
        .sign_in_to("OTHER", "other.admin", common::ADMIN_PASSWORD)
        .await;

    let refused = app
        .send(
            Method::GET,
            &format!("{DEFINITIONS}/{id}"),
            Some(&outsider),
            None,
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "another tenant's workflow was readable: {}",
        refused.body
    );

    // The page, not only the item: a list one row too long is where a leak is
    // least visible (#171's lesson).
    let listed = app.get(DEFINITIONS, Some(&outsider)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        listed.body["data"].as_array().expect("a page").is_empty(),
        "another tenant's workflow appeared in a list: {}",
        listed.body
    );
    assert_eq!(listed.body["meta"]["total"], 0, "{}", listed.body);
}

// ---------------------------------------------------------------------------
// Closing what the mutation campaign found nothing held
// ---------------------------------------------------------------------------
//
// Five predicates in `repository::definition` survived their mutation, which
// means nothing in the suite was the only guard on their behaviour. Four are
// closed here and one is declared, which is the choice coding standard §2.5
// leaves: **a test that removes the first line, or a comment saying it is
// unexercised.** These are the tests.
//
// This is the Sprint 9 retrospective's rule applied inside the sprint that
// measured it: a coverage finding is worth most while there is sprint left to
// close it in.

/// **A workflow's revisions are numbered under its own key** (M05).
///
/// `highest_version` scopes by `workflow_key`, and nothing exercised that: one
/// key cannot tell *scoped by key* from *the highest number anywhere*. Three
/// moves, because two cannot tell "separate" from "shared" — A, B, then **A
/// again**, and the last is `2` only if the counters really are per key
/// (coding standard §2.9's three-move rule).
///
/// **Seen red** against `highest_version`'s `workflow_key = $2` defeated: the
/// second revision of `wf_counter_a` is numbered 3, because it takes the highest
/// version of any workflow in the tenant.
#[tokio::test]
async fn each_workflow_key_numbers_its_own_revisions() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "wf_counter_a").await;
    let _second = published(&app, &token, "wf_counter_b").await;

    let revision = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({}),
        )
        .await;

    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    assert_eq!(
        revision.body["data"]["version"], 2,
        "`wf_counter_a`'s next revision took a number from another workflow's \
         sequence: {}",
        revision.body
    );
    assert_eq!(revision.body["data"]["workflowKey"], "wf_counter_a");
}

/// **A publish that lands first makes the edit apply to nothing** (M06).
///
/// `update_draft` carries `AND status = 'DRAFT'` as a second line of defence:
/// the service reads the revision, sees a draft, and writes — and a publish can
/// land in the gap. Every ordinary test refuses at the service's own read, so
/// the predicate is invisible to all of them.
///
/// **The interleaving is arranged rather than raced for**, which is the
/// technique coding standard §2.5 names and `rad_forms.rs`'s
/// `an_edit_blocked_by_a_publish_applies_to_nothing` established: the publish
/// holds the row lock uncommitted, the edit reaches its statement and blocks,
/// and the edit then runs against the published row. It drives the repository
/// rather than the route, because the route is the layer this is written to get
/// past.
///
/// **Seen red** against the predicate defeated: the edit updates one row, and a
/// published revision changes underneath the instances that started against it.
#[tokio::test]
async fn a_publish_that_lands_first_makes_the_edit_apply_to_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    let created = create(
        &app,
        &token,
        "wf_interleaved",
        approval_workflow("wf_interleaved"),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let id = id_of(&created.body["data"]);

    // The publisher, holding the row lock and not yet committed.
    let mut publishing = app.pool.begin().await.expect("a transaction");
    let published = definition_repo::publish(&mut *publishing, tenant, id, None)
        .await
        .expect("the publish runs");
    assert_eq!(published, 1, "the publish is the one that wins the row");

    // The editor, blocking on that lock. It reached the statement before the
    // publish committed, which is the interleaving the service check cannot see.
    let pool = app.pool.clone();
    let editing = tokio::spawn(async move {
        definition_repo::update_draft(
            &pool,
            tenant,
            id,
            &definition_repo::DefinitionFields {
                name: Some("Edited after the publish"),
                description: None,
                definition_json: None,
                initial_state: None,
                jwss_version: None,
            },
            None,
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    publishing.commit().await.expect("the publish commits");

    let edited = editing
        .await
        .expect("the edit task finishes")
        .expect("the edit runs");

    assert_eq!(
        edited, 0,
        "the edit applied to a revision that had just been published"
    );
}

/// **Two publishes of one draft produce one publisher** (M07).
///
/// `publish` carries `AND status = 'DRAFT'`, so the second `UPDATE` matches no
/// row and the caller is told somebody else published it — their name is on it,
/// which is correct, because the second call published nothing.
///
/// **Seen red** against the predicate defeated: both callers are told they
/// published it and `published_by` is whichever committed last.
#[tokio::test]
async fn two_publishes_of_one_draft_produce_one_publisher() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let created = create(
        &app,
        &token,
        "wf_two_publishes",
        approval_workflow("wf_two_publishes"),
    )
    .await;
    let id = id_of(&created.body["data"]);

    let mut handles = Vec::new();

    for _ in 0..8 {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();

        handles.push(tokio::spawn(async move {
            app.post(
                &format!("{DEFINITIONS}/{id}/publication"),
                Some(&token),
                json!({}),
            )
            .await
        }));
    }

    let mut published_count = 0usize;
    let mut lost = 0usize;

    for handle in handles {
        let response = handle.await.expect("a publish finished");

        match response.status {
            StatusCode::OK => published_count += 1,
            StatusCode::CONFLICT => lost += 1,
            other => panic!(
                "a publish answered {other}, which is neither publishing nor losing: {}",
                response.body
            ),
        }
    }

    assert_eq!(
        published_count, 1,
        "{published_count} callers published one draft"
    );
    assert_eq!(lost, 7);
}

/// **A workflow in another tenant cannot be bound to a document type** (M08).
///
/// `lock_bindable_definition` is tenant-scoped, and nothing exercised that: the
/// binding tests all named a definition in the caller's own tenant, where the
/// scope cannot be told from its absence.
///
/// **Seen red** against the predicate defeated: a document type is bound to
/// another tenant's approval chain, and every document of that type routes into
/// a process the tenant cannot see.
#[tokio::test]
async fn a_workflow_in_another_tenant_cannot_be_bound() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    // The definition lives in the *other* tenant, published there.
    let other = common::fixtures::create_tenant(&app.pool, "OTHER-WF", "Other tenant").await;
    let role = common::fixtures::create_role_with_permissions(
        &app.pool,
        other,
        "WF-OTHER-ADMIN",
        &[
            "workflow:definition:create",
            "workflow:definition:read",
            "workflow:definition:publish",
        ],
    )
    .await;
    common::fixtures::create_user(
        &app.pool,
        other,
        "other.wf.admin",
        "other.wf.admin@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let outsider = app
        .sign_in_to("OTHER-WF", "other.wf.admin", common::ADMIN_PASSWORD)
        .await;

    let elsewhere = published(&app, &outsider, "wf_elsewhere").await;

    let refused = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_FOREIGN_WORKFLOW",
                "name": "PR_FOREIGN_WORKFLOW",
                "workflows": [{ "workflowDefinitionId": elsewhere }],
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a type was bound to another tenant's workflow: {}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"], "NOT_FOUND",
        "{}",
        refused.body
    );

    // And one in this tenant binds, so the refusal is about the tenant rather
    // than about every binding being refused.
    let here = published(&app, &token, "wf_here").await;
    let accepted = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_OWN_WORKFLOW",
                "name": "PR_OWN_WORKFLOW",
                "workflows": [{ "workflowDefinitionId": here }],
            }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::CREATED, "{}", accepted.body);
}

/// **A workflow in another tenant cannot be retired** (M10).
///
/// `soft_delete` is tenant-scoped and nothing reached it: every delete test
/// deleted the caller's own definition.
///
/// **Seen red** against the predicate defeated: the outsider retires the first
/// tenant's approval chain and every document type bound to it stops routing.
#[tokio::test]
async fn a_workflow_in_another_tenant_cannot_be_retired() {
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;

    let mine = published(&app, &token, "wf_mine").await;

    let other = common::fixtures::create_tenant(&app.pool, "OTHER-DEL", "Other tenant").await;
    let role = common::fixtures::create_role_with_permissions(
        &app.pool,
        other,
        "WF-OTHER-DELETER",
        &["workflow:definition:delete", "workflow:definition:read"],
    )
    .await;
    common::fixtures::create_user(
        &app.pool,
        other,
        "other.deleter",
        "other.deleter@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let outsider = app
        .sign_in_to("OTHER-DEL", "other.deleter", common::ADMIN_PASSWORD)
        .await;

    let refused = app
        .delete(&format!("{DEFINITIONS}/{mine}"), Some(&outsider))
        .await;

    assert_eq!(
        refused.status,
        StatusCode::NOT_FOUND,
        "another tenant's workflow was retired: {}",
        refused.body
    );

    let deleted_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT deleted_at FROM workflow_definitions WHERE id = $1")
            .bind(mine)
            .fetch_one(&app.pool)
            .await
            .expect("read the row back");

    assert!(deleted_at.is_none(), "the row was soft-deleted anyway");
}

/// **A retired workflow is not readable** (M02).
///
/// `find_definition` carries `deleted_at IS NULL`, and nothing exercised it: the
/// delete tests asserted the `DELETE` was accepted and never read the row back,
/// so a definition that was soft-deleted and still served would have looked
/// identical from every test in the suite.
///
/// It matters more here than on most tables: a retired definition that still
/// reads is a definition an administrator can still **bind**, and a binding is
/// what routes every future document of a type.
///
/// **Seen red** against `find_definition`'s `deleted_at IS NULL` weakened to
/// `(deleted_at IS NULL OR TRUE)`: the retired workflow reads back with `200`
/// and can be bound to a document type.
#[tokio::test]
async fn a_retired_workflow_is_not_readable_or_bindable() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let id = published(&app, &token, "wf_retired").await;
    let spare = published(&app, &token, "wf_still_here").await;

    let deleted = app
        .delete(&format!("{DEFINITIONS}/{id}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let read = app.get(&format!("{DEFINITIONS}/{id}"), Some(&token)).await;
    assert_eq!(
        read.status,
        StatusCode::NOT_FOUND,
        "a retired workflow was still readable: {}",
        read.body
    );

    // And it is gone from the page, where a leak is least visible.
    let listed = app.get(DEFINITIONS, Some(&token)).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert!(
        !listed.body.to_string().contains("wf_retired"),
        "a retired workflow appeared in the list: {}",
        listed.body
    );

    // The one that was not retired is still there, so the assertions above are
    // not green because the read returns nothing at all.
    let survivor = app
        .get(&format!("{DEFINITIONS}/{spare}"), Some(&token))
        .await;
    assert_eq!(survivor.status, StatusCode::OK, "{}", survivor.body);

    // And binding the retired one is refused for the same reason: the binding
    // check reads through a statement carrying the same predicate.
    let refused = app
        .post(
            "/api/v1/document-types",
            Some(&token),
            json!({
                "typeCode": "PR_RETIRED_WORKFLOW",
                "name": "PR_RETIRED_WORKFLOW",
                "workflows": [{ "workflowDefinitionId": id }],
            }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a retired workflow was bound to a document type: {}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "NOT_FOUND");
}
