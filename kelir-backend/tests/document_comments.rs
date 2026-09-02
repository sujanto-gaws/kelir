//! A conversation about a document: adding to it, replying, editing and
//! deleting (FR-CMT-001 to FR-CMT-004; [#249], [#253]).
//!
//! [#249]: https://github.com/sujanto-gaws/kelir/issues/249
//! [#253]: https://github.com/sujanto-gaws/kelir/issues/253

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use uuid::Uuid;

use common::{fixtures, TestApp, TestResponse};

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
// AC2 — a comment is added, and read back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_comment_is_added_to_a_document_and_read_back_with_its_author() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ADD").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "  is this the right supplier?  " }),
        )
        .await;

    assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    // Trimmed on the way in (#249 AC4).
    assert_eq!(added.body["data"]["body"], "is this the right supplier?");
    assert_eq!(added.body["data"]["authorUsername"], common::ADMIN_USERNAME);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(listed.body["data"].as_array().expect("a page").len(), 1);
    assert_eq!(listed.body["meta"]["total"], 1);
}

#[tokio::test]
async fn a_conversation_is_read_in_the_order_it_was_said() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ORDER").await;
    let document = draft(&app, &token, type_id).await;

    for body in ["first", "second", "third"] {
        let added = app
            .post(
                &format!("/api/v1/documents/{document}/comments"),
                Some(&token),
                json!({ "body": body }),
            )
            .await;

        assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    }

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    let bodies: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|comment| comment["body"].as_str().expect("a body"))
        .collect();

    // **Oldest first**, which is the opposite of every other list in this
    // product: a conversation is read in the order it was said.
    assert_eq!(bodies, vec!["first", "second", "third"]);
}

/// **The count and the page apply the same rule**, which is
/// [#279](https://github.com/sujanto-gaws/kelir/issues/279)'s lesson applied
/// where the same duplication exists: a second document's comments must not
/// reach either.
///
/// **Seen red, 2026-08-31**, with `document_id = $2` dropped from
/// `count_for_document`: the page stayed at 2 and the total became 5.
#[tokio::test]
async fn the_total_counts_this_documents_comments_and_no_others() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_SCOPE").await;
    let mine = draft(&app, &token, type_id).await;
    let theirs = draft(&app, &token, type_id).await;

    for _ in 0..2 {
        app.post(
            &format!("/api/v1/documents/{mine}/comments"),
            Some(&token),
            json!({ "body": "on mine" }),
        )
        .await;
    }

    for _ in 0..3 {
        app.post(
            &format!("/api/v1/documents/{theirs}/comments"),
            Some(&token),
            json!({ "body": "on the other one" }),
        )
        .await;
    }

    let listed = app
        .get(&format!("/api/v1/documents/{mine}/comments"), Some(&token))
        .await;

    let page = listed.body["data"].as_array().expect("a page").len();

    assert_eq!(page, 2, "the page is scoped to this document");
    assert_eq!(
        listed.body["meta"]["total"], 2,
        "and so is the count: a total the page cannot account for is unreadable"
    );
}

// ---------------------------------------------------------------------------
// AC4 — the body's own refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_comment_of_whitespace_is_refused_and_names_the_field() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_EMPTY").await;
    let document = draft(&app, &token, type_id).await;

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "   \n  " }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["path"], "body");
    assert_eq!(refused.body["error"]["details"][0]["code"], "COMMENT_EMPTY");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

#[tokio::test]
async fn a_comment_over_the_bound_is_refused_before_the_document_is_read() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;

    // A document that does not exist: if the bound were checked *after* the
    // read, this would answer 404 instead — which is the ordering the service
    // documents and this is what holds it to it.
    let absent = Uuid::now_v7();
    let refused = app
        .post(
            &format!("/api/v1/documents/{absent}/comments"),
            Some(&token),
            json!({ "body": "x".repeat(4001) }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(refused.body["error"]["details"][0]["code"], "TOO_LONG");
}

// ---------------------------------------------------------------------------
// Who may comment, and on what
// ---------------------------------------------------------------------------

#[tokio::test]
async fn commenting_on_a_document_that_does_not_exist_answers_404_and_stores_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let absent = Uuid::now_v7();

    let refused = app
        .post(
            &format!("/api/v1/documents/{absent}/comments"),
            Some(&token),
            json!({ "body": "about a document that is not there" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(rows, 0);
}

/// **The two permissions are isolated from each other** (coding standard §2.9).
///
/// A caller holding neither would be refused by whichever check ran first, so
/// removing either would leave such a test green — the gate that let five
/// predicates through in Sprint 8, and the one
/// [#244](https://github.com/sujanto-gaws/kelir/issues/244) hit again one module
/// over. This caller holds `document:read` and **not** `comment:create`.
#[tokio::test]
async fn reading_a_document_is_not_permission_to_comment_on_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_ISOLATE").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-CMT-READER",
        &["document:read"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "cmt-reader",
        "cmt-reader@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let reader = app.sign_in("cmt-reader", common::ADMIN_PASSWORD).await;

    let readable = app
        .get(&format!("/api/v1/documents/{document}"), Some(&reader))
        .await;
    assert_eq!(readable.status, StatusCode::OK, "{}", readable.body);

    let refused = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&reader),
            json!({ "body": "may I?" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// And the read side of the same isolation: `comment:create` is not
/// `comment:read`.
#[tokio::test]
async fn commenting_is_not_permission_to_read_the_conversation() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_WRITEONLY").await;
    let document = draft(&app, &token, type_id).await;

    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "ROLE-CMT-WRITER",
        &["document:read", "comment:create"],
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "cmt-writer",
        "cmt-writer@example.test",
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    let writer = app.sign_in("cmt-writer", common::ADMIN_PASSWORD).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&writer),
            json!({ "body": "left for somebody else to read" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    let refused = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&writer),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// The distinction the module documentation exists to state (#249 AC3)
// ---------------------------------------------------------------------------

/// **A comment is not a decision comment**, asserted over the rows rather than
/// over the prose.
///
/// #249 AC3 asks for the distinction in the module documentation; this is what
/// makes it checkable. Commenting on a document writes `comments` and touches
/// none of the three columns FR-TASK-006 writes — so a reader who assumes one is
/// the other has a failing test to read rather than a paragraph to believe.
#[tokio::test]
async fn commenting_writes_no_decision_record() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_DISTINCT").await;
    let document = draft(&app, &token, type_id).await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
            json!({ "body": "a conversation, not a decision" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    for table in ["approval_decisions", "workflow_history", "workflow_tasks"] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&app.pool)
            .await
            .expect("a count");

        assert_eq!(rows, 0, "commenting wrote a row in {table}");
    }

    let comments: i64 = sqlx::query_scalar("SELECT count(*) FROM comments")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(comments, 1);
}

// ---------------------------------------------------------------------------
// #253 AC1 — a reply hangs from a comment, and only from a comment (D-50)
// ---------------------------------------------------------------------------

/// A second account that may hold the comment permissions this file names.
///
/// The role is created per test with the codes the test needs, which is the
/// isolation rule (coding standard §2.9) applied to four permissions instead of
/// two: a caller granted the whole module cannot show that `comment:update` and
/// `comment:delete` are separate locks.
async fn user_with(app: &TestApp, name: &str, permissions: &[&str]) -> String {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", name.to_uppercase()),
        permissions,
    )
    .await;

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        name,
        &format!("{name}@example.test"),
        common::ADMIN_PASSWORD,
        &[role],
    )
    .await;

    app.sign_in(name, common::ADMIN_PASSWORD).await
}

async fn comment(app: &TestApp, token: &str, document: Uuid, body: &str) -> Uuid {
    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(token),
            json!({ "body": body }),
        )
        .await;

    assert_eq!(added.status, StatusCode::OK, "{}", added.body);

    id_of(&added.body["data"])
}

async fn reply(
    app: &TestApp,
    token: &str,
    document: Uuid,
    parent: Uuid,
    body: &str,
) -> TestResponse {
    app.post(
        &format!("/api/v1/documents/{document}/comments"),
        Some(token),
        json!({ "body": body, "parentCommentId": parent }),
    )
    .await
}

/// **The thread's order, which is not the order things were said.**
///
/// A reply reads under the comment it answers even when a later root was said
/// in between — that is what makes a one-level thread readable top to bottom,
/// and it is the half of the ordering a `created_at` sort would get wrong.
#[tokio::test]
async fn a_reply_reads_under_the_comment_it_answers_and_not_where_it_was_said() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_THREAD").await;
    let document = draft(&app, &token, type_id).await;

    let first = comment(&app, &token, document, "is this the right supplier?").await;
    comment(&app, &token, document, "unrelated, about the budget").await;

    let answered = reply(&app, &token, document, first, "yes, they are approved").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert_eq!(answered.body["data"]["parentCommentId"], first.to_string());

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    let bodies: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("a page")
        .iter()
        .map(|comment| comment["body"].as_str().expect("a body"))
        .collect();

    assert_eq!(
        bodies,
        vec![
            "is this the right supplier?",
            "yes, they are approved",
            "unrelated, about the budget",
        ],
        "a reply belongs to its thread, not to the minute it was written"
    );
}

/// **D-50, at the only place that can hold it.**
///
/// `ck_comments_not_its_own_parent` sees one row and this is the hop it cannot
/// see: the parent's own parent. Refused in the service, in a 422 that names the
/// field the caller has to change.
#[tokio::test]
async fn a_reply_to_a_reply_is_refused_and_the_conversation_stays_one_level() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_DEPTH").await;
    let document = draft(&app, &token, type_id).await;

    let root = comment(&app, &token, document, "the root").await;
    let answered = reply(&app, &token, document, root, "the reply").await;
    let first_reply = id_of(&answered.body["data"]);

    let refused = reply(&app, &token, document, first_reply, "a reply to the reply").await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["path"],
        "parentCommentId"
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "REPLY_TO_REPLY"
    );

    let depth: i64 =
        sqlx::query_scalar("SELECT count(*) FROM comments WHERE parent_comment_id IS NOT NULL")
            .fetch_one(&app.pool)
            .await
            .expect("a count");

    assert_eq!(depth, 1, "the refused reply was not stored");
}

/// A comment on **another** document is not a parent this document has, and the
/// refusal says the same thing as no comment at all — `get_document`'s 404 rule,
/// applied to the body of a request.
#[tokio::test]
async fn a_reply_cannot_reach_across_documents() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_CROSS").await;
    let mine = draft(&app, &token, type_id).await;
    let theirs = draft(&app, &token, type_id).await;

    let elsewhere = comment(&app, &token, theirs, "said on the other document").await;

    let refused = reply(&app, &token, mine, elsewhere, "answering across a boundary").await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );
    assert_eq!(
        refused.body["error"]["details"][0]["code"],
        "PARENT_NOT_FOUND"
    );
}

// ---------------------------------------------------------------------------
// #253 AC2 and AC3 — editing is the author's, and an edit is visible as one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_author_edits_their_comment_and_the_edit_is_visible_as_an_edit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_EDIT").await;
    let document = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, document, "is this the right supplier?").await;

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;
    assert!(
        listed.body["data"][0]["editedAt"].is_null(),
        "a comment nobody has edited says nothing about being edited"
    );

    let edited = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&token),
            json!({ "body": "  is this still the right supplier?  " }),
        )
        .await;

    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(
        edited.body["data"]["body"], "is this still the right supplier?",
        "an edited body is trimmed on the way in, as the first one was"
    );
    assert!(
        !edited.body["data"]["editedAt"].is_null(),
        "#253 AC3: a comment whose text changed with nothing saying so is a \
         conversation somebody can rewrite after the fact"
    );
    assert_eq!(
        edited.body["data"]["createdAt"], listed.body["data"][0]["createdAt"],
        "an edit is not a new comment"
    );
}

/// **The permission and the authorship are two questions, and both are asked.**
///
/// This caller holds `comment:update` — the whole permission — and did not write
/// the comment. Nothing in this release lets one account edit another's, which
/// is why `0036_comment_thread.sql` grants no moderator code.
#[tokio::test]
async fn a_comment_is_not_somebody_elses_to_edit() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_NOTYOURS").await;
    let document = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, document, "the administrator's words").await;

    let other = user_with(
        &app,
        "cmt-editor",
        &["document:read", "comment:read", "comment:update"],
    )
    .await;

    let refused = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&other),
            json!({ "body": "somebody else's words, rewritten" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    let body: String = sqlx::query_scalar("SELECT body FROM comments WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the row");

    assert_eq!(body, "the administrator's words");
}

/// And the permission half of the same pair: the author of a comment still needs
/// `comment:update` to edit it.
#[tokio::test]
async fn writing_a_comment_is_not_permission_to_edit_it() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_NOEDIT").await;
    let document = draft(&app, &token, type_id).await;

    let author = user_with(
        &app,
        "cmt-author",
        &["document:read", "comment:read", "comment:create"],
    )
    .await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&author),
            json!({ "body": "mine, and not mine to change" }),
        )
        .await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    let id = id_of(&added.body["data"]);

    let refused = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&author),
            json!({ "body": "changed" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
}

/// **An edit cannot re-parent a comment**, and the type is what refuses it:
/// `EditCommentRequest` carries a body and `deny_unknown_fields` turns the
/// attempt into a 422 naming the field rather than a silent ignore.
#[tokio::test]
async fn an_edit_cannot_move_a_comment_into_another_thread() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_REPARENT").await;
    let document = draft(&app, &token, type_id).await;

    let first = comment(&app, &token, document, "one thread").await;
    let second = comment(&app, &token, document, "another thread").await;

    let refused = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{second}"),
            Some(&token),
            json!({ "body": "another thread", "parentCommentId": first }),
        )
        .await;

    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let parent: Option<Uuid> =
        sqlx::query_scalar("SELECT parent_comment_id FROM comments WHERE id = $1")
            .bind(second)
            .fetch_one(&app.pool)
            .await
            .expect("the row");

    assert!(parent.is_none(), "the comment stayed where it was");
}

// ---------------------------------------------------------------------------
// #253 AC4 — the delete is soft, and the thread's shape survives it (D-51)
// ---------------------------------------------------------------------------

/// **The decision AC2 asks for, asserted rather than described.**
///
/// A deleted comment with replies stays in the conversation as a tombstone —
/// author and time, no body — and the replies are untouched. They are other
/// people's words, and a delete that took them would let one person end a
/// conversation they only started.
#[tokio::test]
async fn deleting_a_comment_that_has_replies_leaves_a_tombstone_and_keeps_them() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_TOMB").await;
    let document = draft(&app, &token, type_id).await;

    let root = comment(&app, &token, document, "a question I regret asking").await;
    let answered = reply(&app, &token, document, root, "an answer somebody wrote").await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);

    let deleted = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{root}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    let page = listed.body["data"].as_array().expect("a page");

    assert_eq!(page.len(), 2, "the thread kept its shape");
    assert_eq!(page[0]["id"], root.to_string());
    assert!(page[0]["body"].is_null(), "a tombstone has no body");
    assert!(
        !page[0]["deletedAt"].is_null(),
        "and says why it has none, rather than reading as a comment of nothing"
    );
    assert_eq!(
        page[1]["body"], "an answer somebody wrote",
        "the reply is somebody else's and survives"
    );
    assert_eq!(
        listed.body["meta"]["total"], 2,
        "the count and the page apply the same rule (#279's lesson)"
    );
}

/// The other half of D-51: with nothing hanging from it, a deleted comment holds
/// no shape and leaves the conversation altogether — while the row, and its
/// text, stay.
#[tokio::test]
async fn deleting_a_comment_nobody_answered_takes_it_out_of_the_conversation() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_SOFT").await;
    let document = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, document, "said and taken back").await;

    let deleted = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    let listed = app
        .get(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&token),
        )
        .await;

    assert_eq!(listed.body["data"].as_array().expect("a page").len(), 0);
    assert_eq!(listed.body["meta"]["total"], 0);

    // **Soft, and the row keeps its text** — which is what makes the audit
    // trail's length meaningful and what an undo would need.
    let row: (Option<String>, bool) =
        sqlx::query_as("SELECT body, deleted_at IS NOT NULL FROM comments WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the row");

    assert_eq!(row.0.as_deref(), Some("said and taken back"));
    assert!(row.1, "the delete was soft and the row is marked");
}

#[tokio::test]
async fn a_comment_is_not_somebody_elses_to_delete_and_deleting_is_its_own_permission() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_DELPERM").await;
    let document = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, document, "the administrator's words").await;

    // Holds the delete permission over comments, and did not write this one.
    let other = user_with(
        &app,
        "cmt-remover",
        &["document:read", "comment:read", "comment:delete"],
    )
    .await;

    let refused = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&other),
        )
        .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    // Wrote this one, and holds every comment permission except the delete.
    let author = user_with(
        &app,
        "cmt-keeper",
        &[
            "document:read",
            "comment:read",
            "comment:create",
            "comment:update",
        ],
    )
    .await;

    let added = app
        .post(
            &format!("/api/v1/documents/{document}/comments"),
            Some(&author),
            json!({ "body": "mine, and not mine to remove" }),
        )
        .await;
    let theirs = id_of(&added.body["data"]);

    let also_refused = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{theirs}"),
            Some(&author),
        )
        .await;
    assert_eq!(
        also_refused.status,
        StatusCode::FORBIDDEN,
        "{}",
        also_refused.body
    );

    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM comments WHERE deleted_at IS NULL")
        .fetch_one(&app.pool)
        .await
        .expect("a count");

    assert_eq!(live, 2, "neither refusal deleted anything");
}

/// A deleted comment is not an editable one, and the surface no longer admits
/// it: the tombstone is a thing the list serves, not a row a write can reach.
#[tokio::test]
async fn a_deleted_comment_cannot_be_edited() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_GONE").await;
    let document = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, document, "briefly said").await;

    app.delete(
        &format!("/api/v1/documents/{document}/comments/{id}"),
        Some(&token),
    )
    .await;

    let refused = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&token),
            json!({ "body": "said again" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);

    let deleted_twice = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{id}"),
            Some(&token),
        )
        .await;

    assert_eq!(
        deleted_twice.status,
        StatusCode::NOT_FOUND,
        "{}",
        deleted_twice.body
    );
}

/// A comment reached through the wrong document is not this caller's comment,
/// and the path is what says which conversation is meant.
#[tokio::test]
async fn a_comment_cannot_be_edited_through_another_documents_path() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_WRONGDOC").await;
    let mine = draft(&app, &token, type_id).await;
    let theirs = draft(&app, &token, type_id).await;

    let id = comment(&app, &token, mine, "said on one document").await;

    let refused = app
        .put(
            &format!("/api/v1/documents/{theirs}/comments/{id}"),
            Some(&token),
            json!({ "body": "edited through the other" }),
        )
        .await;

    assert_eq!(refused.status, StatusCode::NOT_FOUND, "{}", refused.body);
}

// ---------------------------------------------------------------------------
// #253 AC5 — each of the three writes an activity event, in the same
// transaction, and AC6 — none of them is a decision record
// ---------------------------------------------------------------------------

/// **The three events, and what they refuse to carry.**
///
/// #248's rule is that a thing that happened to a document lands on its
/// timeline; **D-45** is that the entry says it happened and links, and nothing
/// more. An edit is where carrying the text would be most tempting — it would
/// put a copy of the old words where deleting the comment cannot reach them.
#[tokio::test]
async fn replying_editing_and_deleting_each_land_on_the_timeline_and_say_nothing() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_TIMELINE").await;
    let document = draft(&app, &token, type_id).await;

    let root = comment(&app, &token, document, "the root").await;
    let answered = reply(&app, &token, document, root, "the reply").await;
    let child = id_of(&answered.body["data"]);

    let edited = app
        .put(
            &format!("/api/v1/documents/{document}/comments/{child}"),
            Some(&token),
            json!({ "body": "the reply, reconsidered" }),
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);

    let deleted = app
        .delete(
            &format!("/api/v1/documents/{document}/comments/{child}"),
            Some(&token),
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);

    for event_type in [
        "Comment.Added",
        "Comment.Replied",
        "Comment.Edited",
        "Comment.Deleted",
    ] {
        let (rows, linked, empty): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*), \
                    count(*) FILTER (WHERE comment_id IS NOT NULL), \
                    count(*) FILTER (WHERE details_json = '{}'::jsonb) \
             FROM activity_events WHERE event_type = $1 AND document_id = $2",
        )
        .bind(event_type)
        .bind(document)
        .fetch_one(&app.pool)
        .await
        .expect("a count");

        assert_eq!(rows, 1, "{event_type} did not write exactly one event");
        assert_eq!(
            linked, 1,
            "{event_type} did not link the comment it is about"
        );
        assert_eq!(empty, 1, "{event_type} carried detail D-45 does not permit");
    }
}

/// **Still not the decision comment** (#253 AC6), asserted over the rows the way
/// #249 asserted it for the add: the tail writes and rewrites `comments` and
/// touches none of the three columns FR-TASK-006 writes, whose whole point is
/// that a decision's reason cannot be edited afterwards.
#[tokio::test]
async fn replying_editing_and_deleting_write_no_decision_record() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let type_id = document_type(&app, &token, "PR_CMT_STILLNOT").await;
    let document = draft(&app, &token, type_id).await;

    let root = comment(&app, &token, document, "a conversation, not a decision").await;
    let answered = reply(&app, &token, document, root, "still not a decision").await;
    let child = id_of(&answered.body["data"]);

    app.put(
        &format!("/api/v1/documents/{document}/comments/{child}"),
        Some(&token),
        json!({ "body": "edited, which a decision's reason may never be" }),
    )
    .await;

    app.delete(
        &format!("/api/v1/documents/{document}/comments/{child}"),
        Some(&token),
    )
    .await;

    for table in ["approval_decisions", "workflow_history", "workflow_tasks"] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&app.pool)
            .await
            .expect("a count");

        assert_eq!(rows, 0, "the comment tail wrote a row in {table}");
    }
}
