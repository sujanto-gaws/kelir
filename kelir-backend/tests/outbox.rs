//! The outbox worker, and the after half of the hook chain it delivers to
//! ([#519]; ADR-0041).
//!
//! # What these tests drive
//!
//! `outbox::worker::pass` — the loop's work with the sleeping taken out — over
//! events a real transition wrote. The schedule is walked by moving a row's
//! `next_attempt_at` into the past rather than by waiting, which is the only
//! way a test can reach an eight-minute retry.
//!
//! # A handler that fails, without shipping one
//!
//! Every handler this build ships either succeeds or is before-only, and a
//! handler that fails on purpose would be product code written for a test. So
//! the failing handler is a **registry row naming a plugin**: there is no write
//! API for the registry (#339 gave it a reader), `workflow_system_tasks.rs`
//! seeds rows the same way, and a `plugin:` reference fails at delivery for the
//! reason it would in production — this build runs no plugins. It is exactly
//! the failure the breaker exists for: a registration pointing at something
//! that is not there.
//!
//! [#519]: https://github.com/sujanto-gaws/kelir/issues/519

mod common;

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use serde_json::{json, Value};
use uuid::Uuid;

const APPROVER_ROLE: &str = "OUTBOX-APPROVER";
const AFTER: &str = "after_workflow_transition";
const ABSENT_PLUGIN: &str = "plugin:absent-plugin:deliver";

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

/// One decision and nothing after it: `SUBMITTED` --APPROVE--> `APPROVED`.
///
/// One transition means one event per document, so a count of attempts or of
/// executions is a count for one delivery.
fn one_step_workflow(key: &str, actions: Value) -> Value {
    json!({
        "workflowKey": key,
        "version": "1.0.0",
        "name": "One step",
        "initialState": "SUBMITTED",
        "states": [
            { "code": "SUBMITTED", "name": "Awaiting approval",
              "mapsToDocumentStatus": "PENDING_APPROVAL",
              "task": { "taskDefinitionKey": "approve", "taskName": "Approve the request",
                        "assignment": { "assigneeType": "ROLE", "roleCode": APPROVER_ROLE } } },
            { "code": "APPROVED", "name": "Approved", "mapsToDocumentStatus": "COMPLETED",
              "isFinal": true }
        ],
        "transitions": [
            { "from": "SUBMITTED", "to": "APPROVED", "action": "APPROVE",
              "allowedBy": format!("ROLE:{APPROVER_ROLE}"),
              "actions": actions }
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
            json!({ "workflowKey": key, "name": "One step", "definition": definition }),
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

async fn document_type(app: &TestApp, token: &str, code: &str, workflow: Uuid) -> Uuid {
    let form_key = code.to_lowercase().replace('_', "-");
    let form = app
        .post(
            "/api/v1/rad/forms",
            Some(token),
            json!({
                "formKey": form_key,
                "title": "Request",
                "definition": {
                    "formId": form_key,
                    "version": "2.0.1",
                    "title": "Request",
                    "components": [{
                        "id": "amount-field", "role": "data", "type": "number",
                        "key": "amount", "label": "Amount",
                        "validation": { "type": "number", "minimum": 0 }
                    }]
                },
            }),
        )
        .await;

    assert_eq!(form.status, StatusCode::CREATED, "{}", form.body);

    let form = id_of(&form.body["data"]);

    app.post(
        &format!("/api/v1/rad/forms/{form}/publish"),
        Some(token),
        json!({}),
    )
    .await;

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

    app.put(
        &format!("/api/v1/document-types/{type_id}/numbering-rule"),
        Some(token),
        json!({
            "ruleTemplate": format!("{code}-{{year}}-{{sequence}}"),
            "sequenceScope": "YEAR",
            "gapPolicy": "GAPLESS",
        }),
    )
    .await;

    type_id
}

async fn approver(app: &TestApp, username: &str) -> String {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM roles WHERE tenant_id = $1 AND role_code = $2 AND deleted_at IS NULL",
    )
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(APPROVER_ROLE)
    .fetch_optional(&app.pool)
    .await
    .expect("look the role up");

    let role = match existing {
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
                    "document:create",
                    "document:submit",
                ],
            )
            .await
        }
    };

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@kelir.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(username, common::ADMIN_PASSWORD).await
}

/// A document of `type_id`, drafted, submitted and approved — one committed
/// transition, and so one `Workflow.Transitioned` event.
async fn approved_document(app: &TestApp, token: &str, type_id: Uuid) -> Uuid {
    let created = app
        .post(
            "/api/v1/documents",
            Some(token),
            json!({
                "documentTypeId": type_id,
                "title": "Two standing desks",
                "formData": { "amount": 100 },
            }),
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

    let task: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_tasks
         WHERE document_id = $1 AND status IN ('CREATED', 'ASSIGNED', 'IN_PROGRESS')
           AND deleted_at IS NULL",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("an open task");

    let decided = app
        .post(
            &format!("/api/v1/workflow/tasks/{task}/decision"),
            Some(token),
            json!({ "action": "APPROVE" }),
        )
        .await;

    // **AC-7**: whatever the after-chain will do, the decision has already
    // succeeded — the chain runs later, from the outbox, and cannot reach this
    // response.
    assert_eq!(decided.status, StatusCode::OK, "{}", decided.body);

    document
}

/// A registry entry on the after hook, for every type in the tenant.
async fn register_after_hook(app: &TestApp, handler: &str) {
    sqlx::query(
        "INSERT INTO document_lifecycle_hooks
             (id, tenant_id, document_type_id, hook_name, handler_reference,
              priority, config_json, is_enabled)
         VALUES ($1, $2, NULL, $3, $4, 150, '{}', true)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(AFTER)
    .bind(handler)
    .execute(&app.pool)
    .await
    .expect("the registration is seeded");
}

/// The one event a one-step document produced.
#[derive(Debug, sqlx::FromRow)]
struct Row {
    id: Uuid,
    status: String,
    attempt_count: i32,
    last_error: Option<String>,
    processed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Seconds from now until `next_attempt_at`, on the database's clock.
    due_in: Option<f64>,
    payload_json: Value,
}

async fn event_of(app: &TestApp, document: Uuid) -> Row {
    sqlx::query_as(
        "SELECT id, status, attempt_count, last_error, processed_at,
                EXTRACT(EPOCH FROM next_attempt_at - now())::float8 AS due_in,
                payload_json
         FROM outbox_events
         WHERE payload_json->'payload'->>'documentId' = $1::text",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the document's one event")
}

/// Moves every waiting row's time into the past — a retry, or a lease, that
/// has come due.
async fn make_due(app: &TestApp) {
    sqlx::query(
        "UPDATE outbox_events SET next_attempt_at = now() - interval '1 second'
         WHERE status IN ('FAILED', 'PROCESSING')",
    )
    .execute(&app.pool)
    .await
    .expect("the retry comes due");
}

async fn runs_on(app: &TestApp, document: Uuid) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT hook_name, handler_reference, result FROM document_hook_executions
         WHERE document_id = $1 ORDER BY executed_at, id",
    )
    .bind(document)
    .fetch_all(&app.pool)
    .await
    .expect("read the hook log")
}

async fn user_id(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("the user")
}

async fn breaker_notifications(app: &TestApp, recipient: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM notifications
         WHERE recipient_user_id = $1 AND notification_type = 'HOOK_CIRCUIT_OPENED'",
    )
    .bind(recipient)
    .fetch_one(&app.pool)
    .await
    .expect("count the notifications")
}

// ---------------------------------------------------------------------------
// The after-chain runs
// ---------------------------------------------------------------------------

/// **JWSS `actions` run**, after commit, and each run is logged `CONTINUE`
/// with the transition it ran on (LHCS §7; ADR-0041 §2).
///
/// Seen red, 2026-09-24: `edge.map(|edge| edge.actions.clone())` replaced by
/// an empty chain in `workflow::service::after_hooks::deliver` — the event was
/// settled `PROCESSED` with nothing in the log, which is ADR-0036's negative
/// passing for a success.
#[tokio::test]
async fn a_transition_s_actions_run_from_the_outbox_and_are_logged_continue() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.one").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        one_step_workflow("outbox_one", json!([{ "handler": "core:continue_always" }])),
    )
    .await;
    let type_id = document_type(&app, &admin, "OUTBOX_ONE", workflow).await;
    let document = approved_document(&app, &token, type_id).await;

    // Nothing has run yet: the transition committed and left an event.
    assert!(runs_on(&app, document).await.is_empty());
    assert_eq!(event_of(&app, document).await.status, "PENDING");

    assert_eq!(app.deliver_outbox().await, 1);

    assert_eq!(
        runs_on(&app, document).await,
        vec![(
            AFTER.to_owned(),
            "core:continue_always".to_owned(),
            "CONTINUE".to_owned()
        )]
    );

    let (source, transition_ref): (String, Option<String>) = sqlx::query_as(
        "SELECT source, workflow_transition_ref FROM document_hook_executions
         WHERE document_id = $1",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the execution");

    assert_eq!(source, "WORKFLOW");
    assert_eq!(
        transition_ref.as_deref(),
        Some("outbox_one@1:SUBMITTED->APPROVED")
    );

    let row = event_of(&app, document).await;

    assert_eq!(row.status, "PROCESSED");
    assert_eq!(row.attempt_count, 1);
    assert!(row.processed_at.is_some());
    assert!(row.last_error.is_none());

    // **And a processed event is not delivered again**: the next pass claims
    // nothing, and the log still holds one run.
    assert_eq!(app.deliver_outbox().await, 0);
    assert_eq!(runs_on(&app, document).await.len(), 1);
}

/// A transition with nothing registered still writes its event, and the
/// worker settles it with nothing run (ADR-0041 §4's first negative).
#[tokio::test]
async fn an_event_whose_chain_is_empty_is_settled_with_nothing_run() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.two").await;

    let workflow = publish_workflow(&app, &admin, one_step_workflow("outbox_two", json!([]))).await;
    let type_id = document_type(&app, &admin, "OUTBOX_TWO", workflow).await;
    let document = approved_document(&app, &token, type_id).await;

    app.deliver_outbox().await;

    assert_eq!(event_of(&app, document).await.status, "PROCESSED");
    assert!(runs_on(&app, document).await.is_empty());
}

// ---------------------------------------------------------------------------
// Retry, dead letter, lease
// ---------------------------------------------------------------------------

/// **ADR-0041 §2's schedule**: 30 s, 1 min, 2 min, 4 min, 8 min — and the sixth
/// failed attempt dead-letters the row, keeping its envelope.
///
/// Seen red, 2026-09-24, two mutations: `MAX_ATTEMPTS` set to 7 in
/// `outbox::domain` (the sixth attempt was `FAILED` again), and
/// `30 * 2u64.pow(exponent)` as `30 * (exponent + 1)` in `retry_delay` (the
/// third attempt was due in 90 s, not 120).
#[tokio::test]
async fn a_failing_delivery_is_retried_on_the_schedule_and_dead_lettered_on_the_sixth_failure() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.three").await;

    register_after_hook(&app, ABSENT_PLUGIN).await;

    let workflow =
        publish_workflow(&app, &admin, one_step_workflow("outbox_three", json!([]))).await;
    let type_id = document_type(&app, &admin, "OUTBOX_THREE", workflow).await;
    let document = approved_document(&app, &token, type_id).await;

    for (attempt, delay) in [(1, 30.0), (2, 60.0), (3, 120.0), (4, 240.0), (5, 480.0)] {
        assert_eq!(
            app.deliver_outbox().await,
            1,
            "attempt {attempt} was claimed"
        );

        let row = event_of(&app, document).await;

        assert_eq!(row.status, "FAILED", "after attempt {attempt}");
        assert_eq!(row.attempt_count, attempt);
        assert!(
            row.last_error
                .as_deref()
                .is_some_and(|error| error.contains(ABSENT_PLUGIN)),
            "the reason is kept: {:?}",
            row.last_error
        );

        let due_in = row.due_in.expect("a retry time");

        assert!(
            due_in > delay - 5.0 && due_in <= delay,
            "attempt {attempt} is retried in {due_in}s, not {delay}s"
        );

        // **Not before its time**: a pass now claims nothing.
        assert_eq!(
            app.deliver_outbox().await,
            0,
            "attempt {attempt} came early"
        );

        make_due(&app).await;
    }

    assert_eq!(app.deliver_outbox().await, 1);

    let row = event_of(&app, document).await;

    assert_eq!(row.status, "DEAD_LETTER");
    assert_eq!(row.attempt_count, 6);
    assert!(row.due_in.is_none(), "a dead letter is never due");
    // The dispatch is exhausted and the event is not: the envelope is intact.
    assert_eq!(row.payload_json["eventId"], row.id.to_string());

    make_due(&app).await;
    assert_eq!(
        app.deliver_outbox().await,
        0,
        "a dead letter is not claimed"
    );
}

/// **A crashed worker's row is delivered again, not lost** (ADR-0041 §2,
/// AC-10). A `PROCESSING` row whose lease has expired is claimed; one whose
/// lease is still running is not.
///
/// Seen red, 2026-09-24: `status IN ('FAILED', 'PROCESSING')` narrowed to
/// `status = 'FAILED'` in `outbox::repository::claim` — the expired lease was
/// never reclaimed, so the event stayed `PROCESSING` for ever.
#[tokio::test]
async fn a_processing_row_past_its_lease_is_delivered_again() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.four").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        one_step_workflow(
            "outbox_four",
            json!([{ "handler": "core:continue_always" }]),
        ),
    )
    .await;
    let type_id = document_type(&app, &admin, "OUTBOX_FOUR", workflow).await;
    let crashed = approved_document(&app, &token, type_id).await;
    let working = approved_document(&app, &token, type_id).await;

    // What a worker that died mid-delivery leaves: claimed, and its lease gone.
    sqlx::query(
        "UPDATE outbox_events SET status = 'PROCESSING', next_attempt_at = now() - interval '1 second'
         WHERE payload_json->'payload'->>'documentId' = $1::text",
    )
    .bind(crashed)
    .execute(&app.pool)
    .await
    .expect("the crashed claim");

    // And one another worker holds right now — the second subject.
    sqlx::query(
        "UPDATE outbox_events SET status = 'PROCESSING', next_attempt_at = now() + interval '4 minutes'
         WHERE payload_json->'payload'->>'documentId' = $1::text",
    )
    .bind(working)
    .execute(&app.pool)
    .await
    .expect("the live claim");

    assert_eq!(app.deliver_outbox().await, 1);

    assert_eq!(event_of(&app, crashed).await.status, "PROCESSED");
    assert_eq!(runs_on(&app, crashed).await.len(), 1);

    assert_eq!(event_of(&app, working).await.status, "PROCESSING");
    assert!(runs_on(&app, working).await.is_empty());
}

/// EES §3: *consumers MUST ignore event types they do not know.* Settled, not
/// held and not failed — and kept.
#[tokio::test]
async fn an_event_type_no_consumer_reads_is_settled_rather_than_held() {
    let app = TestApp::spawn().await;
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO outbox_events
             (id, tenant_id, aggregate_type, aggregate_id, event_type, payload_json)
         VALUES ($1, $2, 'SYSTEM', $1, 'Report.Scheduled', '{}')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("an event of a type this build does not consume");

    assert_eq!(app.deliver_outbox().await, 1);

    let status: String = sqlx::query_scalar("SELECT status FROM outbox_events WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the row is kept");

    assert_eq!(status, "PROCESSED");
}

// ---------------------------------------------------------------------------
// The circuit breaker
// ---------------------------------------------------------------------------

/// **Five `ERROR`s in a row open the breaker, and the tenant's administrators
/// are told once** (ADR-0041 §2, AC-8). While it is open the handler is not
/// run, and the event waits rather than being dropped.
///
/// **A failed trial keeps it open and tells nobody again**: the cool-down
/// passes, the one trial runs and fails, and that sixth consecutive `ERROR` is
/// not an opening. This is the only route to a sixth `ERROR` — while the
/// breaker is open the handler is not run at all — so without the trial step
/// the *told once* assertion had nothing that could break it.
///
/// Seen red, 2026-09-24, two mutations: `breaker_is_open` returning `false`
/// (the second document's delivery ran the handler, a sixth `ERROR`); and
/// `breaker_opened_by_latest` without its sixth-row condition (the failed trial
/// told the administrator a second time). The second mutation came back
/// **green** against this test's first version, which stopped before the
/// trial; the trial step is what that finding added.
#[tokio::test]
async fn five_consecutive_errors_open_the_breaker_and_tell_the_administrators_once() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.five").await;
    let administrator = user_id(&app, common::ADMIN_USERNAME).await;
    let bystander = user_id(&app, "outbox.five").await;

    register_after_hook(&app, ABSENT_PLUGIN).await;

    let workflow =
        publish_workflow(&app, &admin, one_step_workflow("outbox_five", json!([]))).await;
    let type_id = document_type(&app, &admin, "OUTBOX_FIVE", workflow).await;
    let first = approved_document(&app, &token, type_id).await;

    for attempt in 1..=4 {
        app.deliver_outbox().await;
        make_due(&app).await;

        assert_eq!(
            breaker_notifications(&app, administrator).await,
            0,
            "nobody is told before the fifth failure (attempt {attempt})"
        );
    }

    app.deliver_outbox().await;

    assert_eq!(runs_on(&app, first).await.len(), 5);
    assert!(runs_on(&app, first)
        .await
        .iter()
        .all(|(_, _, result)| result == "ERROR"));
    assert_eq!(breaker_notifications(&app, administrator).await, 1);
    // The second subject: somebody who is not an administrator hears nothing.
    assert_eq!(breaker_notifications(&app, bystander).await, 0);

    // A second event while the breaker is open: not run, and held.
    let second = approved_document(&app, &token, type_id).await;

    app.deliver_outbox().await;

    assert!(
        runs_on(&app, second).await.is_empty(),
        "an open breaker must not run the handler"
    );

    let held = event_of(&app, second).await;

    assert_eq!(held.status, "FAILED");
    assert!(
        held.last_error
            .as_deref()
            .is_some_and(|error| error.contains("circuit breaker")),
        "{:?}",
        held.last_error
    );

    // The cool-down passes and the trial fails: a sixth `ERROR`, still open,
    // and still one notification.
    sqlx::query(
        "UPDATE document_hook_executions SET executed_at = executed_at - interval '10 minutes'",
    )
    .execute(&app.pool)
    .await
    .expect("the cool-down passes");
    make_due(&app).await;
    app.deliver_outbox().await;

    let runs = runs_on(&app, first).await;

    assert_eq!(runs.len(), 6, "the trial ran");
    assert_eq!(runs[5].2, "ERROR");
    assert_eq!(breaker_notifications(&app, administrator).await, 1);

    // And the failed trial restarted the cool-down from itself: held again.
    make_due(&app).await;
    app.deliver_outbox().await;

    assert_eq!(runs_on(&app, first).await.len(), 6);
    assert_eq!(runs_on(&app, second).await.len(), 0);
}

/// **After the cool-down one trial runs, and a success closes the breaker**
/// (ADR-0041 §2). Before the cool-down the same handler is held.
///
/// The five failures are seeded into the log directly — the breaker is derived
/// from it and from nothing else, so a log that says five `ERROR`s *is* an open
/// breaker — against `core:continue_always`, the one handler that can then
/// succeed.
///
/// Seen red, 2026-09-24: `&& !recent[0].cooled` removed from `breaker_is_open`
/// — the trial never ran, and the event after the cool-down was held like the
/// one before it.
#[tokio::test]
async fn a_trial_after_the_cool_down_that_succeeds_closes_the_breaker() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.six").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        one_step_workflow("outbox_six", json!([{ "handler": "core:continue_always" }])),
    )
    .await;
    let type_id = document_type(&app, &admin, "OUTBOX_SIX", workflow).await;
    let first = approved_document(&app, &token, type_id).await;

    // Five failures a minute ago: open, and not yet cooled.
    for _ in 0..5 {
        sqlx::query(
            "INSERT INTO document_hook_executions
                 (id, tenant_id, source, document_id, hook_name, handler_reference, result,
                  error_message, executed_at)
             VALUES ($1, $2, 'WORKFLOW', $3, $4, 'core:continue_always', 'ERROR',
                     'seeded', now() - interval '1 minute')",
        )
        .bind(Uuid::now_v7())
        .bind(fixtures::SYSTEM_TENANT_ID)
        .bind(first)
        .bind(AFTER)
        .execute(&app.pool)
        .await
        .expect("a seeded failure");
    }

    app.deliver_outbox().await;

    assert_eq!(event_of(&app, first).await.status, "FAILED");
    assert_eq!(
        runs_on(&app, first).await.len(),
        5,
        "held: the handler did not run"
    );

    // Ten minutes pass since the last failure.
    sqlx::query(
        "UPDATE document_hook_executions SET executed_at = executed_at - interval '10 minutes'",
    )
    .execute(&app.pool)
    .await
    .expect("the cool-down passes");
    make_due(&app).await;

    app.deliver_outbox().await;

    let runs = runs_on(&app, first).await;

    assert_eq!(event_of(&app, first).await.status, "PROCESSED");
    assert_eq!(runs.len(), 6);
    assert_eq!(runs[5].2, "CONTINUE", "the trial ran and succeeded");

    // Closed: the next event runs at once, with no cool-down of its own.
    let second = approved_document(&app, &token, type_id).await;

    app.deliver_outbox().await;

    assert_eq!(event_of(&app, second).await.status, "PROCESSED");
    assert_eq!(
        runs_on(&app, second).await,
        vec![(
            AFTER.to_owned(),
            "core:continue_always".to_owned(),
            "CONTINUE".to_owned()
        )]
    );
}

// ---------------------------------------------------------------------------
// Handler kinds at delivery
// ---------------------------------------------------------------------------

/// **A before-only handler reached through an older definition** is an `ERROR`
/// that does not hold the event, and the rest of the chain runs (ADR-0041 §2).
///
/// The definition is published with a valid `actions` list and then rewritten
/// in place, which is what a definition published before the kind rule looks
/// like to the worker: publish would refuse it today.
///
/// Seen red, 2026-09-24: the kind branch of `hook::service::run_after_chain`
/// pushing its message into `failures` — the event went `FAILED` and would have
/// been retried to a dead letter for a failure no retry can change.
#[tokio::test]
async fn a_before_only_handler_in_an_older_definition_is_an_error_that_does_not_hold_the_event() {
    let app = TestApp::spawn().await;
    let admin = app.administrator_token().await;
    let token = approver(&app, "outbox.seven").await;

    let workflow = publish_workflow(
        &app,
        &admin,
        one_step_workflow(
            "outbox_seven",
            json!([{ "handler": "core:continue_always" }]),
        ),
    )
    .await;

    sqlx::query(
        "UPDATE workflow_definitions
         SET definition_json = jsonb_set(definition_json, '{transitions,0,actions}', $1)
         WHERE id = $2",
    )
    .bind(json!([
        { "handler": "core:reject_when", "priority": 300,
          "config": { "condition": { "==": [1, 1] } } },
        { "handler": "core:continue_always", "priority": 310 }
    ]))
    .bind(workflow)
    .execute(&app.pool)
    .await
    .expect("the definition as an older release stored it");

    let type_id = document_type(&app, &admin, "OUTBOX_SEVEN", workflow).await;
    let document = approved_document(&app, &token, type_id).await;

    app.deliver_outbox().await;

    assert_eq!(
        runs_on(&app, document).await,
        vec![
            (
                AFTER.to_owned(),
                "core:reject_when".to_owned(),
                "ERROR".to_owned()
            ),
            (
                AFTER.to_owned(),
                "core:continue_always".to_owned(),
                "CONTINUE".to_owned()
            ),
        ]
    );

    let message: Option<String> = sqlx::query_scalar(
        "SELECT error_message FROM document_hook_executions
         WHERE document_id = $1 AND result = 'ERROR'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the error row");

    assert!(
        message
            .as_deref()
            .is_some_and(|message| message.contains("before-hook handler")),
        "{message:?}"
    );
    assert_eq!(event_of(&app, document).await.status, "PROCESSED");
}
