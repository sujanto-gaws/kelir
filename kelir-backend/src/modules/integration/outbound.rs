//! The one place Kelir resolves a secret and calls an external system
//! (FR-INT-002, #547; ADR-0043 §2). Infrastructure: the environment, DNS and
//! the HTTP client. What is allowed is decided in `domain::egress` and
//! `domain::secret`; this module applies it.
//!
//! # Every outbound request is built by [`send`]
//!
//! ADR-0043 §5 asks a reviewer to check exactly this, so it is said once
//! here: [`send`] builds a client **per call** with
//!
//! * `redirect::Policy::none()` — a `3xx` is the answer, and its `Location` is
//!   a destination the guard never saw;
//! * `no_proxy()` — a proxy from the environment would carry the call to an
//!   address other than the one checked;
//! * a resolver that answers **only the checked addresses, and only for the
//!   call's own host** ([`PinnedResolver`]) — so the connection goes where the
//!   guard looked, and a second DNS answer (rebinding) is never asked for.
//!   Every address the name resolved to passed the guard, so every one is
//!   pinned, in the resolver's order: a dual-stack name whose first answer
//!   does not connect is tried at the next, as the system resolver would;
//! * the system's `timeout_seconds` as the request timeout, inside a deadline
//!   the caller already holds over resolution and the body read.
//!
//! TLS still verifies the certificate against the **host name**: pinning the
//! address does not weaken `https`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use url::{Host, Url};

use super::domain::egress::EgressPolicy;
use super::domain::secret::{Secret, SecretReference, SecretReferenceError, TenantNamespaces};
use super::domain::test_call::{TestCallError, MAX_BODY_BYTES};
use super::domain::HttpMethod;

/// The header a test call carries its correlation id in, so the called
/// system's own log can be matched with the `integration_logs` row
/// (architectures/03 rule 4).
pub const CORRELATION_HEADER: &str = "X-Correlation-Id";

/// Resolves a `secret_reference` for one call.
///
/// * `env://NAME` reads `NAME` from this process's environment **only if it
///   is one of the caller's tenant's names**,
///   `KELIR_INTEGRATION_SECRET_<CODE>__<NAME>`, and no other live tenant's
///   ([`TenantNamespaces::admits`], #618). Any other name is
///   [`TestCallError::SecretNameNotPermitted`], naming the caller's prefix,
///   and the environment is not read, so a foreign variable answers the same
///   set or unset. Unset, empty or not Unicode is
///   [`TestCallError::SecretNotFound`], reachable only inside the caller's
///   namespace.
/// * `vault://…` is [`TestCallError::SecretBackendNotConfigured`]: the product
///   owner's answer 1, with a HashiCorp Vault KV v2 client named as its
///   successor (ADR-0043 §2).
///
/// `namespaces` is read from `tenants` by the caller's `tenant_id` before this
/// is called; this function reads nothing but the environment.
pub fn resolve_secret(
    reference: &str,
    namespaces: &TenantNamespaces,
) -> Result<Secret, TestCallError> {
    match SecretReference::parse(reference) {
        Ok(SecretReference::Environment { name }) => {
            if !namespaces.admits(name) {
                return Err(TestCallError::SecretNameNotPermitted {
                    prefix: namespaces.caller_prefix(),
                });
            }

            match std::env::var(name) {
                Ok(value) if !value.is_empty() => Ok(Secret::new(value)),
                _ => Err(TestCallError::SecretNotFound {
                    name: name.to_owned(),
                }),
            }
        }
        Ok(SecretReference::Vault { .. }) => Err(TestCallError::SecretBackendNotConfigured),
        Err(SecretReferenceError::Malformed) => Err(TestCallError::SecretReferenceMalformed),
    }
}

/// Where the connection goes: the addresses that passed the guard, in the
/// order the lookup gave them. Never empty.
#[derive(Debug, Clone)]
pub struct Pinned {
    pub addresses: Vec<IpAddr>,
}

/// How a host name becomes addresses: the system resolver, with the test
/// seam's table in front of it (`AppConfig::integration_dns_overrides`, which
/// nothing outside the test harness fills).
#[derive(Debug, Clone, Copy)]
pub struct Lookup<'a> {
    pub overrides: &'a HashMap<String, Vec<IpAddr>>,
}

impl Lookup<'_> {
    async fn addresses(&self, name: &str, port: u16) -> Result<Vec<IpAddr>, TestCallError> {
        if let Some(addresses) = self.overrides.get(&name.to_ascii_lowercase()) {
            return Ok(addresses.clone());
        }

        Ok(tokio::net::lookup_host((name, port))
            .await
            .map_err(|_| TestCallError::HostNotResolved)?
            .map(|socket| socket.ip())
            .collect())
    }
}

/// Resolves the URL's host — once — and holds every answer to `policy`.
///
/// An IP literal is its own answer. A name is looked up through `lookup`;
/// **every** address it returns must pass, and the connection is pinned to all
/// of them.
pub async fn resolve_and_check(
    url: &Url,
    policy: &EgressPolicy,
    lookup: Lookup<'_>,
) -> Result<Pinned, TestCallError> {
    let port = url
        .port_or_known_default()
        .ok_or(TestCallError::TargetUrlInvalid)?;

    let addresses: Vec<IpAddr> = match url.host() {
        Some(Host::Ipv4(v4)) => vec![IpAddr::V4(v4)],
        Some(Host::Ipv6(v6)) => vec![IpAddr::V6(v6)],
        Some(Host::Domain(name)) => lookup.addresses(name, port).await?,
        None => return Err(TestCallError::TargetUrlInvalid),
    };

    match policy.choose(&addresses) {
        Ok(addresses) if addresses.is_empty() => Err(TestCallError::HostNotResolved),
        Ok(addresses) => Ok(Pinned { addresses }),
        Err(refusal) => Err(TestCallError::EgressRefused(refusal.class)),
    }
}

/// What came back, read to at most [`MAX_BODY_BYTES`].
pub struct RawAnswer {
    pub status_code: u16,
    pub body: Vec<u8>,
    /// The body was longer than what was read.
    pub body_cut: bool,
}

/// Sends one request to `url`, connecting only to `pinned`'s addresses.
///
/// `authorization` is the whole header value, marked sensitive so the client's
/// own debug output does not print it. No other caller-controlled header is
/// sent, and no body: a test call is the endpoint's method with nothing in it.
pub async fn send(
    method: HttpMethod,
    url: &Url,
    pinned: &Pinned,
    authorization: Option<&Secret>,
    correlation_id: &str,
    timeout: Duration,
) -> Result<RawAnswer, TestCallError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(timeout)
        .pool_max_idle_per_host(0);

    if let Some(Host::Domain(name)) = url.host() {
        builder = builder.dns_resolver(Arc::new(PinnedResolver {
            host: name.to_ascii_lowercase(),
            addresses: pinned.addresses.clone(),
        }));
    }

    let client = builder
        .build()
        .map_err(|_| TestCallError::UpstreamUnreachable)?;

    let method = match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
    };

    let mut request = client
        .request(method, url.clone())
        .header(CORRELATION_HEADER, correlation_id);

    if let Some(value) = authorization {
        // `from_str` refuses a control character; `domain::secret` has already
        // refused one, so this failing means the two rules drifted.
        let mut header = HeaderValue::from_str(value.expose()).map_err(|_| {
            TestCallError::SecretMalformed("contains a character a header cannot carry")
        })?;
        header.set_sensitive(true);
        request = request.header(AUTHORIZATION, header);
    }

    let mut response = request.send().await.map_err(upstream)?;
    let status_code = response.status().as_u16();

    let mut body = Vec::new();
    let mut body_cut = false;

    while let Some(chunk) = response.chunk().await.map_err(upstream)? {
        let room = MAX_BODY_BYTES - body.len();

        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            body_cut = true;
            break;
        }
        body.extend_from_slice(&chunk);
    }

    Ok(RawAnswer {
        status_code,
        body,
        body_cut,
    })
}

/// A client error as the failure a caller is told. The error's own text stays
/// in the server log: it names the URL, which the caller already has, and
/// nothing a secret is part of.
fn upstream(error: reqwest::Error) -> TestCallError {
    tracing::warn!(error = %error, "an integration test call got no answer");

    if error.is_timeout() {
        // The seconds are filled in by the caller, which knows the system.
        TestCallError::UpstreamTimeout { seconds: 0 }
    } else {
        TestCallError::UpstreamUnreachable
    }
}

/// A resolver that knows one name and the addresses that passed the guard.
///
/// Installed on the per-call client in place of the system resolver, so the
/// client cannot look the host up a second time and get a different answer.
/// Any other name — there should be none — is refused rather than resolved.
/// (reqwest's own `resolve_to_addrs` pins a name the same way, but lets every
/// other name through to the system resolver.)
///
/// The client tries the addresses in order, dividing its connect timeout
/// between them, so an address that does not answer costs its share and not
/// the call.
struct PinnedResolver {
    host: String,
    addresses: Vec<IpAddr>,
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let answer: Result<Addrs, Box<dyn std::error::Error + Send + Sync>> =
            if name.as_str().eq_ignore_ascii_case(&self.host) {
                // Port 0: the client uses the URL's port.
                let addresses: Vec<SocketAddr> = self
                    .addresses
                    .iter()
                    .map(|address| SocketAddr::new(*address, 0))
                    .collect();
                Ok(Box::new(addresses.into_iter()))
            } else {
                Err("only the checked host is resolved for an integration call".into())
            };

        Box::pin(std::future::ready(answer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::integration::domain::egress::AddressClass;

    /// The system tenant, with `TNT-001` live beside it.
    fn system() -> TenantNamespaces {
        let caller = uuid::Uuid::now_v7();
        TenantNamespaces::for_caller(
            caller,
            vec![
                (caller, "SYSTEM".to_owned()),
                (uuid::Uuid::now_v7(), "TNT-001".to_owned()),
            ],
        )
        .expect("the caller is live")
    }

    fn refused() -> Result<(), TestCallError> {
        Err(TestCallError::SecretNameNotPermitted {
            prefix: "KELIR_INTEGRATION_SECRET_SYSTEM__".to_owned(),
        })
    }

    #[test]
    fn a_vault_reference_is_refused_by_name() {
        assert_eq!(
            resolve_secret("vault://kelir/erp/api-key#token", &system()).map(|_| ()),
            Err(TestCallError::SecretBackendNotConfigured)
        );
    }

    #[test]
    fn an_unset_environment_variable_is_named() {
        assert_eq!(
            resolve_secret(
                "env://KELIR_INTEGRATION_SECRET_SYSTEM__UNIT_SURELY_UNSET_9B1",
                &system()
            )
            .map(|_| ()),
            Err(TestCallError::SecretNotFound {
                name: "KELIR_INTEGRATION_SECRET_SYSTEM__UNIT_SURELY_UNSET_9B1".to_owned()
            })
        );
    }

    #[test]
    fn a_name_outside_the_prefix_is_refused_before_the_environment_is_read() {
        // PATH is set in every environment this runs in, so a refusal here is
        // the prefix and not a missing variable.
        assert!(std::env::var("PATH").is_ok());

        assert_eq!(
            resolve_secret("env://PATH", &system()).map(|_| ()),
            refused()
        );
    }

    #[test]
    fn another_tenants_name_is_refused_alike_set_or_unset() {
        // The tenant check comes before the environment is read, so a set
        // foreign variable and an unset one are the same refusal.
        let set = "KELIR_INTEGRATION_SECRET_TNT_001__UNIT_SET_4C1";
        std::env::set_var(set, "kelir-unit-foreign-value");

        assert_eq!(
            resolve_secret(&format!("env://{set}"), &system()).map(|_| ()),
            refused()
        );
        assert_eq!(
            resolve_secret(
                "env://KELIR_INTEGRATION_SECRET_TNT_001__UNIT_SURELY_UNSET_4C2",
                &system()
            )
            .map(|_| ()),
            refused()
        );
    }

    #[test]
    fn a_stored_value_that_is_not_a_reference_is_not_read() {
        assert_eq!(
            resolve_secret("PATH", &system()).map(|_| ()),
            Err(TestCallError::SecretReferenceMalformed)
        );
    }

    #[tokio::test]
    async fn an_ip_literal_is_judged_without_a_lookup() {
        let policy = EgressPolicy::default();

        for (url, class) in [
            ("http://127.0.0.1:8080/x", AddressClass::Loopback),
            ("http://[::ffff:127.0.0.1]/x", AddressClass::Loopback),
            (
                "http://169.254.169.254/latest/meta-data",
                AddressClass::LinkLocal,
            ),
            ("http://[fe80::1]/x", AddressClass::LinkLocal),
            // #622: metadata addresses outside link-local, and `::a.b.c.d`.
            ("http://100.100.100.200/x", AddressClass::Metadata),
            ("http://192.0.0.192/x", AddressClass::Metadata),
            ("http://[fd00:ec2::254]/x", AddressClass::Metadata),
            ("http://[::127.0.0.1]/x", AddressClass::Loopback),
            ("http://10.0.0.1/x", AddressClass::Private),
            // WHATWG URL parsing normalises these to 127.0.0.1 before the
            // guard sees them.
            ("http://2130706433/x", AddressClass::Loopback),
            ("http://127.1/x", AddressClass::Loopback),
        ] {
            let url = Url::parse(url).expect("a URL");
            assert_eq!(
                resolve_and_check(&url, &policy, no_overrides())
                    .await
                    .map(|p| p.addresses),
                Err(TestCallError::EgressRefused(class)),
                "{url}"
            );
        }
    }

    #[tokio::test]
    async fn localhost_is_resolved_and_refused_as_loopback() {
        let url = Url::parse("http://localhost:9/x").expect("a URL");

        assert_eq!(
            resolve_and_check(&url, &EgressPolicy::default(), no_overrides())
                .await
                .map(|p| p.addresses),
            Err(TestCallError::EgressRefused(AddressClass::Loopback))
        );
    }

    fn no_overrides() -> Lookup<'static> {
        static EMPTY: std::sync::OnceLock<HashMap<String, Vec<IpAddr>>> =
            std::sync::OnceLock::new();
        Lookup {
            overrides: EMPTY.get_or_init(HashMap::new),
        }
    }

    fn ip(raw: &str) -> IpAddr {
        raw.parse().expect("an address")
    }

    #[tokio::test]
    async fn an_overridden_name_is_judged_on_every_address_it_is_given() {
        let overrides = HashMap::from([
            (
                "mixed.kelir.test".to_owned(),
                vec![ip("203.0.113.7"), ip("10.0.0.5")],
            ),
            (
                "public.kelir.test".to_owned(),
                vec![ip("203.0.113.7"), ip("2001:db8::7"), ip("203.0.113.7")],
            ),
            ("empty.kelir.test".to_owned(), Vec::new()),
        ]);
        let lookup = Lookup {
            overrides: &overrides,
        };
        let policy = EgressPolicy::default();

        let mut answers = Vec::new();
        for raw in [
            "https://MIXED.kelir.test/x",
            "https://public.kelir.test/x",
            "https://empty.kelir.test/x",
        ] {
            let url = Url::parse(raw).expect("a URL");
            answers.push(
                resolve_and_check(&url, &policy, lookup)
                    .await
                    .map(|p| p.addresses),
            );
        }

        assert_eq!(
            answers,
            vec![
                // A private second answer refuses the name.
                Err(TestCallError::EgressRefused(AddressClass::Private)),
                // Every answer is pinned, in order, once.
                Ok(vec![ip("203.0.113.7"), ip("2001:db8::7")]),
                Err(TestCallError::HostNotResolved),
            ]
        );
    }

    #[tokio::test]
    async fn the_pinned_resolver_answers_its_host_only() {
        let resolver = PinnedResolver {
            host: "erp.example.com".to_owned(),
            addresses: vec![ip("203.0.113.7"), ip("2001:db8::7")],
        };

        let answer: Vec<SocketAddr> = resolver
            .resolve("ERP.example.com".parse().expect("a name"))
            .await
            .expect("its own host resolves")
            .collect();
        assert_eq!(
            answer,
            vec![
                "203.0.113.7:0".parse().expect("an address"),
                "[2001:db8::7]:0".parse().expect("an address"),
            ]
        );

        assert!(resolver
            .resolve("other.example.com".parse().expect("a name"))
            .await
            .is_err());
    }
}
