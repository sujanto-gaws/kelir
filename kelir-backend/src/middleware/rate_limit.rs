//! Rate limiting for authentication endpoints (SRS NFR-SEC-008).
//!
//! Account lockout already stops repeated guesses at *one* account. This stops
//! the other shape of the same attack: many attempts spread across many
//! accounts from one source, which lockout never sees because no single account
//! reaches its threshold.
//!
//! **In-memory, so per-instance.** With several replicas the effective limit is
//! multiplied by the replica count. That is acceptable while staging and
//! production are single-instance (deployment §4.1); a shared store is the fix
//! when they are not, and the limit is small enough that even a multiplied
//! version is far below what a credential-stuffing run needs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Attempts allowed per key within [`WINDOW`].
///
/// Above the 5-failure account lockout, so a person mistyping their password
/// hits the lockout — which explains itself — rather than a bare 429.
pub const MAX_ATTEMPTS: u32 = 10;

/// The sliding window attempts are counted in.
pub const WINDOW: Duration = Duration::from_secs(60);

/// How long a key stays blocked once it exceeds the limit.
pub const BLOCK_DURATION: Duration = Duration::from_secs(15 * 60);

struct Attempts {
    count: u32,
    window_started: Instant,
    blocked_until: Option<Instant>,
}

/// Fixed-window counter keyed by caller.
///
/// A fixed window can allow up to twice the limit across a boundary. That is a
/// real imprecision and an acceptable one here: the purpose is to stop
/// automated volume, not to meter precisely, and the simplicity means no
/// background task and no unbounded growth beyond what [`prune`] handles.
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

    /// Records an attempt and says whether it may proceed.
    pub fn check(&self, key: &str) -> Decision {
        self.check_at(key, Instant::now())
    }

    /// The clock is a parameter so the window and block behaviour can be tested
    /// without sleeping.
    fn check_at(&self, key: &str, now: Instant) -> Decision {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Opportunistic cleanup: without it the map grows with every distinct
        // key seen, which an attacker controls by varying the username.
        if entries.len() > 10_000 {
            entries.retain(|_, attempts| !attempts.is_stale(now));
        }

        let attempts = entries.entry(key.to_owned()).or_insert(Attempts {
            count: 0,
            window_started: now,
            blocked_until: None,
        });

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

        if now.duration_since(attempts.window_started) >= WINDOW {
            attempts.count = 0;
            attempts.window_started = now;
        }

        attempts.count += 1;

        if attempts.count > MAX_ATTEMPTS {
            attempts.blocked_until = Some(now + BLOCK_DURATION);

            return Decision::Block {
                retry_after_seconds: BLOCK_DURATION.as_secs(),
            };
        }

        Decision::Allow
    }

    /// Forgets a key — called after a successful sign-in, so one person's typo
    /// streak does not count against them once they get it right.
    pub fn reset(&self, key: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(key);
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Attempts {
    fn is_stale(&self, now: Instant) -> bool {
        let past_block = self.blocked_until.is_none_or(|until| now >= until);
        let past_window = now.duration_since(self.window_started) >= WINDOW;

        past_block && past_window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit() {
        let limiter = RateLimiter::new();

        for attempt in 1..=MAX_ATTEMPTS {
            assert_eq!(
                limiter.check("1.2.3.4"),
                Decision::Allow,
                "attempt {attempt} should be allowed"
            );
        }
    }

    #[test]
    fn blocks_past_the_limit() {
        let limiter = RateLimiter::new();

        for _ in 0..MAX_ATTEMPTS {
            limiter.check("1.2.3.4");
        }

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

        for _ in 0..=MAX_ATTEMPTS {
            limiter.check("1.2.3.4");
        }

        assert_eq!(limiter.check("5.6.7.8"), Decision::Allow);
    }

    #[test]
    fn the_counter_resets_after_the_window() {
        let limiter = RateLimiter::new();
        let start = Instant::now();

        for _ in 0..MAX_ATTEMPTS {
            limiter.check_at("1.2.3.4", start);
        }

        assert_eq!(limiter.check_at("1.2.3.4", start + WINDOW), Decision::Allow);
    }

    #[test]
    fn a_block_expires() {
        let limiter = RateLimiter::new();
        let start = Instant::now();

        for _ in 0..=MAX_ATTEMPTS {
            limiter.check_at("1.2.3.4", start);
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
    fn a_successful_sign_in_clears_the_count() {
        let limiter = RateLimiter::new();

        for _ in 0..MAX_ATTEMPTS {
            limiter.check("1.2.3.4");
        }
        limiter.reset("1.2.3.4");

        assert_eq!(limiter.check("1.2.3.4"), Decision::Allow);
    }

    #[test]
    fn the_limit_sits_above_the_account_lockout_threshold() {
        // A person mistyping their password should meet the lockout, which
        // explains itself, rather than an opaque 429.
        assert!(MAX_ATTEMPTS > crate::modules::auth::service::MAX_FAILED_LOGINS as u32);
    }
}
