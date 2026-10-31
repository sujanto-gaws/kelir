//! **No password or token reaches a log on the request path** (NFR-SEC-004,
//! [#779](https://github.com/sujanto-gaws/kelir/issues/779) AC3).
//!
//! `SignInRequest`, `ChangePasswordRequest`, `RefreshRequest`,
//! `ResetPasswordRequest` and `SessionResponse` all derive `Debug`, with the
//! secret in a plain field. One `tracing::debug!(?request)` is all it would take,
//! and no test was reading what those routes log. `the_bootstrap_password_…` and
//! `no_log_line_carries_the_secret_even_at_trace` each hold one other path.
//!
//! **This binary holds one test, and installs a process-wide subscriber before
//! anything else runs.** The suite's other captures attach to the thread or to
//! the future, and neither reaches the blocking pool, where `hash_password` runs
//! with the password in its hand. A global one sees every thread, at
//! TRACE, which also takes `sqlx`, `rustls` and anything else below Kelir, and
//! it settles the callsite-interest race `bootstrap.rs` describes, because the
//! subscriber exists before the first callsite does. `common` uses `try_init`,
//! so it leaves this one in place.
//!
//! **The controls.** A silent capture passes every assertion under it, so the
//! test first requires what it must have seen: a line from each path the
//! product already logs, a `sqlx` statement to show TRACE reached the layers
//! below, and a marker written from the blocking pool. It then plants a marker
//! through the same search, so the search is seen to find.
//!
//! **Each path runs its refusals too**, because a refusal is where a log line is
//! most likely to say what it refused: a wrong password, a replayed refresh
//! token, a wrong current password and a reset token that does not exist.
//!
//! What it cannot see: `tracing` is the only logger the backend has and the
//! router adds no trace layer, so the capture is the whole log of these routes.
//! The mailer is the captured one, so the SMTP transport's own logging is not
//! on this path.
//!
//! # Seen to fail under mutation
//!
//! Each mutation was made alone, this file run, and the mutation reverted.
//! **Seen red, 2026-10-31**, in `kelir-rust-tools:1.89`. The binary holds one
//! test, so each row is 1 of 1. The count is of the 18 secrets the search holds.
//!
//! | Mutation | Reddened at |
//! |---|---|
//! | `tracing::debug!(?request)` in `handlers::sign_in`, the `Debug` derive | 3 of 18: the bootstrap, first and wrong passwords |
//! | `tracing::trace!(?request)` in `handlers::change_password` | 3 of 18: the first, wrong current and changed passwords |
//! | `tracing::debug!(token = %request.refresh_token)` in `handlers::refresh` | 1 of 18: the refresh token |
//! | `tracing::info!(?request)` in `handlers::reset_password` | 2 of 18: the reset token and the reset password |
//! | `tracing::trace!(%password)` in `hash_password`, **on the blocking pool** | 5 of 18: every password hashed. This is the row the global subscriber is for |
//! | `tracing::debug!(access = %signed_in.access_token)` in `handlers::sign_in` | 5 of 18: the access tokens, and a signature |
//! | `tracing::debug!(reset = %token.token)` where the reset token is issued | 1 of 18: the reset token |
//! | `"password changed; sessions revoked"` reworded | the control, not a leak: *the capture did not see* that path |
//! | The subscriber installed at `Level::ERROR` | the control: 0 bytes captured |

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use serde_json::json;

use common::{fixtures, TestApp, ADMIN_PASSWORD, ADMIN_USERNAME, JWT_SECRET};

const FIRST: &str = "first-long-password-3b1f";
const WRONG: &str = "wrong-long-password-8c2a";
const WRONG_CURRENT: &str = "wrong-current-password-51de";
const SECOND: &str = "second-long-password-77c9";
const THIRD: &str = "third-long-password-e04b";
const FOURTH: &str = "fourth-long-password-a96d";

const USER: &str = "log.capture";
const ON_THE_BLOCKING_POOL: &str = "row-3-capture-control-on-the-blocking-pool";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the capture")).into_owned()
    }
}

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the capture").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every line that carries one of `secrets`, named.
fn leaks(text: &str, secrets: &[(&str, String)]) -> Vec<String> {
    secrets
        .iter()
        .filter_map(|(what, value)| {
            text.lines()
                .find(|line| line.contains(value.as_str()))
                .map(|line| format!("{what}: {line}"))
        })
        .collect()
}

fn data_string(response: &common::TestResponse, field: &str) -> String {
    response.data()[field]
        .as_str()
        .unwrap_or_else(|| panic!("no {field}: {}", response.body))
        .to_owned()
}

async fn sign_in(app: &TestApp, password: &str) -> common::TestResponse {
    app.post(
        "/api/v1/auth/login",
        None,
        json!({ "username": USER, "password": password }),
    )
    .await
}

#[tokio::test]
async fn no_password_or_token_is_logged_on_sign_in_change_reset_or_refresh() {
    let captured = Captured::default();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .try_init()
        .expect("this binary's first subscriber: it holds one test");

    let app = TestApp::spawn().await;
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        USER,
        "log.capture@example.test",
        FIRST,
        &[],
    )
    .await;

    let mut secrets: Vec<(&str, String)> = vec![
        ("the bootstrap password", ADMIN_PASSWORD.to_owned()),
        ("the signing secret", JWT_SECRET.to_owned()),
        ("the first password", FIRST.to_owned()),
        ("the wrong password", WRONG.to_owned()),
        ("the wrong current password", WRONG_CURRENT.to_owned()),
        ("the changed password", SECOND.to_owned()),
        ("the reset password", THIRD.to_owned()),
        ("the administrator's password", FOURTH.to_owned()),
    ];
    let mut secret = |what, value: String| secrets.push((what, value));

    // --- Sign-in --------------------------------------------------------------
    let refused = sign_in(&app, WRONG).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED, "{}", refused.body);

    let session = sign_in(&app, FIRST).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    let access = data_string(&session, "accessToken");
    let refresh = data_string(&session, "refreshToken");
    let signature = access
        .rsplit('.')
        .next()
        .expect("a signed token")
        .to_owned();
    secret("the access token", access.clone());
    secret("the access token's signature", signature);
    secret("the refresh token", refresh.clone());

    // --- Refresh, and the same token replayed ---------------------------------
    let rotated = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.body);
    secret(
        "the rotated access token",
        data_string(&rotated, "accessToken"),
    );
    secret(
        "the rotated refresh token",
        data_string(&rotated, "refreshToken"),
    );

    let replayed = app
        .post(
            "/api/v1/auth/refresh",
            None,
            json!({ "refreshToken": refresh }),
        )
        .await;
    assert_eq!(
        replayed.status,
        StatusCode::UNAUTHORIZED,
        "{}",
        replayed.body
    );

    // --- Change: wrong current password, and then the right one ---------------
    let session = sign_in(&app, FIRST).await;
    assert_eq!(session.status, StatusCode::OK, "{}", session.body);
    let access = data_string(&session, "accessToken");
    secret("the second access token", access.clone());
    secret(
        "the second refresh token",
        data_string(&session, "refreshToken"),
    );

    let refused = app
        .post(
            "/api/v1/auth/change-password",
            Some(&access),
            json!({ "currentPassword": WRONG_CURRENT, "newPassword": SECOND }),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let changed = app
        .post(
            "/api/v1/auth/change-password",
            Some(&access),
            json!({ "currentPassword": FIRST, "newPassword": SECOND }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);

    // --- Reset: a token that does not exist, and the real one -----------------
    let refused = app
        .post(
            "/api/v1/auth/reset-password",
            None,
            json!({ "token": "0".repeat(64), "newPassword": THIRD }),
        )
        .await;
    assert_eq!(
        refused.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        refused.body
    );

    let asked = app
        .post(
            "/api/v1/auth/forgot-password",
            None,
            json!({ "username": USER }),
        )
        .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
    let mail = app.mail_delivered(1).await;
    let body = &mail.last().expect("a link was sent").body;
    let at = body.find("token=").expect("the body carries a link") + "token=".len();
    let reset_token: String = body[at..]
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    assert_eq!(reset_token.len(), 64, "{body}");
    secret("the reset token", reset_token.clone());

    let reset = app
        .post(
            "/api/v1/auth/reset-password",
            None,
            json!({ "token": reset_token, "newPassword": THIRD }),
        )
        .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);

    // --- An administrator setting a password ----------------------------------
    let administrator = app.sign_in(ADMIN_USERNAME, ADMIN_PASSWORD).await;
    let user: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(USER)
        .fetch_one(&app.pool)
        .await
        .expect("find the user");
    let set = app
        .post(
            &format!("/api/v1/identity/users/{user}/password"),
            Some(&administrator),
            json!({ "password": FOURTH }),
        )
        .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);

    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .expect("read the stored hash");
    secret("the administrator's token", administrator);
    secret("the stored hash", hash);

    // --- The controls -----------------------------------------------------------
    tokio::task::spawn_blocking(|| tracing::trace!("{ON_THE_BLOCKING_POOL}"))
        .await
        .expect("the blocking pool");
    let text = captured.text();

    for seen in [
        "SELECT",
        "revoked refresh token presented",
        "password changed; sessions revoked",
        "password reset",
        ON_THE_BLOCKING_POOL,
    ] {
        assert!(
            text.contains(seen),
            "the capture did not see {seen:?}, so its silence proves nothing ({} bytes)",
            text.len()
        );
    }

    // The search itself is seen to find, on the same capture.
    let planted = "row-3-capture-control-planted-secret";
    tracing::trace!("{planted}");
    let found = leaks(&captured.text(), &[("the plant", planted.to_owned())]);
    assert_eq!(found.len(), 1, "the search did not find what was planted");

    // --- The rule ---------------------------------------------------------------
    let found = leaks(&text, &secrets);
    assert!(
        found.is_empty(),
        "{} of {} secrets reached a log line:\n{}",
        found.len(),
        secrets.len(),
        found.join("\n")
    );
}
