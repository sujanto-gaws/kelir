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
/// the definition's row `FOR UPDATE`, which its `UPDATE ... SET status =
/// 'ACTIVE'` waits on. The delete is then sent through the API. It must wait
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
    sqlx::query("SELECT id FROM workflow_definitions WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *holding)
        .await
        .expect("hold the definition row");

    let publishing = {
        let app = std::sync::Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { publish(&app, &token, id).await })
    };

    assert!(
        waited_on_a_lock(&app, 1, &publishing).await,
        "the publish did not reach its write"
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
    holding
        .rollback()
        .await
        .expect("release the definition row");

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
/// renames the task's role to one no role has, and commits.
#[tokio::test]
#[ignore = "defect (#572): publish checks the definition it read before its \
            transaction, and flips whatever the row holds by then to ACTIVE; an \
            edit landing in between publishes a dead role unchecked"]
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

    let edited = app
        .put(
            &format!("{DEFINITIONS}/{id}"),
            Some(&token),
            json!({ "definition": edited_definition }),
        )
        .await;

    holding.rollback().await.expect("release the role row");
    let published = publishing.await.expect("the publish finished");

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
}
