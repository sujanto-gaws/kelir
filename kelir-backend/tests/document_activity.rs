//! The timeline, and the two properties that make it different from the audit
//! trail (FR-ACT-001, FR-ACT-004; [#247]) — it does not outlive the action it
//! describes, and since [#292] it does not answer another record's question.
//!
//! [#247]: https://github.com/sujanto-gaws/kelir/issues/247
//! [#292]: https://github.com/sujanto-gaws/kelir/issues/292

mod common;

use axum::http::{Method, StatusCode};
use kelir_backend::modules::activity::domain::EventCategory;
use kelir_backend::modules::activity::service::{self as activity, Happening};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp};

fn id_of(value: &Value) -> Uuid {
    value["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a uuid")
}

async fn document_type(app: &TestApp, token: &str, code: &str) -> Uuid {
    let created = app
        .send(
            Method::POST,
            "/api/v1/document-types",
            Some(token),
            Some(json!({ "typeCode": code, "name": code })),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    id_of(&created.body["data"])
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

// ---------------------------------------------------------------------------
// AC2 — the event belongs to the action's transaction
// ---------------------------------------------------------------------------

/// **The property that separates this record from the audit trail**, and the
/// reason `record` takes a transaction where `audit::record` takes a pool.
///
/// An action that rolled back did not happen. A timeline saying it did would be
/// worse than one that never mentioned it — and worse in a way nobody can see,
/// because the row looks exactly like a real one.
///
/// **The mutation for this does not compile, which is the stronger result.**
/// Changing `record` to take a `&PgPool` — the audit trail's shape — fails at
/// every call site with *expected `&Pool<Postgres>`, found `&mut
/// Transaction<'_, Postgres>`*, because each one has a transaction and nothing
/// else. So the property is held by the signature rather than by this test, and
/// what this test guards is the day somebody gives the module a second way in.
/// Tried 2026-08-31; recorded here rather than claimed as a red test, because
/// it never got as far as running.
#[tokio::test]
async fn an_event_does_not_survive_the_action_it_describes_rolling_back() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_ROLLBACK").await;
    let document = draft(&app, &token, type_id).await;

    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    let mut transaction = app.pool.begin().await.expect("a transaction");

    activity::record(
        &mut transaction,
        &Happening {
            tenant_id: fixtures::SYSTEM_TENANT_ID,
            document_id: Some(document),
            workflow_instance_id: None,
            task_id: None,
            attachment_id: None,
            comment_id: None,
            event_type: "Document.Imagined",
            category: EventCategory::Document,
            actor_user_id: None,
            actor_name: Some("nobody"),
            action_summary: "Something that did not happen",
            details: json!({}),
        },
    )
    .await
    .expect("the event to be written into the transaction");

    // The action fails, so the transaction goes back.
    transaction.rollback().await.expect("the rollback");

    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    assert_eq!(
        after, before,
        "an event outlived the action it describes; that is the audit trail's rule, not this one"
    );
}

// ---------------------------------------------------------------------------
// AC4 — append-only, asserted over the columns
// ---------------------------------------------------------------------------

/// **Over `information_schema`, not over the router** (#247 AC4).
///
/// A route that does not exist today is one somebody adds tomorrow. What makes
/// this table append-only is that an edit has nothing to stamp and a soft delete
/// has nowhere to write — which is a fact about the columns, and this is how
/// [#181](https://github.com/sujanto-gaws/kelir/issues/181) AC6 asserted the
/// same property one table over.
#[tokio::test]
async fn the_timeline_is_append_only_by_its_columns() {
    let app = TestApp::spawn().await;

    for column in ["updated_at", "updated_by", "deleted_at"] {
        let present: Option<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'activity_events' AND column_name = $1",
        )
        .bind(column)
        .fetch_optional(&app.pool)
        .await
        .expect("the column list");

        assert!(
            present.is_none(),
            "`activity_events.{column}` exists, so this table is no longer append-only"
        );
    }
}

// ---------------------------------------------------------------------------
// AC5 — the name as it was
// ---------------------------------------------------------------------------

/// **A rename does not rewrite the past.**
///
/// `actor_user_id` still points at the person and is the join for anything that
/// needs them *now*; `actor_name` is what they were called when this happened.
/// The opposite choice is `comments`, which joins `users` for a live name — a
/// conversation has current participants and a history has the people who were
/// there.
#[tokio::test]
async fn a_renamed_actor_does_not_change_what_the_timeline_says() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_RENAME").await;
    let document = draft(&app, &token, type_id).await;

    let recorded: String = sqlx::query_scalar(
        "SELECT actor_name FROM activity_events WHERE document_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the created event");

    assert_eq!(recorded, common::ADMIN_USERNAME);

    sqlx::query("UPDATE users SET username = 'renamed-since' WHERE username = $1")
        .bind(common::ADMIN_USERNAME)
        .execute(&app.pool)
        .await
        .expect("the rename");

    let after: String = sqlx::query_scalar(
        "SELECT actor_name FROM activity_events WHERE document_id = $1 AND event_type = 'Document.Created'",
    )
    .bind(document)
    .fetch_one(&app.pool)
    .await
    .expect("the created event");

    assert_eq!(
        after,
        common::ADMIN_USERNAME,
        "the timeline followed a rename, so it no longer says what happened"
    );
}

// ---------------------------------------------------------------------------
// AC2, AC6 — the events that are actually written, and who can read them
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_document_lifecycle_and_the_workflow_both_reach_the_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_LIFECYCLE").await;
    let document = draft(&app, &token, type_id).await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"][0]["eventType"], "Document.Created");
    assert_eq!(listed.body["data"][0]["eventCategory"], "DOCUMENT");
    assert_eq!(listed.body["data"][0]["actorName"], common::ADMIN_USERNAME);
    assert_eq!(listed.body["meta"]["total"], 1);
}

/// **Tenant scope lives in the statement** (#247 AC6), not in the handler that
/// called it — the [#106](https://github.com/sujanto-gaws/kelir/issues/106) /
/// [#121](https://github.com/sujanto-gaws/kelir/issues/121) lesson, which cost
/// this project three sprints of coverage findings.
///
/// **Seen red, 2026-08-31**, with `tenant_id = $1` dropped from the read.
///
/// The row below is written straight into the table against **this** document
/// but another tenant's id, which is a thing no service would do and exactly
/// what the predicate has to refuse.
#[tokio::test]
async fn an_event_from_another_tenant_is_refused_by_the_query() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_TENANT").await;
    let document = draft(&app, &token, type_id).await;

    let other = fixtures::create_tenant(&app.pool, "OTHER-ACT", "Another tenant").await;

    sqlx::query(
        "INSERT INTO activity_events \
         (id, tenant_id, document_id, event_type, event_category, action_summary) \
         VALUES ($1, $2, $3, 'Document.Created', 'DOCUMENT', 'From somewhere else')",
    )
    .bind(Uuid::now_v7())
    .bind(other)
    .bind(document)
    .execute(&app.pool)
    .await
    .expect("the foreign row");

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let events = listed.body["data"].as_array().expect("a page");

    assert_eq!(
        events.len(),
        1,
        "another tenant's event reached this timeline"
    );
    assert_eq!(
        listed.body["meta"]["total"], 1,
        "and the count is drawn under the same rule as the page"
    );
}

/// **`activity:read` is not `document:read`** (coding standard §2.9).
///
/// **Seen red, 2026-08-31**, with `caller.require(ACTIVITY_READ)?` deleted.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_read_its_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-ACT-READER",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "act-reader",
        "act-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("act-reader", common::ADMIN_PASSWORD).await;

    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&reader),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// **The timeline is not the audit trail**, asserted over the rows.
///
/// #247 AC3 asks for the distinction in the module documentation; this is what
/// makes it checkable. `modules::activity` contains no statement reading
/// `audit_events`, and creating a document writes to both — separately, with
/// neither derived from the other.
#[tokio::test]
async fn the_timeline_and_the_audit_trail_are_written_separately() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_DISTINCT").await;
    let document = draft(&app, &token, type_id).await;

    let timeline: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE document_id = $1")
            .bind(document)
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE object_id = $1")
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(timeline, 1, "the document's creation is on its timeline");
    assert!(
        audited >= 1,
        "and in the audit trail, which is a different table"
    );
}

// ---------------------------------------------------------------------------
// #292 / D-45 — the timeline carries the link and not the subject
// ---------------------------------------------------------------------------

/// A reader who may see the document and its timeline and **nothing that hangs
/// on it**: `activity:read` and `document:read`, without `attachment:read` or
/// `comment:read`.
async fn timeline_only_reader(app: &TestApp, username: &str) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        &["document:read", "activity:read"],
    )
    .await;

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

/// One row of the shape a release before **D-45** wrote: a real event type, and
/// subject detail in `details_json`.
///
/// **Written with `INSERT` on purpose.** The writers no longer produce these
/// keys, so a test that only went through them would be asserting over an empty
/// object and would stay green with the guard deleted. Every deployment that has
/// run Sprint 12 holds rows exactly like this one, and `activity_events` is
/// append-only — the read is the only place left that can answer for them.
async fn legacy_event(
    app: &TestApp,
    document: Uuid,
    event_type: &str,
    category: &str,
    details: Value,
) {
    sqlx::query(
        "INSERT INTO activity_events \
         (id, tenant_id, document_id, event_type, event_category, action_summary, details_json) \
         VALUES ($1, $2, $3, $4, $5, 'Something happened', $6)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(document)
    .bind(event_type)
    .bind(category)
    .bind(details)
    .execute(&app.pool)
    .await
    .expect("the row an earlier release would have written");
}

fn entry_of<'a>(body: &'a Value, event_type: &str) -> &'a Value {
    body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .find(|entry| entry["eventType"] == event_type)
        .unwrap_or_else(|| panic!("no `{event_type}` entry: {body}"))
}

/// **AC1 — a file's name is not on the timeline of a caller who may not read
/// attachments.**
///
/// `modules::attachment`'s header says an attachment is as private as the
/// document it hangs on. Its *name* was more visible than that, because the
/// attachment surface checks `attachment:read` and the timeline did not — and a
/// file name is routinely the sensitive part, which is the whole of #292:
/// *2026-redundancy-list.pdf* needs no contents to do damage.
///
/// **Seen red, 2026-09-01**, with the `domain::disclosable` map deleted from
/// `service::list_activity` — the entry comes back carrying
/// `"originalFileName": "2026-redundancy-list.pdf"` to a caller holding no
/// `attachment:read`.
#[tokio::test]
async fn an_earlier_releases_file_name_is_not_served_to_a_caller_without_attachment_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_D45_ATT").await;
    let document = draft(&app, &token, type_id).await;

    legacy_event(
        &app,
        document,
        "Attachment.Added",
        "ATTACHMENT",
        json!({ "originalFileName": "2026-redundancy-list.pdf", "fileSize": 81_920 }),
    )
    .await;

    let reader = timeline_only_reader(&app, "act-d45-att").await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&reader),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    // **The entry is still there.** D-45 redacts a field; it does not drop a
    // row — filtering the timeline was the other shape #292 offered, and it
    // would have put the page and `meta.total` into disagreement while hiding
    // from this reader that anything had been attached at all.
    let entry = entry_of(&listed.body, "Attachment.Added");

    assert_eq!(entry["details"], json!({}));
    assert!(
        !listed.body.to_string().contains("redundancy"),
        "the file name reached a caller holding nothing that guards it: {}",
        listed.body
    );
    assert_eq!(
        listed.body["meta"]["total"], 2,
        "the creation and the attachment; a redacted entry is still an entry"
    );
}

/// **AC2, first half — the same for a comment and `comment:read`.**
///
/// The body was never on the timeline; its **length** was, which is a
/// measurement of a thing this caller may not read. D-12 and D-32 drew the line
/// for the decision comment, and D-45 puts the length on the same side of it.
///
/// **Seen red, 2026-09-01**, with the `domain::disclosable` map deleted:
/// `"length": 240` comes back to a caller holding no `comment:read`.
#[tokio::test]
async fn an_earlier_releases_comment_length_is_not_served_to_a_caller_without_comment_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_D45_CMT").await;
    let document = draft(&app, &token, type_id).await;

    legacy_event(
        &app,
        document,
        "Comment.Added",
        "COMMENT",
        json!({ "length": 240 }),
    )
    .await;

    let reader = timeline_only_reader(&app, "act-d45-cmt").await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&reader),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        entry_of(&listed.body, "Comment.Added")["details"],
        json!({})
    );
}

/// **AC2, second half — a delegation's second party, and the workflow's own
/// read.**
///
/// This is the one entry D-45 redacts by key rather than wholesale, and the
/// split is the decision in miniature: `action`, `from` and `to` are what moved
/// **this document**, which is the question the timeline exists to answer and
/// which `document:read` already covers. `onBehalfOfUserId` answers a different
/// one — that a delegation happened, and who was behind it — and
/// `workflow_history` keeps it, behind the workflow's read.
///
/// **Seen red, 2026-09-01**, with `"onBehalfOfUserId"` added to
/// `Workflow.Decided`'s permitted keys in `domain::disclosable`: the delegator's
/// id comes back to a caller holding no workflow permission at all.
#[tokio::test]
async fn a_delegations_second_party_is_not_served_from_the_timeline() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_D45_WF").await;
    let document = draft(&app, &token, type_id).await;

    let delegator = Uuid::now_v7();

    legacy_event(
        &app,
        document,
        "Workflow.Decided",
        "WORKFLOW",
        json!({
            "action": "APPROVE",
            "from": "PENDING_MANAGER",
            "to": "PENDING_FINANCE",
            "onBehalfOfUserId": delegator,
        }),
    )
    .await;

    let reader = timeline_only_reader(&app, "act-d45-wf").await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&reader),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);

    let details = &entry_of(&listed.body, "Workflow.Decided")["details"];

    assert_eq!(
        details["action"], "APPROVE",
        "what moved the document stays"
    );
    assert_eq!(details["from"], "PENDING_MANAGER");
    assert_eq!(details["to"], "PENDING_FINANCE");
    assert!(
        details.get("onBehalfOfUserId").is_none(),
        "the timeline named the person a decision was taken for, to a caller \
         who cannot read the workflow: {details}"
    );
    assert!(!listed.body.to_string().contains(&delegator.to_string()));
}

/// **An event type this release does not know serves nothing.**
///
/// The allow-list forgets in the safe direction, which is the reason it is an
/// allow-list: a row written by a release that did not consult the table is a
/// row whose keys nobody has decided about. The entry still renders — the event
/// type, the summary, the actor and the link are a timeline — so refusing the
/// detail costs a label rather than the event.
///
/// **Seen red, 2026-09-01**, with `disclosable`'s `_` arm returning `details`
/// unchanged.
#[tokio::test]
async fn an_unknown_event_type_discloses_nothing_and_still_appears() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_D45_UNKNOWN").await;
    let document = draft(&app, &token, type_id).await;

    legacy_event(
        &app,
        document,
        "Attachment.Renamed",
        "ATTACHMENT",
        json!({ "originalFileName": "2026-redundancy-list.pdf" }),
    )
    .await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        entry_of(&listed.body, "Attachment.Renamed")["details"],
        json!({}),
        "an event type the allow-list has never heard of served its details"
    );
}

/// **The other half of D-45: the writers stopped producing the keys**, and what
/// they produce instead is the link.
///
/// Asserted over `details_json` in the table rather than over the response, so
/// what is measured is the *write* and not the redaction above it. The upload
/// and the comment both go through their real surfaces.
#[tokio::test]
async fn an_upload_and_a_comment_write_a_link_and_no_subject_detail() {
    const PDF: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<<>>\n%%EOF\n";

    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_ACT_D45_WRITE").await;
    let document = draft(&app, &token, type_id).await;

    let uploaded = app
        .post_multipart(
            &format!("/api/v1/documents/{document}/attachments"),
            Some(&token),
            "2026-redundancy-list.pdf",
            "application/pdf",
            PDF,
            None,
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::OK, "{}", uploaded.body);
    let attachment = id_of(&uploaded.body["data"]);

    let commented = app
        .send(
            Method::POST,
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            Some(json!({ "body": "The list is wrong about the third line." })),
        )
        .await;
    assert_eq!(commented.status, StatusCode::OK, "{}", commented.body);
    let comment = id_of(&commented.body["data"]);

    for (event_type, column, subject) in [
        ("Attachment.Added", "attachment_id", attachment),
        ("Comment.Added", "comment_id", comment),
    ] {
        let (details, linked): (Value, Option<Uuid>) = sqlx::query_as(&format!(
            "SELECT details_json, {column} FROM activity_events \
             WHERE document_id = $1 AND event_type = $2"
        ))
        .bind(document)
        .bind(event_type)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("the `{event_type}` row: {error}"));

        assert_eq!(
            details,
            json!({}),
            "`{event_type}` still writes something about its subject into the \
             timeline, so the row the next release reads is the disclosure again"
        );
        assert_eq!(
            linked,
            Some(subject),
            "`{event_type}` carries no detail and no link either, which is not \
             a redaction but a lost event"
        );
    }

    // And the link is what the surface serves, so a reader who *does* hold
    // `attachment:read` has somewhere to go and ask.
    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/activity"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        entry_of(&listed.body, "Attachment.Added")["attachmentId"],
        Value::String(attachment.to_string())
    );
    assert_eq!(
        entry_of(&listed.body, "Comment.Added")["commentId"],
        Value::String(comment.to_string())
    );
}
