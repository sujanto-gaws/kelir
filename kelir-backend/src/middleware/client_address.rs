//! Resolving the caller's network address (SRS NFR-SEC-008, FR-AUD-005).
//!
//! Two things key off this value: the authentication rate limiter and the
//! `ip_address` column on every audit row. Both are worthless — worse than
//! worthless, because they look like evidence — if the caller can choose it.
//!
//! `X-Forwarded-For` is written by whoever is talking to us. A proxy *appends*
//! to it, so a value the caller supplies survives as the leftmost element; the
//! only entries that mean anything are the ones our own proxies wrote. This
//! module therefore walks the chain from the right, skipping exactly as many
//! hops as the deployment says it operates
//! ([`AppConfig::trusted_proxy_hops`](crate::config::AppConfig)), and takes the
//! first address it did not write itself.
//!
//! **Trusting nothing is the default.** With no configuration the header is not
//! read at all and the address is the socket peer, which no caller can forge.
//!
//! Hop counting is sound only while the backend cannot be reached except through
//! those proxies. The deployment enforces that by publishing no backend port
//! (deployment §4.1); a topology that opens one needs a CIDR allow-list instead.

use std::fmt;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::error::AppError;
use crate::state::AppState;

/// The header proxies use to carry the original client address.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// The address the request really came from.
///
/// There is no "unknown" case: the socket peer is always available in a served
/// application, and a shared placeholder key would put every caller into one
/// rate-limit bucket — a denial of authentication for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientAddress(IpAddr);

impl ClientAddress {
    #[cfg(test)]
    pub fn new(address: IpAddr) -> Self {
        Self(address)
    }

    /// The rate-limiter key for this caller.
    ///
    /// The address alone, so every metered authentication endpoint shares one
    /// bucket per source: an attacker must not get a fresh allowance by moving
    /// from `/auth/login` to `/auth/refresh`.
    pub fn rate_limit_key(self) -> String {
        self.0.to_string()
    }
}

impl fmt::Display for ClientAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromRequestParts<AppState> for ClientAddress {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(ConnectInfo(peer)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() else {
            // Only reachable if the service was built without connect info; the
            // binary always installs it. Failing closed beats inventing a shared
            // key that would silently re-open the defect this type exists to fix.
            tracing::error!(
                "no peer address on the request: the service must be served with \
                 into_make_service_with_connect_info"
            );

            return Err(AppError::Internal {
                source: anyhow::anyhow!("the client address is unavailable"),
            });
        };

        let peer = peer.ip();
        let forwarded = parts
            .headers
            .get(FORWARDED_FOR)
            .and_then(|value| value.to_str().ok());

        Ok(Self(resolve(
            forwarded,
            peer,
            state.config.trusted_proxy_hops,
        )))
    }
}

/// Picks the rightmost address in the forwarding chain that we did not write.
///
/// The chain is the `X-Forwarded-For` entries, oldest first, followed by the
/// socket peer — the one hop that is always genuine. Everything to the right of
/// the result was added by a proxy we operate; everything to its left is
/// hearsay.
///
/// Falls back to the peer address whenever the header cannot be believed: no
/// trusted hops, no header, an entry that is not an address, or a chain shorter
/// than the configured hop count. Each of those means the request did not arrive
/// the way the configuration says it does, and the peer is the only value that
/// cannot be forged.
fn resolve(forwarded_for: Option<&str>, peer: IpAddr, trusted_hops: usize) -> IpAddr {
    if trusted_hops == 0 {
        return peer;
    }

    let Some(header) = forwarded_for else {
        return peer;
    };

    let mut chain = Vec::new();

    for entry in header.split(',') {
        let Some(address) = parse_entry(entry) else {
            tracing::debug!("ignoring an X-Forwarded-For chain with an unparseable entry");
            return peer;
        };

        chain.push(address);
    }

    chain.push(peer);

    match chain.len().checked_sub(trusted_hops + 1) {
        Some(index) => chain[index],
        None => peer,
    }
}

/// Reads one chain entry, tolerating the port some proxies append.
fn parse_entry(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();

    if let Ok(address) = raw.parse::<IpAddr>() {
        return Some(address);
    }

    // `203.0.113.7:41234` and `[2001:db8::1]:41234`.
    if let Ok(socket) = raw.parse::<SocketAddr>() {
        return Some(socket.ip());
    }

    // A bracketed IPv6 literal with no port.
    raw.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|rest| rest.parse::<IpAddr>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("a test address parses")
    }

    #[test]
    fn without_a_trusted_proxy_the_header_is_ignored() {
        // The default posture. Anyone may send this header; nobody may be
        // believed.
        let peer = ip("198.51.100.9");

        assert_eq!(resolve(Some("203.0.113.7"), peer, 0), peer);
        assert_eq!(resolve(Some("203.0.113.7, 10.0.0.1"), peer, 0), peer);
        assert_eq!(resolve(None, peer, 0), peer);
    }

    #[test]
    fn behind_one_proxy_the_peer_is_the_proxy_and_the_client_is_the_last_entry() {
        // Caddy appends the address it saw, so the entry it wrote is rightmost.
        let caddy = ip("172.18.0.2");

        assert_eq!(
            resolve(Some("203.0.113.7"), caddy, 1),
            ip("203.0.113.7"),
            "the address the proxy wrote is the client"
        );
    }

    #[test]
    fn a_prepended_entry_does_not_become_the_client_address() {
        // The defect this function exists to close: the caller sends a chain of
        // their own, the proxy appends the real address after it, and taking the
        // leftmost element would hand the caller their own rate-limit bucket.
        let caddy = ip("172.18.0.2");

        assert_eq!(
            resolve(Some("1.1.1.1, 2.2.2.2, 203.0.113.7"), caddy, 1),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn a_missing_header_behind_a_proxy_falls_back_to_the_peer() {
        // A direct connection to the backend, bypassing the proxy: the peer is
        // the truth and there is nothing else to read.
        let peer = ip("10.1.2.3");

        assert_eq!(resolve(None, peer, 1), peer);
        assert_eq!(resolve(Some(""), peer, 1), peer);
    }

    #[test]
    fn a_chain_shorter_than_the_configured_hops_falls_back_to_the_peer() {
        // Every entry would be a proxy of ours, so no client entry exists. The
        // leftmost element is caller-supplied and must not be used instead.
        let peer = ip("10.1.2.3");

        assert_eq!(resolve(Some("203.0.113.7"), peer, 2), peer);
        assert_eq!(resolve(Some("203.0.113.7, 172.18.0.2"), peer, 3), peer);
    }

    #[test]
    fn an_unparseable_entry_discredits_the_whole_chain() {
        // "unknown" is legal in X-Forwarded-For and free text is trivial to
        // inject; either way the chain no longer lines up with the hop count.
        let peer = ip("10.1.2.3");

        assert_eq!(resolve(Some("unknown, 203.0.113.7"), peer, 1), peer);
        assert_eq!(resolve(Some("203.0.113.7, not-an-address"), peer, 1), peer);
    }

    #[test]
    fn entries_may_carry_a_port() {
        let caddy = ip("172.18.0.2");

        assert_eq!(
            resolve(Some("203.0.113.7:41234"), caddy, 1),
            ip("203.0.113.7")
        );
        assert_eq!(
            resolve(Some("[2001:db8::1]:41234"), caddy, 1),
            ip("2001:db8::1")
        );
        assert_eq!(resolve(Some("[2001:db8::1]"), caddy, 1), ip("2001:db8::1"));
    }

    #[test]
    fn two_proxies_skip_two_hops() {
        let ingress = ip("172.18.0.2");

        assert_eq!(
            resolve(Some("203.0.113.7, 198.51.100.4"), ingress, 2),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn the_rate_limit_key_is_the_address() {
        let client = ClientAddress::new(ip("203.0.113.7"));

        assert_eq!(client.rate_limit_key(), "203.0.113.7");
        assert_eq!(client.to_string(), "203.0.113.7");
    }
}
