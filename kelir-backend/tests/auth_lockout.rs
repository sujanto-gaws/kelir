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
    sized_burst_held_at_the_row(
        app,
        backends,
        username,
        WRONG_PASSWORD,
        [GUESSES_PER_BACKEND; 2],
    )
    .await
}

/// [`burst_held_at_the_row`] with the password every attempt sends and the
/// attempts each backend sends chosen by the caller, each within its pool.
async fn sized_burst_held_at_the_row(
    app: &TestApp,
    backends: &[Arc<Backend>; 2],
    username: &str,
    password: &'static str,
    per_backend: [u8; 2],
) -> Vec<JoinHandle<StatusCode>> {
    let mut guesses = Vec::new();
    for ((index, backend), sends) in (0u8..).zip(backends).zip(per_backend) {
        for guess in 0..sends {
            let backend = Arc::clone(backend);
            let username = username.to_owned();
            guesses.push(tokio::spawn(async move {
                backend
                    .attempt(guesser(index, guess), &username, password)
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

// ---------------------------------------------------------------------------
// The closing gate's additions (#855, test-engineer, 2026-11-15)
//
// The tests above prove the burst bound with wrong passwords only. The fix
// counts every attempt before its check, so a *right* password is now
// counted, and may lock, while it is checked: these prove what that does to
// the account's owner, that a success gives the count back, the edges of the
// bound, and the paths that refuse without counting.
// ---------------------------------------------------------------------------

const ACCOUNT_NOT_ACTIVE: &str = "account is not active";
const PASSWORD_CHANGED: &str = "the password changed during the sign-in";

/// The account's `failed_login_count`, as stored.
async fn failure_count(app: &TestApp, user_id: Uuid) -> i32 {
    sqlx::query_scalar("SELECT failed_login_count FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&app.pool)
        .await
        .expect("reads failed_login_count")
}

/// Whether the account is locked out now, by the stored `locked_until`. An
/// expired lock is left in the row until a success clears it, so `is_some`
/// is not the question.
async fn locked_now(app: &TestApp, user_id: Uuid) -> bool {
    stored_lockout(app, user_id)
        .await
        .is_some_and(|until| until > Utc::now())
}

/// The account's `Security.SignInFailed` rows with `reason`.
async fn refusals_for(app: &TestApp, user_id: Uuid, reason: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
         WHERE object_id = $1 AND event_type = 'Security.SignInFailed' AND reason = $2",
    )
    .bind(user_id)
    .bind(reason)
    .fetch_one(&app.pool)
    .await
    .expect("reads the audit rows")
}

async fn execute(app: &TestApp, statement: &str) {
    sqlx::query(statement)
        .execute(&app.pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

/// The advisory key [`TokenGate`] waits on; the issue's number.
const TOKEN_GATE: i64 = 855;

/// A `BEFORE INSERT` trigger on `refresh_tokens` that waits on [`TOKEN_GATE`],
/// as `auth_session.rs`'s `InsertGate` does: a sign-in whose password verified
/// is held at its token's insert, after its count and its check, before it
/// clears the count.
struct TokenGate {
    connection: PgConnection,
}

impl TokenGate {
    async fn close(app: &TestApp) -> Self {
        execute(
            app,
            &format!(
                "CREATE FUNCTION test_hold_token_insert() RETURNS trigger
                 LANGUAGE plpgsql AS $$
                 BEGIN
                     PERFORM pg_advisory_xact_lock_shared({TOKEN_GATE});
                     RETURN NEW;
                 END
                 $$"
            ),
        )
        .await;
        execute(
            app,
            "CREATE TRIGGER test_hold_token_insert BEFORE INSERT ON refresh_tokens
             FOR EACH ROW EXECUTE FUNCTION test_hold_token_insert()",
        )
        .await;

        let mut connection = PgConnection::connect_with(&app.pool.connect_options())
            .await
            .expect("the gate's connection");
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(TOKEN_GATE)
            .execute(&mut connection)
            .await
            .expect("closes the gate");

        TokenGate { connection }
    }

    async fn open(mut self) {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(TOKEN_GATE)
            .execute(&mut self.connection)
            .await
            .expect("opens the gate");
        self.connection
            .close()
            .await
            .expect("closes the gate's connection");
    }
}

/// How many token inserts wait at the [`TokenGate`] now.
async fn tokens_held(app: &TestApp) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
         WHERE datname = current_database() AND wait_event_type = 'Lock'
           AND query ~ '^\\s*INSERT INTO refresh_tokens'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("reads pg_stat_activity")
}

/// Plan 22 row 3, G2: the right password sent as the fifth attempt, after
/// four failures, signs in. It is counted before its check, so it is the
/// attempt that puts the lock on, which the test sees while it is held at its
/// token's insert; its success then clears the lock and the count, and a
/// fresh run of five failures is needed to lock again.
///
/// Fails if the fifth attempt is refused for the lock it put on itself, if a
/// success leaves the lock or the count behind, or if the count is taken
/// after the check again (the lock would not be on while it is held).
#[tokio::test]
async fn the_right_password_as_the_fifth_attempt_signs_in_and_clears_the_lock_it_put_on() {
    let app = Arc::new(TestApp::spawn().await);
    let username = "user.fifth.right";
    let user_id = create_user(&app, username).await;

    for _ in 0..4 {
        assert_eq!(
            attempt(&app, peer(151), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(failure_count(&app, user_id).await, 4);

    let gate = TokenGate::close(&app).await;
    let fifth = {
        let app = Arc::clone(&app);
        tokio::spawn(async move { attempt(&app, peer(152), username, PASSWORD).await })
    };
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while tokens_held(&app).await < 1 {
        assert!(
            !fifth.is_finished(),
            "the fifth attempt answered without reaching its token's insert"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fifth attempt never reached its token's insert"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    assert!(
        locked_now(&app, user_id).await,
        "the fifth attempt is counted before its check, so it put the lock on"
    );
    assert_eq!(failure_count(&app, user_id).await, 0);

    gate.open().await;
    let answered = tokio::time::timeout(PATIENCE, fifth)
        .await
        .expect("the fifth attempt answers once the gate opens")
        .expect("the attempt's task completes");
    assert_eq!(
        answered,
        StatusCode::OK,
        "the right password on the fifth attempt must sign in"
    );
    assert_eq!(
        (
            stored_lockout(&app, user_id).await,
            failure_count(&app, user_id).await
        ),
        (None, 0),
        "the success clears the lock it put on, and the count"
    );
    assert_eq!(
        (
            refusals_for(&app, user_id, INCORRECT_PASSWORD).await,
            refusals_for(&app, user_id, LOCKED_OUT).await
        ),
        (4, 0)
    );

    // The count starts again from the success: four more failures do not
    // lock, and the fifth does.
    for _ in 0..4 {
        assert_eq!(
            attempt(&app, peer(153), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert!(
        !locked_now(&app, user_id).await,
        "four failures after a success must not lock: the success reset the count"
    );
    assert_eq!(
        attempt(&app, peer(153), username, WRONG_PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    assert!(locked_now(&app, user_id).await);
}

/// Plan 22 row 3, G1, both halves of the CHANGELOG's "until one of them
/// succeeds". After four failures, four sign-ins with the *right* password
/// are in flight at once, through two backends, held at the account's row
/// before any is counted. The first counted puts the lock on; the other three
/// find it at their count, not at the read before it (the read saw the
/// account unlocked: they were waiting on the row), and are refused as
/// locked out without a check. While the locking sign-in is held at its
/// token's insert, a fifth right password is refused too. Once it succeeds,
/// the lock is gone and the right password signs in through either backend.
#[tokio::test]
async fn right_passwords_in_flight_at_the_fifth_count_are_refused_until_the_locking_one_succeeds() {
    let app = TestApp::spawn().await;
    let username = "user.inflight.right";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    for _ in 0..4 {
        assert_eq!(
            attempt(&app, peer(161), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }

    let tokens = TokenGate::close(&app).await;
    let row = RowGate::close(&app, user_id).await;
    let mut sign_ins =
        sized_burst_held_at_the_row(&app, &backends, username, PASSWORD, [2, 2]).await;
    assert!(
        !locked_now(&app, user_id).await,
        "every sign-in passed the read before the count while the account was unlocked"
    );
    row.open().await;

    // Three answer, one is held at its token's insert.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let answered = sign_ins.iter().filter(|s| s.is_finished()).count();
        let held = tokens_held(&app).await;
        assert!(
            answered < sign_ins.len(),
            "every sign-in answered while the token gate was closed"
        );
        if answered == sign_ins.len() - 1 && held == 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{answered} sign-ins answered and {held} held at the token's insert; \
             expected 3 and 1"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let held_at = sign_ins
        .iter()
        .position(|s| !s.is_finished())
        .expect("one sign-in is held");
    let locking = sign_ins.remove(held_at);
    let refused = answers(sign_ins).await;
    assert_eq!(
        refused,
        [StatusCode::UNAUTHORIZED; 3],
        "the right password in flight must be refused once another attempt's count locked"
    );
    assert_eq!(
        (
            refusals_for(&app, user_id, INCORRECT_PASSWORD).await,
            refusals_for(&app, user_id, LOCKED_OUT).await
        ),
        (4, 3),
        "refused by the lock at the count, with no password checked"
    );
    assert!(locked_now(&app, user_id).await);

    assert_eq!(
        backends[1].attempt(peer(165), username, PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "while the locking sign-in is still in flight, the right password is refused"
    );

    tokens.open().await;
    assert_eq!(
        tokio::time::timeout(PATIENCE, locking)
            .await
            .expect("the locking sign-in answers once the gate opens")
            .expect("its task completes"),
        StatusCode::OK,
        "the sign-in that put the lock on verified the right password and signs in"
    );
    assert_eq!(
        (
            stored_lockout(&app, user_id).await,
            failure_count(&app, user_id).await
        ),
        (None, 0),
        "its success lifts the lock"
    );
    for (index, backend) in (0u8..).zip(&backends) {
        assert_eq!(
            backend.attempt(peer(166 + index), username, PASSWORD).await,
            StatusCode::OK,
            "once one succeeded, the right password signs in through backend {index}"
        );
    }
    assert_eq!(refusals_for(&app, user_id, INCORRECT_PASSWORD).await, 4);

    for backend in &backends {
        backend.close().await;
    }
}

/// A burst of exactly five wrong guesses, the threshold: every one is
/// checked, none is refused by the lock, and the fifth counted locks for
/// fifteen minutes. Fails if the bound is off by one in either direction.
#[tokio::test]
async fn a_burst_of_exactly_five_guesses_checks_five_passwords_and_locks() {
    let app = TestApp::spawn().await;
    let username = "user.burst.five";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    let gate = RowGate::close(&app, user_id).await;
    let guesses =
        sized_burst_held_at_the_row(&app, &backends, username, WRONG_PASSWORD, [3, 2]).await;
    gate.open().await;

    let statuses = answers(guesses).await;
    assert_eq!(statuses, [StatusCode::UNAUTHORIZED; 5]);
    assert_eq!(
        refusals(&app, user_id).await,
        (5, 0),
        "five guesses at once check five passwords, and the lock refuses none"
    );

    let locked_until = stored_lockout(&app, user_id)
        .await
        .expect("the fifth counted locks");
    let remaining = locked_until - Utc::now();
    assert!(
        remaining > Duration::minutes(14) && remaining <= Duration::minutes(15),
        "the lockout should last fifteen minutes, ends in {remaining}"
    );
    assert_eq!(failure_count(&app, user_id).await, 0);

    for backend in &backends {
        backend.close().await;
    }
}

/// Failures before a burst count against it: after two, a burst of eight
/// checks three passwords, and the lock refuses the other five.
#[tokio::test]
async fn a_burst_after_two_failures_checks_only_three_more_passwords() {
    let app = TestApp::spawn().await;
    let username = "user.burst.after.two";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    for _ in 0..2 {
        assert_eq!(
            attempt(&app, peer(171), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }

    let gate = RowGate::close(&app, user_id).await;
    let guesses = burst_held_at_the_row(&app, &backends, username).await;
    gate.open().await;

    let statuses = answers(guesses).await;
    assert_eq!(statuses, [StatusCode::UNAUTHORIZED; 8]);
    assert_eq!(
        refusals(&app, user_id).await,
        (5, 5),
        "two failures and a burst of eight: five passwords checked in all"
    );
    assert!(locked_now(&app, user_id).await);

    for backend in &backends {
        backend.close().await;
    }
}

/// Guesses refused by a lock in force are not counted, and a lock that
/// expires leaves a count of zero: after it, four failures do not lock and
/// the fifth does. Fails if a refused guess counts, or if the count left at
/// the lock carries over (one failure after expiry would lock again).
#[tokio::test]
async fn guesses_during_a_lockout_are_not_counted_and_five_fresh_failures_lock_again() {
    let app = TestApp::spawn().await;
    let username = "user.relock";
    let user_id = lock_out(&app, username, peer(181)).await;

    for _ in 0..3 {
        assert_eq!(
            attempt(&app, peer(182), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        (
            refusals(&app, user_id).await,
            failure_count(&app, user_id).await
        ),
        ((5, 3), 0),
        "guesses refused by the lock check no password and count nothing"
    );

    expire_lockout(&app, user_id).await;

    for failure in 1..=4 {
        assert_eq!(
            attempt(&app, peer(183), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
        assert!(
            !locked_now(&app, user_id).await,
            "failure {failure} after the lock expired locked the account again"
        );
    }
    assert_eq!(failure_count(&app, user_id).await, 4);

    assert_eq!(
        attempt(&app, peer(183), username, WRONG_PASSWORD).await,
        StatusCode::UNAUTHORIZED
    );
    let locked_until = stored_lockout(&app, user_id)
        .await
        .expect("the fifth fresh failure locks");
    let remaining = locked_until - Utc::now();
    assert!(
        remaining > Duration::minutes(14) && remaining <= Duration::minutes(15),
        "a fresh lockout of fifteen minutes, ends in {remaining}"
    );
    assert_eq!(refusals(&app, user_id).await, (10, 3));
}

/// A deactivated account is refused before its attempt is counted: its
/// sign-ins, right or wrong, check no password, count nothing and never lock,
/// and each is audited as not active. Reactivated, it signs in at once.
#[tokio::test]
async fn a_deactivated_accounts_sign_ins_are_refused_uncounted_and_never_lock() {
    let app = TestApp::spawn().await;
    let username = "user.deactivated";
    let user_id = create_user(&app, username).await;
    execute(
        &app,
        &format!("UPDATE users SET status = 'INACTIVE' WHERE id = '{user_id}'"),
    )
    .await;

    for _ in 0..6 {
        assert_eq!(
            attempt(&app, peer(191), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        attempt(&app, peer(192), username, PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "a deactivated account must refuse the right password"
    );

    assert_eq!(
        (
            failure_count(&app, user_id).await,
            stored_lockout(&app, user_id).await
        ),
        (0, None),
        "a deactivated account's sign-ins are not counted"
    );
    assert_eq!(
        (
            refusals_for(&app, user_id, ACCOUNT_NOT_ACTIVE).await,
            refusals(&app, user_id).await
        ),
        (7, (0, 0)),
        "each refusal says the account is not active, and none checked a password"
    );

    execute(
        &app,
        &format!("UPDATE users SET status = 'ACTIVE' WHERE id = '{user_id}'"),
    )
    .await;
    assert_eq!(
        attempt(&app, peer(193), username, PASSWORD).await,
        StatusCode::OK
    );
}

/// A tripwire's advisory key: a statement that decrements the failure count,
/// as a give-back would, waits on it, so a give-back reintroduced in that
/// shape is seen held.
const GIVE_BACK_GATE: i64 = 8_550;

/// #855's closing gate, a regression guard: a password write that overtakes a
/// right-password sign-in returns no guess to an attacker.
///
/// The sign-in's count is erased by the write, which resets the counter. A
/// sign-in refused because the password changed is not counted, and that
/// holds without a give-back. A give-back here took one from the failures
/// counted against the new password, and let an attacker check six passwords
/// before the lock; the fix gives nothing back.
///
/// Staged with two triggers on `users`:
/// - when the sign-in's count raises the counter, the same statement writes
///   the password as `set_password_hash` does (a new hash, the count and lock
///   reset): a write committed after the count and before the lock, so the
///   comparison under the lock refuses;
/// - a statement-level tripwire holds any statement that decrements the count,
///   and the test fails if one is ever held.
///
/// Three wrong guesses against the new password then leave the count at
/// three, and the attacker's run checks five passwords in all before the lock.
#[tokio::test]
async fn a_password_write_overtaking_a_sign_in_returns_no_guess_to_an_attacker() {
    let app = Arc::new(TestApp::spawn().await);
    let username = "user.given.back";
    let user_id = create_user(&app, username).await;
    let new_password = "the-password-written-during-the-verify";
    let donor = fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        "user.hash.donor",
        "user.hash.donor@kelir.test",
        new_password,
        &[],
    )
    .await;

    execute(
        &app,
        &format!(
            "CREATE FUNCTION test_write_on_count() RETURNS trigger
             LANGUAGE plpgsql AS $$
             DECLARE written text := (SELECT password_hash FROM users WHERE id = '{donor}');
             BEGIN
                 IF NEW.id = '{user_id}' AND OLD.password_hash IS DISTINCT FROM written
                    AND NEW.failed_login_count > OLD.failed_login_count THEN
                     NEW.password_hash := written;
                     NEW.failed_login_count := 0;
                     NEW.locked_until := NULL;
                 END IF;
                 RETURN NEW;
             END
             $$"
        ),
    )
    .await;
    execute(
        &app,
        "CREATE TRIGGER test_write_on_count BEFORE UPDATE ON users
         FOR EACH ROW EXECUTE FUNCTION test_write_on_count()",
    )
    .await;
    execute(
        &app,
        &format!(
            "CREATE FUNCTION test_hold_give_back() RETURNS trigger
             LANGUAGE plpgsql AS $$
             BEGIN
                 IF current_query() LIKE '%GREATEST(failed_login_count - 1%' THEN
                     PERFORM pg_advisory_xact_lock_shared({GIVE_BACK_GATE});
                 END IF;
                 RETURN NULL;
             END
             $$"
        ),
    )
    .await;
    execute(
        &app,
        "CREATE TRIGGER test_hold_give_back BEFORE UPDATE ON users
         FOR EACH STATEMENT EXECUTE FUNCTION test_hold_give_back()",
    )
    .await;

    let mut gate = PgConnection::connect_with(&app.pool.connect_options())
        .await
        .expect("the gate's connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(GIVE_BACK_GATE)
        .execute(&mut gate)
        .await
        .expect("closes the gate");

    let overtaken = {
        let app = Arc::clone(&app);
        tokio::spawn(async move { attempt(&app, peer(201), username, PASSWORD).await })
    };
    // The sign-in gives nothing back, so it answers with the tripwire still
    // closed. A statement that decrements the count is held by it, and fails
    // the test here.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !overtaken.is_finished() {
        let held: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'
               AND query LIKE '%GREATEST(failed_login_count - 1%'",
        )
        .fetch_one(&app.pool)
        .await
        .expect("reads pg_stat_activity");
        assert_eq!(held, 0, "a count was given back");
        assert!(
            tokio::time::Instant::now() < deadline,
            "the overtaken sign-in never answered"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        tokio::time::timeout(PATIENCE, overtaken)
            .await
            .expect("the overtaken sign-in answers")
            .expect("its task completes"),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        failure_count(&app, user_id).await,
        0,
        "the write reset the count the overtaken sign-in took"
    );

    // Three wrong guesses against the new password, counted after the write.
    for guess in 0..3 {
        assert_eq!(
            attempt(&app, peer(202 + guess), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(failure_count(&app, user_id).await, 3);

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(GIVE_BACK_GATE)
        .execute(&mut gate)
        .await
        .expect("opens the gate");
    assert_eq!(refusals_for(&app, user_id, PASSWORD_CHANGED).await, 1);
    let after_the_guesses = failure_count(&app, user_id).await;

    // The attacker guesses on until the lock refuses one.
    let mut guess = 0;
    while refusals_for(&app, user_id, LOCKED_OUT).await == 0 {
        assert!(guess < 10, "ten more guesses and no lock");
        assert_eq!(
            attempt(&app, peer(210 + guess), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
        guess += 1;
    }
    let checked_since_the_write = refusals_for(&app, user_id, INCORRECT_PASSWORD).await;

    assert_eq!(
        (after_the_guesses, checked_since_the_write),
        (3, 5),
        "a count was given back, or the count was not reset by the write: the count \
         read {after_the_guesses} for three guesses, and {checked_since_the_write} \
         passwords were checked against the new one before the lock; NFR-SEC-008 allows 5"
    );
}

/// The advisory key [`a_late_success_reset_after_a_password_write_erases_no_guess`]
/// holds a sign-in's success reset on.
const SUCCESS_RESET_GATE: i64 = 8_551;

/// #855's closing gate, second pass: a sign-in's success reset clears only the
/// count of the hash it verified.
///
/// A right-password sign-in passes ADR-0048's comparison, stores its token and
/// commits; its reset of the count and the lock (`record_successful_login`)
/// runs after, on the pool. A password write that commits in between resets
/// the count itself, and an attacker's failures are then counted against the
/// new password. A reset that matched no hash erased them, and the attacker
/// was checked more than five times against the new password before the lock.
///
/// Staged with a statement-level trigger on `users` that holds the success
/// reset before it touches any row, so the write and four wrong guesses run
/// while it waits. The write is an administrator's set-password, through the
/// API. The administrator signs in before the trigger exists, since a sign-in
/// of theirs would be held too.
#[tokio::test]
async fn a_late_success_reset_after_a_password_write_erases_no_guess() {
    let app = Arc::new(TestApp::spawn().await);
    let username = "user.late.reset";
    let user_id = create_user(&app, username).await;
    let admin = app.administrator_token().await;

    execute(
        &app,
        &format!(
            "CREATE FUNCTION test_hold_success_reset() RETURNS trigger
             LANGUAGE plpgsql AS $$
             BEGIN
                 IF current_query() LIKE '%last_login_at = now()%' THEN
                     PERFORM pg_advisory_xact_lock_shared({SUCCESS_RESET_GATE});
                 END IF;
                 RETURN NULL;
             END
             $$"
        ),
    )
    .await;
    execute(
        &app,
        "CREATE TRIGGER test_hold_success_reset BEFORE UPDATE ON users
         FOR EACH STATEMENT EXECUTE FUNCTION test_hold_success_reset()",
    )
    .await;

    let mut gate = PgConnection::connect_with(&app.pool.connect_options())
        .await
        .expect("the gate's connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SUCCESS_RESET_GATE)
        .execute(&mut gate)
        .await
        .expect("closes the gate");

    let signing_in = {
        let app = Arc::clone(&app);
        tokio::spawn(async move { attempt(&app, peer(221), username, PASSWORD).await })
    };
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let held: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'
               AND query LIKE '%last_login_at = now()%'",
        )
        .fetch_one(&app.pool)
        .await
        .expect("reads pg_stat_activity");
        if held == 1 {
            break;
        }
        assert!(
            !signing_in.is_finished(),
            "the sign-in answered without reaching its success reset"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "the sign-in's success reset was never held"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // The write, committed after the sign-in's comparison and before its reset.
    let new_password = "the-password-written-before-the-reset";
    let written = app
        .post(
            &format!("/api/v1/identity/users/{user_id}/password"),
            Some(&admin),
            json!({ "password": new_password }),
        )
        .await;
    assert_eq!(written.status, StatusCode::NO_CONTENT, "{}", written.body);

    // Four wrong guesses against the new password, counted after the write.
    for guess in 0..4 {
        assert_eq!(
            attempt(&app, peer(222 + guess), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(failure_count(&app, user_id).await, 4);

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SUCCESS_RESET_GATE)
        .execute(&mut gate)
        .await
        .expect("opens the gate");
    assert_eq!(
        tokio::time::timeout(PATIENCE, signing_in)
            .await
            .expect("the sign-in answers")
            .expect("its task completes"),
        StatusCode::OK,
        "the password was right when the sign-in verified it"
    );
    let after_reset = failure_count(&app, user_id).await;

    // The attacker guesses on until the lock refuses one.
    let mut guess = 0;
    while refusals_for(&app, user_id, LOCKED_OUT).await == 0 {
        assert!(guess < 10, "ten more guesses and no lock");
        assert_eq!(
            attempt(&app, peer(230 + guess), username, WRONG_PASSWORD).await,
            StatusCode::UNAUTHORIZED
        );
        guess += 1;
    }
    let checked_since_the_write = refusals_for(&app, user_id, INCORRECT_PASSWORD).await;

    assert_eq!(
        (after_reset, checked_since_the_write),
        (4, 5),
        "a success reset for the old password erased failures counted against the new \
         one: the count read {after_reset} for four guesses, and \
         {checked_since_the_write} passwords were checked against the new one before \
         the lock; NFR-SEC-008 allows 5"
    );
}

// ---------------------------------------------------------------------------
// #1003 — the lock is audited on the account it locks
// ---------------------------------------------------------------------------

/// The account's `Security.AccountLocked` rows, oldest first, read back
/// through `GET /api/v1/audit` (FR-AUD-004) as the administrator, who may read
/// a USER row's values: each row's action, object, actor and `lockedUntil`.
async fn lock_rows(app: &TestApp, user_id: Uuid) -> Vec<serde_json::Value> {
    let token = app.administrator_token().await;
    let uri = format!(
        "/api/v1/audit?objectType=USER&objectId={user_id}&eventType=Security.AccountLocked"
    );
    let read = app.get(&uri, Some(&token)).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    let rows = read.body["data"].as_array().expect("a page").iter().rev();
    let fields = ["action", "objectType", "objectId", "actorUserId"];
    rows.map(|row| json!([fields.map(|f| &row[f]), row["newValue"]["lockedUntil"]]))
        .collect()
}

/// `times` wrong passwords, each refused.
async fn fail(app: &TestApp, from: SocketAddr, username: &str, times: usize) {
    for _ in 0..times {
        let refused = attempt(app, from, username, WRONG_PASSWORD).await;
        assert_eq!(refused, StatusCode::UNAUTHORIZED);
    }
}

/// The row a lock writes: on the user, by the system, with `locked_until`.
async fn lock_row(app: &TestApp, user_id: Uuid) -> serde_json::Value {
    let until = stored_lockout(app, user_id).await.expect("locked");
    json!([["LOCK", "USER", user_id, null], until])
}

/// #1003: the failure that engages the lock writes one row on the user, with
/// no actor and the stored `locked_until`, and fills no reserved column.
/// Guesses during the lock write none and keep their `LOGIN_FAILED` refusal; a
/// fresh lock after expiry writes a second.
#[tokio::test]
async fn the_locking_failure_audits_the_lock_once_with_no_actor() {
    let app = TestApp::spawn().await;
    let username = "user.lock.audited";
    let user_id = lock_out(&app, username, peer(241)).await;
    let first = lock_row(&app, user_id).await;
    assert_eq!(lock_rows(&app, user_id).await, std::slice::from_ref(&first));

    let reserved: bool = sqlx::query_scalar(
        "SELECT bool_and(entity_type IS NULL AND entity_id IS NULL AND actor_role_id IS NULL)
         FROM audit_events WHERE object_id = $1 AND event_type = 'Security.AccountLocked'",
    )
    .bind(user_id)
    .fetch_one(&app.pool)
    .await
    .expect("the row");
    assert!(
        reserved,
        "entity_type, entity_id and actor_role_id are not filled"
    );

    fail(&app, peer(242), username, 3).await;
    assert_eq!(
        lock_rows(&app, user_id).await,
        std::slice::from_ref(&first),
        "none in the lock"
    );
    assert_eq!(refusals(&app, user_id).await, (5, 3), "the refusals stay");

    expire_lockout(&app, user_id).await;
    fail(&app, peer(243), username, 5).await;
    let second = lock_row(&app, user_id).await;
    assert_eq!(lock_rows(&app, user_id).await, [first, second]);
}

/// #1003: the right password as the fifth attempt puts the lock on at its
/// count and clears it at its success: no lock stood, so no row. The success
/// resets the count, and only a fresh fifth failure writes one.
#[tokio::test]
async fn a_lock_a_success_clears_writes_no_lock_row() {
    let app = TestApp::spawn().await;
    let username = "user.lock.cleared";
    let user_id = create_user(&app, username).await;

    fail(&app, peer(244), username, 4).await;
    assert_eq!(
        attempt(&app, peer(245), username, PASSWORD).await,
        StatusCode::OK
    );
    assert!(
        lock_rows(&app, user_id).await.is_empty(),
        "the lock was cleared"
    );

    fail(&app, peer(246), username, 4).await;
    assert!(
        lock_rows(&app, user_id).await.is_empty(),
        "the success reset the count"
    );
    fail(&app, peer(246), username, 1).await;
    assert_eq!(
        lock_rows(&app, user_id).await,
        [lock_row(&app, user_id).await]
    );
}

/// #1003: eight wrong guesses in flight at once through two backends lock the
/// account once, so the trail holds one lock row.
#[tokio::test]
async fn a_burst_that_locks_writes_one_lock_row() {
    let app = TestApp::spawn().await;
    let username = "user.lock.burst";
    let user_id = create_user(&app, username).await;
    let backends = [Backend::beside(&app), Backend::beside(&app)];

    let gate = RowGate::close(&app, user_id).await;
    let guesses = burst_held_at_the_row(&app, &backends, username).await;
    gate.open().await;
    assert_eq!(answers(guesses).await, [StatusCode::UNAUTHORIZED; 8]);
    assert_eq!(refusals(&app, user_id).await, (5, 3));
    assert_eq!(
        lock_rows(&app, user_id).await,
        [lock_row(&app, user_id).await]
    );

    for backend in &backends {
        backend.close().await;
    }
}
