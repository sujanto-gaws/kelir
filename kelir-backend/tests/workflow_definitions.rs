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

/// Creates `WF-APPROVER`, the role [`approval_workflow`] names, in the tenant
/// that owns definition `id`, unless it is live there already: a publish
/// refuses a definition naming a role that is not live (D-111, #572).
async fn approver_role_for(app: &TestApp, id: Uuid) {
    let tenant: Uuid =
        sqlx::query_scalar("SELECT tenant_id FROM workflow_definitions WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("read the definition's tenant");

    let live: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM roles WHERE tenant_id = $1 AND role_code = 'WF-APPROVER' \
         AND deleted_at IS NULL",
    )
    .bind(tenant)
    .fetch_optional(&app.pool)
    .await
    .expect("look for the approver role");

    if live.is_none() {
        common::fixtures::create_role_with_permissions(&app.pool, tenant, "WF-APPROVER", &[]).await;
    }
}

/// Creates and publishes a workflow, failing here rather than at the next
/// assertion if either half was refused.
pub async fn published(app: &TestApp, token: &str, key: &str) -> Uuid {
    let created = create(app, token, key, approval_workflow(key)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let id = id_of(&created.body["data"]);
    approver_role_for(app, id).await;

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
/// `publish_definition` locks the row before it reads it (#572), so the second
/// caller waits, finds the revision `ACTIVE` and is refused with a 409 — the
/// first caller's name is on it, which is correct, because the second call
/// published nothing. `publish` also carries `AND status = 'DRAFT'`, which no
/// longer decides between them.
///
/// **Seen red** against the predicate defeated, before the lock: both callers
/// were told they published it and `published_by` was whichever committed
/// last.
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
    approver_role_for(&app, id).await;

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

// ---------------------------------------------------------------------------
// D-111 (#572) — publish refuses a definition naming a role that is not live
// ---------------------------------------------------------------------------

async fn publish(app: &TestApp, token: &str, id: Uuid) -> common::TestResponse {
    app.post(
        &format!("{DEFINITIONS}/{id}/publication"),
        Some(token),
        json!({}),
    )
    .await
}

/// Saves a definition, which must succeed: save does no role lookup (D-111).
async fn saved(app: &TestApp, token: &str, key: &str, definition: Value) -> Uuid {
    let created = create(app, token, key, definition).await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "save must not check that roles are live: {}",
        created.body
    );
    id_of(&created.body["data"])
}

/// Creates a live role in the system tenant, the one every test here signs in to.
async fn role(app: &TestApp, code: &str) -> Uuid {
    common::fixtures::create_role_with_permissions(
        &app.pool,
        common::fixtures::SYSTEM_TENANT_ID,
        code,
        &[],
    )
    .await
}

/// The `(path, code)` of every detail a refusal carries.
fn refusals(response: &common::TestResponse) -> Vec<(String, String)> {
    response.body["error"]["details"]
        .as_array()
        .unwrap_or_else(|| panic!("a refusal with details: {}", response.body))
        .iter()
        .map(|detail| {
            (
                detail["path"].as_str().unwrap_or_default().to_owned(),
                detail["code"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn not_live(paths: &[&str]) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), "ROLE_NOT_LIVE".to_owned()))
        .collect()
}

async fn status_of(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM workflow_definitions WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("read the status back")
}

/// **A deleted role is refused at publish, at every path that names it.**
///
/// `approval_workflow` names `WF-APPROVER` three times: the task's assignment in
/// object form, and both edges as `ROLE:WF-APPROVER`. Each is a detail, so the
/// author is told about all three at once.
///
/// **Seen red** against `publish_definition` without the role read: the
/// definition reaches `ACTIVE`, and its first submission would be refused as
/// `ASSIGNMENT_UNRESOLVED` in front of the submitter.
#[tokio::test]
async fn a_definition_naming_a_deleted_role_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_dead_role",
        approval_workflow("wf_dead_role"),
    )
    .await;

    // Through the API: a draft does not hold its roles (D-91 (3)).
    let deleted = app
        .delete(&format!("/api/v1/identity/roles/{approver}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&[
            "definition.states.0.task.assignment.roleCode",
            "definition.transitions.0.allowedBy",
            "definition.transitions.1.allowedBy",
        ])
    );
    assert!(
        refused.body.to_string().contains("`WF-APPROVER`"),
        "the refusal must name the role: {}",
        refused.body
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");

    let projected: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_states WHERE workflow_definition_id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count the projection");
    assert_eq!(projected, 0, "a refused publish wrote its projection");
}

/// **A code no role has ever had is refused the same way**, and a role that is
/// live is not reported beside it.
#[tokio::test]
async fn a_role_code_no_role_has_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_unknown_role");
    definition["states"][0]["task"]["assignment"] =
        json!({ "assigneeType": "ROLE", "roleCode": "WF-NEVER-EXISTED" });

    let id = saved(&app, &token, "wf_unknown_role", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&["definition.states.0.task.assignment.roleCode"])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **The `"ROLE:X"` shorthand is checked as the object form is**, and its path
/// ends at the rule, because the shorthand is a string with no `roleCode`.
///
/// **Seen red** against `role_references` reading object-form rules only: the
/// `REJECT` edge names a role nobody has, and the definition publishes.
#[tokio::test]
async fn a_shorthand_role_that_is_not_live_is_refused_at_its_path() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_shorthand_role");
    definition["transitions"][1]["allowedBy"] = json!("ROLE:WF-NOBODY");

    let id = saved(&app, &token, "wf_shorthand_role", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&["definition.transitions.1.allowedBy"])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **A `DEPARTMENT_ROLE` names a role too**, in an `assignment` and in an
/// `allowedBy`, and once its role exists the same draft publishes.
#[tokio::test]
async fn a_department_role_that_is_not_live_is_refused_until_it_is() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let scoped = json!({
        "assigneeType": "DEPARTMENT_ROLE",
        "roleCode": "WF-DEPT-HEAD",
        "departmentScope": "REQUESTED_DEPARTMENT"
    });
    let mut definition = approval_workflow("wf_department_role");
    definition["states"][0]["task"]["assignment"] = scoped.clone();
    definition["transitions"][0]["allowedBy"] = scoped;

    let id = saved(&app, &token, "wf_department_role", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&[
            "definition.states.0.task.assignment.roleCode",
            "definition.transitions.0.allowedBy.roleCode",
        ])
    );

    role(&app, "WF-DEPT-HEAD").await;

    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **A role is live in its own tenant only.** Another tenant's role with the
/// code is not one this tenant's tasks can be offered to.
///
/// **Seen red** against `lock_live_roles` without its `tenant_id` predicate:
/// the definition publishes on the strength of a role in `TNT-572`.
#[tokio::test]
async fn a_role_live_only_in_another_tenant_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let elsewhere = common::fixtures::create_tenant(&app.pool, "TNT-572", "Elsewhere").await;
    common::fixtures::create_role_with_permissions(&app.pool, elsewhere, "WF-APPROVER", &[]).await;

    let id = saved(
        &app,
        &token,
        "wf_foreign_role",
        approval_workflow("wf_foreign_role"),
    )
    .await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refusals(&refused).len(), 3, "{}", refused.body);
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **A publish racing a role delete is refused** (#572's race).
///
/// The delete is `identity::service::delete_role`'s own statements, held
/// uncommitted: the role row locked `FOR UPDATE`, then soft-deleted. The
/// publish is sent while that transaction is open, and the delete commits
/// after.
///
/// **This proves the refusal, not the serialisation.** Whether the publish's
/// `FOR KEY SHARE` waits on the delete's lock and then finds the row no longer
/// live, or runs after the commit and finds it gone, the answer is the same, so
/// the test does not depend on how the two interleave. What it rules out is the
/// publish taking the role as live while its delete is in flight, which a read
/// without the lock does: it sees the row as it was before the uncommitted
/// delete, and the publish commits naming a role that is gone a moment later.
///
/// **Seen red** against `lock_live_roles` without `FOR KEY SHARE`: the
/// publish answers 200.
#[tokio::test]
async fn a_publish_racing_a_role_delete_is_refused() {
    use kelir_backend::modules::identity::repository as identity_repo;

    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_race_role",
        approval_workflow("wf_race_role"),
    )
    .await;

    let mut deleting = app.pool.begin().await.expect("a transaction");
    let is_system = identity_repo::lock_role_for_delete(&mut deleting, tenant, approver)
        .await
        .expect("the lock runs");
    assert_eq!(is_system, Some(false), "the role is live and deletable");
    let deleted = identity_repo::soft_delete_role(&mut *deleting, tenant, approver, None)
        .await
        .expect("the delete runs");
    assert_eq!(deleted, 1);

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    deleting.commit().await.expect("the delete commits");

    let refused = publishing.await.expect("the publish finished");

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a publish racing the delete of its role reached ACTIVE: {}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&[
            "definition.states.0.task.assignment.roleCode",
            "definition.transitions.0.allowedBy",
            "definition.transitions.1.allowedBy",
        ])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

// ---------------------------------------------------------------------------
// D-111 (#572) — the test-engineer campaign: every place, the races both ways
// ---------------------------------------------------------------------------

/// A valid definition naming a role in every place JWSS lets one stand, each
/// with its own code, plus the escalation the engine never executes:
///
/// | code | where |
/// |---|---|
/// | `WF-SYNC-TASK` | `states.0.task.assignment`, object `ROLE` |
/// | `WF-SYNC-ESCALATION` | `states.0.task.escalation.assignment`, never resolved |
/// | `WF-SYNC-DEPT-TASK` | `states.1.task.assignment`, object `DEPARTMENT_ROLE` |
/// | `WF-SYNC-SHORT` | `transitions.0` and `transitions.3`, `"ROLE:X"` |
/// | `WF-SYNC-EDGE` | `transitions.1.allowedBy`, object `ROLE` |
/// | `WF-SYNC-DEPT-EDGE` | `transitions.2.allowedBy`, object `DEPARTMENT_ROLE` |
///
/// `task.assignment` is object-only in the meta-schema; only `allowedBy` takes
/// the shorthand.
fn every_place_workflow(key: &str) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "Every place a role may be named",
        "initialState": "MANAGER_APPROVAL",
        "states": [
            { "code": "MANAGER_APPROVAL", "name": "Manager approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "manager_approval", "taskName": "Approve",
                        "assignment": { "assigneeType": "ROLE", "roleCode": "WF-SYNC-TASK" },
                        "escalation": { "afterHours": 24,
                                        "assignment": { "assigneeType": "ROLE",
                                                        "roleCode": "WF-SYNC-ESCALATION" } } } },
            { "code": "DEPARTMENT_APPROVAL", "name": "Department approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "department_approval", "taskName": "Approve",
                        "assignment": { "assigneeType": "DEPARTMENT_ROLE",
                                        "roleCode": "WF-SYNC-DEPT-TASK",
                                        "departmentScope": "OWNER_DEPARTMENT" } } },
            { "code": "OWNER_REVIEW", "name": "Owner review", "mapsToDocumentStatus": "IN_REVIEW",
              "task": { "taskDefinitionKey": "owner_review", "taskName": "Review",
                        "assignment": { "assigneeType": "OWNER" } } },
            { "code": "COMPLETED", "name": "Completed", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true },
            { "code": "REJECTED", "name": "Rejected", "mapsToDocumentStatus": "REJECTED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "MANAGER_APPROVAL", "to": "DEPARTMENT_APPROVAL", "action": "APPROVE",
              "allowedBy": "ROLE:WF-SYNC-SHORT" },
            { "from": "MANAGER_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": { "assigneeType": "ROLE", "roleCode": "WF-SYNC-EDGE" } },
            { "from": "DEPARTMENT_APPROVAL", "to": "OWNER_REVIEW", "action": "APPROVE",
              "allowedBy": { "assigneeType": "DEPARTMENT_ROLE", "roleCode": "WF-SYNC-DEPT-EDGE",
                             "departmentScope": "OWNER_DEPARTMENT" } },
            { "from": "DEPARTMENT_APPROVAL", "to": "REJECTED", "action": "REJECT",
              "allowedBy": "ROLE:WF-SYNC-SHORT" },
            { "from": "OWNER_REVIEW", "to": "COMPLETED", "action": "COMPLETE",
              "allowedBy": "OWNER" }
        ]
    })
}

/// How many sessions in this test's database are waiting on a lock.
async fn lock_waiters(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity \
         WHERE datname = current_database() AND wait_event_type = 'Lock'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("count the sessions waiting on a lock")
}

/// Waits until `count` sessions are waiting on a lock, and answers whether
/// they were. It gives up, answering `false`, when `running` finishes first,
/// which is what a request that was never made to wait does, or after ten
/// seconds. Polled rather than slept, so the interleaving is observed rather
/// than assumed (coding standard §2.5's technique).
async fn waited_on_a_lock<T>(
    app: &TestApp,
    count: i64,
    running: &tokio::task::JoinHandle<T>,
) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

    while std::time::Instant::now() < deadline {
        if lock_waiters(app).await >= count {
            return true;
        }

        if running.is_finished() {
            return false;
        }

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    false
}

/// **`graph::role_references` and `definitions_naming_role` cover the same
/// places** (#572 and D-91 (3)).
///
/// They are the two halves of one guarantee: the publish refuses a role the
/// first finds dead, and the delete refuses a role the second finds named. A
/// place one reads and the other does not is a role that can be deleted from
/// under a published revision, or one a publish never checks. One definition
/// naming a distinct role in every place is published, and each role is put to
/// both.
///
/// **The escalation's role is in neither**, which is the rule both state
/// (nothing executes `escalation.assignment`, JWSS §3.1); a role nobody names
/// is the control.
#[tokio::test]
async fn role_references_and_definitions_naming_role_agree_on_every_place() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    let codes = [
        "WF-SYNC-TASK",
        "WF-SYNC-ESCALATION",
        "WF-SYNC-DEPT-TASK",
        "WF-SYNC-SHORT",
        "WF-SYNC-EDGE",
        "WF-SYNC-DEPT-EDGE",
        "WF-SYNC-UNNAMED",
    ];
    let mut roles = Vec::new();

    for code in codes {
        roles.push((code, role(&app, code).await));
    }

    let definition = every_place_workflow("wf_every_place");
    let id = saved(&app, &token, "wf_every_place", definition.clone()).await;
    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);

    let checked: std::collections::BTreeSet<String> =
        kelir_backend::modules::workflow::domain::role_references(&definition)
            .into_iter()
            .map(|reference| reference.role_code)
            .collect();

    let mut held = std::collections::BTreeSet::new();

    for (code, role_id) in &roles {
        let naming = definition_repo::definitions_naming_role(&app.pool, tenant, *role_id)
            .await
            .expect("ask which revisions name the role");

        if !naming.is_empty() {
            held.insert((*code).to_owned());
        }
    }

    assert_eq!(
        checked, held,
        "the places a publish checks and the places a delete is refused over differ"
    );

    let expected: std::collections::BTreeSet<String> = [
        "WF-SYNC-TASK",
        "WF-SYNC-DEPT-TASK",
        "WF-SYNC-SHORT",
        "WF-SYNC-EDGE",
        "WF-SYNC-DEPT-EDGE",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(checked, expected, "a place the fixture names was not read");
}

/// **One dead role named in several places is refused at each, a live one is
/// not, and the refusal has the S-rule shape** (#572 criterion 1).
///
/// Of [`every_place_workflow`]'s roles, three are live, `WF-SYNC-DEPT-TASK` is
/// deleted through the API, and `WF-SYNC-SHORT` never existed and is named
/// twice. The escalation's role never existed either, and is not reported.
/// The details come in document order, states before transitions, one per
/// place; the envelope, status and detail keys are those of an S-rule refusal.
#[tokio::test]
async fn a_dead_role_named_in_several_places_is_refused_at_each_in_the_s_rule_shape() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for code in ["WF-SYNC-TASK", "WF-SYNC-EDGE", "WF-SYNC-DEPT-EDGE"] {
        role(&app, code).await;
    }

    let department_task = role(&app, "WF-SYNC-DEPT-TASK").await;
    let deleted = app
        .delete(
            &format!("/api/v1/identity/roles/{department_task}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let id = saved(
        &app,
        &token,
        "wf_several_places",
        every_place_workflow("wf_several_places"),
    )
    .await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&[
            "definition.states.1.task.assignment.roleCode",
            "definition.transitions.0.allowedBy",
            "definition.transitions.3.allowedBy",
        ])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");

    // The shape: an S-rule refusal, here a state nothing routes to, refused at
    // save, against the liveness refusal.
    let mut orphaned = approval_workflow("wf_shape_orphan");
    orphaned["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({ "code": "ORPHAN", "name": "Orphan", "mapsToDocumentStatus": "IN_REVIEW" }));
    let s_rule = create(&app, &token, "wf_shape_orphan", orphaned).await;
    assert_eq!(s_rule.status, refused.status, "{}", s_rule.body);

    let keys = |value: &Value| -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .unwrap_or_else(|| panic!("an object: {value}"))
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };

    assert_eq!(refused.body["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(refused.body["error"]["code"], s_rule.body["error"]["code"]);
    assert_eq!(keys(&refused.body["error"]), keys(&s_rule.body["error"]));

    for detail in refused.body["error"]["details"]
        .as_array()
        .expect("details")
    {
        assert_eq!(keys(detail), keys(&s_rule.body["error"]["details"][0]));
        assert_eq!(detail["rule"], "liveRole");
        assert!(
            detail["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{detail}"
        );
    }
}

/// **A role deleted and recreated with its code is live again**, and a
/// definition naming the code publishes.
///
/// `uq_roles_tenant_id_role_code` is partial on `deleted_at IS NULL`, so the
/// code is reusable; the publish must read the new row, and must not be put
/// off by the soft-deleted one beside it.
#[tokio::test]
async fn a_role_deleted_and_recreated_with_its_code_is_live_again() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_recreated_role",
        approval_workflow("wf_recreated_role"),
    )
    .await;

    let deleted = app
        .delete(&format!("/api/v1/identity/roles/{first}"), Some(&token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let refused = publish(&app, &token, id).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let recreated = app
        .post(
            "/api/v1/identity/roles",
            Some(&token),
            json!({ "roleCode": "WF-APPROVER", "name": "Approver, again" }),
        )
        .await;
    assert_eq!(recreated.status, StatusCode::CREATED, "{}", recreated.body);
    assert_ne!(id_of(&recreated.body["data"]), first);

    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **A role code is matched exactly**: a code differing in case, or carrying
/// a trailing space, is not the live role.
///
/// `assignment::direct` resolves by exact code, and role codes are stored
/// trimmed, so a publish that matched loosely would accept a definition whose
/// first submission is refused as `ASSIGNMENT_UNRESOLVED`.
#[tokio::test]
async fn a_role_code_differing_in_case_or_space_is_not_live() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_role_case");
    definition["states"][0]["task"]["assignment"]["roleCode"] = json!("wf-approver");
    definition["transitions"][1]["allowedBy"] =
        json!({ "assigneeType": "ROLE", "roleCode": "WF-APPROVER " });

    let id = saved(&app, &token, "wf_role_case", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&[
            "definition.states.0.task.assignment.roleCode",
            "definition.transitions.1.allowedBy.roleCode",
        ])
    );
}

/// **A definition naming no role publishes**, in a tenant whose roles have
/// nothing to do with it: the owner decides every step.
#[tokio::test]
async fn a_definition_naming_no_role_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_no_role");
    definition["states"][0]["task"]["assignment"] = json!({ "assigneeType": "OWNER" });
    definition["transitions"][0]["allowedBy"] = json!("OWNER");
    definition["transitions"][1]["allowedBy"] = json!({ "assigneeType": "OWNER" });

    let id = saved(&app, &token, "wf_no_role", definition).await;
    let published = publish(&app, &token, id).await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **An escalation naming a dead role does not stop a publish.**
///
/// JWSS §3.1 stores `escalation.assignment` and nothing executes it: `TaskSpec`
/// has no escalation, and `workflow_escalations` is read by nothing. So its
/// role resolves nowhere, and D-111 does not check it. Whatever schedules
/// escalations (FR-WF-010) must add it here and to `definitions_naming_role`,
/// and this test is the one that will then go red.
#[tokio::test]
async fn an_escalation_naming_a_dead_role_does_not_stop_a_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_escalation_role");
    definition["states"][0]["task"]["escalation"] = json!({
        "afterHours": 24,
        "assignment": { "assigneeType": "ROLE", "roleCode": "WF-NOBODY-ESCALATES" }
    });

    let id = saved(&app, &token, "wf_escalation_role", definition).await;
    let published = publish(&app, &token, id).await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
}

/// **An edit naming a dead role is saved, and refused only at publish**
/// (D-111; ADR-0019 is unchanged). `saved` proves it for a create; this is the
/// other save path, `PUT`.
#[tokio::test]
async fn an_edit_naming_a_dead_role_is_saved_and_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_edited_role",
        approval_workflow("wf_edited_role"),
    )
    .await;

    let mut definition = approval_workflow("wf_edited_role");
    definition["transitions"][0]["allowedBy"] = json!("ROLE:WF-GHOST");

    let edited = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": definition }),
        )
        .await;
    assert_eq!(
        edited.status,
        StatusCode::OK,
        "an edit must not check that roles are live: {}",
        edited.body
    );

    let refused = publish(&app, &token, id).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_live(&["definition.transitions.0.allowedBy"])
    );
}

/// **A publish sent while a role delete holds its lock waits for it, and is
/// then refused** (#572's race, the delete first).
///
/// The builder's `a_publish_racing_a_role_delete_is_refused` sleeps 200 ms
/// and commits, so it proves the refusal whichever way the two interleave, but
/// it goes red against a publish without `FOR KEY SHARE` only when the publish
/// reaches its read inside those 200 ms. This one observes the publish
/// waiting on the delete's row lock before it commits the delete, so it proves
/// the serialisation too: the role row exists, soft-deleted, so there is a row
/// to wait on.
///
/// **Seen red** against `lock_live_roles` without `FOR KEY SHARE`: the
/// publish never waits.
#[tokio::test]
async fn a_publish_waits_on_an_uncommitted_role_delete_and_is_refused() {
    use kelir_backend::modules::identity::repository as identity_repo;

    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_waits_on_delete",
        approval_workflow("wf_waits_on_delete"),
    )
    .await;

    let mut deleting = app.pool.begin().await.expect("a transaction");
    let is_system = identity_repo::lock_role_for_delete(&mut deleting, tenant, approver)
        .await
        .expect("the lock runs");
    assert_eq!(is_system, Some(false));
    identity_repo::soft_delete_role(&mut *deleting, tenant, approver, None)
        .await
        .expect("the delete runs");

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    let waited = waited_on_a_lock(&app, 1, &publishing).await;
    deleting.commit().await.expect("the delete commits");
    let refused = publishing.await.expect("the publish finished");

    assert!(
        waited,
        "the publish did not wait on the delete's row lock; it answered {}: {}",
        refused.status, refused.body
    );
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refusals(&refused).len(), 3, "{}", refused.body);
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **A role delete sent while a publish holds the role waits for it, and is
/// then refused** (#572's race, the publish first; D-91 (3)).
///
/// The publish is held open after `lock_live_roles` by a transaction holding
/// `workflow_transitions` in `SHARE` mode, which the projection's `DELETE`
/// waits on. (Holding the definition's row no longer works: the publish locks
/// that row first, before its roles, #572.) The delete's own read of
/// `workflow_transitions` is not blocked by `SHARE`. The delete is then sent
/// through the API. It must wait
/// on the publish's `FOR KEY SHARE`, then, once the publish commits, find the
/// revision `ACTIVE` and refuse as `ROLE_NAMED_BY_PUBLISHED_DEFINITION`. The
/// two cannot both succeed.
///
/// **Seen red** against `lock_live_roles` without `FOR KEY SHARE`, and
/// against `lock_role_for_delete` taking `FOR NO KEY UPDATE`: the delete does
/// not wait, finds the revision still a draft, and commits; the publish then
/// commits a revision naming a role that is gone.
#[tokio::test]
async fn a_role_delete_waits_on_a_publish_in_flight_and_is_refused() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_delete_waits",
        approval_workflow("wf_delete_waits"),
    )
    .await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    sqlx::query("LOCK TABLE workflow_transitions IN SHARE MODE")
        .execute(&mut *holding)
        .await
        .expect("hold the projection");

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    assert!(
        waited_on_a_lock(&app, 1, &publishing).await,
        "the publish did not reach its projection"
    );

    let deleting = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.delete(&format!("/api/v1/identity/roles/{approver}"), Some(&token))
                .await
        })
    };

    let delete_waited = waited_on_a_lock(&app, 2, &deleting).await;
    holding.rollback().await.expect("release the projection");

    let published = publishing.await.expect("the publish finished");
    let deleted = deleting.await.expect("the delete finished");

    assert!(
        delete_waited,
        "the delete did not wait on the publish's role lock; publish {} {}, delete {} {}",
        published.status, published.body, deleted.status, deleted.body
    );
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(
        deleted.status,
        StatusCode::CONFLICT,
        "a role was deleted from under a publish in flight: {}",
        deleted.body
    );
    assert_eq!(
        deleted.body["error"]["code"], "ROLE_NAMED_BY_PUBLISHED_DEFINITION",
        "{}",
        deleted.body
    );
    assert!(common::fixtures::live_role(
        &app.pool,
        common::fixtures::SYSTEM_TENANT_ID,
        "WF-APPROVER"
    )
    .await
    .is_some());
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **A draft edited while its publish is in flight is not published
/// unchecked** (#572 criterion 1).
///
/// `publish_definition` reads the definition with the pool, before its
/// transaction, checks the roles that copy names, and then flips the row to
/// `ACTIVE` with `WHERE status = 'DRAFT'`, which does not ask whether the
/// definition is still the one it checked. An edit landing between the read
/// and the write is published without its roles being checked (and with the
/// projection of the revision before it).
///
/// The gap is held open by a transaction holding the role row `FOR UPDATE`,
/// which the publish's `FOR KEY SHARE` waits on after its read. The edit then
/// renames the task's role to one no role has.
///
/// **The publish now holds the definition row `FOR UPDATE` before it reads
/// it** (`repo::lock_for_publish`), so the edit waits behind the publish, finds
/// the revision `ACTIVE` when it may go on, and is refused as `NOT_A_DRAFT` (422). The
/// edit is sent from its own task for that reason: awaited here, it would wait
/// on the publish, which waits on the role this test releases after it.
///
/// **Seen red** against the publish reading the definition with the pool
/// before its transaction, as it did: the revision reaches `ACTIVE` naming
/// `WF-GHOST`.
#[tokio::test]
async fn a_draft_edited_while_its_publish_is_in_flight_is_not_published_unchecked() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_edited_mid_publish",
        approval_workflow("wf_edited_mid_publish"),
    )
    .await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE")
        .bind(approver)
        .execute(&mut *holding)
        .await
        .expect("hold the role row");

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    assert!(
        waited_on_a_lock(&app, 1, &publishing).await,
        "the publish did not reach its role read"
    );

    let mut edited_definition = approval_workflow("wf_edited_mid_publish");
    edited_definition["states"][0]["task"]["assignment"]["roleCode"] = json!("WF-GHOST");

    let editing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.put(
                &format!("{DEFINITIONS}/{id}"),
                Some(&token),
                json!({ "definition": edited_definition }),
            )
            .await
        })
    };

    // The edit waits on the definition row the publish holds; one that
    // finishes instead edited the draft under the publish.
    let edit_waited = waited_on_a_lock(&app, 2, &editing).await;

    holding.rollback().await.expect("release the role row");
    let published = publishing.await.expect("the publish finished");
    let edited = editing.await.expect("the edit finished");

    let (status, named): (String, Option<String>) = sqlx::query_as(
        "SELECT status, definition_json #>> '{states,0,task,assignment,roleCode}' \
         FROM workflow_definitions WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("read the revision back");

    assert!(
        !(status == "ACTIVE" && named.as_deref() == Some("WF-GHOST")),
        "a revision naming a role no role has reached ACTIVE: edit {} {}, publish {} {}",
        edited.status,
        edited.body,
        published.status,
        published.body
    );

    // And the order the lock gives: the revision it checked is published, and
    // the edit is refused because that revision is no longer a draft.
    assert!(edit_waited, "the edit did not wait on the publish");
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(
        edited.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        edited.body
    );
    assert_eq!(
        edited.body["error"]["details"][0]["code"], "NOT_A_DRAFT",
        "{}",
        edited.body
    );
    assert_eq!(
        (status.as_str(), named.as_deref()),
        ("ACTIVE", Some("WF-APPROVER"))
    );
}

/// **A role edit is not held up by a publish in flight**: the publish's lock
/// is `FOR KEY SHARE`, which keeps the row from being deleted and nothing
/// else.
///
/// The publish is held after `lock_live_roles` as in
/// `a_role_delete_waits_on_a_publish_in_flight_and_is_refused`. A rename of
/// the role it holds takes `FOR NO KEY UPDATE`, which `FOR KEY SHARE` does not
/// conflict with, so the rename finishes while the publish is still open.
///
/// **Seen red** against `lock_live_roles` taking `FOR SHARE`, which also
/// stops the delete but makes every edit of a named role wait on every
/// publish naming it.
#[tokio::test]
async fn a_role_edit_is_not_held_up_by_a_publish_in_flight() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;

    let approver = role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_edit_not_held",
        approval_workflow("wf_edit_not_held"),
    )
    .await;

    let mut holding = app.pool.begin().await.expect("a transaction");
    sqlx::query("LOCK TABLE workflow_transitions IN SHARE MODE")
        .execute(&mut *holding)
        .await
        .expect("hold the projection");

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    assert!(
        waited_on_a_lock(&app, 1, &publishing).await,
        "the publish did not reach its projection"
    );

    let editing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move {
            app.put(
                &format!("/api/v1/identity/roles/{approver}"),
                Some(&token),
                json!({ "name": "Approver, renamed" }),
            )
            .await
        })
    };

    let edit_waited = waited_on_a_lock(&app, 2, &editing).await;
    let edit_finished_first = editing.is_finished();

    // **The pause point is sound only if the publish already holds its role
    // lock there.** `LOCK TABLE workflow_transitions IN SHARE MODE` stops the
    // publish at the projection's `DELETE`, which runs after `lock_live_roles`
    // today. Were the projection moved before the role read, the publish would
    // pause holding no role lock and the edit above would pass vacuously, so
    // this asks PostgreSQL: a `FOR UPDATE NOWAIT` on the role must fail with
    // `lock_not_available` (55P03) while the publish is paused.
    let mut probe = app.pool.begin().await.expect("a transaction");
    let probed = sqlx::query("SELECT id FROM roles WHERE id = $1 FOR UPDATE NOWAIT")
        .bind(approver)
        .execute(&mut *probe)
        .await;
    probe.rollback().await.expect("end the probe");
    let held_by_the_publish = matches!(
        &probed,
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03")
    );

    holding.rollback().await.expect("release the projection");

    let edited = editing.await.expect("the edit finished");
    let published = publishing.await.expect("the publish finished");

    assert!(
        held_by_the_publish,
        "the publish was paused before it locked its roles, so this proves          nothing about its lock: {probed:?}"
    );
    assert!(
        !edit_waited && edit_finished_first,
        "a role edit waited on a publish naming the role: edit {} {}",
        edited.status,
        edited.body
    );
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
}

/// **A draft edited before its publish takes the row is checked as edited**
/// (#572, the fix in `efd60d7`).
///
/// `a_draft_edited_while_its_publish_is_in_flight_is_not_published_unchecked`
/// pauses the publish at its role read, which is after `lock_for_publish`, so
/// it cannot see a publish that reads the definition before it locks the row.
/// This one opens that gap: the edit is made, uncommitted, by the transaction
/// that holds the row, so the publish waits at `lock_for_publish`, and the
/// edit, renaming the task's role to one no role has, commits while it waits.
/// A publish that reads after it locks sees the edit and refuses it.
///
/// **Seen red** against `before` read with the pool before the transaction
/// (the revision reaches `ACTIVE` naming `WF-GHOST`), against
/// `lock_for_publish` removed, and against it without `FOR UPDATE`.
#[tokio::test]
async fn a_draft_edited_before_its_publish_takes_the_row_is_checked_as_edited() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_edited_before_lock",
        approval_workflow("wf_edited_before_lock"),
    )
    .await;

    let mut ghost = approval_workflow("wf_edited_before_lock");
    ghost["states"][0]["task"]["assignment"]["roleCode"] = json!("WF-GHOST");

    let mut editing = app.pool.begin().await.expect("a transaction");
    let edited = definition_repo::update_draft(
        &mut *editing,
        tenant,
        id,
        &definition_repo::DefinitionFields {
            name: None,
            description: None,
            definition_json: Some(&ghost),
            initial_state: None,
            jwss_version: None,
        },
        None,
    )
    .await
    .expect("the edit runs");
    assert_eq!(edited, 1);

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    let waited = waited_on_a_lock(&app, 1, &publishing).await;
    editing.commit().await.expect("the edit commits");
    let published = publishing.await.expect("the publish finished");

    assert!(
        waited,
        "the publish did not wait on the edit's row: {} {}",
        published.status, published.body
    );
    assert_eq!(
        published.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a publish checked a copy older than the one it published: {}",
        published.body
    );
    assert_eq!(
        refusals(&published),
        not_live(&["definition.states.0.task.assignment.roleCode"])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

// ---------------------------------------------------------------------------
// D-114 (#613) — publish refuses a `departmentScope` on a `ROLE` rule
// ---------------------------------------------------------------------------

/// A `ROLE` rule naming [`approval_workflow`]'s role, with a scope it does not
/// take.
fn scoped_role(scope: &str) -> Value {
    json!({ "assigneeType": "ROLE", "roleCode": "WF-APPROVER", "departmentScope": scope })
}

fn takes_no_scope(paths: &[&str]) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), "ROLE_TAKES_NO_SCOPE".to_owned()))
        .collect()
}

/// How many rows the publish's projection wrote for definition `id`.
async fn projected_rows(app: &TestApp, id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workflow_states WHERE workflow_definition_id = $1) \
              + (SELECT count(*) FROM workflow_transitions WHERE workflow_definition_id = $1)",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("count the projection")
}

/// Writes `value` at `pointer` (a `{a,b,c}` text path) of a stored definition,
/// behind the API's back, and reads it back so the fixture is known to hold.
async fn stored_with(app: &TestApp, id: Uuid, pointer: &str, value: Value) {
    let updated = sqlx::query(
        "UPDATE workflow_definitions SET definition_json = jsonb_set(definition_json, $2::text[], $3) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(pointer)
    .bind(&value)
    .execute(&app.pool)
    .await
    .expect("write the definition behind the API's back");
    assert_eq!(updated.rows_affected(), 1);

    let stored: Value = sqlx::query_scalar(
        "SELECT definition_json #> $2::text[] FROM workflow_definitions WHERE id = $1",
    )
    .bind(id)
    .bind(pointer)
    .fetch_one(&app.pool)
    .await
    .expect("read the definition back");
    assert_eq!(stored, value, "the fixture did not store the scope");
}

/// **A scope on a `ROLE` assignment is refused at publish** (**D-114** A, #613;
/// JWSS §5.3). The engine resolves a `ROLE` rule tenant-wide and never reads its
/// scope, so a definition saying "the approver of the requested department"
/// would offer the task to every approver in the tenant. The save takes it, and
/// the publish, which is where the author is, says to use `DEPARTMENT_ROLE`.
#[tokio::test]
async fn a_role_assignment_with_a_department_scope_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_role_scope");
    definition["states"][0]["task"]["assignment"] = scoped_role("REQUESTED_DEPARTMENT");

    let id = saved(&app, &token, "wf_role_scope", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a scoped ROLE rule was published: {}",
        refused.body
    );
    assert_eq!(refused.body["success"], false);
    assert_eq!(refused.body["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(
        refusals(&refused),
        takes_no_scope(&["definition.states.0.task.assignment.departmentScope"])
    );

    let detail = &refused.body["error"]["details"][0];
    assert_eq!(detail["rule"], "roleScope");
    let message = detail["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("`REQUESTED_DEPARTMENT`") && message.contains("DEPARTMENT_ROLE"),
        "the refusal must name the scope and the type to use: {message}"
    );

    assert_eq!(status_of(&app, id).await, "DRAFT");
    assert_eq!(
        projected_rows(&app, id).await,
        0,
        "a refused publish projected"
    );
}

/// **Every kind of scope is refused**: the two keywords, a department code, and
/// `""`, which is a key the author wrote rather than no scope.
#[tokio::test]
async fn every_kind_of_scope_on_a_role_rule_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    for (index, scope) in ["REQUESTED_DEPARTMENT", "OWNER_DEPARTMENT", "DEPT-FIN", ""]
        .into_iter()
        .enumerate()
    {
        let key = format!("wf_scope_kind_{index}");
        let mut definition = approval_workflow(&key);
        definition["states"][0]["task"]["assignment"] = scoped_role(scope);

        let id = saved(&app, &token, &key, definition).await;
        let refused = publish(&app, &token, id).await;

        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "`{scope}` on a ROLE rule was published: {}",
            refused.body
        );
        assert_eq!(
            refusals(&refused),
            takes_no_scope(&["definition.states.0.task.assignment.departmentScope"]),
            "`{scope}`"
        );
        assert!(
            refused.body["error"]["details"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains(&format!("`{scope}`"))),
            "the refusal must name `{scope}`: {}",
            refused.body
        );
        assert_eq!(status_of(&app, id).await, "DRAFT", "`{scope}`");
    }
}

/// **A scope on a `ROLE` `allowedBy` is refused the same way**: the decision is
/// checked tenant-wide as the task is offered tenant-wide.
#[tokio::test]
async fn a_role_allowed_by_with_a_department_scope_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_edge_scope");
    definition["transitions"][0]["allowedBy"] = scoped_role("OWNER_DEPARTMENT");

    let id = saved(&app, &token, "wf_edge_scope", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        takes_no_scope(&["definition.transitions.0.allowedBy.departmentScope"])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **A scope in each place is refused at each, in document order, in the
/// S-rule shape**, so an author is told about all of them at once.
#[tokio::test]
async fn a_scope_on_each_role_rule_is_refused_at_each() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_scope_each");
    definition["states"][0]["task"]["assignment"] = scoped_role("DEPT-FIN");
    definition["transitions"][0]["allowedBy"] = scoped_role("REQUESTED_DEPARTMENT");
    definition["transitions"][1]["allowedBy"] = scoped_role("OWNER_DEPARTMENT");

    let id = saved(&app, &token, "wf_scope_each", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        takes_no_scope(&[
            "definition.states.0.task.assignment.departmentScope",
            "definition.transitions.0.allowedBy.departmentScope",
            "definition.transitions.1.allowedBy.departmentScope",
        ])
    );

    // The shape: an S-rule refusal, a state nothing routes to, refused at save.
    let mut orphaned = approval_workflow("wf_scope_orphan");
    orphaned["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({ "code": "ORPHAN", "name": "Orphan", "mapsToDocumentStatus": "IN_REVIEW" }));
    let s_rule = create(&app, &token, "wf_scope_orphan", orphaned).await;
    assert_eq!(s_rule.status, refused.status, "{}", s_rule.body);

    let keys = |value: &Value| -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .unwrap_or_else(|| panic!("an object: {value}"))
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };

    assert_eq!(keys(&refused.body), keys(&s_rule.body));
    assert_eq!(refused.body["error"]["code"], s_rule.body["error"]["code"]);
    assert_eq!(keys(&refused.body["error"]), keys(&s_rule.body["error"]));

    for detail in refused.body["error"]["details"]
        .as_array()
        .expect("details")
    {
        assert_eq!(keys(detail), keys(&s_rule.body["error"]["details"][0]));
        assert_eq!(detail["rule"], "roleScope");
        assert!(
            detail["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
            "{detail}"
        );
    }
}

/// **The shorthand carries no scope, and a colon after the role code is not
/// one.** `"ROLE:X"` is §5.2's string form, so a definition using it publishes
/// as it did. `"ROLE:X:Y"` does not match the meta-schema's shorthand pattern,
/// so it is a shape refusal at save, and at publish if it reached the row
/// another way, and never a scope refusal.
#[tokio::test]
async fn a_shorthand_role_publishes_and_a_colon_suffix_is_not_a_scope() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let id = saved(
        &app,
        &token,
        "wf_shorthand_kept",
        approval_workflow("wf_shorthand_kept"),
    )
    .await;
    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");

    let mut suffixed = approval_workflow("wf_colon_suffix");
    suffixed["transitions"][1]["allowedBy"] = json!("ROLE:WF-APPROVER:DEPT-FIN");

    let at_save = create(&app, &token, "wf_colon_suffix", suffixed.clone()).await;
    assert_eq!(
        at_save.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        at_save.body
    );
    let details = refusals(&at_save);
    assert!(
        details
            .iter()
            .any(|(path, code)| path == "definition.transitions.1.allowedBy"
                && code == "INVALID_DEFINITION"),
        "{}",
        at_save.body
    );
    assert!(
        details
            .iter()
            .all(|(_, code)| code != "ROLE_TAKES_NO_SCOPE"),
        "a colon suffix was read as a scope: {}",
        at_save.body
    );

    // Past the save, the publish's own conformance check refuses it first.
    let smuggled = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO workflow_definitions
            (id, tenant_id, workflow_key, name, version, jwss_version, definition_json,
             initial_state, status)
        VALUES ($1, $2, 'wf_colon_suffix', 'Suffixed', 1, '1.0.0', $3, 'MANAGER_APPROVAL', 'DRAFT')
        "#,
    )
    .bind(smuggled)
    .bind(common::fixtures::SYSTEM_TENANT_ID)
    .bind(&suffixed)
    .execute(&app.pool)
    .await
    .expect("write a definition the API would have refused");

    let at_publish = publish(&app, &token, smuggled).await;
    assert_eq!(
        at_publish.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        at_publish.body
    );
    assert!(
        refusals(&at_publish)
            .iter()
            .all(|(_, code)| code == "INVALID_DEFINITION"),
        "{}",
        at_publish.body
    );
    assert_eq!(status_of(&app, smuggled).await, "DRAFT");
}

/// **A scope and a dead role are told together, the scope first**, and each
/// goes away as it is fixed: the scope removed leaves the dead role, and the
/// role created lets the draft publish. **An S-rule fault hides both**: the
/// conformance check runs first and its refusal is the whole answer.
#[tokio::test]
async fn a_scoped_role_rule_naming_a_dead_role_is_refused_for_both() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // No `WF-APPROVER`.
    let mut definition = approval_workflow("wf_scope_dead");
    definition["states"][0]["task"]["assignment"] = scoped_role("DEPT-FIN");

    let id = saved(&app, &token, "wf_scope_dead", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let mut expected = takes_no_scope(&["definition.states.0.task.assignment.departmentScope"]);
    expected.extend(not_live(&[
        "definition.states.0.task.assignment.roleCode",
        "definition.transitions.0.allowedBy",
        "definition.transitions.1.allowedBy",
    ]));
    assert_eq!(refusals(&refused), expected);

    let unscoped = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": approval_workflow("wf_scope_dead") }),
        )
        .await;
    assert_eq!(unscoped.status, StatusCode::OK, "{}", unscoped.body);

    let still = publish(&app, &token, id).await;
    assert_eq!(
        still.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        still.body
    );
    assert_eq!(
        refusals(&still),
        not_live(&[
            "definition.states.0.task.assignment.roleCode",
            "definition.transitions.0.allowedBy",
            "definition.transitions.1.allowedBy",
        ])
    );

    role(&app, "WF-APPROVER").await;
    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");

    // An S-rule fault beside a scope: a dead end, written behind the API.
    let mut stuck = approval_workflow("wf_scope_stuck");
    stuck["states"][0]["task"]["assignment"] = scoped_role("DEPT-FIN");
    stuck["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({ "code": "STUCK", "name": "Stuck", "mapsToDocumentStatus": "IN_REVIEW" }));
    stuck["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "STUCK", "action": "RETURN",
            "allowedBy": "ROLE:WF-APPROVER"
        }));

    let smuggled = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO workflow_definitions
            (id, tenant_id, workflow_key, name, version, jwss_version, definition_json,
             initial_state, status)
        VALUES ($1, $2, 'wf_scope_stuck', 'Stuck', 1, '1.0.0', $3, 'MANAGER_APPROVAL', 'DRAFT')
        "#,
    )
    .bind(smuggled)
    .bind(common::fixtures::SYSTEM_TENANT_ID)
    .bind(&stuck)
    .execute(&app.pool)
    .await
    .expect("write a definition the API would have refused");

    let s_rule = publish(&app, &token, smuggled).await;
    assert_eq!(
        s_rule.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        s_rule.body
    );
    let details = refusals(&s_rule);
    assert!(
        details.iter().any(|(_, code)| code == "DEAD_END_STATE"),
        "{}",
        s_rule.body
    );
    assert!(
        details
            .iter()
            .all(|(_, code)| code != "ROLE_TAKES_NO_SCOPE"),
        "an S-rule fault did not hide the scope: {}",
        s_rule.body
    );
}

/// **A `DEPARTMENT_ROLE` takes a scope, and still publishes with each kind**,
/// in an assignment and in an `allowedBy`.
#[tokio::test]
async fn a_department_role_with_a_scope_still_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    for (index, scope) in ["REQUESTED_DEPARTMENT", "OWNER_DEPARTMENT", "DEPT-FIN"]
        .into_iter()
        .enumerate()
    {
        let key = format!("wf_department_scope_{index}");
        let rule = json!({
            "assigneeType": "DEPARTMENT_ROLE",
            "roleCode": "WF-APPROVER",
            "departmentScope": scope
        });
        let mut definition = approval_workflow(&key);
        definition["states"][0]["task"]["assignment"] = rule.clone();
        definition["transitions"][0]["allowedBy"] = rule;

        let id = saved(&app, &token, &key, definition).await;
        let published = publish(&app, &token, id).await;

        assert_eq!(
            published.status,
            StatusCode::OK,
            "a DEPARTMENT_ROLE scoped to `{scope}` was refused: {}",
            published.body
        );
        assert_eq!(status_of(&app, id).await, "ACTIVE", "`{scope}`");
    }
}

/// **A draft with a scoped `ROLE` rule is saved, not refused**: a create and
/// an edit both take it. Save stays open for drafts, and the publish is where
/// the scope is refused.
#[tokio::test]
async fn a_draft_with_a_scope_on_a_role_rule_is_saved_not_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let mut definition = approval_workflow("wf_scope_draft");
    definition["states"][0]["task"]["assignment"] = scoped_role("REQUESTED_DEPARTMENT");

    let created = create(&app, &token, "wf_scope_draft", definition).await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "a save refused a scoped ROLE rule: {}",
        created.body
    );
    let id = id_of(&created.body["data"]);

    let mut edited = approval_workflow("wf_scope_draft");
    edited["transitions"][1]["allowedBy"] = scoped_role("OWNER_DEPARTMENT");

    let updated = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": edited }),
        )
        .await;
    assert_eq!(
        updated.status,
        StatusCode::OK,
        "an edit refused a scoped ROLE rule: {}",
        updated.body
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// **A revision cloned from a published definition carrying a scope is a
/// draft, refused at its publish.** Revision 1 got its scope behind the API's
/// back, the shape a revision published before D-114 has; the clone copies it
/// unvalidated, and the author is told when the clone is published. Revision 1
/// is not re-checked and stays `ACTIVE`.
#[tokio::test]
async fn a_revision_of_a_published_definition_carrying_the_scope_is_a_draft_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "wf_scope_revised").await;
    stored_with(
        &app,
        first,
        "{states,0,task,assignment,departmentScope}",
        json!("OWNER_DEPARTMENT"),
    )
    .await;

    let revision = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    assert_eq!(revision.body["data"]["status"], "DRAFT");
    assert_eq!(revision.body["data"]["version"], 2);
    let second = id_of(&revision.body["data"]);

    let refused = publish(&app, &token, second).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a revision carrying a scoped ROLE rule was published: {}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        takes_no_scope(&["definition.states.0.task.assignment.departmentScope"])
    );
    assert_eq!(status_of(&app, second).await, "DRAFT");
    assert_eq!(status_of(&app, first).await, "ACTIVE");
}

/// **A refused scope writes nothing**: the draft's JSON, its publication
/// columns, its projection and the audit trail are as they were.
#[tokio::test]
async fn a_refused_scope_writes_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_scope_nothing");
    definition["transitions"][0]["allowedBy"] = scoped_role("DEPT-FIN");

    let id = saved(&app, &token, "wf_scope_nothing", definition).await;

    let read = |app: &TestApp| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_as::<
                _,
                (
                    Value,
                    Option<chrono::DateTime<chrono::Utc>>,
                    Option<Uuid>,
                    String,
                ),
            >(
                "SELECT definition_json, published_at, published_by, status \
                 FROM workflow_definitions WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read the draft")
        }
    };
    let audited = |app: &TestApp| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM audit_events \
                 WHERE object_id = $1 AND event_type = 'Workflow.Published'",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("count the publication audit rows")
        }
    };

    let before = read(&app).await;
    let audited_before = audited(&app).await;

    let refused = publish(&app, &token, id).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let after = read(&app).await;
    assert_eq!(after, before, "a refused publish changed the draft");
    assert_eq!(after.1, None, "a refused publish stamped published_at");
    assert_eq!(after.3, "DRAFT");
    assert_eq!(
        projected_rows(&app, id).await,
        0,
        "a refused publish projected"
    );
    assert_eq!(
        audited(&app).await,
        audited_before,
        "a refused publish was audited as published"
    );
}

/// **A draft fixed after the refusal publishes**, either way the author may
/// fix it: the scope removed, or the rule retyped to `DEPARTMENT_ROLE`.
#[tokio::test]
async fn a_draft_fixed_after_the_refusal_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let fixes = [
        json!({ "assigneeType": "ROLE", "roleCode": "WF-APPROVER" }),
        json!({ "assigneeType": "DEPARTMENT_ROLE", "roleCode": "WF-APPROVER",
                "departmentScope": "OWNER_DEPARTMENT" }),
    ];

    for (index, fix) in fixes.into_iter().enumerate() {
        let key = format!("wf_scope_fixed_{index}");
        let mut definition = approval_workflow(&key);
        definition["states"][0]["task"]["assignment"] = scoped_role("OWNER_DEPARTMENT");

        let id = saved(&app, &token, &key, definition.clone()).await;
        let refused = publish(&app, &token, id).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.body
        );

        definition["states"][0]["task"]["assignment"] = fix.clone();
        let edited = app
            .put(
                &format!("{DEFINITIONS}/{id}"),
                Some(&token),
                json!({ "definition": definition }),
            )
            .await;
        assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);

        let published = publish(&app, &token, id).await;
        assert_eq!(
            published.status,
            StatusCode::OK,
            "the fix {fix} did not publish: {}",
            published.body
        );
        assert_eq!(status_of(&app, id).await, "ACTIVE");
    }
}

/// **A scope on an escalation's `ROLE` is not checked**: nothing executes
/// `escalation.assignment` (JWSS §3.1), so D-114 reads the places D-111 reads.
/// Whatever schedules escalations (FR-WF-010) must add it, and this test is the
/// one that will then go red.
#[tokio::test]
async fn a_scope_on_an_escalation_role_is_not_checked() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = approval_workflow("wf_scope_escalation");
    definition["states"][0]["task"]["escalation"] = json!({
        "afterHours": 24,
        "assignment": scoped_role("REQUESTED_DEPARTMENT")
    });

    let id = saved(&app, &token, "wf_scope_escalation", definition).await;
    let published = publish(&app, &token, id).await;

    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **A scope added by an edit the publish waited on is checked as edited.**
/// The mirror of `a_draft_edited_before_its_publish_takes_the_row_is_checked_as_edited`:
/// the edit holds the row, the publish waits on it, and the scope check reads
/// the copy the publish locked rather than one read before.
#[tokio::test]
async fn a_scope_added_to_a_draft_while_its_publish_waits_is_checked_as_edited() {
    let app = std::sync::Arc::new(TestApp::spawn().await);
    let token = app.administrator_token().await;
    let tenant = common::fixtures::SYSTEM_TENANT_ID;

    role(&app, "WF-APPROVER").await;
    let id = saved(
        &app,
        &token,
        "wf_scope_held",
        approval_workflow("wf_scope_held"),
    )
    .await;

    let mut scoped = approval_workflow("wf_scope_held");
    scoped["states"][0]["task"]["assignment"] = scoped_role("REQUESTED_DEPARTMENT");

    let mut editing = app.pool.begin().await.expect("a transaction");
    let edited = definition_repo::update_draft(
        &mut *editing,
        tenant,
        id,
        &definition_repo::DefinitionFields {
            name: None,
            description: None,
            definition_json: Some(&scoped),
            initial_state: None,
            jwss_version: None,
        },
        None,
    )
    .await
    .expect("the edit runs");
    assert_eq!(edited, 1);

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    let waited = waited_on_a_lock(&app, 1, &publishing).await;
    editing.commit().await.expect("the edit commits");
    let published = publishing.await.expect("the publish finished");

    assert!(
        waited,
        "the publish did not wait on the edit's row: {} {}",
        published.status, published.body
    );
    assert_eq!(
        published.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a publish checked a copy older than the one it published: {}",
        published.body
    );
    assert_eq!(
        refusals(&published),
        takes_no_scope(&["definition.states.0.task.assignment.departmentScope"])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");
}

/// A workflow whose `ROLE` rules sit behind places that hold no rule: a final
/// state first, then four task states, and an `AUTO` edge first, then three
/// decided ones. The `USER` and `OWNER` rules carry a scope as well, and every
/// scope is the same value.
fn scopes_among_every_kind_of_place(key: &str) -> Value {
    let scope = "DEPT-FIN";
    let user = json!({ "assigneeType": "USER", "userId": Uuid::nil().to_string(),
                       "departmentScope": scope });
    let owner = json!({ "assigneeType": "OWNER", "departmentScope": scope });
    let task = |code: &str, assignment: Value| {
        json!({ "code": code, "name": code, "mapsToDocumentStatus": "PENDING_APPROVAL",
                "task": { "taskDefinitionKey": code.to_lowercase(), "taskName": code,
                          "assignment": assignment } })
    };

    json!({
        "workflowKey": key, "version": "1.0.0", "name": "Scopes in every place",
        "initialState": "A",
        "states": [
            { "code": "DONE", "name": "Done", "mapsToDocumentStatus": "COMPLETED", "isFinal": true },
            task("A", scoped_role(scope)),
            task("B", user.clone()),
            task("C", owner.clone()),
            task("D", scoped_role(scope))
        ],
        "transitions": [
            { "from": "D", "to": "DONE", "action": "AUTO" },
            { "from": "A", "to": "B", "action": "APPROVE", "allowedBy": scoped_role(scope) },
            { "from": "B", "to": "C", "action": "APPROVE", "allowedBy": user },
            { "from": "C", "to": "D", "action": "APPROVE", "allowedBy": owner }
        ]
    })
}

/// **Each scope is refused at its own index, once a place, and on a `ROLE`
/// rule only** (JWSS §5.3). The index is the place's among all the states or
/// all the transitions, not among the ones holding a rule; two places with the
/// same scope are two details; a `USER` or `OWNER` rule's scope is not this
/// refusal, and the definition publishes once only the `ROLE` scopes are gone.
#[tokio::test]
async fn each_scope_is_refused_at_its_own_index_and_only_on_a_role_rule() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = scopes_among_every_kind_of_place("wf_scope_places");
    let id = saved(&app, &token, "wf_scope_places", definition.clone()).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        takes_no_scope(&[
            "definition.states.1.task.assignment.departmentScope",
            "definition.states.4.task.assignment.departmentScope",
            "definition.transitions.1.allowedBy.departmentScope",
        ])
    );

    for pointer in [
        "/states/1/task/assignment",
        "/states/4/task/assignment",
        "/transitions/1/allowedBy",
    ] {
        definition
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .unwrap_or_else(|| panic!("a rule at {pointer}"))
            .remove("departmentScope");
    }

    let edited = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": definition }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);

    let published = publish(&app, &token, id).await;
    assert_eq!(
        published.status,
        StatusCode::OK,
        "a scope on a USER or OWNER rule was refused: {}",
        published.body
    );
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **The scope details lead the 422 as a group**, not by where they sit: a
/// scope on the second edge is told before a dead role on the first state.
#[tokio::test]
async fn a_scope_on_a_later_edge_is_told_ahead_of_a_dead_role_in_an_earlier_state() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // No `WF-APPROVER`.
    let mut definition = approval_workflow("wf_scope_later");
    definition["transitions"][1]["allowedBy"] = scoped_role("DEPT-FIN");

    let id = saved(&app, &token, "wf_scope_later", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let mut expected = takes_no_scope(&["definition.transitions.1.allowedBy.departmentScope"]);
    expected.extend(not_live(&[
        "definition.states.0.task.assignment.roleCode",
        "definition.transitions.0.allowedBy",
        "definition.transitions.1.allowedBy.roleCode",
    ]));
    assert_eq!(refusals(&refused), expected);
}

// ---------------------------------------------------------------------------
// #706 (D-115 B) — publish refuses a condition path the engine does not build
// ---------------------------------------------------------------------------

fn not_built(paths: &[&str]) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), "CONDITION_PATH_NOT_BUILT".to_owned()))
        .collect()
}

/// [`approval_workflow`] with `condition` on its `APPROVE` edge.
fn conditioned(key: &str, condition: Value) -> Value {
    let mut definition = approval_workflow(key);
    definition["transitions"][0]["condition"] = condition;
    definition
}

/// The messages of every detail a refusal carries, in order.
fn messages(response: &common::TestResponse) -> Vec<String> {
    response.body["error"]["details"]
        .as_array()
        .unwrap_or_else(|| panic!("a refusal with details: {}", response.body))
        .iter()
        .map(|detail| detail["message"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// **#706 AC7: each path the engine does not build is refused at publish, at
/// the expression that reads it, one detail per distinct path.**
/// `document.amount`, `document.foo`, `actor.email`, `amount` (no root) and
/// `variables.undeclared` in one condition are five details at the
/// condition's path, in the order they are read; `document.amount` read twice
/// is told once. A variable `source` is checked the same way, after the
/// transitions, and a declared variable beside an undeclared one is accepted.
///
/// Each detail is `conditionPath`'s, names the path it read, and names the
/// built paths; `document.amount`'s names `formData.amount`. The draft was
/// saved, stays a draft, and projects nothing.
#[tokio::test]
async fn each_path_the_engine_does_not_build_is_refused_at_publish_once_per_expression() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = conditioned(
        "wf_paths_refused",
        json!({ "and": [
            { ">": [{ "var": "document.amount" }, 1] },
            { "==": [{ "var": "document.foo" }, 1] },
            { "==": [{ "var": "actor.email" }, "someone@example.test"] },
            { ">": [{ "var": "amount" }, 1] },
            { "==": [{ "var": "variables.undeclared" }, 1] },
            { "<": [{ "var": "document.amount" }, 9] }
        ] }),
    );
    definition["variables"] = json!([
        { "key": "amount", "dataType": "NUMBER", "source": { "var": "document.amount" } },
        { "key": "budget", "dataType": "NUMBER",
          "source": { "+": [{ "var": "variables.amount" }, { "var": "variables.nope" }] } }
    ]);

    let id = saved(&app, &token, "wf_paths_refused", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a definition reading unbuilt paths was published: {}",
        refused.body
    );
    assert_eq!(refused.body["error"]["code"], "VALIDATION_ERROR");

    let condition = "definition.transitions.0.condition";
    assert_eq!(
        refusals(&refused),
        not_built(&[
            condition,
            condition,
            condition,
            condition,
            condition,
            "definition.variables.0.source",
            "definition.variables.1.source",
        ])
    );

    let details = refused.body["error"]["details"]
        .as_array()
        .expect("details");
    assert!(
        details
            .iter()
            .all(|detail| detail["rule"] == "conditionPath"),
        "{}",
        refused.body
    );

    let messages = messages(&refused);
    for (message, read) in messages.iter().zip([
        "document.amount",
        "document.foo",
        "actor.email",
        "amount",
        "variables.undeclared",
        "document.amount",
        "variables.nope",
    ]) {
        assert!(
            message.contains(&format!("`{read}`")),
            "the refusal must name the path it read, `{read}`: {message}"
        );
        assert!(
            message.contains("document.documentTypeKey") && message.contains("actor.departmentId"),
            "the refusal must name the built paths: {message}"
        );
    }
    for index in [0, 5] {
        assert!(
            messages[index].contains("`formData.amount`"),
            "document.amount's refusal must name formData.amount: {}",
            messages[index]
        );
    }

    assert_eq!(status_of(&app, id).await, "DRAFT");
    assert_eq!(
        projected_rows(&app, id).await,
        0,
        "a refused publish projected"
    );
}

/// **#706 AC7, AC8: every path the engine builds is accepted**, in each form a
/// path takes: a `var` with a string, with a default beside it, a bare root,
/// an index after `actor.roles`, anything under `formData`, a declared
/// variable, `missing` and `missing_some`, a `var` with no path or a
/// non-string one, and an iterator whose element-scoped operand reads `qty`,
/// which addresses the element and not the context. **#789 AC9**: none of the
/// non-string operands computes a path. `[1, "document.amount"]` looks like one
/// and is a literal default beside the key `1`.
#[tokio::test]
async fn every_path_the_engine_builds_is_accepted_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let mut definition = conditioned(
        "wf_paths_built",
        json!({ "and": [
            { "==": [{ "var": "document.status" }, "SUBMITTED"] },
            { "!=": [{ "var": "document.documentTypeId" }, ""] },
            { "!=": [{ "var": "document.documentNumber" }, ""] },
            { "==": [{ "var": "document.documentTypeKey" }, "PURCHASE_REQUISITION"] },
            { "!=": [{ "var": "actor.userId" }, ""] },
            { "in": ["APPROVER", { "var": "actor.roles" }] },
            { "==": [{ "var": "actor.roles.0" }, "APPROVER"] },
            { "!=": [{ "var": "actor.departmentId" }, ""] },
            { "!!": [{ "var": "formData" }] },
            { ">": [{ "var": ["formData.amount", 0] }, 1] },
            { ">": [{ "var": "formData.lines.0.qty" }, 1] },
            { ">": [{ "var": "variables.amount" }, 1] },
            { "!!": [{ "var": "document" }] },
            { "!!": [{ "var": "actor" }] },
            { "!!": [{ "var": "variables" }] },
            { "!": [{ "missing": ["formData.amount", "document.status"] }] },
            { "!": [{ "missing_some": [1, ["formData.a", "actor.userId"]] }] },
            { "!!": [{ "var": "" }] },
            { "!!": [{ "var": 0 }] },
            { "!!": [{ "var": null }] },
            { "!!": [{ "var": [] }] },
            { "!!": [{ "var": [1, "document.amount"] }] },
            { "!!": [{ "var": ["formData.x", 0] }] },
            { "some": [{ "var": "formData.lines" }, { ">": [{ "var": "qty" }, 1] }] },
            { "all": [{ "var": "formData.lines" }, { ">": [{ "var": "price" }, 0] }] }
        ] }),
    );
    definition["variables"] = json!([
        { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } },
        { "key": "doubled", "dataType": "NUMBER",
          "source": { "*": [{ "var": "variables.amount" }, 2] } }
    ]);

    let id = saved(&app, &token, "wf_paths_built", definition).await;
    let published = publish(&app, &token, id).await;

    assert_eq!(
        published.status,
        StatusCode::OK,
        "a path the engine builds was refused: {}",
        published.body
    );
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// **#706 AC8: `missing` and `missing_some` name paths, and an iterator's
/// source is a path too.** Each unbuilt one is refused: `document.amount`
/// inside `missing`, `actor.email` inside `missing_some`, `document.foo` in a
/// `var` with a default, and `document.lines`, the source an iterator walks.
/// The element's `qty` beside it is not.
#[tokio::test]
async fn paths_named_by_missing_and_by_an_iterators_source_are_checked() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let definition = conditioned(
        "wf_paths_named",
        json!({ "or": [
            { "missing": ["formData.amount", "document.amount"] },
            { "missing_some": [1, ["actor.email", "formData.a"]] },
            { "var": ["document.foo", 0] },
            { "some": [{ "var": "document.lines" }, { ">": [{ "var": "qty" }, 1] }] }
        ] }),
    );

    let id = saved(&app, &token, "wf_paths_named", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let condition = "definition.transitions.0.condition";
    assert_eq!(
        refusals(&refused),
        not_built(&[condition, condition, condition, condition])
    );

    let messages = messages(&refused);
    for (message, read) in messages.iter().zip([
        "document.amount",
        "actor.email",
        "document.foo",
        "document.lines",
    ]) {
        assert!(message.contains(&format!("`{read}`")), "{read}: {message}");
    }
}

/// **#706 AC9: the path details sit between the scope details and the
/// dead-role details** in one refusal, so an author learns all three at once.
#[tokio::test]
async fn path_refusals_are_told_between_the_scope_and_the_dead_role_refusals() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // No `WF-APPROVER`.
    let mut definition = conditioned(
        "wf_paths_ordered",
        json!({ ">": [{ "var": "document.amount" }, 1] }),
    );
    definition["transitions"][1]["allowedBy"] = scoped_role("DEPT-FIN");

    let id = saved(&app, &token, "wf_paths_ordered", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    let mut expected = takes_no_scope(&["definition.transitions.1.allowedBy.departmentScope"]);
    expected.extend(not_built(&["definition.transitions.0.condition"]));
    expected.extend(not_live(&[
        "definition.states.0.task.assignment.roleCode",
        "definition.transitions.0.allowedBy",
        "definition.transitions.1.allowedBy.roleCode",
    ]));
    assert_eq!(refusals(&refused), expected);
}

/// **#706 AC9: saving is unchanged.** A draft reading `document.amount` is
/// created and edited without refusal, as a draft with a scoped `ROLE` rule
/// is; its publish is refused, and the draft fixed to `formData.amount`
/// publishes.
#[tokio::test]
async fn a_draft_reading_an_unbuilt_path_is_saved_and_publishes_once_fixed() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let unbuilt = json!({ ">": [{ "var": "document.amount" }, 1] });
    let id = saved(
        &app,
        &token,
        "wf_paths_draft",
        conditioned("wf_paths_draft", unbuilt.clone()),
    )
    .await;

    let mut edited = conditioned("wf_paths_draft", unbuilt);
    edited["variables"] = json!([
        { "key": "amount", "dataType": "NUMBER", "source": { "var": "document.amount" } }
    ]);
    let updated = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": edited }),
        )
        .await;
    assert_eq!(
        updated.status,
        StatusCode::OK,
        "an edit refused an unbuilt path: {}",
        updated.body
    );

    let refused = publish(&app, &token, id).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_built(&[
            "definition.transitions.0.condition",
            "definition.variables.0.source"
        ])
    );
    assert_eq!(status_of(&app, id).await, "DRAFT");

    let mut fixed = conditioned(
        "wf_paths_draft",
        json!({ ">": [{ "var": "formData.amount" }, 1] }),
    );
    fixed["variables"] = json!([
        { "key": "amount", "dataType": "NUMBER", "source": { "var": "formData.amount" } }
    ]);
    let updated = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": fixed }),
        )
        .await;
    assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

    let published = publish(&app, &token, id).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_eq!(status_of(&app, id).await, "ACTIVE");
}

/// A draft with `condition` on its `APPROVE` edge and an S-rule fault beside it,
/// written behind the API, which would have refused the fault at save.
async fn smuggled_with_a_dead_end(app: &TestApp, key: &str, condition: Value) -> Uuid {
    let mut stuck = conditioned(key, condition);
    stuck["states"]
        .as_array_mut()
        .expect("states")
        .push(json!({ "code": "STUCK", "name": "Stuck", "mapsToDocumentStatus": "IN_REVIEW" }));
    stuck["transitions"]
        .as_array_mut()
        .expect("transitions")
        .push(json!({
            "from": "MANAGER_APPROVAL", "to": "STUCK", "action": "RETURN",
            "allowedBy": "ROLE:WF-APPROVER"
        }));

    let smuggled = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO workflow_definitions
            (id, tenant_id, workflow_key, name, version, jwss_version, definition_json,
             initial_state, status)
        VALUES ($1, $2, $3, 'Stuck', 1, '1.0.0', $4, 'MANAGER_APPROVAL', 'DRAFT')
        "#,
    )
    .bind(smuggled)
    .bind(common::fixtures::SYSTEM_TENANT_ID)
    .bind(key)
    .bind(&stuck)
    .execute(&app.pool)
    .await
    .expect("write a definition the API would have refused");

    smuggled
}

/// **#706 AC9: an S-rule fault hides the path refusal**, as it hides the
/// scope's (R-16): the conformance check runs first and its refusal is the
/// whole answer.
#[tokio::test]
async fn an_s_rule_fault_hides_an_unbuilt_path() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let smuggled = smuggled_with_a_dead_end(
        &app,
        "wf_paths_stuck",
        json!({ ">": [{ "var": "document.amount" }, 1] }),
    )
    .await;

    let s_rule = publish(&app, &token, smuggled).await;
    assert_eq!(
        s_rule.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        s_rule.body
    );
    let details = refusals(&s_rule);
    assert!(
        details.iter().any(|(_, code)| code == "DEAD_END_STATE"),
        "{}",
        s_rule.body
    );
    assert!(
        details
            .iter()
            .all(|(_, code)| code != "CONDITION_PATH_NOT_BUILT"),
        "an S-rule fault did not hide the unbuilt path: {}",
        s_rule.body
    );
}

/// **#706 AC9: a published revision is not re-checked, and its clone is.**
/// Revision 1 gets a `document.amount` condition behind the API's back, the
/// shape a revision published before #706 has, and stays `ACTIVE`. The
/// revision cloned from it is a draft, and its publish is refused at the path.
#[tokio::test]
async fn a_published_revision_reading_an_unbuilt_path_stays_and_its_clone_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    let first = published(&app, &token, "wf_paths_revised").await;
    stored_with(
        &app,
        first,
        "{transitions,0,condition}",
        json!({ ">": [{ "var": "document.amount" }, 1] }),
    )
    .await;

    let revision = app
        .post(
            &format!("{DEFINITIONS}/{first}/revisions"),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
    let second = id_of(&revision.body["data"]);

    let refused = publish(&app, &token, second).await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a revision reading document.amount was published: {}",
        refused.body
    );
    assert_eq!(
        refusals(&refused),
        not_built(&["definition.transitions.0.condition"])
    );
    assert_eq!(status_of(&app, second).await, "DRAFT");
    assert_eq!(status_of(&app, first).await, "ACTIVE");
}

// ---------------------------------------------------------------------------
// #789 (R-18) — a publish reads every path a condition can reach
// ---------------------------------------------------------------------------

const CONDITION: &str = "definition.transitions.0.condition";

fn computed(paths: &[&str]) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), "CONDITION_PATH_COMPUTED".to_owned()))
        .collect()
}

/// `reduce` over `formData.lines`, summing, with `initial` as its third operand
/// when one is given.
fn folding(initial: Option<Value>) -> Value {
    let mut operands = vec![
        json!({ "var": "formData.lines" }),
        json!({ "+": [{ "var": "current" }, { "var": "accumulator" }] }),
    ];
    operands.extend(initial);

    json!({ "reduce": operands })
}

/// The publish of a saved definition holding `condition`, which is refused.
async fn refused_publish(
    app: &TestApp,
    token: &str,
    key: &str,
    condition: Value,
) -> common::TestResponse {
    let id = saved(app, token, key, conditioned(key, condition)).await;
    let refused = publish(app, token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{key} was published: {}",
        refused.body
    );
    assert_eq!(refused.body["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(status_of(app, id).await, "DRAFT", "{key}");
    assert_eq!(projected_rows(app, id).await, 0, "{key} projected");

    refused
}

/// **#789 AC1, AC3: `reduce`'s initial value is read in the outer scope.** The
/// draft saves; its publish is refused at the condition, for `document.amount`,
/// and the message points at `formData.amount`. A bare `qty` there is the
/// context's and not an element's, so it is refused as `qty`.
#[tokio::test]
async fn a_reduces_initial_value_is_read_in_the_outer_scope_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let amount = refused_publish(
        &app,
        &token,
        "wf_r35_initial",
        json!({ "==": [folding(Some(json!({ "var": "document.amount" }))), 7] }),
    )
    .await;
    assert_eq!(refusals(&amount), not_built(&[CONDITION]));

    let messages_of_amount = messages(&amount);
    assert!(
        messages_of_amount[0].contains("`document.amount`")
            && messages_of_amount[0].contains("`formData.amount`"),
        "{}",
        messages_of_amount[0]
    );
    assert_eq!(amount.body["error"]["details"][0]["rule"], "conditionPath");

    let element = refused_publish(
        &app,
        &token,
        "wf_r35_initial_qty",
        json!({ "==": [folding(Some(json!({ "var": "qty" }))), 7] }),
    )
    .await;
    assert_eq!(refusals(&element), not_built(&[CONDITION]));
    assert!(messages(&element)[0].contains("`qty`"));
}

/// **#789 AC2: a `reduce` reading nothing unbuilt still publishes**: with a
/// literal initial value, with `formData.start`, with a declared variable, and
/// with two arguments.
#[tokio::test]
async fn a_reduce_with_a_built_or_literal_initial_value_publishes() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    for (key, initial) in [
        ("wf_r35_literal", Some(json!(0))),
        ("wf_r35_form", Some(json!({ "var": "formData.start" }))),
        ("wf_r35_declared", Some(json!({ "var": "variables.start" }))),
        ("wf_r35_two", None),
    ] {
        let mut definition = conditioned(key, json!({ ">": [folding(initial), 1] }));
        definition["variables"] = json!([
            { "key": "start", "dataType": "NUMBER", "source": { "var": "formData.start" } }
        ]);

        let id = saved(&app, &token, key, definition).await;
        let answer = publish(&app, &token, id).await;

        assert_eq!(answer.status, StatusCode::OK, "{key}: {}", answer.body);
        assert_eq!(status_of(&app, id).await, "ACTIVE", "{key}");
    }
}

/// **#789 AC6, record 24's P2.** Both conditions read `document.amount`, both
/// save, and neither publishes: the `reduce` as the unbuilt path it names, the
/// computed `var` as a path that cannot be named.
#[tokio::test]
async fn an_unbuilt_path_read_another_way_is_refused_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let reduce = refused_publish(
        &app,
        &token,
        "wf_r24_reduce",
        json!({ "==": [{ "reduce": [
            { "var": "formData.lines" },
            { "+": [{ "var": "current" }, { "var": "accumulator" }] },
            { "var": "document.amount" }
        ] }, 7] }),
    )
    .await;
    assert_eq!(refusals(&reduce), not_built(&[CONDITION]));

    let computed_var = refused_publish(
        &app,
        &token,
        "wf_r24_computed",
        json!({ "==": [{ "var": { "if": [true, "document.amount", "x"] } }, 7] }),
    )
    .await;
    assert_eq!(refusals(&computed_var), computed(&[CONDITION]));
}

/// **#789 AC8: a path that cannot be checked is refused at publish**, in each
/// shape and wherever it sits, a body included, because the scope jump works at
/// any depth. Each draft saves. The message says why, and what to write, and
/// names no path: the one thing it does not know is the path.
#[tokio::test]
async fn a_computed_path_is_refused_at_publish_wherever_it_sits() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let chosen = json!({ "if": [true, "formData.amount", "x"] });

    for (key, condition) in [
        ("wf_r35_object", json!({ "var": chosen })),
        ("wf_r35_opening_object", json!({ "var": [chosen, 0] })),
        (
            "wf_r35_opening_array",
            json!({ "var": [[1], "formData.amount"] }),
        ),
        (
            "wf_r35_in_a_body",
            json!({ "some": [{ "var": "formData.lines" }, {
                "==": [{ "var": [[1], "document.amount"] }, 7]
            }] }),
        ),
        (
            "wf_r35_in_a_reduce_body",
            json!({ "reduce": [{ "var": "formData.lines" }, { "var": { "var": "kind" } }, 0] }),
        ),
    ] {
        let refused = refused_publish(&app, &token, key, condition).await;

        assert_eq!(refusals(&refused), computed(&[CONDITION]), "{key}");
        assert_eq!(refused.body["error"]["details"][0]["rule"], "conditionPath");

        let message = &messages(&refused)[0];
        assert!(
            message.contains("cannot check") && message.contains("as a string"),
            "{message}"
        );
        assert!(
            !message.contains("document.") && !message.contains("formData."),
            "a computed path has no name to give: {message}"
        );
    }
}

/// **#789 AC10, AC11: what a `var`'s default, `missing` and `missing_some` read
/// is read.** Each draft saves. A nested `var` is refused for its own path; a
/// nested list in `missing` holds names; a name worked out as the condition
/// runs is refused as computed, after the path it was worked out from.
#[tokio::test]
async fn a_default_and_the_operands_of_missing_are_read_at_publish() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let amount = json!({ "var": "document.amount" });

    for (key, condition, expected) in [
        (
            "wf_r35_default",
            json!({ "var": ["formData.x", amount] }),
            not_built(&[CONDITION]),
        ),
        (
            "wf_r35_default_by_number",
            json!({ "var": [0, amount] }),
            not_built(&[CONDITION]),
        ),
        (
            "wf_r35_nested_names",
            json!({ "missing": [["formData.a", "document.amount"]] }),
            not_built(&[CONDITION]),
        ),
        (
            "wf_r35_count",
            json!({ "missing_some": [amount, ["formData.a", "formData.b"]] }),
            not_built(&[CONDITION]),
        ),
        (
            "wf_r35_var_names",
            json!({ "missing": amount }),
            [not_built(&[CONDITION]), computed(&[CONDITION])].concat(),
        ),
        (
            "wf_r35_some_names",
            json!({ "missing_some": [1, amount] }),
            [not_built(&[CONDITION]), computed(&[CONDITION])].concat(),
        ),
        (
            "wf_r35_built_names",
            json!({ "missing": { "var": "formData.names" } }),
            computed(&[CONDITION]),
        ),
    ] {
        let refused = refused_publish(&app, &token, key, condition).await;

        assert_eq!(refusals(&refused), expected, "{key}");
    }

    let definition = conditioned(
        "wf_r35_built",
        json!({ "and": [
            { "var": ["formData.x", { "var": "actor.userId" }] },
            { "!": [{ "missing": [["formData.a", "document.status"], "actor.userId"] }] },
            { "!": [{ "missing_some": [{ "var": "formData.count" }, ["formData.a", "formData.b"]] }] }
        ] }),
    );
    let id = saved(&app, &token, "wf_r35_built", definition).await;
    let answer = publish(&app, &token, id).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
}

/// **#789 AC8, AC12: the order, and one computed refusal per expression.** In
/// one refusal, the scope details, then each expression's unbuilt paths, a
/// `reduce`'s initial value after its source and the expression's computed path
/// after them, however many it holds, transitions before variables, then the
/// dead roles.
#[tokio::test]
async fn path_refusals_keep_their_order_with_a_computed_path_among_them() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // No `WF-APPROVER`.
    let mut definition = conditioned(
        "wf_r35_ordered",
        json!({ "and": [
            { "var": { "var": "formData.field" } },
            { "reduce": [
                { "var": "document.lines" },
                { "+": [{ "var": "current" }, { "var": "accumulator" }] },
                { "var": "document.amount" }
            ] },
            { "var": [[1], "formData.amount"] }
        ] }),
    );
    definition["transitions"][1]["allowedBy"] = scoped_role("DEPT-FIN");
    definition["variables"] = json!([
        { "key": "worked_out", "dataType": "NUMBER",
          "source": { "+": [{ "var": [[1], "x"] }, { "var": "actor.email" }] } }
    ]);

    let id = saved(&app, &token, "wf_r35_ordered", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let source = "definition.variables.0.source";
    let mut expected = takes_no_scope(&["definition.transitions.1.allowedBy.departmentScope"]);
    expected.extend(not_built(&[CONDITION, CONDITION]));
    expected.extend(computed(&[CONDITION]));
    expected.extend(not_built(&[source]));
    expected.extend(computed(&[source]));
    expected.extend(not_live(&[
        "definition.states.0.task.assignment.roleCode",
        "definition.transitions.0.allowedBy",
        "definition.transitions.1.allowedBy.roleCode",
    ]));
    assert_eq!(refusals(&refused), expected);

    let messages = messages(&refused);
    assert!(messages[1].contains("`document.lines`"), "{}", messages[1]);
    assert!(messages[2].contains("`document.amount`"), "{}", messages[2]);
}

/// **#789 AC12, at its most mixed.** Two conditions and two sources, a path
/// repeated within an expression and across expressions, an expression holding
/// two computed paths, a scope and a dead role: each expression is told once per
/// path and once for its computed paths, and no expression is excused for what
/// an earlier one was told.
#[tokio::test]
async fn a_refusal_tells_each_expression_its_own_paths_whatever_the_others_said() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // No `WF-APPROVER`.
    let mut definition = conditioned(
        "wf_r35_mixed",
        json!({ "and": [
            { "var": "actor.email" },
            { "var": { "var": "formData.field" } },
            { "var": "document.amount" },
            { "var": "actor.email" },
            { "var": [[1], "formData.amount"] }
        ] }),
    );
    definition["transitions"][1]["condition"] = json!({ "or": [
        { "var": [[1], "formData.amount"] },
        { "var": "actor.email" }
    ] });
    definition["transitions"][1]["allowedBy"] = scoped_role("DEPT-FIN");
    definition["variables"] = json!([
        { "key": "worked_out", "dataType": "NUMBER",
          "source": { "some": [{ "var": "formData.lines" }, { "var": { "var": "name" } }] } },
        { "key": "amount", "dataType": "NUMBER", "source": { "var": "document.amount" } }
    ]);

    let id = saved(&app, &token, "wf_r35_mixed", definition).await;
    let refused = publish(&app, &token, id).await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let second = "definition.transitions.1.condition";
    let mut expected = takes_no_scope(&["definition.transitions.1.allowedBy.departmentScope"]);
    expected.extend(not_built(&[CONDITION, CONDITION]));
    expected.extend(computed(&[CONDITION]));
    expected.extend(not_built(&[second]));
    expected.extend(computed(&[second]));
    expected.extend(computed(&["definition.variables.0.source"]));
    expected.extend(not_built(&["definition.variables.1.source"]));
    expected.extend(not_live(&[
        "definition.states.0.task.assignment.roleCode",
        "definition.transitions.0.allowedBy",
        "definition.transitions.1.allowedBy.roleCode",
    ]));
    assert_eq!(refusals(&refused), expected);

    let messages = messages(&refused);
    assert!(messages[1].contains("`actor.email`"), "{}", messages[1]);
    assert!(messages[2].contains("`document.amount`"), "{}", messages[2]);
    assert!(messages[4].contains("`actor.email`"), "{}", messages[4]);
    assert!(messages[7].contains("`document.amount`"), "{}", messages[7]);
    assert_eq!(status_of(&app, id).await, "DRAFT");
    assert_eq!(projected_rows(&app, id).await, 0);
}

/// **#789 AC13: an S-rule fault hides a computed path and an initial value**, as
/// it hides the other path refusals.
#[tokio::test]
async fn an_s_rule_fault_hides_a_computed_path() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    role(&app, "WF-APPROVER").await;

    let smuggled = smuggled_with_a_dead_end(
        &app,
        "wf_r35_stuck",
        json!({ "and": [
            { "var": { "var": "formData.field" } },
            folding(Some(json!({ "var": "document.amount" })))
        ] }),
    )
    .await;

    let s_rule = publish(&app, &token, smuggled).await;

    assert_eq!(
        s_rule.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        s_rule.body
    );
    let details = refusals(&s_rule);
    assert!(
        details.iter().any(|(_, code)| code == "DEAD_END_STATE"),
        "{}",
        s_rule.body
    );
    assert!(
        details
            .iter()
            .all(|(_, code)| !code.starts_with("CONDITION_PATH")),
        "an S-rule fault did not hide the path refusals: {}",
        s_rule.body
    );
}

/// **#789 AC14: a published revision is not re-checked, and its clone is.** Each
/// shape is written behind the API, as a revision published before #789 holds
/// it. The revision stays `ACTIVE`; its clone is a draft, refused at publish.
#[tokio::test]
async fn a_published_revision_holding_either_shape_stays_and_its_clone_is_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    for (key, condition, expected) in [
        (
            "wf_r35_revised_reduce",
            json!({ "==": [folding(Some(json!({ "var": "document.amount" }))), 7] }),
            not_built(&[CONDITION]),
        ),
        (
            "wf_r35_revised_var",
            json!({ "var": [[1], "document.amount"] }),
            computed(&[CONDITION]),
        ),
    ] {
        let first = published(&app, &token, key).await;
        stored_with(&app, first, "{transitions,0,condition}", condition).await;

        let revision = app
            .post(
                &format!("{DEFINITIONS}/{first}/revisions"),
                Some(&token),
                json!({}),
            )
            .await;
        assert_eq!(revision.status, StatusCode::CREATED, "{}", revision.body);
        let second = id_of(&revision.body["data"]);

        let refused = publish(&app, &token, second).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{key}'s clone was published: {}",
            refused.body
        );
        assert_eq!(refusals(&refused), expected, "{key}");
        assert_eq!(status_of(&app, second).await, "DRAFT", "{key}");
        assert_eq!(status_of(&app, first).await, "ACTIVE", "{key}");
    }
}
