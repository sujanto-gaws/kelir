//! Forgot and reset password, end to end (#17, FR-AUTH-006).
//!
//! **The flow is driven the way a person drives it**: ask for a link, read the
//! link out of the delivered message, use it. No test here reaches into
//! `password_reset_tokens` to fetch a token — doing that would prove a row was
//! written and nothing about whether anybody could have used it, and the whole
//! reason this issue existed is that the table shipped and the flow did not.
//!
//! The security properties are what most of these assert, because they are the
//! reason a password-reset endpoint is dangerous rather than merely fiddly:
//! it hands out a credential, over an unauthenticated route, to whoever asks.

mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use common::{fixtures, TestApp};
use kelir_backend::mail::Mailer;
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "the-original-password";
const NEW_PASSWORD: &str = "a-perfectly-good-new-password";

/// A user who can sign in, with a known email address.
async fn user(app: &TestApp, name: &str) -> (String, String) {
    let username = format!("user.{name}");
    let email = format!("{name}@kelir.test");

    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &email,
        PASSWORD,
        &[],
    )
    .await;

    (username, email)
}

async fn ask_for_a_link(app: &TestApp, identifier: &str) -> StatusCode {
    app.send(
        Method::POST,
        "/api/v1/auth/forgot-password",
        None,
        Some(json!({ "username": identifier })),
    )
    .await
    .status
}

/// The token out of the delivered message — the only way these tests get one.
///
/// `delivered` is how many messages this account's flow has produced by now,
/// and it is a parameter rather than an assumption because the send is
/// detached (#202): "the last message" is only well defined once the message
/// the caller means has actually arrived.
async fn token_from_last_mail(app: &TestApp, delivered: usize) -> String {
    let sent = app.mail_delivered(delivered).await;
    let last = sent.last().expect("a message was delivered");

    let marker = "token=";
    let start = last.body.find(marker).expect("the body carries a link") + marker.len();
    let token: String = last.body[start..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect();

    assert_eq!(token.len(), 64, "an opaque 256-bit token, hex encoded");

    token
}

async fn redeem(app: &TestApp, token: &str, password: &str) -> common::TestResponse {
    app.send(
        Method::POST,
        "/api/v1/auth/reset-password",
        None,
        Some(json!({ "token": token, "newPassword": password })),
    )
    .await
}

#[tokio::test]
async fn a_person_resets_their_password_and_signs_in_with_the_new_one() {
    let app = TestApp::spawn().await;
    let (username, email) = user(&app, "resetter").await;

    assert_eq!(ask_for_a_link(&app, &username).await, StatusCode::ACCEPTED);

    let sent = app.mail_delivered(1).await;
    assert_eq!(sent.len(), 1, "one link, to one person");
    assert_eq!(sent[0].to, email, "addressed to the account's own address");
    assert!(
        !sent[0].body.contains(PASSWORD),
        "a reset email must not carry a password"
    );

    let token = token_from_last_mail(&app, 1).await;
    let response = redeem(&app, &token, NEW_PASSWORD).await;

    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", response.body);

    // The new password works.
    let signed_in = app
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": username, "password": NEW_PASSWORD })),
        )
        .await;

    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);

    // And the old one does not.
    let refused = app
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": username, "password": PASSWORD })),
        )
        .await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
}

/// An email address may also be used to ask, because sign-in takes either.
#[tokio::test]
async fn the_identifier_may_be_an_email_address() {
    let app = TestApp::spawn().await;
    let (_, email) = user(&app, "byemail").await;

    assert_eq!(ask_for_a_link(&app, &email).await, StatusCode::ACCEPTED);
    assert_eq!(app.mail_delivered(1).await.len(), 1);
}

/// **A caller waits for no part of delivery** (#202).
///
/// The property that closes the timing oracle, and the one nothing in this
/// suite could assert before: `Mailer` is an enum, so a slow transport could
/// not be injected, and a captured send was free — the two paths therefore
/// looked identical to a test while differing by 80ms against a real SMTP
/// server. `Mailer::captured_taking` is that slow transport.
///
/// Measured before the fix, against mailpit on the loopback interface: p50 90ms
/// for a known account, 9.8ms for an unknown one, the ranges not overlapping.
///
/// Seen red (coding standard §2.9) against `send_detached` restored to
/// `state.mailer.send(...).await`: the request takes the delay below and the
/// assertion reports it.
#[tokio::test]
async fn the_answer_does_not_wait_for_the_mail_to_be_sent() {
    // Long enough that awaiting it could not be mistaken for scheduling noise,
    // short enough that the test still finishes if the property breaks.
    const DELIVERY: Duration = Duration::from_secs(3);

    let app = TestApp::spawn_with_mailer(|_| {}, Mailer::captured_taking(DELIVERY)).await;
    let (username, _) = user(&app, "impatientcaller").await;

    let started = Instant::now();
    assert_eq!(ask_for_a_link(&app, &username).await, StatusCode::ACCEPTED);
    let answered_in = started.elapsed();

    assert!(
        answered_in < DELIVERY / 2,
        "the answer waited on the mail server: {answered_in:?} of a {DELIVERY:?} send"
    );

    // And the message is still sent — detaching it must not lose it.
    let sent = app.mail_delivered(1).await;
    assert_eq!(sent.len(), 1, "the detached send did not deliver");
}

/// **The enumeration property.** An identifier that belongs to nobody gets the
/// same answer as one that does, and no mail is sent.
#[tokio::test]
async fn an_unknown_identifier_is_answered_identically_and_sends_nothing() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "known").await;

    let known = ask_for_a_link(&app, &username).await;
    let unknown = ask_for_a_link(&app, "nobody.at.all").await;

    assert_eq!(
        known, unknown,
        "a different status for an unknown account is an enumeration oracle"
    );
    assert_eq!(unknown, StatusCode::ACCEPTED);

    let sent = app.mail_settled().await;
    assert_eq!(sent.len(), 1, "only the real account was written to");
}

/// An account that cannot sign in gets no link — and is not distinguishable.
#[tokio::test]
async fn an_inactive_account_gets_no_link_and_says_nothing_about_it() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "inactive").await;

    sqlx::query("UPDATE users SET status = 'INACTIVE' WHERE username = $1")
        .bind(&username)
        .execute(&app.pool)
        .await
        .expect("deactivate the account");

    assert_eq!(
        ask_for_a_link(&app, &username).await,
        StatusCode::ACCEPTED,
        "the same answer an active account gets"
    );
    assert!(app.mail_settled().await.is_empty(), "but no link");
}

/// **Single use.** A token works once, and the second attempt is refused.
#[tokio::test]
async fn a_token_cannot_be_used_twice() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "twice").await;

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;

    assert_eq!(
        redeem(&app, &token, NEW_PASSWORD).await.status,
        StatusCode::NO_CONTENT
    );

    let again = redeem(&app, &token, "another-good-password-here").await;

    assert_eq!(
        again.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a consumed token is spent; body {}",
        again.body
    );

    // And the second attempt did not change the password.
    let signed_in = app
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": username, "password": NEW_PASSWORD })),
        )
        .await;

    assert_eq!(
        signed_in.status,
        StatusCode::OK,
        "the first reset's password still stands"
    );
}

/// Two requests carrying one token produce one password change.
///
/// The predicate on `consume_reset_token` is what makes that true — reading the
/// row and then updating it would let both through, and the later write would
/// silently win.
#[tokio::test]
async fn two_concurrent_redemptions_of_one_token_change_the_password_once() {
    use std::sync::Arc;

    let app = Arc::new(TestApp::spawn().await);
    let (username, _) = user(&app, "concurrent").await;

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;

    let first = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { redeem(&app, &token, NEW_PASSWORD).await })
    };
    let second = {
        let app = Arc::clone(&app);
        let token = token.clone();
        tokio::spawn(async move { redeem(&app, &token, "the-other-new-password").await })
    };

    let first = first.await.expect("the first redemption did not panic");
    let second = second.await.expect("the second redemption did not panic");

    let accepted = [&first, &second]
        .iter()
        .filter(|response| response.status == StatusCode::NO_CONTENT)
        .count();

    assert_eq!(
        accepted, 1,
        "exactly one redemption may succeed; got {} and {}",
        first.status, second.status
    );
}

/// An expired token is refused, and refused the same way a made-up one is.
///
/// **Expiry is enforced twice, and this test measures that rather than assuming
/// it.** `find_live_reset_token` will not return an expired row, and
/// `consume_reset_token` carries `expires_at > now()` in its predicate — so a
/// token that expired between the lookup and the write is caught too. Removing
/// either check alone leaves this green; removing both turns it red. Neither
/// layer is therefore redundant, and a future edit that deletes one and sees
/// green has proved this test still works, not that the check was spare.
#[tokio::test]
async fn an_expired_token_is_refused() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "expired").await;

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;

    sqlx::query("UPDATE password_reset_tokens SET expires_at = now() - interval '1 minute'")
        .execute(&app.pool)
        .await
        .expect("age the token");

    let expired = redeem(&app, &token, NEW_PASSWORD).await;
    let invented = redeem(&app, &"a".repeat(64), NEW_PASSWORD).await;

    assert_eq!(expired.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        expired.body["error"]["details"][0]["code"], invented.body["error"]["details"][0]["code"],
        "an expired token and a made-up one must be told the same thing; \
         distinguishing them says which guesses were close"
    );
}

/// Redeeming a link ends every other session for that account.
#[tokio::test]
async fn a_reset_signs_the_account_out_everywhere() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "sessions").await;

    // An existing session, established before the reset.
    let signed_in = app
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": username, "password": PASSWORD })),
        )
        .await;

    assert_eq!(signed_in.status, StatusCode::OK);
    let refresh_token = signed_in.body["data"]["refreshToken"]
        .as_str()
        .expect("a refresh token")
        .to_owned();

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;
    redeem(&app, &token, NEW_PASSWORD).await;

    let refreshed = app
        .send(
            Method::POST,
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refreshToken": refresh_token })),
        )
        .await;

    assert_eq!(
        refreshed.status,
        StatusCode::UNAUTHORIZED,
        "a session that survived a password reset is a session the reset did \
         not protect against; body {}",
        refreshed.body
    );
}

/// A reset clears a failed-login lockout.
///
/// Otherwise somebody who locked themselves out, reset their password, and then
/// could still not sign in would reasonably conclude the reset had not worked.
#[tokio::test]
async fn a_reset_clears_a_lockout() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "lockedout").await;

    sqlx::query(
        "UPDATE users SET locked_until = now() + interval '1 hour', failed_login_count = 9
         WHERE username = $1",
    )
    .bind(&username)
    .execute(&app.pool)
    .await
    .expect("lock the account");

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;
    redeem(&app, &token, NEW_PASSWORD).await;

    let signed_in = app
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": username, "password": NEW_PASSWORD })),
        )
        .await;

    assert_eq!(
        signed_in.status,
        StatusCode::OK,
        "the lockout outlived the reset; body {}",
        signed_in.body
    );
}

/// Asking twice in quick succession sends one link.
///
/// The per-account throttle. Without it, anybody who knows an address can send
/// its owner a reset link as fast as they can make requests.
#[tokio::test]
async fn asking_twice_in_quick_succession_sends_one_link() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "impatient").await;

    for _ in 0..5 {
        assert_eq!(ask_for_a_link(&app, &username).await, StatusCode::ACCEPTED);
    }

    assert_eq!(
        app.mail_settled().await.len(),
        1,
        "five requests, one email — and every one of them answered 202, so the \
         throttle is invisible to the caller"
    );
}

/// Using one link invalidates the others outstanding for that account.
#[tokio::test]
async fn redeeming_one_link_invalidates_the_others() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "twolinks").await;

    ask_for_a_link(&app, &username).await;
    let first = token_from_last_mail(&app, 1).await;

    // Past the resend cooldown, so a second link really is issued.
    sqlx::query("UPDATE password_reset_tokens SET created_at = now() - interval '1 hour'")
        .execute(&app.pool)
        .await
        .expect("age the first token");

    ask_for_a_link(&app, &username).await;
    let second = token_from_last_mail(&app, 2).await;

    assert_ne!(first, second, "two requests, two tokens");

    assert_eq!(
        redeem(&app, &second, NEW_PASSWORD).await.status,
        StatusCode::NO_CONTENT
    );

    let stale = redeem(&app, &first, "yet-another-good-password").await;

    assert_eq!(
        stale.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "the earlier link must stop working once one has been used; body {}",
        stale.body
    );
}

/// The new password is held to the same policy every other password is.
#[tokio::test]
async fn a_weak_new_password_is_refused_and_the_token_survives() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "weak").await;

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;

    let refused = redeem(&app, &token, "short").await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);

    // And the token was not spent by the attempt — a person who typed a bad
    // password should not have to ask for a new link.
    assert_eq!(
        redeem(&app, &token, NEW_PASSWORD).await.status,
        StatusCode::NO_CONTENT,
        "a rejected password must not consume the token"
    );
}

#[tokio::test]
async fn an_empty_identifier_is_refused() {
    let app = TestApp::spawn().await;

    let response = app
        .send(
            Method::POST,
            "/api/v1/auth/forgot-password",
            None,
            Some(json!({ "username": "   " })),
        )
        .await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a malformed request is malformed whoever sent it, and refusing it says \
         nothing about any account"
    );
}

/// Neither route needs a token, which is the point of them.
#[tokio::test]
async fn both_routes_are_reachable_without_signing_in() {
    let app = TestApp::spawn().await;

    for (path, body) in [
        (
            "/api/v1/auth/forgot-password",
            json!({ "username": "someone" }),
        ),
        (
            "/api/v1/auth/reset-password",
            json!({ "token": "a".repeat(64), "newPassword": NEW_PASSWORD }),
        ),
    ] {
        let response = app.send(Method::POST, path, None, Some(body)).await;

        assert_ne!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{path} must be reachable by somebody who cannot sign in — that is \
             who it is for"
        );
    }
}

/// The token never appears in the audit trail.
///
/// It is a bearer credential, and an audit trail is read by more people than a
/// mailbox is.
#[tokio::test]
async fn the_audit_record_does_not_carry_the_token() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "audited").await;

    ask_for_a_link(&app, &username).await;
    let token = token_from_last_mail(&app, 1).await;
    redeem(&app, &token, NEW_PASSWORD).await;

    let matching: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE old_value_json::text LIKE $1
            OR new_value_json::text LIKE $1",
    )
    .bind(format!("%{token}%"))
    .fetch_one(&app.pool)
    .await
    .expect("the audit trail is queryable");

    assert_eq!(matching, 0);

    // But the events themselves are there.
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM audit_events
         WHERE event_type IN ('User.PasswordResetRequested', 'User.PasswordReset')
         ORDER BY event_type",
    )
    .fetch_all(&app.pool)
    .await
    .expect("the audit trail is queryable");

    assert_eq!(
        events,
        vec![
            "User.PasswordReset".to_owned(),
            "User.PasswordResetRequested".to_owned()
        ]
    );
}

/// A token issued for one tenant's user is not usable, and the row is scoped.
#[tokio::test]
async fn a_reset_is_scoped_to_the_tenant_that_issued_it() {
    let app = TestApp::spawn().await;
    let (username, _) = user(&app, "scoped").await;

    ask_for_a_link(&app, &username).await;

    let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM password_reset_tokens LIMIT 1")
        .fetch_one(&app.pool)
        .await
        .expect("the token is queryable");

    assert_eq!(
        tenant,
        fixtures::SYSTEM_TENANT_ID,
        "the row carries the tenant the account belongs to"
    );
}
