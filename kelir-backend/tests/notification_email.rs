//! The email channel and its templates (FR-NTF-004; [#257]).
//!
//! # What these tests drive
//!
//! `notification::worker::pass` — the loop's work with the sleeping taken out,
//! which is `attachment::worker::pass`'s seam and exists for the same reason.
//! The mailer is `Mailer::Captured`, so what would have been sent is a value a
//! test can read rather than a message somebody has to go and look for.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Four mutations, run 2026-09-02:
//!
//! | Mutation | Reddened |
//! |---|---|
//! | The channel row not read — the sender assuming email | *a disabled channel sends nothing and loses nothing*; *a failed delivery is recorded and the notification is not lost* |
//! | `mark_delivered` writing over any status rather than only `PENDING` | *a delivery already recorded is not overwritten by a second worker* |
//! | The plain-notification fallback removed, so a bad template sends nothing | *a template that cannot render sends the notification plain* |
//! | The failure path marking the notification `SENT` | *a failed delivery is recorded and the notification is not lost*; *a deactivated recipient is not emailed and the attempt says why* |
//!
//! The second one is why this file has a test that calls `mark_delivered`
//! directly. The mutation first went green: `pending_deliveries` filters on
//! `PENDING`, so no *pass* re-reads a delivered row and the predicate looked
//! covered when nothing was covering it. The race it actually guards is two
//! workers sharing one batch, and only a caller arriving like the second one
//! reddens it.
//!
//! [#257]: https://github.com/sujanto-gaws/kelir/issues/257

mod common;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;

use common::{fixtures, TestApp};

/// The system tenant's seeded email channel, from `0039`.
const EMAIL_CHANNEL: &str = "00000000-0000-0000-0004-000000000001";

/// A person with an address, and the notification centre's permission.
async fn recipient(app: &TestApp, username: &str) -> Uuid {
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-{}", username.to_uppercase()),
        &["notification:read"],
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
    .await
}

/// A notification of the shape `service::notify` writes, without a workflow in
/// front of it: this file is about delivery, and driving an approval to reach
/// one row would test the workflow twice and the sender once.
async fn pending_notification(
    app: &TestApp,
    recipient: Uuid,
    notification_type: &str,
    title: &str,
    body: &str,
) -> Uuid {
    let id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO notifications
            (id, tenant_id, recipient_user_id, notification_type, title, body, channel, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'IN_APP', 'PENDING')",
    )
    .bind(id)
    .bind(fixtures::SYSTEM_TENANT_ID)
    .bind(recipient)
    .bind(notification_type)
    .bind(title)
    .bind(body)
    .execute(&app.pool)
    .await
    .expect("the notification");

    id
}

async fn status_of(app: &TestApp, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM notifications WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .expect("the notification")
}

async fn attempts(app: &TestApp, id: Uuid) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT channel, status, error_message FROM notification_logs
         WHERE notification_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(&app.pool)
    .await
    .expect("the attempts")
}

// ---------------------------------------------------------------------------
// AC1, AC4 — the channel is data, and the template says what it looks like
// ---------------------------------------------------------------------------

/// **Which channels a notification uses is data, not a branch in the sender**
/// (AC1). Nothing in the worker names `TASK_ASSIGNED`; it reads the tenant's
/// enabled channels and looks for a template.
#[tokio::test]
async fn a_notification_is_delivered_on_the_channels_its_tenant_turned_on() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-one").await;
    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request on PR-2026-000001",
    )
    .await;

    app.deliver_notifications().await;

    let sent = app.sent_mail();

    assert_eq!(
        sent.len(),
        1,
        "one channel is enabled, so one email is sent"
    );
    assert_eq!(sent[0].to, "ntf-email-one@example.test");
    // AC4: the subject and the body come from the template, not from a literal
    // in the sender — `0039` seeds `{{title}}` and a body with a closing line.
    assert_eq!(sent[0].subject, "A task is waiting for you");
    assert!(
        sent[0]
            .body
            .contains("Approve the request on PR-2026-000001"),
        "{}",
        sent[0].body
    );
    assert!(
        sent[0].body.contains("Open Kelir"),
        "the template's own words are missing: {}",
        sent[0].body
    );

    assert_eq!(status_of(&app, id).await, "SENT");
    assert_eq!(
        attempts(&app, id).await,
        vec![("EMAIL".to_owned(), "SENT".to_owned(), None)]
    );
}

/// **A tenant that turns the channel off sends nothing**, and the notification
/// is still delivered — to the centre, which is where it always was.
#[tokio::test]
async fn a_disabled_channel_sends_nothing_and_loses_nothing() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-off").await;

    sqlx::query("UPDATE notification_channels SET is_enabled = false WHERE id = $1")
        .bind(Uuid::parse_str(EMAIL_CHANNEL).expect("a uuid"))
        .execute(&app.pool)
        .await
        .expect("the channel");

    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;

    assert!(app.sent_mail().is_empty(), "a disabled channel sent mail");
    assert_eq!(
        status_of(&app, id).await,
        "SENT",
        "an in-app notification is delivered by being written"
    );
    assert!(
        attempts(&app, id).await.is_empty(),
        "there was no attempt, so there is nothing to log"
    );
}

// ---------------------------------------------------------------------------
// AC5 — a template that cannot render sends the notification plain
// ---------------------------------------------------------------------------

/// **Silence is the failure this epic exists to end**, so a template naming a
/// placeholder the sender cannot resolve sends the notification's own words
/// rather than nothing — and does not put `{{dueDate}}` in somebody's subject
/// line either.
#[tokio::test]
async fn a_template_that_cannot_render_sends_the_notification_plain() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-broken").await;

    sqlx::query(
        "UPDATE notification_templates
         SET subject_template = '{{title}} — due {{dueDate}}'
         WHERE notification_type = 'TASK_ASSIGNED' AND channel = 'EMAIL'",
    )
    .execute(&app.pool)
    .await
    .expect("the template");

    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;

    let sent = app.sent_mail();

    assert_eq!(sent.len(), 1, "the notification was not sent at all");
    assert_eq!(
        sent[0].subject, "A task is waiting for you",
        "the fallback is the notification's own title"
    );
    assert!(
        !sent[0].subject.contains("{{"),
        "an unrendered placeholder reached a mailbox: {}",
        sent[0].subject
    );
    assert_eq!(status_of(&app, id).await, "SENT");
}

/// A type nobody has written an email for is delivered plain rather than
/// refused — the template table says what a channel *may* say, not which
/// notifications exist.
#[tokio::test]
async fn a_type_with_no_template_is_still_delivered() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-untemplated").await;
    let id = pending_notification(
        &app,
        person,
        "SOMETHING_LATER_RELEASES_ADD",
        "Something happened",
        "and here is what it was",
    )
    .await;

    app.deliver_notifications().await;

    let sent = app.sent_mail();

    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].subject, "Something happened");
    assert_eq!(status_of(&app, id).await, "SENT");
}

// ---------------------------------------------------------------------------
// AC2 — a failed send does not lose the notification
// ---------------------------------------------------------------------------

/// **The in-app record is the storage** (AC2).
///
/// A tenant that has turned on a channel this build cannot send — `SMS`, or a
/// plugin's — gets a failed attempt recorded against that channel, an email on
/// the one that works, and a notification still sitting in the centre exactly as
/// it was. **The delivery failed; the notification did not.**
#[tokio::test]
async fn a_failed_delivery_is_recorded_and_the_notification_is_not_lost() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-partial").await;

    sqlx::query(
        "INSERT INTO notification_channels
            (id, tenant_id, channel_code, channel_type, is_enabled)
         VALUES ($1, $2, 'SMS', 'SMS', true)",
    )
    .bind(Uuid::now_v7())
    .bind(fixtures::SYSTEM_TENANT_ID)
    .execute(&app.pool)
    .await
    .expect("the channel");

    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;

    assert_eq!(
        app.sent_mail().len(),
        1,
        "the channel that works still worked"
    );

    let logged = attempts(&app, id).await;

    assert_eq!(logged.len(), 2, "both attempts are recorded: {logged:?}");
    assert!(logged
        .iter()
        .any(|(channel, status, _)| channel == "EMAIL" && status == "SENT"));
    assert!(logged
        .iter()
        .any(|(channel, status, error)| channel == "SMS"
            && status == "FAILED"
            && error.as_deref().unwrap_or_default().contains("no sender")));

    assert_eq!(
        status_of(&app, id).await,
        "FAILED",
        "a notification one of whose channels failed is not fully delivered"
    );

    // And the notification itself is untouched: the centre still has it, unread.
    let (title, read): (String, bool) =
        sqlx::query_as("SELECT title, read_at IS NOT NULL FROM notifications WHERE id = $1")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("the notification");

    assert_eq!(title, "A task is waiting for you");
    assert!(!read);
}

/// **A recipient who has been deactivated is not emailed**, and the attempt says
/// why.
///
/// `users.email` is `NOT NULL`, so this is the only way a notification has
/// nowhere to go — the account was removed between the notification being
/// written and the pass that would have delivered it.
#[tokio::test]
async fn a_deactivated_recipient_is_not_emailed_and_the_attempt_says_why() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-gone").await;
    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    sqlx::query("UPDATE users SET deleted_at = now() WHERE id = $1")
        .bind(person)
        .execute(&app.pool)
        .await
        .expect("the user");

    app.deliver_notifications().await;

    assert!(app.sent_mail().is_empty(), "a removed account was emailed");
    assert_eq!(status_of(&app, id).await, "FAILED");

    let logged = attempts(&app, id).await;

    assert_eq!(logged.len(), 1);
    assert!(
        logged[0]
            .2
            .as_deref()
            .unwrap_or_default()
            .contains("no active account"),
        "the log says nothing about why: {logged:?}"
    );
}

/// **A second pass does not send the same notification twice**: a delivered
/// notification is no longer `PENDING`, so the next pass does not read it at
/// all. This is the loop running twice, which is what a poll loop does every few
/// seconds for the life of the process.
#[tokio::test]
async fn a_second_pass_does_not_send_the_same_notification_twice() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-once").await;
    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;
    app.deliver_notifications().await;

    assert_eq!(app.sent_mail().len(), 1, "the second pass sent it again");
    assert_eq!(attempts(&app, id).await.len(), 1);
    assert_eq!(status_of(&app, id).await, "SENT");
}

/// **Two workers holding one batch send one email twice and change one row
/// once**, because `mark_delivered` writes only over `PENDING`.
///
/// The pass above cannot show this: it re-reads `pending_deliveries`, which
/// filters on `PENDING` itself, so the second pass never sees the row. The race
/// this guards is two processes that read the *same* batch before either wrote —
/// so the second writer is called directly, as it would arrive, and finds
/// nothing of its own to move. It is `record_scan_result`'s guarantee one module
/// over, for the same reason.
#[tokio::test]
async fn a_delivery_already_recorded_is_not_overwritten_by_a_second_worker() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-race").await;
    let id = pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;
    assert_eq!(status_of(&app, id).await, "SENT");

    // The second worker, arriving with the batch it read before the first wrote.
    let moved = kelir_backend::modules::notification::repository::mark_delivered(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        id,
        "FAILED",
    )
    .await
    .expect("the write");

    assert_eq!(
        moved, 0,
        "the second worker moved a row that was not its own"
    );
    assert_eq!(
        status_of(&app, id).await,
        "SENT",
        "a delivered notification was marked failed by a duplicated pass"
    );
}

/// The centre is unaffected by any of this: a delivered notification is still
/// unread, and reading it is still the recipient's own action.
#[tokio::test]
async fn delivery_does_not_mark_a_notification_read() {
    let app = TestApp::spawn().await;
    let person = recipient(&app, "ntf-email-unread").await;
    pending_notification(
        &app,
        person,
        "TASK_ASSIGNED",
        "A task is waiting for you",
        "Approve the request",
    )
    .await;

    app.deliver_notifications().await;

    let token = app
        .sign_in("ntf-email-unread", common::ADMIN_PASSWORD)
        .await;
    let unread = app
        .get("/api/v1/notifications/unread-count", Some(&token))
        .await;

    assert_eq!(unread.status, StatusCode::OK, "{}", unread.body);
    assert_eq!(unread.body["data"]["unread"], json!(1));
}
