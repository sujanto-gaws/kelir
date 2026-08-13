//! Rate limiting for the authentication endpoints (SRS NFR-SEC-008).
//!
//! Account lockout already stops repeated guesses at *one* account. This stops
//! the other shape of the same attack: many attempts spread across many
//! accounts from one source, which lockout never sees because no single account
//! reaches its threshold.
//!
//! Applied as one layer over `/auth/login`, `/auth/refresh` and
//! `/auth/change-password`, sharing a single bucket per source address so a
//! caller gains nothing by moving between them. `/auth/refresh` is
//! unauthenticated and hits the database on every call; `/auth/change-password`
//! runs a full Argon2id verification on the blocking pool and, before this,
//! counted nothing at all — making it both a way to burn CPU and an oracle for
//! guessing the current password that account lockout does not cover.
//!
//! **Failures are what count.** A success decays the counter by one instead of
//! clearing it, because clearing it let anyone holding one valid credential
//! guess ten passwords, sign in once, and start again from the same address for
//! as long as they liked.
//!
//! **In-memory, so per-instance.** With several replicas the effective limit is
//! multiplied by the replica count. That is acceptable while staging and
//! production are single-instance (deployment §4.1); a shared store is the fix
//! when they are not, and the limit is small enough that even a multiplied
//! version is far below what a credential-stuffing run needs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::middleware::client_address::ClientAddress;
use crate::state::AppState;

/// Failed attempts allowed per key within [`WINDOW`].
///
/// Above the 5-failure account lockout, so a person mistyping their password
/// hits the lockout — which explains itself — rather than a bare 429.
pub const MAX_ATTEMPTS: u32 = 10;

/// The sliding window failures are counted in.
pub const WINDOW: Duration = Duration::from_secs(60);

/// How long a key stays blocked once it exceeds the limit.
pub const BLOCK_DURATION: Duration = Duration::from_secs(15 * 60);

/// Meters the authentication endpoints, keyed by resolved client address.
///
/// A layer rather than a line in each handler, so it also sees the requests a
/// handler never runs for — a malformed body, a missing bearer token — which are
/// the cheapest attempts to make and would otherwise be free.
///
/// Only 4xx counts as a failure. A 5xx is our fault, not the caller's, and
/// counting it would turn a database outage into fifteen minutes of lockout for
/// every user on top of the outage itself.
pub async fn limit_authentication_attempts(
    State(state): State<AppState>,
    client: ClientAddress,
    request: Request,
    next: Next,
) -> Response {
    let key = client.rate_limit_key();

    if let Decision::Block {
        retry_after_seconds,
    } = state.rate_limiter.check(&key)
    {
        tracing::warn!(
            client = %client,
            path = %request.uri().path(),
            "authentication rate limit exceeded"
        );

        return AppError::TooManyRequests {
            retry_after_seconds,
        }
        .into_response();
    }

    let response = next.run(request).await;
    let status = response.status();

    if status.is_client_error() {
        state.rate_limiter.record_failure(&key);
    } else if status.is_success() {
        state.rate_limiter.record_success(&key);
    }

    response
}

struct Attempts {
    count: u32,
    window_started: Instant,
    blocked_until: Option<Instant>,
}

/// Fixed-window counter of failed attempts, keyed by caller.
///
/// A fixed window can allow up to twice the limit across a boundary. That is a
/// real imprecision and an acceptable one here: the purpose is to stop
/// automated volume, not to meter precisely, and the simplicity means no
/// background task and no unbounded growth beyond what pruning handles.
pub struct RateLimiter {
    entries: Mutex<HashMap<String, Attempts>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Blocked, with the seconds remaining — returned to the caller so a
    /// legitimate client can wait rather than retry blindly.
    Block {
        retry_after_seconds: u64,
    },
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Says whether this key may proceed. Counts nothing.
    pub fn check(&self, key: &str) -> Decision {
        self.check_at(key, Instant::now())
    }

    /// Counts an attempt that failed, and starts a block once there are enough.
    pub fn record_failure(&self, key: &str) {
        self.record_failure_at(key, Instant::now());
    }

    /// Lets a success work off one earlier failure.
    ///
    /// Decays rather than clears: removing the key would mean one valid
    /// credential buys a fresh allowance of guesses every time it is used.
    pub fn record_success(&self, key: &str) {
        self.record_success_at(key, Instant::now());
    }

    /// The clock is a parameter so the window and block behaviour can be tested
    /// without sleeping.
    fn check_at(&self, key: &str, now: Instant) -> Decision {
        let mut entries = self.lock();

        let Some(attempts) = entries.get_mut(key) else {
            return Decision::Allow;
        };

        if let Some(until) = attempts.blocked_until {
            if now < until {
                return Decision::Block {
                    retry_after_seconds: (until - now).as_secs().max(1),
                };
            }

            // The block expired; start clean rather than resuming the old count.
            attempts.blocked_until = None;
            attempts.count = 0;
            attempts.window_started = now;
        }

        Decision::Allow
    }

    fn record_failure_at(&self, key: &str, now: Instant) {
        let mut entries = self.lock();

        // Opportunistic cleanup: without it the map grows with every distinct
        // key seen. Only this path inserts, so it is the only one that needs it.
        if entries.len() > 10_000 {
            entries.retain(|_, attempts| !attempts.is_stale(now));
        }

        let attempts = entries.entry(key.to_owned()).or_insert(Attempts {
            count: 0,
            window_started: now,
            blocked_until: None,
        });

        if attempts.is_blocked(now) {
            // Already blocked; the request never reached the endpoint, so there
            // is nothing further to count and the block must not be extended by
            // continued hammering.
            return;
        }

        if now.duration_since(attempts.window_started) >= WINDOW {
            attempts.count = 0;
            attempts.window_started = now;
        }

        attempts.count += 1;

        if attempts.count >= MAX_ATTEMPTS {
            attempts.blocked_until = Some(now + BLOCK_DURATION);
        }
    }

    fn record_success_at(&self, key: &str, now: Instant) {
        let mut entries = self.lock();

        let Some(attempts) = entries.get_mut(key) else {
            return;
        };

        if attempts.is_blocked(now) {
            return;
        }

        attempts.count = attempts.count.saturating_sub(1);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Attempts>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Attempts {
    fn is_blocked(&self, now: Instant) -> bool {
        self.blocked_until.is_some_and(|until| now < until)
    }

    fn is_stale(&self, now: Instant) -> bool {
        let past_block = self.blocked_until.is_none_or(|until| now >= until);
        let past_window = now.duration_since(self.window_started) >= WINDOW;

        past_block && past_window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::router::create_router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request as HttpRequest, StatusCode};
    use std::net::SocketAddr;
    use tower::ServiceExt;

    fn fail(limiter: &RateLimiter, key: &str, times: u32) {
        for _ in 0..times {
            limiter.record_failure(key);
        }
    }

    #[test]
    fn allows_up_to_the_limit() {
        let limiter = RateLimiter::new();

        for attempt in 1..MAX_ATTEMPTS {
            fail(&limiter, "1.2.3.4", 1);
            assert_eq!(
                limiter.check("1.2.3.4"),
                Decision::Allow,
                "attempt {attempt} should still be allowed"
            );
        }
    }

    #[test]
    fn blocks_past_the_limit() {
        let limiter = RateLimiter::new();

        fail(&limiter, "1.2.3.4", MAX_ATTEMPTS);

        assert!(matches!(
            limiter.check("1.2.3.4"),
            Decision::Block { retry_after_seconds } if retry_after_seconds > 0
        ));
    }

    #[test]
    fn keys_are_independent() {
        // Otherwise one noisy client would lock out everyone behind the same
        // proxy — or worse, one attacker could deny service to every user.
        let limiter = RateLimiter::new();

        fail(&limiter, "1.2.3.4", MAX_ATTEMPTS);

        assert_eq!(limiter.check("5.6.7.8"), Decision::Allow);
    }

    #[test]
    fn a_successful_attempt_is_not_counted() {
        // The meter is for failures. A busy client signing in legitimately, over
        // and over, must never approach the limit.
        let limiter = RateLimiter::new();

        for _ in 0..MAX_ATTEMPTS * 5 {
            limiter.record_success("1.2.3.4");
        }

        assert_eq!(limiter.check("1.2.3.4"), Decision::Allow);
    }

    #[test]
    fn a_success_decays_the_count_rather_than_clearing_it() {
        // The hole this closes: clearing the key meant one valid credential
        // bought ten fresh guesses every time it was used, forever, from one
        // address.
        let limiter = RateLimiter::new();

        fail(&limiter, "1.2.3.4", MAX_ATTEMPTS - 1);
        limiter.record_success("1.2.3.4");

        // One success bought exactly one more guess, not a clean slate.
        fail(&limiter, "1.2.3.4", 1);
        assert_eq!(limiter.check("1.2.3.4"), Decision::Allow);

        fail(&limiter, "1.2.3.4", 1);
        assert!(matches!(limiter.check("1.2.3.4"), Decision::Block { .. }));
    }

    #[test]
    fn a_success_does_not_lift_a_block() {
        let limiter = RateLimiter::new();

        fail(&limiter, "1.2.3.4", MAX_ATTEMPTS);
        limiter.record_success("1.2.3.4");

        assert!(matches!(limiter.check("1.2.3.4"), Decision::Block { .. }));
    }

    #[test]
    fn the_counter_resets_after_the_window() {
        let limiter = RateLimiter::new();
        let start = Instant::now();

        for _ in 0..MAX_ATTEMPTS - 1 {
            limiter.record_failure_at("1.2.3.4", start);
        }

        limiter.record_failure_at("1.2.3.4", start + WINDOW);

        assert_eq!(limiter.check_at("1.2.3.4", start + WINDOW), Decision::Allow);
    }

    #[test]
    fn a_block_expires() {
        let limiter = RateLimiter::new();
        let start = Instant::now();

        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure_at("1.2.3.4", start);
        }

        assert!(matches!(
            limiter.check_at("1.2.3.4", start + BLOCK_DURATION - Duration::from_secs(1)),
            Decision::Block { .. }
        ));
        assert_eq!(
            limiter.check_at("1.2.3.4", start + BLOCK_DURATION + Duration::from_secs(1)),
            Decision::Allow
        );
    }

    #[test]
    fn hammering_a_blocked_key_does_not_extend_the_block() {
        // Otherwise a client retrying on a timer could never get back in, and an
        // attacker could hold a third party's address blocked indefinitely.
        let limiter = RateLimiter::new();
        let start = Instant::now();

        for _ in 0..MAX_ATTEMPTS {
            limiter.record_failure_at("1.2.3.4", start);
        }
        for _ in 0..100 {
            limiter.record_failure_at("1.2.3.4", start + Duration::from_secs(60));
        }

        assert_eq!(
            limiter.check_at("1.2.3.4", start + BLOCK_DURATION + Duration::from_secs(1)),
            Decision::Allow
        );
    }

    #[test]
    fn the_limit_sits_above_the_account_lockout_threshold() {
        // A person mistyping their password should meet the lockout, which
        // explains itself, rather than an opaque 429.
        assert!(MAX_ATTEMPTS > crate::modules::auth::service::MAX_FAILED_LOGINS as u32);
    }

    // ---------------------------------------------------------------------
    // Through the router.
    //
    // The unit tests above all passed while the limit was trivially evadable,
    // because they call the limiter directly and the defect was in what the
    // limiter was keyed on. These drive real requests instead, which is the only
    // place the key is chosen.
    //
    // None of them need a database: every attempt they make is refused before
    // the handler runs — a malformed body, or a missing bearer token — which is
    // also the cheapest attempt an attacker can make.
    // ---------------------------------------------------------------------

    fn state_with(trusted_proxy_hops: usize) -> AppState {
        let pool = crate::db::create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("lazy pool builds without a server");

        let mut config = AppConfig::test_default();
        config.trusted_proxy_hops = trusted_proxy_hops;

        AppState::new(pool, config)
    }

    /// A login attempt with a body the handler will refuse before touching the
    /// database, from `peer`, optionally claiming `forwarded_for`.
    fn attempt(peer: &str, forwarded_for: Option<&str>) -> HttpRequest<Body> {
        attempt_on("/api/v1/auth/login", peer, forwarded_for)
    }

    fn attempt_on(uri: &str, peer: &str, forwarded_for: Option<&str>) -> HttpRequest<Body> {
        let peer: SocketAddr = format!("{peer}:41234").parse().expect("a peer address");

        let mut builder = HttpRequest::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");

        if let Some(value) = forwarded_for {
            builder = builder.header("x-forwarded-for", value);
        }

        let mut request = builder.body(Body::from("{}")).expect("the request builds");

        // What axum::serve installs via into_make_service_with_connect_info.
        request.extensions_mut().insert(ConnectInfo(peer));

        request
    }

    async fn send(state: &AppState, request: HttpRequest<Body>) -> Response {
        create_router(state.clone())
            .oneshot(request)
            .await
            .expect("the router responds")
    }

    #[tokio::test]
    async fn a_rotating_forwarded_header_does_not_evade_the_limit() {
        // The defect: one caller sent a different X-Forwarded-For per request
        // and landed in a fresh bucket every time, so the limit never applied.
        let state = state_with(0);

        for attempt_number in 1..=MAX_ATTEMPTS {
            let forwarded = format!("203.0.113.{attempt_number}");
            let response = send(&state, attempt("198.51.100.7", Some(&forwarded))).await;

            assert!(
                response.status().is_client_error(),
                "attempt {attempt_number} should be refused by the endpoint"
            );
            assert_ne!(
                response.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "attempt {attempt_number} is within the allowance"
            );
        }

        let response = send(&state, attempt("198.51.100.7", Some("203.0.113.250"))).await;

        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "a new header value must not buy a new allowance"
        );
    }

    #[tokio::test]
    async fn a_forged_header_cannot_aim_the_limit_at_a_third_party() {
        // Eleven requests carrying someone else's address must not stop that
        // someone from signing in.
        let state = state_with(0);

        for _ in 0..=MAX_ATTEMPTS {
            send(&state, attempt("198.51.100.7", Some("203.0.113.9"))).await;
        }

        let victim = send(&state, attempt("203.0.113.9", None)).await;

        assert_ne!(
            victim.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the address named in the header belongs to someone who never called"
        );
    }

    #[tokio::test]
    async fn one_source_does_not_throttle_another() {
        let state = state_with(0);

        for _ in 0..=MAX_ATTEMPTS {
            send(&state, attempt("198.51.100.7", None)).await;
        }

        assert_eq!(
            send(&state, attempt("198.51.100.7", None)).await.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_ne!(
            send(&state, attempt("198.51.100.8", None)).await.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "a second address is a second bucket"
        );
    }

    #[tokio::test]
    async fn callers_without_a_forwarded_header_are_not_one_bucket() {
        // The old fallback keyed every header-less caller as "unknown", so
        // eleven failures from anyone returned 429 to everyone.
        let state = state_with(0);

        for _ in 0..=MAX_ATTEMPTS {
            send(&state, attempt("10.0.0.1", None)).await;
        }

        for peer in ["10.0.0.2", "10.0.0.3", "10.0.0.4"] {
            assert_ne!(
                send(&state, attempt(peer, None)).await.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "{peer} shares no bucket with 10.0.0.1"
            );
        }
    }

    #[tokio::test]
    async fn behind_a_trusted_proxy_the_forwarded_address_is_the_bucket() {
        // The staging topology: one hop, so callers arriving through it are
        // metered individually rather than all as the proxy.
        let state = state_with(1);

        for _ in 0..=MAX_ATTEMPTS {
            send(&state, attempt("172.18.0.2", Some("203.0.113.9"))).await;
        }

        assert_eq!(
            send(&state, attempt("172.18.0.2", Some("203.0.113.9")))
                .await
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_ne!(
            send(&state, attempt("172.18.0.2", Some("203.0.113.10")))
                .await
                .status(),
            StatusCode::TOO_MANY_REQUESTS,
            "another client through the same proxy is another bucket"
        );
    }

    #[tokio::test]
    async fn a_rate_limited_response_carries_retry_after() {
        let state = state_with(0);

        for _ in 0..=MAX_ATTEMPTS {
            send(&state, attempt("198.51.100.7", None)).await;
        }

        let response = send(&state, attempt("198.51.100.7", None)).await;

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let retry_after = response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("a 429 says how long to wait")
            .to_str()
            .expect("Retry-After is ascii");

        assert!(
            retry_after.parse::<u64>().is_ok_and(|seconds| seconds > 0),
            "Retry-After must be a number of seconds, found {retry_after:?}"
        );
    }

    #[tokio::test]
    async fn refresh_and_change_password_are_metered_too() {
        // NFR-SEC-008 says authentication endpoints, plural. change-password
        // burns an Argon2id verification per call and refresh hits the database
        // unauthenticated; neither was counted at all.
        for uri in ["/api/v1/auth/refresh", "/api/v1/auth/change-password"] {
            let state = state_with(0);

            for _ in 0..=MAX_ATTEMPTS {
                send(&state, attempt_on(uri, "198.51.100.7", None)).await;
            }

            assert_eq!(
                send(&state, attempt_on(uri, "198.51.100.7", None))
                    .await
                    .status(),
                StatusCode::TOO_MANY_REQUESTS,
                "{uri} must be metered"
            );
        }
    }

    #[tokio::test]
    async fn the_metered_endpoints_share_one_bucket_per_source() {
        // Otherwise the allowance is multiplied by the number of endpoints, and
        // an attacker simply rotates between them.
        let state = state_with(0);

        for _ in 0..MAX_ATTEMPTS {
            send(
                &state,
                attempt_on("/api/v1/auth/login", "198.51.100.7", None),
            )
            .await;
        }

        assert_eq!(
            send(
                &state,
                attempt_on("/api/v1/auth/change-password", "198.51.100.7", None)
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS,
            "failures at /auth/login count against /auth/change-password"
        );
    }
}
