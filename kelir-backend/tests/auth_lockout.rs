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
//!   in-product recovery that #55 found missing;
//! * a burst of guesses in flight at once, sent through two backends that
//!   share no memory, checks no more passwords than the same guesses sent one
//!   at a time (#855).
//!
//! **The numbers are written out here rather than imported from
//! `MAX_FAILED_LOGINS` and `LOCKOUT_MINUTES`.** A test that reads the constants
//! passes whatever they are changed to; the requirement baselines five and
//! fifteen (SRS NFR-SEC-008, decision D-5), so five and fifteen are what these
//! assertions name.

mod common;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use chrono::{DateTime, Duration, Utc};
use common::{fixtures, TestApp};
use kelir_backend::mail::Mailer;
use kelir_backend::state::AppState;
use kelir_backend::{db, router};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use tokio::task::JoinHandle;
use tower::ServiceExt;
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

// ---------------------------------------------------------------------------
// A burst of guesses (#855)
//
// The tests above send one guess at a time, so each sees the lock the one
// before it left. A burst does not: every guess in flight read the account
// before any of them was counted. NFR-SEC-008 bounds the guesses, not the
// order they arrive in, so a burst must check no more passwords than the same
// guesses sent one at a time.
//
// **Two backends, each with its own pool and its own state**, over the test's
// database, so the guesses share nothing but the database: the deployment
// NFR-SCA-001 scales to, where a bound held in one process's memory bounds
// nothing.
//
// **Staged with a row lock**, as ADR-0048's tests in `auth_session.rs` stage
// their races: a connection of the test's own holds the account's row
// `FOR NO KEY UPDATE`, every guess is seen waiting on that row at once, and
// only then is the row released. A guess counts its failure with an `UPDATE`
// of the row, wherever in the sign-in that runs, so every guess is in flight
// before any is counted.
//
// **A checked password is an audit row**: `incorrect password` is written
// after a hash was checked and failed, and `account is locked out after
// repeated failures` by a refusal that checked none. Every guess here is
// wrong, so the first count is the hash checks the burst was let make.
// ---------------------------------------------------------------------------

/// Guesses each backend sends. Two backends make eight, above the five
/// NFR-SEC-008 allows, and within each backend's pool.
const GUESSES_PER_BACKEND: u8 = 4;

/// Each backend's pool: one more than its guesses, so no guess waits on a
/// connection instead of on the row.
const BACKEND_CONNECTIONS: u32 = 5;

/// How long a staged burst is given to reach the row, or to answer.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

const INCORRECT_PASSWORD: &str = "incorrect password";
const LOCKED_OUT: &str = "account is locked out after repeated failures";

/// A second application process in all but the process: its own pool, its
/// own rate limiter and its own router, over the test's database.
struct Backend {
    state: AppState,
}

impl Backend {
    fn beside(app: &TestApp) -> Arc<Self> {
        let pool = db::create_pool_with_max_connections(
            &app.state.config.database_url,
            BACKEND_CONNECTIONS,
        )
        .expect("a backend's pool");

        Arc::new(Backend {
            state: AppState::with_mailer(pool, (*app.state.config).clone(), Mailer::captured()),
        })
    }

    /// One sign-in through this backend's router, as [`attempt`] sends one
    /// through the test's.
    async fn attempt(&self, from: SocketAddr, username: &str, password: &str) -> StatusCode {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "username": username, "password": password }).to_string(),
            ))
            .expect("a sign-in request");
        request.extensions_mut().insert(ConnectInfo(from));

        router::create_router(self.state.clone())
            .oneshot(request)
            .await
            .expect("the backend answers")
            .status()
    }

    async fn close(&self) {
        self.state.pool.close().await;
    }
}

/// A connection of the test's own, holding the account's row
/// `FOR NO KEY UPDATE` until [`RowGate::open`]: the mode an `UPDATE` of the
/// row takes, so every write of the row waits on it and no plain read does.
struct RowGate {
    connection: PgConnection,
}

impl RowGate {
    async fn close(app: &TestApp, user_id: Uuid) -> Self {
        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the gate's connection");
        sqlx::query("BEGIN")
            .execute(&mut connection)
            .await
            .expect("begins the gate");
        sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR NO KEY UPDATE")
            .bind(user_id)
            .fetch_one(&mut connection)
            .await
            .expect("closes the gate on the account's row");

        RowGate { connection }
    }

    async fn open(mut self) {
        sqlx::query("COMMIT")
            .execute(&mut self.connection)
            .await
            .expect("opens the gate");
        self.connection
            .close()
            .await
            .expect("closes the gate's connection");
    }
}

/// How many statements wait on a lock to write a `users` row now.
async fn writes_waiting_on_users(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = current_database() AND wait_event_type = 'Lock'
           AND query ~ '^\\s*UPDATE users'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("reads pg_stat_activity")
}

/// The caller address of one guess: its own, so no guess meets the address
/// rate limiter, which counts per backend in any case.
fn guesser(backend: u8, guess: u8) -> SocketAddr {
    peer(100 + backend * 10 + guess)
}

/// Every guess of the burst, sent at once through both backends at
/// `username` while the gate holds the account's row. Returns once every guess
/// has been seen waiting on the row together, with none answered.
async fn burst_held_at_the_row(
    app: &TestApp,
    backends: &[Arc<Backend>; 2],
    username: &str,
) -> Vec<JoinHandle<StatusCode>> {
    let mut guesses = Vec::new();
    for (index, backend) in (0u8..).zip(backends) {
        for guess in 0..GUESSES_PER_BACKEND {
            let backend = Arc::clone(backend);
            let username = username.to_owned();
            guesses.push(tokio::spawn(async move {
                backend
                    .attempt(guesser(index, guess), &username, WRONG_PASSWORD)
                    .await
            }));
        }
    }

    let expected = i64::try_from(guesses.len()).expect("a small count");
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let waiting = writes_waiting_on_users(app).await;
        assert!(
            !guesses.iter().any(JoinHandle::is_finished),
            "a guess answered while the gate held the row: the staging does not overlap them"
        );
        if waiting == expected {
            return guesses;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{waiting} of {expected} guesses were seen waiting on the account's row"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

async fn answers(guesses: Vec<JoinHandle<StatusCode>>) -> Vec<StatusCode> {
    let mut statuses = Vec::new();
    for guess in guesses {
        statuses.push(
            tokio::time::timeout(PATIENCE, guess)
                .await
                .expect("a guess answers once the gate opens")
                .expect("a guess's task completes"),
        );
    }
    statuses
}

/// How the account's failed sign-ins were refused: passwords checked, and
/// guesses the lock refused without checking one.
async fn refusals(app: &TestApp, user_id: Uuid) -> (i64, i64) {
    let mut counts = [0_i64; 2];
    for (count, reason) in counts.iter_mut().zip([INCORRECT_PASSWORD, LOCKED_OUT]) {
        *count = sqlx::query_scalar(
            "SELECT count(*) FROM audit_events
             WHERE object_id = $1 AND event_type = 'Security.SignInFailed' AND reason = $2",
        )
        .bind(user_id)
        .bind(reason)
        .fetch_one(&app.pool)
        .await
        .expect("reads the audit rows");
    }

    (counts[0], counts[1])
}

/// The control the burst's numbers are compared with: the same eight guesses,
/// sent one at a time and alternating backends, at an account of their own.
async fn one_at_a_time(app: &TestApp, backends: &[Arc<Backend>; 2]) -> (i64, i64) {
    let username = "user.serial";
    let user_id = create_user(app, username).await;

    for guess in 0..GUESSES_PER_BACKEND {
        for (index, backend) in (0u8..).zip(backends) {
            assert_eq!(
                backend
                    .attempt(guesser(index + 2, guess), username, WRONG_PASSWORD)
                    .await,
                StatusCode::UNAUTHORIZED
            );
        }
    }

    refusals(app, user_id).await
}

/// The control on the staging: every guess of the burst is in flight at once,
/// seen waiting on the account's row together with none answered, whichever
/// statement of the sign-in that is. It asserts nothing about how many
/// passwords are checked, so it holds before #855's fix and after it. Without
/// it, a burst test that passed could be one whose guesses ran one after
/// another.
#[tokio::test]
async fn a_staged_burst_holds_every_guess_in_flight_at_once() {
    let app = TestApp::spawn().await;
    let username = "user.overlapped";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    let gate = RowGate::close(&app, user_id).await;
    let guesses = burst_held_at_the_row(&app, &backends, username).await;
    gate.open().await;

    let statuses = answers(guesses).await;
    assert!(
        statuses
            .iter()
            .all(|status| *status == StatusCode::UNAUTHORIZED),
        "every wrong guess is refused, got {statuses:?}"
    );

    for backend in &backends {
        backend.close().await;
    }
}

/// #855: eight wrong guesses in flight at once, through two backends, check
/// five passwords, as the same eight sent one at a time do; the other three
/// are refused by the lock, and the lock then holds for fifteen minutes on
/// both backends.
///
/// Before the fix every guess read the account unlocked, checked its password
/// and only then counted its failure, so all eight were checked: a burst of
/// any size was that many guesses.
#[tokio::test]
async fn a_burst_of_guesses_through_two_backends_checks_no_more_than_five_passwords() {
    let app = TestApp::spawn().await;
    let username = "user.burst";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    let control = one_at_a_time(&app, &backends).await;
    assert_eq!(
        control,
        (5, 3),
        "the control: eight guesses one at a time check five passwords, and the lock refuses three"
    );

    let gate = RowGate::close(&app, user_id).await;
    let guesses = burst_held_at_the_row(&app, &backends, username).await;
    gate.open().await;

    let statuses = answers(guesses).await;
    assert!(
        statuses
            .iter()
            .all(|status| *status == StatusCode::UNAUTHORIZED),
        "every wrong guess is refused, got {statuses:?}"
    );

    let (checked, refused_by_the_lock) = refusals(&app, user_id).await;
    assert!(
        checked <= 5,
        "a burst of {} guesses checked {checked} passwords; NFR-SEC-008 allows 5",
        statuses.len()
    );
    assert_eq!(
        (checked, refused_by_the_lock),
        control,
        "the burst must be bounded as the same guesses one at a time are"
    );

    // And the lock holds: fifteen minutes, refusing the right password through
    // either backend, from addresses that never guessed.
    let locked_until = stored_lockout(&app, user_id)
        .await
        .expect("the burst should have locked the account");
    let remaining = locked_until - Utc::now();
    assert!(
        remaining > Duration::minutes(14) && remaining <= Duration::minutes(15),
        "the lockout should last fifteen minutes, ends in {remaining}"
    );
    for (index, backend) in (0u8..).zip(&backends) {
        assert_eq!(
            backend.attempt(peer(90 + index), username, PASSWORD).await,
            StatusCode::UNAUTHORIZED,
            "the locked account must refuse the correct password through backend {index}"
        );
    }

    for backend in &backends {
        backend.close().await;
    }
}
