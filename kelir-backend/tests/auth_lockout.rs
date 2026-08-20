//! The failed-login lockout, end to end (NFR-SEC-008, issue #55).
//!
//! Sprint 4 shipped a lockout that never ended. Seven unit tests covered the
//! rate limiter and one covered the lockout — `assert_eq!(MAX_FAILED_LOGINS, 5)`
//! — and all eight were green while five wrong passwords permanently disabled an
//! account, because a constant compared against itself says nothing about what
//! the login path does with it. This file asserts the behaviour instead, through
//! real requests against a real database:
//!
//! * the fifth failure locks, the fourth does not;
//! * the lock lasts fifteen minutes and then ends on its own;
//! * the refusal belongs to the account, not to the caller's address — the
//!   distinction the rate limiter tests could not make, since both controls
//!   answer 401 to the same request;
//! * an administrator can end a lockout before it expires, which is the
//!   in-product recovery that #55 found missing.
//!
//! **The numbers are written out here rather than imported from
//! `MAX_FAILED_LOGINS` and `LOCKOUT_MINUTES`.** A test that reads the constants
//! passes whatever they are changed to; the requirement baselines five and
//! fifteen (SRS NFR-SEC-008, decision D-5), so five and fifteen are what these
//! assertions name.

mod common;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::http::{Method, StatusCode};
use chrono::{DateTime, Duration, Utc};
use common::{fixtures, TestApp};
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "the-correct-account-password";
const WRONG_PASSWORD: &str = "not-the-correct-password";

/// A caller address. TEST-NET-1 (RFC 5737), as in the harness.
///
/// Tests that must not be confused by the *address* rate limiter give each
/// scenario its own address: the limiter counts ten failures per address per
/// minute, and a lockout test that shared one address with its own setup would
/// eventually be asserting the wrong control.
fn peer(host: u8) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, host)), 41234)
}

async fn create_user(app: &TestApp, username: &str) -> Uuid {
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        username,
        &format!("{username}@kelir.test"),
        PASSWORD,
        &[],
    )
    .await
}

/// One sign-in attempt from a chosen address, returning only the status.
async fn attempt(app: &TestApp, from: SocketAddr, username: &str, password: &str) -> StatusCode {
    app.send_from(
        from,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({ "username": username, "password": password })),
    )
    .await
    .status
}

/// What the database holds, not what the API reports — the two are different
/// claims, and the lockout is only real if it is stored.
async fn stored_lockout(app: &TestApp, user_id: Uuid) -> Option<DateTime<Utc>> {
    sqlx::query_scalar!("SELECT locked_until FROM users WHERE id = $1", user_id)
        .fetch_one(&app.pool)
        .await
        .expect("read locked_until")
}

/// Moves a lockout into the past.
///
/// The alternative is waiting fifteen minutes. Expiry is a comparison against
/// `now()` with nothing scheduled behind it, so a lockout that ended a second
/// ago and one that ended a week ago are the same state, and moving the stored
/// time backwards tests exactly what the passage of time would.
async fn expire_lockout(app: &TestApp, user_id: Uuid) {
    sqlx::query!(
        "UPDATE users SET locked_until = now() - interval '1 second' WHERE id = $1",
        user_id
    )
    .execute(&app.pool)
    .await
    .expect("backdate locked_until");
}

/// Drives an account into lockout and returns its id.
async fn lock_out(app: &TestApp, username: &str, from: SocketAddr) -> Uuid {
    let user_id = create_user(app, username).await;

    for _ in 0..5 {
        assert_eq!(
            attempt(app, from, username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }

    user_id
}

// ---------------------------------------------------------------------------
// The lock goes on, and comes off again
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_fifth_failure_locks_the_account_for_fifteen_minutes() {
    let app = TestApp::spawn().await;
    let username = "user.locked";
    let user_id = create_user(&app, username).await;

    for attempt_number in 1..=5 {
        assert_eq!(
            attempt(&app, peer(11), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED,
            "failure {attempt_number} should be refused"
        );
    }

    let locked_until = stored_lockout(&app, user_id)
        .await
        .expect("the fifth failure should have locked the account");

    // Fifteen minutes, not "some time". The permanent lock of #55 and a
    // five-minute one both satisfy an `is_some()` assertion; only the window
    // catches a duration that has stopped matching the requirement.
    let remaining = locked_until - Utc::now();
    assert!(
        remaining > Duration::minutes(14) && remaining <= Duration::minutes(15),
        "lockout should last fifteen minutes, ends in {remaining}"
    );

    // The correct password, from an address that has never failed: neither the
    // password nor the rate limiter can explain this refusal.
    assert_eq!(
        attempt(&app, peer(12), username, PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "a locked-out account must refuse even the correct password"
    );
}

#[tokio::test]
async fn the_lockout_ends_on_its_own_and_leaves_no_trace() {
    let app = TestApp::spawn().await;
    let username = "user.expires";
    let user_id = lock_out(&app, username, peer(21)).await;

    expire_lockout(&app, user_id).await;

    // This is the assertion #55 exists for: the account comes back without an
    // administrator, without a scheduled job, and without direct SQL.
    assert_eq!(
        attempt(&app, peer(21), username, PASSWORD).await,
        StatusCode::OK,
        "an expired lockout must let the correct password through"
    );

    assert_eq!(
        stored_lockout(&app, user_id).await,
        None,
        "a successful sign-in should clear the expired lockout, not leave it lying in the row"
    );
}

#[tokio::test]
async fn four_failures_do_not_lock() {
    let app = TestApp::spawn().await;
    let username = "user.four";
    let user_id = create_user(&app, username).await;

    for _ in 0..4 {
        assert_eq!(
            attempt(&app, peer(31), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }

    assert_eq!(
        stored_lockout(&app, user_id).await,
        None,
        "four failures are below the baselined threshold"
    );

    // Without this case, a lockout that fired on the *first* failure would
    // satisfy every other test in this file.
    assert_eq!(
        attempt(&app, peer(31), username, PASSWORD).await,
        StatusCode::OK,
        "an account below the threshold must still sign in"
    );
}

// ---------------------------------------------------------------------------
// Which control refused, and whom it refused
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_lockout_belongs_to_the_account_not_to_the_caller_address() {
    let app = TestApp::spawn().await;
    let attacker = peer(41);

    let locked = "user.target";
    lock_out(&app, locked, attacker).await;

    // Same address, different account. The rate limiter keys on the address and
    // would refuse this; the lockout keys on the account and must not. Without
    // this pair, every assertion above is equally consistent with the address
    // limiter having done the work.
    let bystander = "user.bystander";
    create_user(&app, bystander).await;
    assert_eq!(
        attempt(&app, attacker, bystander, PASSWORD).await,
        StatusCode::OK,
        "locking one account must not refuse another from the same address"
    );

    // Different address, same account: the refusal travels with the account.
    assert_eq!(
        attempt(&app, peer(42), locked, PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "the lockout must not be escapable by changing address"
    );
}

// ---------------------------------------------------------------------------
// Recovery inside the product
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_administrator_can_see_a_lockout_and_end_it_early() {
    let app = TestApp::spawn().await;
    let username = "user.rescued";
    let user_id = lock_out(&app, username, peer(51)).await;
    let admin = app.administrator_token().await;

    // Seeing it matters as much as ending it. The account's status is still
    // ACTIVE — a failed-login lockout is not an administrative decision — so
    // without `lockedUntil` in the response an administrator would be looking at
    // an active account that refuses to sign in, with nothing to explain why.
    let before = app
        .get(&format!("/api/v1/identity/users/{user_id}"), Some(&admin))
        .await;
    assert_eq!(before.status, StatusCode::OK);
    assert_eq!(before.data()["status"], "ACTIVE");
    assert!(
        before.data()["lockedUntil"].is_string(),
        "an administrator must be able to see the lockout, got {}",
        before.data()
    );

    let cleared = app
        .put(
            &format!("/api/v1/identity/users/{user_id}"),
            Some(&admin),
            json!({ "status": "ACTIVE" }),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK);

    assert_eq!(
        stored_lockout(&app, user_id).await,
        None,
        "setting the account active again must clear the lockout, or the administrator's fix does nothing"
    );
    assert_eq!(
        attempt(&app, peer(52), username, PASSWORD).await,
        StatusCode::OK,
        "the user must sign in once an administrator has cleared the lockout"
    );
}

#[tokio::test]
async fn an_administrator_resetting_the_password_ends_the_lockout() {
    let app = TestApp::spawn().await;
    let username = "user.reset";
    let user_id = lock_out(&app, username, peer(61)).await;
    let admin = app.administrator_token().await;

    let new_password = "a-freshly-issued-account-password";
    let reset = app
        .post(
            &format!("/api/v1/identity/users/{user_id}/password"),
            Some(&admin),
            json!({ "password": new_password }),
        )
        .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT, "body {}", reset.body);

    // The recovery an administrator reaches for first. Before this change the
    // reset cleared `failed_login_count` and left the lock in place, so the
    // account still refused the password that had just been set for it.
    assert_eq!(
        attempt(&app, peer(62), username, new_password).await,
        StatusCode::OK,
        "a password the administrator just set must work immediately"
    );
}
