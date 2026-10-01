//! An administrator's test call to an integration endpoint (FR-INT-002, #547;
//! ADR-0043), through the API, against a mock system on this machine.
//!
//! # The test seam
//!
//! The mock listens on `127.0.0.1`, and loopback is an address the egress
//! guard always refuses. So the tests that need an answer start the app with
//! `AppConfig::integration_allow_loopback = true` through
//! [`TestApp::spawn_with`] — a field `AppConfig::from_env` never sets, which
//! opens loopback and nothing else (`domain::egress`'s unit tests hold that).
//! **The tests that prove the guard use the harness default**, which is the
//! production default: no seam, no allow-list.
//!
//! A second code-only seam, `AppConfig::integration_dns_overrides`, answers a
//! chosen host name with chosen addresses ahead of the system resolver. The
//! tests of the every-address rule, the pinning and the fall-through to a
//! second address use it; the guard judges its answers like any other.
//!
//! # A planted secret
//!
//! Each test that resolves a secret sets its own environment variable (a
//! unique name, so parallel tests do not share one) to a value that appears
//! nowhere else, and the mock **echoes back the `Authorization` header it
//! received** — the worst case, a system that repeats the secret in its body.
//! The assertions then read the response, the whole `integration_logs` row and
//! every `audit_events` row as text, and look for the value in every form it
//! was sent in.
//!
//! # Seen to fail (coding standard §2.9)
//!
//! Each mutation was made, this file run, the named tests observed red, and
//! the mutation reverted. **Seen red, 2026-09-29.**
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `domain::secret::environment_name_is_resolvable` accepts any non-empty name | `a_name_outside_the_integration_prefix_is_refused_and_its_value_goes_nowhere` |
//! | `EgressPolicy::check` answers `Ok` for every class | `loopback_is_refused_in_every_spelling_and_nothing_is_sent`, `the_metadata_address_and_an_unlisted_private_address_are_refused`, `a_listed_private_range_passes_the_guard` |
//! | `domain::test_call::redact` returns its input | `a_bearer_secret_reaches_the_system_and_nothing_kelir_keeps_or_returns`, `a_basic_pair_is_sent_as_basic_and_its_password_is_nowhere` |
//! | `redirect(Policy::none())` removed from `outbound::send` | `a_redirect_is_the_answer_and_its_location_is_not_requested` |
//! | The outer `tokio::time::timeout` alone widened to 60 s | green — the client's own timeout held; two layers |
//! | Both layers widened to 60 s (`budget` in `service::test_call`) | `a_slow_system_fails_at_its_timeout` |
//! | `tracing::info!(authorization = value.expose(), ..)` added in `outbound::send` | `no_log_line_carries_the_secret_even_at_trace` |
//! | E7b: `resolve_and_check` passes only the first address to `EgressPolicy::choose` | `a_name_resolving_to_a_public_and_a_private_address_is_refused`, `a_first_address_that_does_not_connect_falls_through_to_the_next` |
//! | PR1: `outbound::send` never installs the `PinnedResolver` | `the_connection_goes_to_the_checked_address_and_not_a_second_lookup`, `a_first_address_that_does_not_connect_falls_through_to_the_next` |
//! | `PinnedResolver` answers only the first pinned address | `a_first_address_that_does_not_connect_falls_through_to_the_next` |
//! | `domain::test_call::without_nul` returns its input | `a_nul_in_the_answer_is_stored_and_answered` |
//! | `domain::secret::encoded_forms` returns nothing | `an_encoded_echo_of_the_secret_is_redacted` |
//! | `domain::test_call::mask_text` returns its input | `a_json_body_over_the_read_cap_is_masked_by_key` |
//!
//! **#618's predicates, seen red 2026-10-01**, each also reddening
//! `domain::secret`'s or `outbound`'s unit tests. Before the fix, the first
//! four tests below were red on the prefix-only rule: P1 answered `200` and
//! the collector received the system tenant's bearer.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `TenantNamespaces::admits` keeps only the #547 prefix check | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `another_tenants_variable_answers_the_same_set_or_unset`, `a_name_two_tenants_codes_both_map_to_is_refused_for_both`, `a_single_tenant_deployment_reads_only_its_tenants_namespace` |
//! | `namespace_segment` does not write `-` as `_` | the first three above, and `a_caller_in_another_tenant_calls_their_own_endpoint_and_neither_reaches_the_other` |
//! | `namespace_segment` does not upper-case the code | unit tests only: stored codes are upper case |
//! | The name compared case-insensitively | unit tests only |
//! | The ambiguity refusal removed | `a_name_two_tenants_codes_both_map_to_is_refused_for_both` |
//! | The environment read before the tenant check | `another_tenants_variable_answers_the_same_set_or_unset` |
//! | `NAMESPACE_SEPARATOR` is one underscore | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `a_single_tenant_deployment_reads_only_its_tenants_namespace` |
//! | The caller's own gate dropped, the ambiguity rule kept | `a_single_tenant_deployment_reads_only_its_tenants_namespace` |
//! | The refusal names the bare prefix, not the caller's | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `a_single_tenant_deployment_reads_only_its_tenants_namespace` |
//!
//! **The test-engineer campaign on #618, seen red 2026-10-01**: twenty-two
//! mutations the table above does not list. A row that names tests was made,
//! `--lib modules::integration`, this file and `integration_logs` run, the
//! named tests observed red, and the mutation reverted; most also redden
//! `domain::secret`'s or `outbound`'s unit tests. A row that says green left
//! all three suites green, and says why.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `organization::repository::live_codes` drops `deleted_at IS NULL` | `a_tenant_that_is_not_active_still_holds_its_names_and_a_deleted_one_does_not`. Until #650 also `a_deleted_tenants_open_session_resolves_no_secret_its_own_or_another`; run again 2026-10-01 after it, that test is green under this mutation, because a deleted tenant's caller is now a 401 before the namespace is read |
//! | `live_codes` adds `AND status = 'ACTIVE'` | `a_tenant_that_is_not_active_still_holds_its_names_and_a_deleted_one_does_not` |
//! | `service::test_call` takes the first live tenant's code for the caller's | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `another_tenants_variable_answers_the_same_set_or_unset`, `a_name_two_tenants_codes_both_map_to_is_refused_for_both`, `a_caller_in_another_tenant_calls_their_own_endpoint_and_neither_reaches_the_other` |
//! | `-` admitted in a name after the separator | unit tests only. No request reaches it: `SecretReference::parse` refuses the shape first, which `a_stored_reference_outside_the_name_alphabet_is_malformed_and_reads_nothing` holds |
//! | Lower case admitted in a name after the separator | unit tests only, for the same reason |
//! | A tenant's prefix with no name after it admitted | `a_tenants_prefix_with_nothing_after_it_is_refused_set_or_not` |
//! | The separator dropped from the match | `a_tenants_prefix_with_nothing_after_it_is_refused_set_or_not`, `a_code_ending_in_an_underscore_and_the_code_without_it_share_no_name` |
//! | Ambiguity only between codes that map to the same segment | `a_name_two_tenants_codes_both_map_to_is_refused_for_both`, `a_code_ending_in_an_underscore_and_the_code_without_it_share_no_name` |
//! | Ambiguity only against a code longer than the caller's | `a_name_two_tenants_codes_both_map_to_is_refused_for_both`, `a_code_ending_in_an_underscore_and_the_code_without_it_share_no_name`, `a_tenant_that_is_not_active_still_holds_its_names_and_a_deleted_one_does_not`, `a_code_no_route_stores_reads_upper_case_names_or_nothing` |
//! | The refusal lists the other tenants' codes | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing` |
//! | The refusal repeats the refused name | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `another_tenants_variable_answers_the_same_set_or_unset` |
//! | A refused name returns before its `integration_logs` row is written | `a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing`, `another_tenants_variable_answers_the_same_set_or_unset`, `a_name_two_tenants_codes_both_map_to_is_refused_for_both`, `a_single_tenant_deployment_reads_only_its_tenants_namespace`, `a_name_outside_the_integration_prefix_is_refused_and_its_value_goes_nowhere` |
//! | A `vault://` reference refused as `SECRET_NAME_NOT_PERMITTED` | `a_vault_reference_fails_named_and_writes_one_log_row` |
//! | `SECRET_NOT_FOUND` names the caller's prefix, not the variable | `another_tenants_variable_answers_the_same_set_or_unset` |
//! | `admits`' length check on the #547 prefix removed | green — equivalent: the tenant prefix begins with the #547 prefix and is longer |
//! | `admits`' `starts_with` on the #547 prefix removed | green — equivalent, for the same reason |
//! | `std::env::var` called before `admits`, its answer dropped on a refusal | green — the answer is the same bytes, so no request observes it; the order is held by review of `outbound::resolve_secret` |
//! | A caller whose tenant is not live is given the first live tenant's code | **green since #650**, run again 2026-10-01: all three suites. No request reaches the branch any more — a deleted tenant's token is refused by `middleware::auth` first — so it is left only to a tenant deleted between that read and the namespace read, which no test can stage. Held by review of `service::test_call`; `domain::secret`'s unit test holds only that `TenantNamespaces::for_caller` answers `None`. Until #650 this reddened `a_deleted_tenants_open_session_resolves_no_secret_its_own_or_another` |
//! | `SecretReference::parse` skips the shape check | `a_stored_reference_outside_the_name_alphabet_is_malformed_and_reads_nothing` |
//! | An empty value is a secret (`outbound::resolve_secret`) | `a_variable_set_to_nothing_is_not_found_and_nothing_is_sent` |
//! | `namespace_segment` does not upper-case the code | `a_code_no_route_stores_reads_upper_case_names_or_nothing` — the builder's row above, now reached through a code written past the route |
//! | Ambiguity judged on the other tenant's prefix alone, its name allowed to be empty | green — refuses more, not less: only a name that is exactly another tenant's prefix differs |
//!
//! **#622's predicates, seen red 2026-10-01.** Before the fix, the three
//! tests the first row names were red on the unchanged guard: `100.100.100.200`
//! answered `UPSTREAM_TIMEOUT` and `::127.0.0.1` `UPSTREAM_UNREACHABLE`, as
//! verification record 20's P4 found. Each mutation was made, `--lib
//! modules::integration utils::cidr` and this file run, the named tests
//! observed red, and the mutation reverted. `egress::` and `cidr::` name unit
//! tests in `domain::egress` and `utils::cidr`.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | The metadata arm removed from `egress::classify` | `a_cloud_metadata_address_is_refused_whatever_is_listed`, `a_name_resolving_to_a_metadata_address_is_refused`, `an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries`; two `egress::` tests |
//! | `100.100.100.200` dropped from `METADATA_ADDRESSES` | the same three; two `egress::` tests |
//! | `192.0.0.192` dropped from it | `a_cloud_metadata_address_is_refused_whatever_is_listed`; `egress::a_cloud_metadata_address_is_refused_whatever_is_listed` |
//! | `fd00:ec2::254` dropped from it | the same two |
//! | The arm reads the address before `canonical` unwraps it | `a_cloud_metadata_address_is_refused_whatever_is_listed`, `an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries`; two `egress::` tests |
//! | `EgressPolicy::check` lets a listed range open `Metadata` as it opens `Private` | the same two; two `egress::` tests |
//! | The arm widened to all of `fd00:ec2::/32` | `a_cloud_metadata_address_is_refused_whatever_is_listed`, at its `fd00:ec2::253` control; `egress::the_addresses_beside_a_metadata_address_keep_their_class` |
//! | The arm widened to `100.100.100.0/24` and `192.0.0.0/24` | unit only: `egress::the_addresses_beside_a_metadata_address_keep_their_class`. No request here names an IPv4 neighbour |
//! | `cidr::ipv4_compatible` answers `None` | `an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries`; two `egress::` tests and `cidr::a_v4_network_contains_the_compatible_form_of_its_addresses` |
//! | `ipv4_compatible` unwraps `::1` | `loopback_is_refused_in_every_spelling_and_nothing_is_sent` (`localhost` and `[::1]` read as unspecified); three `egress::` tests, `outbound::localhost_is_resolved_and_refused_as_loopback`, `cidr::ipv6_loopback_and_unspecified_are_not_ipv4_compatible_addresses` |
//! | `ipv4_compatible` unwraps `::` | unit only: `cidr::ipv6_loopback_and_unspecified_are_not_ipv4_compatible_addresses`. Equivalent to the guard: `0.0.0.0` is unspecified too |
//! | `ipv4_compatible` reads `::/80`, not `::/96` | unit only: `egress::an_ipv4_compatible_address_is_judged_as_the_address_it_carries`, `cidr::ipv6_loopback_and_unspecified_are_not_ipv4_compatible_addresses` |
//! | `TestCallError`'s message offers `KELIR_INTEGRATION_ALLOWED_CIDRS` for every class but loopback | `a_cloud_metadata_address_is_refused_whatever_is_listed`; no unit test |
//!
//! **The test-engineer campaign on #622, seen red 2026-10-01**: thirteen
//! mutations, eleven the table above does not list and two of its rows run
//! again. Each was made, `--lib modules::integration utils::cidr`, this file
//! and `integration_logs` run, the named tests observed red, and the mutation
//! reverted. **The first four left every test of the builder's green**, and
//! redden only the campaign's tests at the foot of this file.
//!
//! | Mutation | Reddened |
//! |---|---|
//! | `EgressPolicy::check`: the test seam opens `Metadata` as it opens `Loopback` | `the_test_seam_does_not_open_a_metadata_address`; no unit test |
//! | `check`: a listed `/32` or `/128` host route opens `Metadata` | `a_metadata_address_is_refused_in_every_spelling_a_url_host_can_take`, `a_name_resolving_to_a_metadata_address_in_any_form_is_refused`; no unit test |
//! | `TestCallError`'s message offers the setting for link-local, `169.254.169.254` included | `a_metadata_address_is_refused_in_every_spelling_a_url_host_can_take`, `the_ipv4_compatible_range_ends_where_it_is_written_to`; no unit test |
//! | `cidr::ipv4_compatible` leaves `::2` alone with `::` and `::1` | `the_ipv4_compatible_range_ends_where_it_is_written_to`; no unit test |
//! | `METADATA_ADDRESSES`: AWS's last group written `254`, not `0x0254` | `a_cloud_metadata_address_is_refused_whatever_is_listed` and four campaign tests; `egress::a_cloud_metadata_address_is_refused_whatever_is_listed`, `outbound::an_ip_literal_is_judged_without_a_lookup` |
//! | `ipv4_compatible` reads `::/97`: addresses from `128.0.0.0` up are not unwrapped | `an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries` and three campaign tests; one `egress::` test |
//! | `ipv4_compatible` reads `::/95` | `the_ipv4_compatible_range_ends_where_it_is_written_to`; one `egress::` and one `cidr::` test. No test of the builder's here |
//! | `ipv4_compatible` skips the addresses that carry `0.0.0.0/8` | `an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries`, `the_ipv4_compatible_range_ends_where_it_is_written_to`; one `egress::` test |
//! | `Cidr::contains` unwraps the mapped form and not the compatible one | `the_ipv4_compatible_range_ends_where_it_is_written_to`; one `egress::` and one `cidr::` test. No test of the builder's here |
//! | `Cidr::contains` reads the address as written, neither form unwrapped | the same campaign test; two `egress::` and two `cidr::` tests. No test of the builder's here |
//! | `Cidr::contains` answers yes across families | the same campaign test, at a list entry written `::10.255.254.0/120`; one `egress::` and three `cidr::` tests. No test of the builder's here |
//! | The builder's row again: the arm widened to `100.100.100.0/24` and `192.0.0.0/24` | now also `the_addresses_beside_a_metadata_address_are_judged_by_their_range`: a request names each IPv4 neighbour |
//! | The builder's row again: `ipv4_compatible` reads `::/80` | now also `the_ipv4_compatible_range_ends_where_it_is_written_to` |
//!
//! **What the guard does not unwrap**, probed and left as ADR-0043 §2 and §6
//! decide it: NAT64 (`64:ff9b::/96`, and the local-use `64:ff9b:1::/48`),
//! 6to4 (`2002::/16`), Teredo (`2001::/32`), ISATAP under a public prefix and
//! the IPv4-translated `::ffff:0:a.b.c.d` are public whatever IPv4 address
//! they carry, the four metadata addresses included. No test here holds that
//! either way: it is a decision, and its revisit trigger is the ADR's.

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use common::{fixtures, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

const BASE: &str = "/api/v1/integration/external-systems";
const PASSWORD: &str = "integration-test-call-password";
const TENANTS: &str = "/api/v1/organization/tenants";
const TENANT_PASSWORD: &str = "a-sufficiently-long-password";

// ---------------------------------------------------------------------------
// The mock system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    /// The `Host` header: the name the call was addressed to, whatever
    /// address it connected to.
    host: Option<String>,
    authorization: Option<String>,
    correlation_id: Option<String>,
}

struct Mock {
    address: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    async fn start() -> Self {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let recorder = seen.clone();

        let router = Router::new().fallback(move |request: Request| {
            let recorder = recorder.clone();
            async move { answer(request, recorder).await }
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the mock system");
        let address = listener.local_addr().expect("its address");

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("the mock serves");
        });

        Self { address, seen }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the mock's record").clone()
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn answer(request: Request, recorder: Arc<Mutex<Vec<Seen>>>) -> Response {
    let authorization = header_text(request.headers(), "authorization");
    let path = request.uri().path().to_owned();

    recorder.lock().expect("the mock's record").push(Seen {
        method: request.method().to_string(),
        path: path.clone(),
        host: header_text(request.headers(), "host"),
        authorization: authorization.clone(),
        correlation_id: header_text(request.headers(), "x-correlation-id"),
    });

    let echoed = authorization.unwrap_or_default();

    match path.as_str() {
        // The worst case: the secret comes back in the body, in JSON, beside a
        // key the preview masks whatever it holds.
        "/echo" => axum::Json(json!({
            "youSent": echoed,
            "access_token": "a-token-the-system-issued",
            "status": "ok",
        }))
        .into_response(),
        "/text-echo" => format!("you sent {echoed}").into_response(),
        "/fail" => (StatusCode::INTERNAL_SERVER_ERROR, "the system broke").into_response(),
        "/slow" => {
            tokio::time::sleep(Duration::from_secs(6)).await;
            "too late".into_response()
        }
        "/redirect" => {
            (StatusCode::FOUND, [(header::LOCATION, "/landed")], "moved").into_response()
        }
        "/big" => "x".repeat(100 * 1024).into_response(),
        // Each part of the credential on its own, never the whole header: a
        // system that logs the token, or decodes a Basic pair and repeats the
        // password. Only the needle for that part can redact it.
        "/parts" => parts_of(&echoed).into_response(),
        "/exactly-preview" => "y".repeat(2048).into_response(),
        // The header begins 18 characters before the preview's cut, and the
        // body runs on past it either way, so a cut made before redaction
        // would show the header's first 18 characters.
        "/straddle" => format!("{}{echoed}{}", "x".repeat(2030), "y".repeat(100)).into_response(),
        "/one-past-preview" => "y".repeat(2049).into_response(),
        // U+0000, which PostgreSQL stores in neither `text` nor `jsonb`: raw,
        // and as a JSON escape.
        "/nul" => b"before\0after".to_vec().into_response(),
        "/nul-json" => (
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"note":"x\u0000y","status":"ok"}"#,
        )
            .into_response(),
        // The bearer token echoed in an encoding: a JSONP callback that
        // `\u`-escapes every character, a URL that percent-encodes it, and its
        // base64. None contains the token as written.
        "/jsonp-echo" => format!(
            "callback({{\"echo\":\"{}\"}})",
            unicode_escaped(token_of(&echoed))
        )
        .into_response(),
        "/percent-echo" => format!(
            "https://return.example/cb?t={}",
            percent_encoded(token_of(&echoed))
        )
        .into_response(),
        "/base64-echo" => format!("seen={}", STANDARD.encode(token_of(&echoed))).into_response(),
        // JSON longer than the read cap, so it is cut and never parsed, with
        // a token the system issued at its very start.
        "/big-json" => (
            [(header::CONTENT_TYPE, "application/json")],
            format!(
                r#"{{"access_token":"issued-by-the-system-9999","pad":"{}"}}"#,
                "x".repeat(70 * 1024)
            ),
        )
            .into_response(),
        status if status.starts_with("/status/") => {
            let code = status["/status/".len()..]
                .parse::<u16>()
                .ok()
                .and_then(|code| StatusCode::from_u16(code).ok())
                .expect("a status code in the path");
            (code, "answered").into_response()
        }
        _ => "ok".into_response(),
    }
}

/// The token of a `Bearer` value.
fn token_of(authorization: &str) -> &str {
    authorization
        .strip_prefix("Bearer ")
        .unwrap_or(authorization)
}

/// Every character as a JSON `\uXXXX` escape, lower-case hex.
fn unicode_escaped(text: &str) -> String {
    text.encode_utf16()
        .map(|unit| format!("\\u{unit:04x}"))
        .collect()
}

/// `encodeURIComponent`: everything outside RFC 3986's unreserved set.
fn percent_encoded(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// The parts of an `Authorization` value, each alone, space-separated: the
/// token of a bearer; the base64 and, decoded, the password of a Basic pair.
fn parts_of(authorization: &str) -> String {
    match authorization.split_once(' ') {
        Some(("Bearer", token)) => format!("token={token}"),
        Some(("Basic", encoded)) => {
            let pair = STANDARD
                .decode(encoded)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default();
            let password = pair.split_once(':').map(|(_, p)| p).unwrap_or_default();
            format!("encoded={encoded} password={password}")
        }
        _ => "nothing".to_owned(),
    }
}

/// A system on loopback that speaks raw TCP, for the answers axum will not
/// give: a connection closed before any response, and a body that never ends.
struct RawSystem {
    address: SocketAddr,
}

impl RawSystem {
    /// Accepts each connection and closes it without a byte.
    async fn hanging_up() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the raw system");
        let address = listener.local_addr().expect("its address");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });
        Self { address }
    }

    /// Answers `200` and then writes a body until the client goes away.
    async fn endless() -> Self {
        use tokio::io::AsyncWriteExt as _;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the raw system");
        let address = listener.local_addr().expect("its address");
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // Read the request head before answering, so the client
                    // is not reset mid-send.
                    let mut head = [0_u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut head).await;
                    let _ = stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    // 8 KiB every 10 ms: the 64 KiB cap is reached in about
                    // 80 ms, and a cap even a thousand times larger is not
                    // reached within the system's two seconds.
                    let chunk = vec![b'z'; 8 * 1024];
                    while stream.write_all(&chunk).await.is_ok() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                });
            }
        });
        Self { address }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// An app whose egress guard lets the mock on loopback through — the seam.
async fn app_reaching_loopback() -> TestApp {
    TestApp::spawn_with(|config| config.integration_allow_loopback = true).await
}

/// A planted secret behind a fresh environment variable in the system
/// tenant's namespace; returns `(reference, value)`.
fn plant(value: &str) -> (String, String) {
    plant_in("SYSTEM", value)
}

/// A planted secret behind a fresh variable in the namespace of the tenant
/// whose code maps to `code_segment` (#618):
/// `KELIR_INTEGRATION_SECRET_<CODE>__TEST_<unique>`.
fn plant_in(code_segment: &str, value: &str) -> (String, String) {
    let name = format!(
        "KELIR_INTEGRATION_SECRET_{code_segment}__TEST_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    std::env::set_var(&name, value);
    (format!("env://{name}"), value.to_owned())
}

/// A tenant created through `POST /organization/tenants`, whose administrator
/// has granted themselves `integration:credential:create` — record 20's P1.
/// Provisioning withholds that code, and the administrator may grant it to
/// their own `ROLE-ADMIN` (`organization::service` says the withholding "is
/// not a boundary"). Returns a token carrying the grant.
async fn created_tenant_administrator(app: &TestApp, system_admin: &str, code: &str) -> String {
    let username = format!("admin.{}", code.to_lowercase().replace('_', "-"));
    let created = app
        .post(
            TENANTS,
            Some(system_admin),
            json!({
                "tenantCode": code,
                "name": format!("{code} Limited"),
                "administrator": {
                    "username": username,
                    "email": format!("{username}@example.test"),
                    "displayName": "Tenant Administrator",
                    "password": TENANT_PASSWORD,
                },
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);

    let token = app.sign_in_to(code, &username, TENANT_PASSWORD).await;

    let catalogue = app.get("/api/v1/identity/permissions", Some(&token)).await;
    let credential_create = catalogue
        .data()
        .as_array()
        .expect("the catalogue is an array")
        .iter()
        .find(|permission| permission["permissionCode"] == "integration:credential:create")
        .map(|permission| permission["id"].clone())
        .expect("integration:credential:create is catalogued");

    let roles = app.get("/api/v1/identity/roles", Some(&token)).await;
    let role = roles.body["data"]
        .as_array()
        .expect("roles are an array")
        .iter()
        .find(|role| role["roleCode"] == "ROLE-ADMIN")
        .cloned()
        .unwrap_or_else(|| panic!("no ROLE-ADMIN in {}", roles.body));
    let mut permission_ids: Vec<Value> = role["permissions"]
        .as_array()
        .expect("a role lists its permissions")
        .iter()
        .map(|permission| permission["id"].clone())
        .collect();
    permission_ids.push(credential_create);

    let granted = app
        .put(
            &format!(
                "/api/v1/identity/roles/{}",
                role["id"].as_str().expect("a role id")
            ),
            Some(&token),
            json!({ "permissionIds": permission_ids }),
        )
        .await;
    assert_eq!(granted.status, StatusCode::OK, "{}", granted.body);

    // A fresh token, so the claims carry what was just granted.
    app.sign_in_to(code, &username, TENANT_PASSWORD).await
}

/// An app serving more than one tenant, whose guard lets the mock through.
async fn multi_tenant_app_reaching_loopback() -> TestApp {
    TestApp::spawn_with(|config| {
        config.integration_allow_loopback = true;
        config.multi_tenant = true;
    })
    .await
}

fn id_of(response: &TestResponse) -> Uuid {
    response.body["data"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no id in {}", response.body))
}

struct Target {
    system: Uuid,
    endpoint: Uuid,
}

impl Target {
    fn call_uri(&self) -> String {
        format!(
            "{BASE}/{}/endpoints/{}/test-call",
            self.system, self.endpoint
        )
    }
}

/// A system at `base_url` with one endpoint at `path`, registered through the
/// API.
async fn target(app: &TestApp, token: &str, base_url: &str, method: &str, path: &str) -> Target {
    let code = format!("SYS_{}", Uuid::now_v7().simple());
    let system = app
        .post(
            BASE,
            Some(token),
            json!({
                "systemCode": code,
                "systemName": "Mock system",
                "baseUrl": base_url,
                "timeoutSeconds": 2,
            }),
        )
        .await;
    assert_eq!(system.status, StatusCode::CREATED, "{}", system.body);
    let system = id_of(&system);

    let endpoint = app
        .post(
            &format!("{BASE}/{system}/endpoints"),
            Some(token),
            json!({
                "endpointCode": "PROBE",
                "name": "Probe",
                "method": method,
                "path": path,
            }),
        )
        .await;
    assert_eq!(endpoint.status, StatusCode::CREATED, "{}", endpoint.body);

    Target {
        system,
        endpoint: id_of(&endpoint),
    }
}

/// Sets a system's `timeout_seconds` directly: the API's floor is the
/// schema's, and a test needs a second either side of it.
async fn set_timeout(app: &TestApp, system: Uuid, seconds: i32) {
    sqlx::query("UPDATE external_systems SET timeout_seconds = $2 WHERE id = $1")
        .bind(system)
        .bind(seconds)
        .execute(&app.pool)
        .await
        .expect("set the system's timeout");
}

async fn credential(app: &TestApp, token: &str, system: Uuid, body: Value) -> Uuid {
    let response = app
        .post(&format!("{BASE}/{system}/credentials"), Some(token), body)
        .await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.body);
    id_of(&response)
}

async fn bearer(app: &TestApp, token: &str, system: Uuid, reference: &str) -> Uuid {
    credential(
        app,
        token,
        system,
        json!({ "credentialType": "BEARER_TOKEN", "secretReference": reference }),
    )
    .await
}

async fn call(app: &TestApp, token: &str, target: &Target) -> TestResponse {
    app.post(&target.call_uri(), Some(token), json!({})).await
}

/// Every `integration_logs` row for an endpoint, whole, as JSON.
async fn log_rows(app: &TestApp, endpoint: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT row_to_json(l) FROM integration_logs l WHERE entity_id = $1 ORDER BY started_at",
    )
    .bind(endpoint)
    .fetch_all(&app.pool)
    .await
    .expect("read integration_logs")
}

async fn audit_rows_containing(app: &TestApp, needle: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_events a WHERE row_to_json(a)::text LIKE '%' || $1 || '%'",
    )
    .bind(needle)
    .fetch_one(&app.pool)
    .await
    .expect("read audit_events")
}

async fn all_log_text(app: &TestApp) -> String {
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT row_to_json(l)::text FROM integration_logs l")
            .fetch_all(&app.pool)
            .await
            .expect("read integration_logs");
    rows.join("\n")
}

/// A caller in the system tenant holding exactly `permissions`.
async fn caller_holding(app: &TestApp, label: &str, permissions: &[&str]) -> String {
    let role_id = fixtures::create_role_with_permissions(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &format!("ROLE-CALL-{label}"),
        permissions,
    )
    .await;

    let username = format!("user.call.{}", label.to_lowercase());
    fixtures::create_user(
        &app.pool,
        fixtures::SYSTEM_TENANT_ID,
        &username,
        &format!("call.{}@kelir.test", label.to_lowercase()),
        PASSWORD,
        &[role_id],
    )
    .await;

    app.sign_in(&username, PASSWORD).await
}

fn error_message(response: &TestResponse) -> String {
    response.body["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

// ---------------------------------------------------------------------------
// The secret goes to the system and nowhere else (AC3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_bearer_secret_reaches_the_system_and_nothing_kelir_keeps_or_returns() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-bearer-5d1c9e");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let data = response.data();
    assert_eq!(data["status"], "SUCCESS");
    assert_eq!(data["statusCode"], 200);
    assert_eq!(data["method"], "GET");
    assert_eq!(data["url"], format!("{}/echo", mock.base_url()));
    assert!(data["durationMs"].as_i64().is_some());

    // The system got it, as a bearer token.
    let seen = mock.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/echo");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Bearer {secret}").as_str())
    );

    // It echoed it back, and the preview has it redacted, with the masked key.
    let preview: Value = serde_json::from_str(data["bodyPreview"].as_str().expect("a preview"))
        .expect("the preview of a JSON body is JSON");
    assert_eq!(preview["youSent"], "[REDACTED]");
    assert_eq!(preview["access_token"], "[REDACTED]");
    assert_eq!(preview["status"], "ok");

    // Nowhere in the response.
    assert!(
        !response.body.to_string().contains(&secret),
        "the response carries the secret: {}",
        response.body
    );

    // Nowhere in the log row, which is one row.
    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert!(
        !row.to_string().contains(&secret),
        "the log row carries it: {row}"
    );
    assert_eq!(row["id"], data["logId"]);
    assert_eq!(row["status"], "SUCCESS");
    assert_eq!(row["status_code"], 200);
    assert_eq!(row["direction"], "OUTBOUND");
    assert_eq!(row["integration_type"], "REST");
    assert_eq!(row["method"], "GET");
    assert_eq!(row["endpoint"], format!("{}/echo", mock.base_url()));
    assert_eq!(row["entity_type"], "IntegrationEndpoint");
    assert_eq!(
        row["request_payload_json"]["headers"]["Authorization"],
        "[REDACTED]"
    );
    assert!(row["completed_at"].is_string());
    assert!(row["duration_ms"].as_i64().is_some());

    // The correlation id the system saw is the row's.
    assert_eq!(
        seen[0].correlation_id.as_deref(),
        row["correlation_id"].as_str()
    );

    // Nowhere in the audit trail — which does hold the reference, from the
    // credential's creation, so the search is not simply empty.
    assert_eq!(audit_rows_containing(&app, &secret).await, 0);
    assert!(audit_rows_containing(&app, &reference).await > 0);
}

/// Everything the backend traces, at every level, from one thread.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
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

#[tokio::test]
async fn no_log_line_carries_the_secret_even_at_trace() {
    // `#[tokio::test]` is a current-thread runtime, so the handler, the client
    // and the connection it drives all run on this thread, under this
    // thread's subscriber: every `tracing` event the call emits — Kelir's,
    // reqwest's and hyper's, at TRACE — lands in `captured`.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-traced-6e6e");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(captured.clone())
        .finish();
    let response = {
        let _guard = tracing::subscriber::set_default(subscriber);
        call(&app, &token, &target).await
    };

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let text = String::from_utf8_lossy(&captured.0.lock().expect("the capture")).into_owned();
    assert!(
        text.contains("integration test call"),
        "the capture saw nothing, so its silence proves nothing: {text}"
    );
    for form in [secret.clone(), format!("Bearer {secret}")] {
        assert!(
            !text.contains(&form),
            "a log line carries the secret: {text}"
        );
    }
}

#[tokio::test]
async fn a_basic_pair_is_sent_as_basic_and_its_password_is_nowhere() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, pair) = plant("svc-kelir:planted-basic-password-81f0");
    let encoded = STANDARD.encode(&pair);

    let target = target(&app, &token, &mock.base_url(), "POST", "/text-echo").await;
    credential(
        &app,
        &token,
        target.system,
        json!({ "credentialType": "BASIC_AUTH", "secretReference": reference }),
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let seen = mock.seen();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Basic {encoded}").as_str())
    );
    assert_eq!(response.data()["bodyPreview"], "you sent [REDACTED]");

    let everything = format!("{}\n{}", response.body, all_log_text(&app).await);
    for form in [
        pair.as_str(),
        "planted-basic-password-81f0",
        encoded.as_str(),
    ] {
        assert!(!everything.contains(form), "{form} escaped: {everything}");
        assert_eq!(audit_rows_containing(&app, form).await, 0, "{form}");
    }
}

// ---------------------------------------------------------------------------
// Secret resolution failures are named and logged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_vault_reference_fails_named_and_writes_one_log_row() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(
        &app,
        &token,
        target.system,
        "vault://kelir/erp/api-key#token",
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("SECRET_BACKEND_NOT_CONFIGURED"));
    assert!(mock.seen().is_empty(), "nothing was sent");

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["status_code"].is_null());
    assert!(rows[0]["error_message"]
        .as_str()
        .is_some_and(|message| message.starts_with("SECRET_BACKEND_NOT_CONFIGURED")));
    assert!(
        error_message(&response).contains(rows[0]["id"].as_str().expect("an id")),
        "the failure names its log row: {}",
        response.body
    );
}

#[tokio::test]
async fn an_unset_environment_variable_is_named_and_logged() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(
        &app,
        &token,
        target.system,
        "env://KELIR_INTEGRATION_SECRET_SYSTEM__NEVER_SET_ANYWHERE",
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_NOT_FOUND"),
        "{}",
        response.body
    );
    assert!(mock.seen().is_empty());
    assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
}

#[tokio::test]
async fn a_name_outside_the_integration_prefix_is_refused_and_its_value_goes_nowhere() {
    // The product owner's decision on #547, as #618 narrowed it: only the
    // caller's tenant's KELIR_INTEGRATION_SECRET_<CODE>__* is read, so a name
    // outside the prefix is outside every tenant's namespace.
    // KELIR_JWT_SECRET is the case the rule exists for — a caller who
    // can write a reference and a baseUrl would otherwise be sent the key that
    // signs every session.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let jwt = std::env::var("KELIR_JWT_SECRET").unwrap_or_else(|_| {
        let planted = "kelir-planted-jwt-signing-key-c0ffee".to_owned();
        std::env::set_var("KELIR_JWT_SECRET", &planted);
        planted
    });
    // A second, unprefixed name holding a value that exists nowhere else.
    let outside = format!(
        "KELIR_TEST_CALL_OUTSIDE_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    let outside_value = "kelir-planted-outside-prefix-e4e4";
    std::env::set_var(&outside, outside_value);

    for (name, value) in [
        ("KELIR_JWT_SECRET", jwt.as_str()),
        (outside.as_str(), outside_value),
    ] {
        let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
        bearer(&app, &token, target.system, &format!("env://{name}")).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{name}: {}",
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{name}"
        );
        assert!(!response.body.to_string().contains(value), "{name}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(
            rows.len(),
            1,
            "{name}: the refused call still writes its row"
        );
        assert!(rows[0]["error_message"]
            .as_str()
            .is_some_and(|message| message.starts_with("SECRET_NAME_NOT_PERMITTED")));
        assert!(!all_log_text(&app).await.contains(value), "{name}");
        assert_eq!(audit_rows_containing(&app, value).await, 0, "{name}");
    }

    assert!(
        mock.seen().is_empty(),
        "nothing was sent: {:?}",
        mock.seen()
    );

    // Control: the same system shape with a prefixed name resolves and calls.
    let (reference, secret) = plant("kelir-planted-prefixed-ok-1a1a");
    assert!(reference.starts_with("env://KELIR_INTEGRATION_SECRET_"));
    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {secret}"))
    );
}

#[tokio::test]
async fn an_unbuilt_credential_type_is_refused_before_its_secret_is_read() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    for kind in [
        "API_KEY",
        "OAUTH2_CLIENT_CREDENTIALS",
        "JWT",
        "HMAC_SECRET",
        "CERTIFICATE",
        "SFTP_PASSWORD",
    ] {
        let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
        // An unset variable: had the secret been read first, the answer would
        // be SECRET_NOT_FOUND.
        credential(
            &app,
            &token,
            target.system,
            json!({
                "credentialType": kind,
                "secretReference": "env://KELIR_INTEGRATION_SECRET_SYSTEM__NEVER_SET_ANYWHERE",
            }),
        )
        .await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{kind}: {}",
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("CREDENTIAL_TYPE_NOT_SUPPORTED"),
            "{kind}"
        );
        assert!(error_message(&response).contains(kind), "{kind}");
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{kind}");
    }

    assert!(mock.seen().is_empty());
}

#[tokio::test]
async fn a_basic_secret_that_is_not_a_pair_is_named() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("no-colon-in-this-planted-value");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    credential(
        &app,
        &token,
        target.system,
        json!({ "credentialType": "BASIC_AUTH", "secretReference": reference }),
    )
    .await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_MALFORMED"),
        "{}",
        response.body
    );
    assert!(!response.body.to_string().contains(&secret));
    assert!(!all_log_text(&app).await.contains(&secret));
    assert!(mock.seen().is_empty());
}

// ---------------------------------------------------------------------------
// Which credential
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exactly_one_active_credential_valid_today_is_required() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-choice-3b7e");

    // None at all.
    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("NO_USABLE_CREDENTIAL"),
        "{}",
        response.body
    );

    // One that expired yesterday and one that is switched off: still none.
    credential(
        &app,
        &token,
        target.system,
        json!({
            "credentialType": "BEARER_TOKEN",
            "secretReference": "env://KELIR_INTEGRATION_SECRET_SYSTEM__EXPIRED",
            "validFrom": "2020-01-01",
            "validTo": "2020-12-31",
        }),
    )
    .await;
    credential(
        &app,
        &token,
        target.system,
        json!({
            "credentialType": "BEARER_TOKEN",
            "secretReference": "env://KELIR_INTEGRATION_SECRET_SYSTEM__OFF",
            "isActive": false,
        }),
    )
    .await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("NO_USABLE_CREDENTIAL"),
        "{}",
        response.body
    );

    // The one that qualifies is the one used.
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {secret}"))
    );

    // A second that qualifies makes it ambiguous rather than a coin toss.
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("AMBIGUOUS_CREDENTIAL"),
        "{}",
        response.body
    );

    assert_eq!(
        log_rows(&app, target.endpoint).await.len(),
        4,
        "four calls, four rows"
    );
    assert_eq!(mock.seen().len(), 1, "one call reached the system");
}

// ---------------------------------------------------------------------------
// Egress (AC4) — the harness default, which is production's
// ---------------------------------------------------------------------------

#[tokio::test]
async fn loopback_is_refused_in_every_spelling_and_nothing_is_sent() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let port = mock.address.port();
    let (reference, _) = plant("kelir-planted-loopback-0a2d");

    for base in [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::ffff:127.0.0.1]:{port}"),
        format!("http://[::1]:{port}"),
        format!("http://2130706433:{port}"),
    ] {
        let target = target(&app, &token, &base, "GET", "/echo").await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{base}: {}",
            response.body
        );
        assert_eq!(response.error_code(), Some("EGRESS_REFUSED"), "{base}");
        assert!(error_message(&response).contains("loopback"), "{base}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1, "{base}");
        assert!(rows[0]["error_message"]
            .as_str()
            .is_some_and(|message| message.starts_with("EGRESS_REFUSED")));
        assert!(
            rows[0]["request_payload_json"]["headers"]["Authorization"].is_null(),
            "{base}: no Authorization was sent, so none is recorded"
        );
    }

    assert!(
        mock.seen().is_empty(),
        "the mock was reached past the guard: {:?}",
        mock.seen()
    );
}

#[tokio::test]
async fn the_metadata_address_and_an_unlisted_private_address_are_refused() {
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-private-6c4f");

    for (base, class) in [
        ("http://169.254.169.254", "link-local"),
        ("http://[fe80::1]", "link-local"),
        ("http://0.0.0.0:8080", "unspecified"),
        ("http://224.0.0.1", "multicast"),
        ("http://10.255.255.1", "private"),
        ("http://192.168.0.1", "private"),
        ("http://[fd00::1]", "private"),
    ] {
        let target = target(&app, &token, base, "GET", "/latest/meta-data").await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base}: {}",
            response.body
        );
        assert!(
            error_message(&response).contains(class),
            "{base}: {}",
            response.body
        );
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{base}");
    }
}

#[tokio::test]
async fn a_listed_private_range_passes_the_guard() {
    // The address is unroutable from a test runner, so the call fails — at
    // the network, not at the guard. That is the observation: listed, it is
    // no longer EGRESS_REFUSED.
    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = vec!["10.255.255.0/24".parse().expect("a CIDR")];
    })
    .await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-listed-9e21");

    let listed = target(&app, &token, "http://10.255.255.1", "GET", "/x").await;
    // The address is unroutable, so the call waits out its budget: one second.
    set_timeout(&app, listed.system, 1).await;
    bearer(&app, &token, listed.system, &reference).await;
    let response = call(&app, &token, &listed).await;

    assert!(
        matches!(
            response.error_code(),
            Some("UPSTREAM_TIMEOUT" | "UPSTREAM_UNREACHABLE")
        ),
        "a listed address is let through to the network: {}",
        response.body
    );

    // The next address over is outside the list.
    let unlisted = target(&app, &token, "http://10.255.254.1", "GET", "/x").await;
    bearer(&app, &token, unlisted.system, &reference).await;
    let response = call(&app, &token, &unlisted).await;

    assert_eq!(
        response.error_code(),
        Some("EGRESS_REFUSED"),
        "{}",
        response.body
    );
}

/// The allow-list verification record 20's P4 used, and wider: Installation
/// §7.1's own `fd00::/8` example, CGNAT, the IETF block Oracle's address sits
/// in, and both whole families.
async fn app_listing_everything() -> TestApp {
    TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = [
            "fd00::/8",
            "100.64.0.0/10",
            "192.0.0.0/24",
            "0.0.0.0/0",
            "::/0",
        ]
        .iter()
        .map(|cidr| cidr.parse().expect("a CIDR"))
        .collect();
    })
    .await
}

#[tokio::test]
async fn a_cloud_metadata_address_is_refused_whatever_is_listed() {
    // #622: Alibaba's, Oracle Compute Classic's and AWS's IPv6 metadata
    // addresses. The first two are public by range and the third is
    // unique-local, so before #622 two went out with no setting and the third
    // with the guide's example.
    let app = app_listing_everything().await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-metadata-2b7d");

    for base in [
        "http://100.100.100.200",
        "http://192.0.0.192",
        "http://[fd00:ec2::254]",
        "http://[::ffff:100.100.100.200]",
        "http://[::ffff:192.0.0.192]",
    ] {
        let target = target(&app, &token, base, "GET", "/latest/meta-data").await;
        // Unrefused, the call waits out its budget: one second, not thirty.
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{base}: {}",
            response.body
        );
        assert_eq!(response.error_code(), Some("EGRESS_REFUSED"), "{base}");
        let message = error_message(&response);
        assert!(
            message.contains("a cloud metadata address"),
            "{base}: {message}"
        );
        assert!(
            !message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{base}: no setting opens it, so none is offered: {message}"
        );

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1, "{base}");
        assert!(rows[0]["error_message"]
            .as_str()
            .is_some_and(|message| message.starts_with("EGRESS_REFUSED")));
        assert!(
            rows[0]["request_payload_json"]["headers"]["Authorization"].is_null(),
            "{base}: nothing was sent"
        );
        assert!(!rows[0].to_string().contains(&secret), "{base}");
    }

    // The same list does open the unique-local address next to AWS's, so the
    // refusals above are the class and not a list that failed to match.
    let neighbour = target(&app, &token, "http://[fd00:ec2::253]", "GET", "/x").await;
    set_timeout(&app, neighbour.system, 1).await;
    bearer(&app, &token, neighbour.system, &reference).await;
    let response = call(&app, &token, &neighbour).await;

    assert!(
        matches!(
            response.error_code(),
            Some("UPSTREAM_TIMEOUT" | "UPSTREAM_UNREACHABLE")
        ),
        "a listed unique-local address is let through to the network: {}",
        response.body
    );
}

#[tokio::test]
async fn an_ipv4_compatible_address_is_judged_as_the_ipv4_address_it_carries() {
    // #622: `::a.b.c.d` (`::/96`) was read as a public IPv6 address, so
    // `::127.0.0.1` went out wherever the host routed it. Everything is
    // listed, so only the always-refused classes are left to refuse.
    let app = app_listing_everything().await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-compatible-71ce");

    for (base, class) in [
        ("http://[::127.0.0.1]", "a loopback address"),
        ("http://[::7f00:1]", "a loopback address"),
        ("http://[::169.254.169.254]", "a link-local address"),
        ("http://[::0.0.0.9]", "an unspecified address"),
        ("http://[::100.100.100.200]", "a cloud metadata address"),
    ] {
        let target = target(&app, &token, base, "GET", "/x").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base}: {}",
            response.body
        );
        assert!(
            error_message(&response).contains(class),
            "{base}: {}",
            response.body
        );
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{base}");
    }

    // With nothing listed, the private address it carries is refused as
    // private, and the refusal offers the setting.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let private = target(&app, &token, "http://[::10.255.255.1]", "GET", "/x").await;
    set_timeout(&app, private.system, 1).await;
    bearer(&app, &token, private.system, &reference).await;
    let response = call(&app, &token, &private).await;

    assert_eq!(
        response.error_code(),
        Some("EGRESS_REFUSED"),
        "{}",
        response.body
    );
    let message = error_message(&response);
    assert!(message.contains("a private address"), "{message}");
    assert!(
        message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
        "{message}"
    );
}

#[tokio::test]
async fn a_name_resolving_to_a_metadata_address_is_refused() {
    // The literal is one way in; a zone that answers the address is the
    // other. 203.0.113.7 (TEST-NET-3) is public and first.
    let app = app_resolving(
        "metadata.kelir.test",
        &["203.0.113.7", "100.100.100.200"],
        false,
    )
    .await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-metadata-name-5e0a");

    let target = target(&app, &token, "http://metadata.kelir.test", "GET", "/x").await;
    set_timeout(&app, target.system, 1).await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("EGRESS_REFUSED"),
        "{}",
        response.body
    );
    assert!(
        error_message(&response).contains("a cloud metadata address"),
        "{}",
        response.body
    );
    for text in [
        response.body.to_string(),
        log_rows(&app, target.endpoint).await[0].to_string(),
    ] {
        assert!(
            !text.contains("100.100.100.200"),
            "no address is shown: {text}"
        );
    }
}

#[tokio::test]
async fn a_redirect_is_the_answer_and_its_location_is_not_requested() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-redirect-44aa");

    let target = target(&app, &token, &mock.base_url(), "GET", "/redirect").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["statusCode"], 302);
    assert_eq!(response.data()["status"], "FAILED");

    let paths: Vec<String> = mock.seen().into_iter().map(|seen| seen.path).collect();
    assert_eq!(paths, vec!["/redirect"], "the Location was followed");
}

// ---------------------------------------------------------------------------
// Time (AC5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_slow_system_fails_at_its_timeout() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-slow-2f8b");

    let target = target(&app, &token, &mock.base_url(), "GET", "/slow").await;
    sqlx::query("UPDATE external_systems SET timeout_seconds = 1 WHERE id = $1")
        .bind(target.system)
        .execute(&app.pool)
        .await
        .expect("shorten the timeout");
    bearer(&app, &token, target.system, &reference).await;

    let clock = Instant::now();
    let response = call(&app, &token, &target).await;
    let elapsed = clock.elapsed();

    assert_eq!(
        response.status,
        StatusCode::GATEWAY_TIMEOUT,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("UPSTREAM_TIMEOUT"));
    assert!(
        elapsed < Duration::from_secs(4),
        "the call waited {elapsed:?} for a system with a one-second timeout"
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["duration_ms"].as_i64().is_some_and(|ms| ms >= 900));
}

// ---------------------------------------------------------------------------
// Answers, and the one-row rule (AC6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_system_error_is_an_answer_marked_failed() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-fail-7d0e");

    let target = target(&app, &token, &mock.base_url(), "DELETE", "/fail").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["status"], "FAILED");
    assert_eq!(response.data()["statusCode"], 500);
    assert_eq!(response.data()["bodyPreview"], "the system broke");
    assert_eq!(mock.seen()[0].method, "DELETE");

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows[0]["status"], "FAILED");
    assert_eq!(rows[0]["status_code"], 500);
    assert_eq!(
        rows[0]["response_payload_json"]["bodyPreview"],
        "the system broke"
    );
}

#[tokio::test]
async fn a_long_body_is_cut_in_the_preview_and_the_log() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-big-1c3d");

    let target = target(&app, &token, &mock.base_url(), "GET", "/big").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["bodyTruncated"], true);
    assert_eq!(
        response.data()["bodyPreview"]
            .as_str()
            .map(|text| text.chars().count()),
        Some(2048)
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows[0]["response_payload_json"]["bodyTruncated"], true);
}

#[tokio::test]
async fn an_inactive_system_or_endpoint_is_refused_and_logged() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-inactive-5a5a");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let retired = app
        .put(
            &format!("{BASE}/{}/endpoints/{}", target.system, target.endpoint),
            Some(&token),
            json!({ "status": "INACTIVE" }),
        )
        .await;
    assert_eq!(retired.status, StatusCode::OK, "{}", retired.body);
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("ENDPOINT_NOT_ACTIVE"),
        "{}",
        response.body
    );

    app.put(
        &format!("{BASE}/{}/endpoints/{}", target.system, target.endpoint),
        Some(&token),
        json!({ "status": "ACTIVE" }),
    )
    .await;
    let deactivated = app
        .post(
            &format!("{BASE}/{}/deactivate", target.system),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(deactivated.status, StatusCode::OK, "{}", deactivated.body);
    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("EXTERNAL_SYSTEM_NOT_ACTIVE"),
        "{}",
        response.body
    );

    assert_eq!(log_rows(&app, target.endpoint).await.len(), 2);
    assert!(mock.seen().is_empty());
}

#[tokio::test]
async fn a_refused_caller_or_a_missing_target_writes_no_row() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-norow-8b8b");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    // Everything about the system except the call.
    let without = caller_holding(
        &app,
        "NOCALL",
        &[
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:credential:read",
            "integration:credential:create",
        ],
    )
    .await;
    let response = call(&app, &without, &target).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "{}", response.body);

    // The call alone is enough.
    let with = caller_holding(&app, "CALLONLY", &["integration:endpoint:call"]).await;
    let response = call(&app, &with, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);

    // An endpoint that is not on this system, and a system in another tenant.
    let stray = Target {
        system: target.system,
        endpoint: Uuid::now_v7(),
    };
    assert_eq!(
        call(&app, &token, &stray).await.status,
        StatusCode::NOT_FOUND
    );

    let other = fixtures::create_tenant(&app.pool, "TNT-CALL-X", "Other tenant").await;
    let theirs = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO external_systems (id, tenant_id, system_code, system_name, base_url)
         VALUES ($1, $2, 'THEIRS', 'Theirs', $3)",
    )
    .bind(theirs)
    .bind(other)
    .bind(mock.base_url())
    .execute(&app.pool)
    .await
    .expect("another tenant's system");
    let their_endpoint = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO integration_endpoints
             (id, tenant_id, external_system_id, endpoint_code, name, method, path)
         VALUES ($1, $2, $3, 'PROBE', 'Probe', 'GET', '/echo')",
    )
    .bind(their_endpoint)
    .bind(other)
    .bind(theirs)
    .execute(&app.pool)
    .await
    .expect("its endpoint");
    let crossing = Target {
        system: theirs,
        endpoint: their_endpoint,
    };
    assert_eq!(
        call(&app, &token, &crossing).await.status,
        StatusCode::NOT_FOUND
    );

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM integration_logs")
        .fetch_one(&app.pool)
        .await
        .expect("count integration_logs");
    assert_eq!(
        total, 1,
        "only the permitted call on a real endpoint wrote a row"
    );
    assert_eq!(mock.seen().len(), 1);
}

// ---------------------------------------------------------------------------
// The permission and the document (AC1, AC7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_permission_is_catalogued_at_its_id_and_held_by_the_administrator() {
    let app = TestApp::spawn().await;

    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM permissions WHERE permission_code = 'integration:endpoint:call'",
    )
    .fetch_one(&app.pool)
    .await
    .expect("the catalogue row");
    assert_eq!(id, uuid::uuid!("00000000-0000-0000-0001-000000000076"));

    let granted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM role_permissions
                        WHERE role_id = $1 AND permission_id = $2 AND deleted_at IS NULL)",
    )
    .bind(fixtures::ADMIN_ROLE_ID)
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .expect("the grant");
    assert!(granted);
}

#[tokio::test]
async fn the_route_is_in_the_openapi_document_and_its_answer_has_no_headers() {
    let app = TestApp::spawn().await;
    let document = app
        .send(Method::GET, "/api/docs/openapi.json", None, None)
        .await;

    let operation = &document.body["paths"]
        [&format!("{BASE}/{{id}}/endpoints/{{endpointId}}/test-call")]["post"];
    assert!(
        operation.is_object(),
        "the route is missing from the document"
    );
    assert!(
        operation["requestBody"].is_null(),
        "a test call takes no body"
    );

    let schema = &document.body["components"]["schemas"]["TestCallResponse"]["properties"];
    let mut fields: Vec<&str> = schema
        .as_object()
        .expect("TestCallResponse has properties")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "bodyPreview",
            "bodyTruncated",
            "durationMs",
            "logId",
            "method",
            "status",
            "statusCode",
            "url"
        ],
        "a field added to the answer is a decision about what it may carry"
    );
}

// ---------------------------------------------------------------------------
// Added by the test-engineer campaign (#547, 2026-09-29): each test below
// reddened under a mutation the builder's table does not list, and is green
// on the unmutated code.
//
// | Mutation | Reddened |
// |---|---|
// | `deleted_at IS NULL` dropped from `credential::active_credentials` | `a_soft_deleted_credential_is_not_chosen_and_its_replacement_is` |
// | the bare secret's needle dropped from `secret::redactions` | `each_part_of_a_credential_echoed_alone_is_redacted` |
// | the base64 needle dropped from `secret::redactions` | `each_part_of_a_credential_echoed_alone_is_redacted` |
// | the password needle dropped from `secret::redactions` | `each_part_of_a_credential_echoed_alone_is_redacted` |
// | an `UPSTREAM_UNREACHABLE` outcome returned before the row is written | `a_system_that_hangs_up_is_a_502_and_writes_one_row` |
// | `TestCallStatus::for_status_code` takes `200..=300` | `success_is_exactly_the_2xx_range` |
// | a preview of exactly 2048 characters marked truncated | `a_body_of_exactly_the_preview_length_is_whole_and_one_more_is_cut` |
// | `MAX_BODY_BYTES` raised to 64 MiB | `a_body_that_never_ends_is_cut_at_the_read_cap_and_answered` |
// | `preview` cuts at 2048 characters before it redacts | `a_secret_that_straddles_the_preview_cut_is_redacted_whole` |
// | a row written to `outbox_events` after the log row | `a_test_call_writes_no_outbox_event_and_runs_no_hook` |
// | `.no_proxy()` removed from `outbound::send` | `a_proxy_named_in_the_environment_is_not_used` |
// | `log_repo::insert_log` skipped for `HOST_NOT_RESOLVED` | `a_host_that_does_not_resolve_is_named_and_writes_one_row` |
// | `service::test_call` writes its row under the system tenant | `a_caller_in_another_tenant_calls_their_own_endpoint_and_neither_reaches_the_other` |
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_soft_deleted_credential_is_not_chosen_and_its_replacement_is() {
    // The credential choice: `deleted_at IS NULL` in the read.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (retired_reference, retired) = plant("kelir-planted-deleted-0d0d");
    let (reference, secret) = plant("kelir-planted-replacement-1e1e");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    let deleted = bearer(&app, &token, target.system, &retired_reference).await;
    let response = app
        .delete(
            &format!("{BASE}/{}/credentials/{deleted}", target.system),
            Some(&token),
        )
        .await;
    assert!(response.status.is_success(), "{}", response.body);

    let response = call(&app, &token, &target).await;
    assert_eq!(
        response.error_code(),
        Some("NO_USABLE_CREDENTIAL"),
        "a deleted credential was used: {}",
        response.body
    );

    // Its replacement is the one, not one of two.
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let sent: Vec<Option<String>> = mock.seen().into_iter().map(|s| s.authorization).collect();
    assert_eq!(sent, vec![Some(format!("Bearer {secret}"))]);
    assert!(!format!("{sent:?}").contains(&retired));
}

#[tokio::test]
async fn each_part_of_a_credential_echoed_alone_is_redacted() {
    // AC3: the bare token, the base64 of a Basic pair and its password each
    // need their own needle; the whole header's needle covers none of them.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;

    let (reference, secret) = plant("kelir-planted-bare-token-2f2f");
    let target_bearer = target(&app, &token, &mock.base_url(), "GET", "/parts").await;
    bearer(&app, &token, target_bearer.system, &reference).await;
    let response = call(&app, &token, &target_bearer).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["bodyPreview"], "token=[REDACTED]");

    let (reference, pair) = plant("svc-kelir:planted-part-password-3a3a");
    let encoded = STANDARD.encode(&pair);
    let target_basic = target(&app, &token, &mock.base_url(), "GET", "/parts").await;
    credential(
        &app,
        &token,
        target_basic.system,
        json!({ "credentialType": "BASIC_AUTH", "secretReference": reference }),
    )
    .await;
    let response = call(&app, &token, &target_basic).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        response.data()["bodyPreview"],
        "encoded=[REDACTED] password=[REDACTED]"
    );

    let everything = format!("{}\n{}", response.body, all_log_text(&app).await);
    for form in [
        secret.as_str(),
        encoded.as_str(),
        "planted-part-password-3a3a",
    ] {
        assert!(!everything.contains(form), "{form} escaped: {everything}");
    }
}

#[tokio::test]
async fn a_system_that_hangs_up_is_a_502_and_writes_one_row() {
    // AC6 on the UPSTREAM_UNREACHABLE path, which no other test reaches.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let system = RawSystem::hanging_up().await;
    let (reference, secret) = plant("kelir-planted-hangup-4b4b");

    let target = target(&app, &token, &system.base_url(), "GET", "/x").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.status,
        StatusCode::BAD_GATEWAY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("UPSTREAM_UNREACHABLE"));

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["status_code"].is_null());
    assert!(rows[0]["error_message"]
        .as_str()
        .is_some_and(|message| message.starts_with("UPSTREAM_UNREACHABLE")));
    assert_eq!(
        rows[0]["request_payload_json"]["headers"]["Authorization"], "[REDACTED]",
        "the header was sent, so it is recorded as sent"
    );
    assert!(error_message(&response).contains(rows[0]["id"].as_str().expect("an id")));
    assert!(!response.body.to_string().contains(&secret));
    assert!(!rows[0].to_string().contains(&secret));
}

#[tokio::test]
async fn a_host_that_does_not_resolve_is_named_and_writes_one_row() {
    // AC4/AC6 on the HOST_NOT_RESOLVED path: `.invalid` never resolves
    // (RFC 6761), so nothing is sent anywhere.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (reference, _) = plant("kelir-planted-nxdomain-5c5c");

    let target = target(&app, &token, "http://kelir-test-call.invalid", "GET", "/x").await;
    // Resolution is inside the call's budget, and a negative answer took over
    // two seconds on a loaded Windows runner: give it the default thirty.
    set_timeout(&app, target.system, 30).await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("HOST_NOT_RESOLVED"));
    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0]["request_payload_json"]["headers"]["Authorization"].is_null());
}

#[tokio::test]
async fn success_is_exactly_the_2xx_range() {
    // The SUCCESS/FAILED boundary at both of its edges, in the answer and the
    // row alike.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-boundary-6d6d");

    for (code, expected) in [
        (200, "SUCCESS"),
        (204, "SUCCESS"),
        (299, "SUCCESS"),
        (300, "FAILED"),
        (404, "FAILED"),
    ] {
        let target = target(
            &app,
            &token,
            &mock.base_url(),
            "GET",
            &format!("/status/{code}"),
        )
        .await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(response.status, StatusCode::OK, "{code}: {}", response.body);
        assert_eq!(response.data()["statusCode"], code, "{code}");
        assert_eq!(response.data()["status"], expected, "{code}");
        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows[0]["status"], expected, "{code}");
        assert_eq!(rows[0]["status_code"], code, "{code}");
    }
}

#[tokio::test]
async fn a_body_of_exactly_the_preview_length_is_whole_and_one_more_is_cut() {
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-edge-7e7e");

    for (path, truncated) in [("/exactly-preview", false), ("/one-past-preview", true)] {
        let target = target(&app, &token, &mock.base_url(), "GET", path).await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.body);
        assert_eq!(response.data()["bodyTruncated"], truncated, "{path}");
        assert_eq!(response.data()["bodyPreview"], "y".repeat(2048), "{path}");
        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(
            rows[0]["response_payload_json"]["bodyTruncated"], truncated,
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_body_that_never_ends_is_cut_at_the_read_cap_and_answered() {
    // The 64 KiB read cap: without it the client reads until the system's
    // timeout and the call is a 504 with nothing to show.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let system = RawSystem::endless().await;
    let (reference, _) = plant("kelir-planted-endless-8f8f");

    let target = target(&app, &token, &system.base_url(), "GET", "/stream").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["statusCode"], 200);
    assert_eq!(response.data()["bodyTruncated"], true);
    assert_eq!(response.data()["bodyPreview"], "z".repeat(2048));
}

#[tokio::test]
async fn a_caller_in_another_tenant_calls_their_own_endpoint_and_neither_reaches_the_other() {
    // Tenant isolation with both tenants' systems present, in both directions,
    // and the row written under the caller's tenant.
    let app = TestApp::spawn_with(|config| {
        config.integration_allow_loopback = true;
        config.multi_tenant = true;
    })
    .await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let mock = Mock::start().await;
    // Each tenant names a variable in its own namespace (#618): the system
    // tenant's under `SYSTEM__`, tenant B's under `TNT_CALL_B__`.
    let (reference, _) = plant("kelir-planted-tenant-9a9a");
    let (their_reference, their_secret) = plant_in("TNT_CALL_B", "kelir-planted-tenant-b-8b8b");

    let other = fixtures::create_tenant(&app.pool, "TNT-CALL-B", "Tenant B").await;
    let role = fixtures::create_role_with_permissions(
        &app.pool,
        other,
        "ROLE-CALL-B",
        &[
            "integration:external-system:create",
            "integration:external-system:read",
            "integration:external-system:update",
            "integration:credential:create",
            "integration:endpoint:call",
        ],
    )
    .await;
    fixtures::create_user(
        &app.pool,
        other,
        "user.call.b",
        "call.b@kelir.test",
        PASSWORD,
        &[role],
    )
    .await;
    let theirs_token = app.sign_in_to("TNT-CALL-B", "user.call.b", PASSWORD).await;

    let ours = target(&app, &system_admin, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &system_admin, ours.system, &reference).await;
    let theirs = target(&app, &theirs_token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &theirs_token, theirs.system, &their_reference).await;

    // Each calls its own.
    let response = call(&app, &theirs_token, &theirs).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {their_secret}"))
    );
    let rows = log_rows(&app, theirs.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tenant_id"], other.to_string());

    // Neither reaches the other's, whole or mixed.
    let mixed_ours = Target {
        system: ours.system,
        endpoint: theirs.endpoint,
    };
    let mixed_theirs = Target {
        system: theirs.system,
        endpoint: ours.endpoint,
    };
    for (token, crossing) in [
        (&system_admin, &theirs),
        (&theirs_token, &ours),
        (&system_admin, &mixed_ours),
        (&theirs_token, &mixed_theirs),
    ] {
        let response = call(&app, token, crossing).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
    }

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM integration_logs")
        .fetch_one(&app.pool)
        .await
        .expect("count integration_logs");
    assert_eq!(total, 1, "only tenant B's own call wrote a row");
    assert_eq!(mock.seen().len(), 1);
}

// ---------------------------------------------------------------------------
// Each tenant resolves only its own variables (#618, record 20's P1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_created_tenant_naming_the_system_tenants_variable_is_refused_and_sends_nothing() {
    // Record 20's P1, as a test. On the prefix-only rule the collector received
    // `Bearer <the system tenant's value>`.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;
    let (system_reference, system_value) = plant_in("SYSTEM", "r20-system-tenant-erp-token-5c5c");

    let tenant = created_tenant_administrator(&app, &system_admin, "TNT-P1").await;
    let foreign = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, foreign.system, &system_reference).await;

    let response = call(&app, &tenant, &foreign).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("SECRET_NAME_NOT_PERMITTED"));
    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );
    // The message names the caller's own prefix, and no other tenant's.
    let message = error_message(&response);
    assert!(
        message.contains("KELIR_INTEGRATION_SECRET_TNT_P1__"),
        "{message}"
    );
    assert!(!message.contains("SYSTEM"), "{message}");
    assert!(!response.body.to_string().contains(&system_value));

    let rows = log_rows(&app, foreign.endpoint).await;
    assert_eq!(rows.len(), 1, "one row for the refused call");
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["error_message"]
        .as_str()
        .is_some_and(|message| message.starts_with("SECRET_NAME_NOT_PERMITTED")));
    assert!(!all_log_text(&app).await.contains(&system_value));

    // Control: the tenant's own variable resolves and is sent.
    let (own_reference, own_value) = plant_in("TNT_P1", "kelir-planted-tnt-p1-own-3d3d");
    let own = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, own.system, &own_reference).await;
    let response = call(&app, &tenant, &own).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {own_value}"))
    );

    // Control: the system tenant's own call resolves the same variable.
    let theirs = target(&app, &system_admin, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &system_admin, theirs.system, &system_reference).await;
    let response = call(&app, &system_admin, &theirs).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {system_value}"))
    );
    assert_eq!(collector.seen().len(), 2);
}

/// A response body with its integration log id replaced, so two answers can
/// be compared byte for byte apart from the row each wrote.
fn without_log_id(response: &TestResponse) -> String {
    let mut body = response.body.to_string();
    let message = error_message(response);
    let log_id = message
        .rsplit_once("(integration log ")
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("no log id in {message}"))
        .to_owned();
    body = body.replace(&log_id, "<log>");
    body
}

#[tokio::test]
async fn another_tenants_variable_answers_the_same_set_or_unset() {
    // Criterion 5: a foreign name is refused before the environment is read,
    // so whether it is set cannot change a byte of the answer. On the
    // prefix-only rule, the set one answered 200 and the unset one
    // SECRET_NOT_FOUND, naming it.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;
    let tenant = created_tenant_administrator(&app, &system_admin, "TNT-SAME").await;

    let (set_reference, set_value) = plant_in("SYSTEM", "kelir-planted-foreign-set-6e6e");
    let unset_reference = format!(
        "env://KELIR_INTEGRATION_SECRET_SYSTEM__NEVER_SET_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );

    let mut answers = Vec::new();
    let mut stored = Vec::new();
    for reference in [&set_reference, &unset_reference] {
        let target = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
        bearer(&app, &tenant, target.system, reference).await;
        let response = call(&app, &tenant, &target).await;
        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{reference}: {}",
            response.body
        );
        answers.push((response.status, without_log_id(&response)));
        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1);
        stored.push(rows[0]["error_message"].clone());
    }

    assert_eq!(answers[0], answers[1], "set and unset answer alike");
    assert_eq!(stored[0], stored[1], "and are stored alike");
    assert!(!answers[0].1.contains(&set_value));
    assert!(!answers[0].1.contains("NEVER_SET"), "{}", answers[0].1);
    assert!(collector.seen().is_empty());

    // SECRET_NOT_FOUND is reachable inside the caller's own namespace, and
    // still names the variable there.
    let own_unset = format!(
        "KELIR_INTEGRATION_SECRET_TNT_SAME__NEVER_SET_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    let target = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, target.system, &format!("env://{own_unset}")).await;
    let response = call(&app, &tenant, &target).await;
    assert_eq!(
        response.error_code(),
        Some("SECRET_NOT_FOUND"),
        "{}",
        response.body
    );
    assert!(error_message(&response).contains(&own_unset));
}

#[tokio::test]
async fn a_name_two_tenants_codes_both_map_to_is_refused_for_both() {
    // Criterion 3: `AMB-X` and `AMB_X` both map to `AMB_X`, and `DBL` and
    // `DBL__X` both claim `..._DBL__X__TOKEN`. Such a name belongs to nobody,
    // so it is refused for every tenant it matches.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;

    let hyphen = created_tenant_administrator(&app, &system_admin, "AMB-X").await;
    let underscore = created_tenant_administrator(&app, &system_admin, "AMB_X").await;
    let short = created_tenant_administrator(&app, &system_admin, "DBL").await;
    let long = created_tenant_administrator(&app, &system_admin, "DBL__X").await;

    let (shared, shared_value) = plant_in("AMB_X", "kelir-planted-ambiguous-7f7f");
    let (nested, nested_value) = plant_in("DBL__X", "kelir-planted-nested-8a8a");

    for (token, reference, value) in [
        (&hyphen, &shared, &shared_value),
        (&underscore, &shared, &shared_value),
        (&short, &nested, &nested_value),
        (&long, &nested, &nested_value),
    ] {
        let target = target(&app, token, &collector.base_url(), "GET", "/echo").await;
        bearer(&app, token, target.system, reference).await;
        let response = call(&app, token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{reference}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains(value.as_str()));
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
    }
    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );

    // Control: the refusal is of the name, not the tenant. `DBL`'s name that
    // `DBL__X`'s prefix does not reach is still its own.
    let (own, own_value) = plant_in("DBL", "kelir-planted-dbl-own-9b9b");
    let target = target(&app, &short, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &short, target.system, &own).await;
    let response = call(&app, &short, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {own_value}"))
    );
}

#[tokio::test]
async fn a_single_tenant_deployment_reads_only_its_tenants_namespace() {
    // A1: the rule is the same with one tenant. A name under the bare prefix,
    // as #547 allowed, is refused, and nothing is sent.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let collector = Mock::start().await;

    let bare = format!(
        "KELIR_INTEGRATION_SECRET_ERP_TOKEN_{}",
        Uuid::now_v7().simple().to_string().to_uppercase()
    );
    std::env::set_var(&bare, "kelir-planted-bare-prefix-1c1c");

    let target = target(&app, &token, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &format!("env://{bare}")).await;
    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_NAME_NOT_PERMITTED"),
        "{}",
        response.body
    );
    assert!(
        error_message(&response).contains("KELIR_INTEGRATION_SECRET_SYSTEM__"),
        "{}",
        response.body
    );
    assert!(collector.seen().is_empty());
    assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
}

#[tokio::test]
async fn a_secret_that_straddles_the_preview_cut_is_redacted_whole() {
    // Redaction runs on the whole body before the cut: cut first, and the
    // secret's first characters would sit at the end of the preview, matched
    // by no needle.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-straddle-c3c3");

    let target = target(&app, &token, &mock.base_url(), "GET", "/straddle").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let preview = response.data()["bodyPreview"]
        .as_str()
        .expect("a preview")
        .to_owned();
    assert_eq!(preview.chars().count(), 2048);
    assert_eq!(response.data()["bodyTruncated"], true);
    let head = &secret[..8];
    assert!(
        !preview.contains("Bearer ") && !preview.contains(head),
        "the start of the header survived the cut: {}",
        &preview[2000..]
    );
    assert!(!log_rows(&app, target.endpoint).await[0]
        .to_string()
        .contains(head));
}

#[tokio::test]
async fn a_test_call_writes_no_outbox_event_and_runs_no_hook() {
    // ADR-0041 §6 stays untripped: the call, answered or refused, adds no
    // `outbox_events` row and no `document_hook_executions` row.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-outbox-a1a1");

    let answered = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, answered.system, &reference).await;
    let refused = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(
        &app,
        &token,
        refused.system,
        "vault://kelir/erp/api-key#token",
    )
    .await;

    let count = |table: &'static str| {
        let pool = app.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .expect("count")
        }
    };
    let outbox_before = count("outbox_events").await;
    let hooks_before = count("document_hook_executions").await;

    assert_eq!(call(&app, &token, &answered).await.status, StatusCode::OK);
    assert_eq!(
        call(&app, &token, &refused).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    assert_eq!(count("outbox_events").await, outbox_before);
    assert_eq!(count("document_hook_executions").await, hooks_before);
    assert_eq!(count("integration_logs").await, 2, "the calls did happen");
}

#[tokio::test]
async fn a_proxy_named_in_the_environment_is_not_used() {
    // `no_proxy()` in `outbound::send`: a proxy would carry the call, and its
    // Authorization header, to an address the guard never checked. The
    // variables are process-wide, so they are set for this test only and are
    // harmless to every other test while the line holds.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let proxy = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-proxy-b2b2");

    let target = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let names = ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"];
    for name in names {
        std::env::set_var(name, proxy.base_url());
    }
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let response = call(&app, &token, &target).await;
    for name in names {
        std::env::remove_var(name);
    }

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert!(
        proxy.seen().is_empty(),
        "the call went through the proxy: {:?}",
        proxy.seen()
    );
    assert_eq!(
        mock.seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {secret}"))
    );
}

// ---------------------------------------------------------------------------
// Added with the campaign's findings (#547, 2026-09-29): a NUL in an answer,
// the product owner's two decisions — encoded echoes redacted, keys masked in
// text — and the lookup seam that makes the address rules testable.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_nul_in_the_answer_is_stored_and_answered() {
    // AC6: the credentialed request was sent, so the call has its row — even
    // when the answer holds the one character PostgreSQL will not store.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-nul-8a8a");

    for (path, expected) in [
        ("/nul", json!("before\u{FFFD}after")),
        ("/nul-json", json!({"note": "x\u{FFFD}y", "status": "ok"})),
    ] {
        let target = target(&app, &token, &mock.base_url(), "GET", path).await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.body);
        let preview = response.data()["bodyPreview"]
            .as_str()
            .expect("a preview")
            .to_owned();
        let shown: Value = serde_json::from_str(&preview).unwrap_or(Value::String(preview));
        assert_eq!(shown, expected, "{path}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1, "{path}: {rows:?}");
        assert_eq!(rows[0]["status"], "SUCCESS");
        assert_eq!(
            rows[0]["response_payload_json"]["bodyPreview"],
            response.data()["bodyPreview"],
            "{path}: the row holds what was answered"
        );
    }
}

#[tokio::test]
async fn an_encoded_echo_of_the_secret_is_redacted() {
    // The product owner's decision on #547: a system that echoes the token
    // encoded has it redacted as surely as one that echoes it as written.
    // The token has `/`, `+` and `=` so that its percent-encoding differs from
    // it.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir/planted+encoded=7a7a");

    for (path, form, shown) in [
        (
            "/jsonp-echo",
            unicode_escaped(&secret),
            r#"callback({"echo":"[REDACTED]"})"#,
        ),
        (
            "/percent-echo",
            percent_encoded(&secret),
            "https://return.example/cb?t=[REDACTED]",
        ),
        ("/base64-echo", STANDARD.encode(&secret), "seen=[REDACTED]"),
    ] {
        assert_ne!(form, secret, "{path}: the echo is encoded");
        let target = target(&app, &token, &mock.base_url(), "GET", path).await;
        bearer(&app, &token, target.system, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(response.status, StatusCode::OK, "{path}: {}", response.body);
        assert_eq!(response.data()["bodyPreview"], shown, "{path}");

        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), 1, "{path}");
        assert_eq!(
            rows[0]["response_payload_json"]["bodyPreview"], shown,
            "{path}"
        );
        // As text, too: the response and the row are JSON, so a `\u` form
        // would sit in them with its backslash doubled.
        for text in [response.body.to_string(), rows[0].to_string()] {
            for needle in [form.clone(), form.replace('\\', "\\\\"), secret.clone()] {
                assert!(!text.contains(&needle), "{path}: {needle} in {text}");
            }
        }
    }
}

#[tokio::test]
async fn a_json_body_over_the_read_cap_is_masked_by_key() {
    // The product owner's decision on #547: a body cut at 64 KiB is not
    // parsed as JSON, and its sensitive pairs are still masked in the text.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-bigjson-9b9b");

    let target = target(&app, &token, &mock.base_url(), "GET", "/big-json").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["bodyTruncated"], true);
    let preview = response.data()["bodyPreview"]
        .as_str()
        .expect("a preview")
        .to_owned();
    assert!(
        preview.starts_with(r#"{"access_token":"[REDACTED]","pad":"xxx"#),
        "{preview}"
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    for text in [response.body.to_string(), rows[0].to_string()] {
        assert!(!text.contains("issued-by-the-system-9999"), "{text}");
    }
}

/// An app whose lookup answers `name` with `addresses` (the seam
/// `AppConfig::integration_dns_overrides`), with loopback opened or not.
async fn app_resolving(name: &str, addresses: &[&str], loopback: bool) -> TestApp {
    let addresses: Vec<std::net::IpAddr> = addresses
        .iter()
        .map(|address| address.parse().expect("an address"))
        .collect();
    let name = name.to_owned();

    TestApp::spawn_with(move |config| {
        config.integration_allow_loopback = loopback;
        config.integration_dns_overrides.insert(name, addresses);
    })
    .await
}

#[tokio::test]
async fn a_name_resolving_to_a_public_and_a_private_address_is_refused() {
    // E7b: every answer must pass, not the first. 203.0.113.7 (TEST-NET-3) is
    // public and first; were it the only one judged, the call would go out to
    // it and time out instead of being refused here.
    let app = app_resolving("mixed.kelir.test", &["203.0.113.7", "10.0.0.5"], false).await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-mixed-c3c3");

    let target = target(&app, &token, "http://mixed.kelir.test", "GET", "/x").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        response.body
    );
    assert_eq!(response.error_code(), Some("EGRESS_REFUSED"));
    assert!(
        error_message(&response).contains("a private address"),
        "the refusal names the class that failed: {}",
        response.body
    );

    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["status"], "FAILED");
    assert!(rows[0]["error_message"]
        .as_str()
        .is_some_and(|message| message.starts_with("EGRESS_REFUSED")));
    assert!(
        rows[0]["request_payload_json"]["headers"]["Authorization"].is_null(),
        "nothing was sent"
    );
    for text in [response.body.to_string(), rows[0].to_string()] {
        assert!(!text.contains(&secret));
        assert!(!text.contains("10.0.0.5"), "no address is shown: {text}");
    }
}

#[tokio::test]
async fn the_connection_goes_to_the_checked_address_and_not_a_second_lookup() {
    // PR1: `pinned.kelir.test` exists only in the seam's table — `.test` is
    // never delegated (RFC 6761) — so a client that looked the name up again
    // would find nothing, and only the pinned address reaches the mock.
    let app = app_resolving("pinned.kelir.test", &["127.0.0.1"], true).await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-pinned-d4d4");
    let base_url = format!("http://pinned.kelir.test:{}", mock.address.port());

    let target = target(&app, &token, &base_url, "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["url"], format!("{base_url}/echo"));
    let seen = mock.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(
        seen[0].host.as_deref(),
        Some(format!("pinned.kelir.test:{}", mock.address.port()).as_str()),
        "the request is addressed to the name, and connected to the address"
    );
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Bearer {secret}").as_str())
    );
    assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
}

#[tokio::test]
async fn a_first_address_that_does_not_connect_falls_through_to_the_next() {
    // Dual-stack pinning: every address passed the guard, so every one is
    // pinned. 127.0.0.2 is loopback — which the seam opens — but nothing
    // listens there, so the connection is refused and the client goes on to
    // 127.0.0.1, where the mock is. Pinned to the first address alone, the
    // call would fail UPSTREAM_UNREACHABLE.
    let app = app_resolving("dual.kelir.test", &["127.0.0.2", "127.0.0.1"], true).await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, _) = plant("kelir-planted-dual-e5e5");
    let base_url = format!("http://dual.kelir.test:{}", mock.address.port());

    let target = target(&app, &token, &base_url, "GET", "/echo").await;
    // The client divides its connect timeout between the addresses, and a
    // refused loopback connection takes about two seconds on Windows: give
    // each address five.
    set_timeout(&app, target.system, 10).await;
    bearer(&app, &token, target.system, &reference).await;

    let response = call(&app, &token, &target).await;

    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(response.data()["status"], "SUCCESS");
    assert_eq!(mock.seen().len(), 1);
    assert_eq!(log_rows(&app, target.endpoint).await.len(), 1);
}

// ---------------------------------------------------------------------------
// Added by the test-engineer campaign (#618, 2026-10-01): the tests the
// campaign's mutations called for. The header's campaign table names the
// mutation each one reddens under.
// ---------------------------------------------------------------------------

/// A tenant's id, by its code.
async fn tenant_id_of(app: &TestApp, code: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM tenants WHERE tenant_code = $1")
        .bind(code)
        .fetch_one(&app.pool)
        .await
        .expect("the tenant exists")
}

/// Writes a credential's reference past the API, which refuses these at save
/// (`integration_external_systems` holds that): what a restored backup or a
/// hand-run statement could leave in the column.
async fn set_reference(app: &TestApp, credential: Uuid, reference: &str) {
    sqlx::query("UPDATE integration_credentials SET secret_reference = $2 WHERE id = $1")
        .bind(credential)
        .bind(reference)
        .execute(&app.pool)
        .await
        .expect("set the stored reference");
}

/// Writes a tenant's code past the API, which stores only upper-case
/// `A-Z 0-9 - _`.
async fn set_tenant_code(app: &TestApp, tenant: Uuid, code: &str) {
    sqlx::query("UPDATE tenants SET tenant_code = $2 WHERE id = $1")
        .bind(tenant)
        .bind(code)
        .execute(&app.pool)
        .await
        .expect("set the tenant's code");
}

#[tokio::test]
async fn a_deleted_tenants_open_session_resolves_no_secret_its_own_or_another() {
    // A deleted tenant's access token is refused before the route is reached
    // (#650, D-105; it answers #648): the call is a 401, not the 500 the
    // missing namespace used to produce. Nothing is resolved, nothing is
    // sent, and no `integration_logs` row is written, because no endpoint
    // was found for a caller who was never admitted (SDD §9.3.6).
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;
    let tenant = created_tenant_administrator(&app, &system_admin, "TNT-GONE").await;

    let (own_reference, own_value) = plant_in("TNT_GONE", "kelir-planted-deleted-own-2e2e");
    let (system_reference, system_value) = plant_in("SYSTEM", "kelir-planted-deleted-foreign-3f3f");
    let own = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, own.system, &own_reference).await;
    let foreign = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, foreign.system, &system_reference).await;

    // Control: before the deletion, its own variable resolves.
    let response = call(&app, &tenant, &own).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(collector.seen().len(), 1);

    let id = tenant_id_of(&app, "TNT-GONE").await;
    let deleted = app
        .delete(&format!("{TENANTS}/{id}"), Some(&system_admin))
        .await;
    assert!(deleted.status.is_success(), "{}", deleted.body);

    let rows_before = all_log_text(&app).await;

    for (target, value, rows_expected) in [(&own, &own_value, 1), (&foreign, &system_value, 0)] {
        let response = call(&app, &tenant, target).await;
        assert_eq!(
            response.status,
            StatusCode::UNAUTHORIZED,
            "{}",
            response.body
        );
        assert_eq!(
            response.error_code(),
            Some("UNAUTHORIZED"),
            "{}",
            response.body
        );
        assert!(!response.body.to_string().contains(value.as_str()));
        // Which names exist is not said either.
        assert!(
            !response
                .body
                .to_string()
                .contains("KELIR_INTEGRATION_SECRET_"),
            "{}",
            response.body
        );
        // The control's one row for its own endpoint, and no other.
        assert_eq!(
            log_rows(&app, target.endpoint).await.len(),
            rows_expected,
            "a refused caller's call wrote a row"
        );
    }
    assert_eq!(
        all_log_text(&app).await,
        rows_before,
        "integration_logs changed under a refused caller"
    );
    assert_eq!(
        collector.seen().len(),
        1,
        "the collector received {:?}",
        collector.seen()
    );
    assert!(!all_log_text(&app).await.contains(&system_value));
    assert!(!all_log_text(&app).await.contains(&own_value));
}

/// The caller as the extractor admits them: `Authenticated` cannot be built
/// any other way, so this is the request's own admission, taken while the
/// token's tenant and user are live.
async fn admitted(app: &TestApp, token: &str) -> kelir_backend::middleware::auth::Authenticated {
    use axum::extract::FromRequestParts;

    let (mut parts, ()) = axum::http::Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(())
        .expect("a request")
        .into_parts();
    parts
        .extensions
        .insert(axum::extract::ConnectInfo(common::TEST_PEER));

    kelir_backend::middleware::auth::Authenticated::from_request_parts(&mut parts, &app.state)
        .await
        .expect("a live tenant's token is admitted")
}

/// **The race #650's criterion 8 leaves** (`test-engineer` campaign,
/// 2026-10-02). A deleted tenant's token is a 401 before the route, so no
/// request reaches `service::test_call`'s "the caller's tenant is not live"
/// branch. A tenant deleted *between* the extractor's read and the namespace
/// read does, and that is staged here as it happens: the caller is admitted
/// by the extractor while the tenant is live, the tenant is deleted, and the
/// service is called with the admitted caller.
///
/// It fails closed: an `INTERNAL_ERROR`, one `FAILED` row that names no
/// variable and holds no value, nothing sent, and neither the tenant's own
/// variable nor another tenant's read.
///
/// Planned and not run (the campaign's cargo runs were stopped for memory):
/// `TenantNamespaces::for_caller` answering the first live tenant's namespace
/// when the caller's is not among them, which the builder's table records as
/// green in every suite; and the branch answering with an empty namespace
/// instead of failing.
#[tokio::test]
async fn a_tenant_deleted_after_its_caller_was_admitted_resolves_nothing_and_fails_closed() {
    use kelir_backend::modules::integration::service::test_call::test_call;

    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;
    let tenant = created_tenant_administrator(&app, &system_admin, "TNT-RACE").await;

    let (own_reference, own_value) = plant_in("TNT_RACE", "kelir-planted-raced-own-7a7a");
    let (system_reference, system_value) = plant_in("SYSTEM", "kelir-planted-raced-foreign-8b8b");
    let own = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, own.system, &own_reference).await;
    let foreign = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, foreign.system, &system_reference).await;

    // Admitted while live, and the service called directly serves them: the
    // control for everything refused below.
    let caller = admitted(&app, &tenant).await;
    let served = test_call(&app.state, &caller, own.system, own.endpoint)
        .await
        .expect("a live tenant's call is made");
    assert_eq!(served.status_code, 200);
    assert_eq!(collector.seen().len(), 1);

    // The tenant goes between the admission and the call.
    let id = tenant_id_of(&app, "TNT-RACE").await;
    let deleted = app
        .delete(&format!("{TENANTS}/{id}"), Some(&system_admin))
        .await;
    assert!(deleted.status.is_success(), "{}", deleted.body);

    for (target, rows_expected) in [(&own, 2), (&foreign, 1)] {
        let error = test_call(&app.state, &caller, target.system, target.endpoint)
            .await
            .expect_err("a caller whose tenant is gone is not served");
        assert_eq!(error.code(), "INTERNAL_ERROR", "{error:?}");

        // Exactly one row for the call, and it says a call failed and no more.
        let rows = log_rows(&app, target.endpoint).await;
        assert_eq!(rows.len(), rows_expected, "{rows:?}");
        let row = rows.last().expect("the failed call's row");
        assert_eq!(row["status"], "FAILED", "{row}");
        assert_eq!(
            row["error_message"], "INTERNAL_ERROR: the call could not be completed",
            "{row}"
        );
        assert_eq!(row["status_code"], Value::Null, "{row}");
        assert_eq!(
            row["request_payload_json"]["headers"]["Authorization"],
            Value::Null,
            "the row says a credential was attached: {row}"
        );
        assert!(
            !row.to_string().contains("KELIR_INTEGRATION_SECRET_"),
            "the row names a variable: {row}"
        );
    }

    assert_eq!(
        collector.seen().len(),
        1,
        "the collector received {:?}",
        collector.seen()
    );
    let logged = all_log_text(&app).await;
    for value in [&own_value, &system_value] {
        assert!(!logged.contains(value.as_str()), "a row holds a secret");
        assert_eq!(audit_rows_containing(&app, value).await, 0);
    }
}

#[tokio::test]
async fn a_tenant_that_is_not_active_still_holds_its_names_and_a_deleted_one_does_not() {
    // The ambiguity rule counts every tenant that is not deleted, whatever
    // its status: a suspended or inactive tenant can be made active again,
    // and its variables stay in the environment meanwhile. So `SUS_X` cannot
    // read `..._SUS_X__...` while `SUS-X` is suspended or inactive.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;

    let _hyphen = created_tenant_administrator(&app, &system_admin, "SUS-X").await;
    let underscore = created_tenant_administrator(&app, &system_admin, "SUS_X").await;
    let (shared, shared_value) = plant_in("SUS_X", "kelir-planted-suspended-4a4a");
    let id = tenant_id_of(&app, "SUS-X").await;

    for status in ["SUSPENDED", "INACTIVE"] {
        let updated = app
            .put(
                &format!("{TENANTS}/{id}"),
                Some(&system_admin),
                json!({ "status": status }),
            )
            .await;
        assert_eq!(updated.status, StatusCode::OK, "{}", updated.body);

        let target = target(&app, &underscore, &collector.base_url(), "GET", "/echo").await;
        bearer(&app, &underscore, target.system, &shared).await;
        let response = call(&app, &underscore, &target).await;

        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{status}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains(shared_value.as_str()));
    }
    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );

    // The rule as decided: only a live tenant holds a name. Once `SUS-X` is
    // deleted the name is `SUS_X`'s alone, and is read.
    let deleted = app
        .delete(&format!("{TENANTS}/{id}"), Some(&system_admin))
        .await;
    assert!(deleted.status.is_success(), "{}", deleted.body);

    let target = target(&app, &underscore, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &underscore, target.system, &shared).await;
    let response = call(&app, &underscore, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {shared_value}"))
    );
}

#[tokio::test]
async fn a_tenants_prefix_with_nothing_after_it_is_refused_set_or_not() {
    // `env://KELIR_INTEGRATION_SECRET_SYSTEM__` has a reference's shape, so the
    // registry stores it, and a variable of that name can be set. It names
    // nothing in the tenant's namespace, and neither does the code with one
    // underscore, with none, or the bare prefix: each is refused by name, the
    // variable set, and nothing is sent.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let collector = Mock::start().await;
    let value = "kelir-planted-nameless-5b5b";

    for name in [
        "KELIR_INTEGRATION_SECRET_SYSTEM__",
        "KELIR_INTEGRATION_SECRET_SYSTEM_",
        "KELIR_INTEGRATION_SECRET_SYSTEM",
        "KELIR_INTEGRATION_SECRET_",
    ] {
        std::env::set_var(name, value);

        let target = target(&app, &token, &collector.base_url(), "GET", "/echo").await;
        bearer(&app, &token, target.system, &format!("env://{name}")).await;
        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{name}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains(value), "{name}");
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{name}");
    }

    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );
    assert!(!all_log_text(&app).await.contains(value));
}

#[tokio::test]
async fn a_stored_reference_outside_the_name_alphabet_is_malformed_and_reads_nothing() {
    // Why no request reaches `TenantNamespaces::admits`'s own alphabet check:
    // a name outside `A-Z 0-9 _` is refused at save, and one written past the
    // API is refused when the call parses it, before the tenant check and
    // before the environment. Each variable below is set, in the system
    // tenant's own namespace, and none is read.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let collector = Mock::start().await;
    let value = "kelir-planted-off-alphabet-6c6c";
    let unique = Uuid::now_v7().simple().to_string();

    let names = [
        format!("KELIR_INTEGRATION_SECRET_SYSTEM__test_{unique}"),
        format!(
            "KELIR_INTEGRATION_SECRET_SYSTEM__TEST-{}",
            unique.to_uppercase()
        ),
        format!(
            "KELIR_INTEGRATION_SECRET_SYSTEM__TEST.{}",
            unique.to_uppercase()
        ),
        format!(
            "KELIR_INTEGRATION_SECRET_SYSTEM__TEST {}",
            unique.to_uppercase()
        ),
    ];

    for name in &names {
        std::env::set_var(name, value);
        let reference = format!("env://{name}");

        let target = target(&app, &token, &collector.base_url(), "GET", "/echo").await;
        let refused = app
            .post(
                &format!("{BASE}/{}/credentials", target.system),
                Some(&token),
                json!({ "credentialType": "BEARER_TOKEN", "secretReference": reference }),
            )
            .await;
        assert_eq!(
            refused.body["error"]["details"][0]["code"], "NOT_A_SECRET_REFERENCE",
            "{name}: {}",
            refused.body
        );

        let credential = bearer(
            &app,
            &token,
            target.system,
            "env://KELIR_INTEGRATION_SECRET_SYSTEM__TO_BE_OVERWRITTEN",
        )
        .await;
        set_reference(&app, credential, &reference).await;

        let response = call(&app, &token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("SECRET_REFERENCE_MALFORMED"),
            "{name}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains(value), "{name}");
        assert_eq!(log_rows(&app, target.endpoint).await.len(), 1, "{name}");
    }

    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );
    assert!(!all_log_text(&app).await.contains(value));
}

#[tokio::test]
async fn a_variable_set_to_nothing_is_not_found_and_nothing_is_sent() {
    // An empty value is no secret: the call fails as it does for a variable
    // that is not set, rather than sending `Authorization: Bearer ` or naming
    // the value malformed.
    let app = app_reaching_loopback().await;
    let token = app.administrator_token().await;
    let collector = Mock::start().await;
    let (reference, _) = plant("");
    let name = reference.trim_start_matches("env://").to_owned();

    let target = target(&app, &token, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &token, target.system, &reference).await;
    let response = call(&app, &token, &target).await;

    assert_eq!(
        response.error_code(),
        Some("SECRET_NOT_FOUND"),
        "{}",
        response.body
    );
    assert!(error_message(&response).contains(&name));
    assert!(collector.seen().is_empty());
    let rows = log_rows(&app, target.endpoint).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["request_payload_json"]["headers"]["Authorization"].is_null());
}

#[tokio::test]
async fn a_code_ending_in_an_underscore_and_the_code_without_it_share_no_name() {
    // `TRL_` + `__` + `X` and `TRL` + `__` + `_X` are the same variable. The
    // separator does not tell them apart, so the ambiguity rule must: every
    // name of `TRL_`'s is also one of `TRL`'s, and is read by neither.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;

    let short = created_tenant_administrator(&app, &system_admin, "TRL").await;
    let long = created_tenant_administrator(&app, &system_admin, "TRL_").await;

    let (shared, shared_value) = plant_in("TRL_", "kelir-planted-trailing-7d7d");
    assert!(shared.contains("_TRL___TEST_"), "{shared}");
    let (own, own_value) = plant_in("TRL", "kelir-planted-trl-own-8e8e");

    for (token, reference, prefix) in [
        (&short, &shared, "KELIR_INTEGRATION_SECRET_TRL__"),
        (&long, &shared, "KELIR_INTEGRATION_SECRET_TRL___"),
        // `TRL`'s own name is not under `TRL_`'s prefix at all.
        (&long, &own, "KELIR_INTEGRATION_SECRET_TRL___"),
    ] {
        let target = target(&app, token, &collector.base_url(), "GET", "/echo").await;
        bearer(&app, token, target.system, reference).await;
        let response = call(&app, token, &target).await;

        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{reference}: {}",
            response.body
        );
        // The refusal names the caller's own prefix, whole, and no more.
        let message = error_message(&response);
        assert!(message.contains(&format!("{prefix}<NAME>")), "{message}");
        for value in [&shared_value, &own_value] {
            assert!(!response.body.to_string().contains(value.as_str()));
        }
    }
    assert!(
        collector.seen().is_empty(),
        "the collector received {:?}",
        collector.seen()
    );

    // Control: `TRL`'s name that does not begin with an underscore is its own.
    let target = target(&app, &short, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &short, target.system, &own).await;
    let response = call(&app, &short, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {own_value}"))
    );
}

#[tokio::test]
async fn a_code_no_route_stores_reads_upper_case_names_or_nothing() {
    // The route stores a code upper case, of `A-Z 0-9 - _`, and refuses any
    // other. A code written past it is still held to the rule: a lower-case
    // one reads its upper-case namespace (variable names are upper case), two
    // codes that differ only in case share every name and read none, and a
    // code with a character no variable name holds reads nothing at all.
    let app = multi_tenant_app_reaching_loopback().await;
    let system_admin = app
        .sign_in_to("SYSTEM", common::ADMIN_USERNAME, common::ADMIN_PASSWORD)
        .await;
    let collector = Mock::start().await;

    for code in ["ODD.CO", "ODD CO", "ODD/CO", "ÓDD-CO", "ODD__CO\n1"] {
        let refused = app
            .post(
                TENANTS,
                Some(&system_admin),
                json!({
                    "tenantCode": code,
                    "name": "Odd",
                    "administrator": {
                        "username": "admin.odd",
                        "email": "admin.odd@example.test",
                        "displayName": "Tenant Administrator",
                        "password": TENANT_PASSWORD,
                    },
                }),
            )
            .await;
        assert!(
            refused.status.is_client_error(),
            "{code:?}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"][0]["code"], "INVALID_FORMAT",
            "{code:?}: {}",
            refused.body
        );
    }

    // Typed in lower case, the route stores it upper case.
    let tenant = created_tenant_administrator(&app, &system_admin, "odd-co").await;
    let id = tenant_id_of(&app, "ODD-CO").await;

    let (reference, value) = plant_in("ODD_CO", "kelir-planted-odd-code-9f9f");
    let target = target(&app, &tenant, &collector.base_url(), "GET", "/echo").await;
    bearer(&app, &tenant, target.system, &reference).await;

    // Lower case in the column: the same, upper-case, namespace.
    set_tenant_code(&app, id, "odd-co").await;
    let response = call(&app, &tenant, &target).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(
        collector
            .seen()
            .last()
            .and_then(|seen| seen.authorization.clone()),
        Some(format!("Bearer {value}"))
    );

    // A second tenant whose code differs only in case: the name is both's,
    // and neither's.
    let twin = fixtures::create_tenant(&app.pool, "ODD-CO", "Odd twin").await;
    let response = call(&app, &tenant, &target).await;
    assert_eq!(
        response.error_code(),
        Some("SECRET_NAME_NOT_PERMITTED"),
        "{}",
        response.body
    );
    set_tenant_code(&app, twin, "ODD-CO-TWIN").await;

    // A character no variable name can hold: nothing is this tenant's.
    for code in ["ODD.CO", "ODD CO", "ÓDD_CO"] {
        set_tenant_code(&app, id, code).await;
        let response = call(&app, &tenant, &target).await;
        assert_eq!(
            response.error_code(),
            Some("SECRET_NAME_NOT_PERMITTED"),
            "{code}: {}",
            response.body
        );
        assert!(!response.body.to_string().contains(&value), "{code}");
    }

    assert_eq!(
        collector.seen().len(),
        1,
        "the collector received {:?}",
        collector.seen()
    );
}

// ---------------------------------------------------------------------------
// Added by the test-engineer campaign (#622, 2026-10-01): the spellings, the
// names, the neighbours and the edges of `::/96` that the campaign's
// mutations called for. The header's campaign table names the mutation each
// one reddens under.
// ---------------------------------------------------------------------------

/// [`app_listing_everything`]'s list, and a host route to each metadata
/// address in every form a list can write one: the widest list and the
/// narrowest.
fn everything_and_each_metadata_address() -> Vec<kelir_backend::utils::cidr::Cidr> {
    [
        "fd00::/8",
        "100.64.0.0/10",
        "192.0.0.0/24",
        "169.254.0.0/16",
        "0.0.0.0/0",
        "::/0",
        "100.100.100.200/32",
        "192.0.0.192/32",
        "fd00:ec2::254/128",
        "169.254.169.254/32",
        "::ffff:100.100.100.200/128",
        "::100.100.100.200/128",
    ]
    .iter()
    .map(|cidr| cidr.parse().expect("a CIDR"))
    .collect()
}

/// Calls `target` and holds the answer to the whole of a refusal: `422
/// EGRESS_REFUSED` naming `class`, one `FAILED` row saying the same, no
/// `Authorization` recorded as sent, and the secret nowhere. Returns the
/// message.
async fn refused_as(
    app: &TestApp,
    token: &str,
    target: &Target,
    class: &str,
    secret: &str,
    label: &str,
) -> String {
    let response = call(app, token, target).await;

    assert_eq!(
        response.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{label}: {}",
        response.body
    );
    assert_eq!(response.error_code(), Some("EGRESS_REFUSED"), "{label}");
    let message = error_message(&response);
    assert!(message.contains(class), "{label}: {message}");

    let rows = log_rows(app, target.endpoint).await;
    assert_eq!(rows.len(), 1, "{label}: {rows:?}");
    assert_eq!(rows[0]["status"], "FAILED", "{label}");
    // The caller's message ends with the row's id; the row carries the rest.
    let told = message.split(" (integration log ").next().unwrap_or("");
    assert_eq!(
        rows[0]["error_message"].as_str(),
        Some(format!("EGRESS_REFUSED: {told}").as_str()),
        "{label}: the row says what the caller was told"
    );
    assert!(
        rows[0]["request_payload_json"]["headers"]["Authorization"].is_null(),
        "{label}: nothing was sent"
    );
    for text in [response.body.to_string(), rows[0].to_string()] {
        assert!(!text.contains(secret), "{label}: {text}");
    }

    message
}

/// A call to `base`, which is an address nothing answers at from a test
/// runner: past the guard, the call ends at the network within its
/// one-second budget, with any answer but `EGRESS_REFUSED`.
async fn call_to(app: &TestApp, token: &str, reference: &str, base: &str) -> TestResponse {
    let target = target(app, token, base, "GET", "/x").await;
    set_timeout(app, target.system, 1).await;
    bearer(app, token, target.system, reference).await;

    call(app, token, &target).await
}

#[tokio::test]
async fn a_metadata_address_is_refused_in_every_spelling_a_url_host_can_take() {
    // The URL parser reads each of these as one address before the guard
    // sees it; the guard then reads the IPv6 forms that carry an IPv4
    // address. Everything is listed, each address by its own host route too.
    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = everything_and_each_metadata_address();
    })
    .await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-spelling-8d13");

    let metadata = "a cloud metadata address";
    let link_local = "a link-local address";

    for (base, class) in [
        // 100.100.100.200: one number, hex, upper-case hex, octal, the short
        // forms, mixed radix, a trailing dot, percent-encoded, full-width
        // digits and the ideographic full stop.
        ("http://1684301000", metadata),
        ("http://0x646464c8", metadata),
        ("http://0x64.0x64.0x64.0xc8", metadata),
        ("http://0X64.0X64.0X64.0XC8", metadata),
        ("http://0144.0144.0144.0310", metadata),
        ("http://100.100.25800", metadata),
        ("http://100.6579400", metadata),
        ("http://0x64.100.0144.200", metadata),
        ("http://100.100.100.200.", metadata),
        ("http://100.100.100.200:80", metadata),
        ("http://%31%30%30.100.100.200", metadata),
        ("http://１００.１００.１００.２００", metadata),
        ("http://100。100。100。200", metadata),
        ("https://100.100.100.200", metadata),
        // The same address inside an IPv6 literal.
        ("http://[::FFFF:6464:64C8]", metadata),
        ("http://[0:0:0:0:0:ffff:6464:64c8]", metadata),
        ("http://[::6464:64c8]", metadata),
        ("http://[0000:0000:0000:0000:0000:0000:6464:64c8]", metadata),
        // 192.0.0.192.
        ("http://3221225664", metadata),
        ("http://0xc0.0.0.0xc0", metadata),
        ("http://0300.0.0.0300", metadata),
        ("http://192.192", metadata),
        ("http://192.0.192", metadata),
        ("http://192.0.0.192.", metadata),
        ("http://[::ffff:c000:c0]", metadata),
        ("http://[::192.0.0.192]", metadata),
        ("http://[::c000:c0]", metadata),
        // fd00:ec2::254.
        ("http://[FD00:EC2::254]", metadata),
        ("http://[fd00:0ec2:0000:0000:0000:0000:0000:0254]", metadata),
        ("http://[fd00:ec2:0:0:0:0:0:254]", metadata),
        ("http://[fd00:ec2::0.0.2.84]", metadata),
        ("http://[fd00:ec2::254]:8080", metadata),
        // The address ADR-0043 named first, in the same spellings: link-local,
        // and no setting is offered for it either.
        ("http://2852039166", link_local),
        ("http://0xa9.0xfe.0xa9.0xfe", link_local),
        ("http://0251.0376.0251.0376", link_local),
        ("http://169.254.43518", link_local),
        ("http://169.254.169.254.", link_local),
        ("http://[::ffff:a9fe:a9fe]", link_local),
        ("http://[::a9fe:a9fe]", link_local),
    ] {
        let target = target(&app, &token, base, "GET", "/latest/meta-data").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let message = refused_as(&app, &token, &target, class, &secret, base).await;

        assert!(
            !message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{base}: no setting opens it, so none is offered: {message}"
        );
    }

    // A zone id is not a host the API stores, so there is nothing to call.
    for base in ["http://[fd00:ec2::254%25eth0]", "http://[fe80::1%eth0]"] {
        let response = app
            .post(
                BASE,
                Some(&token),
                json!({
                    "systemCode": format!("SYS_{}", Uuid::now_v7().simple()),
                    "systemName": "Zoned",
                    "baseUrl": base,
                }),
            )
            .await;

        assert_eq!(
            response.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{base}: {}",
            response.body
        );
        assert_eq!(response.error_code(), Some("VALIDATION_ERROR"), "{base}");
    }
}

#[tokio::test]
async fn a_name_resolving_to_a_metadata_address_in_any_form_is_refused() {
    // The zone's answer is judged as a literal is: each of the three, alone
    // and behind a public address, and in the IPv6 forms a resolver may give
    // an IPv4 answer in. 203.0.113.7 (TEST-NET-3) is public. The third
    // member is text the refusal must not show.
    let names: [(&str, &[&str], &str); 9] = [
        (
            "alibaba.kelir.test",
            &["100.100.100.200"],
            "100.100.100.200",
        ),
        ("oracle.kelir.test", &["192.0.0.192"], "192.0.0.192"),
        (
            "oracle-second.kelir.test",
            &["203.0.113.7", "192.0.0.192"],
            "192.0.0.192",
        ),
        ("aws.kelir.test", &["fd00:ec2::254"], "fd00:ec2::254"),
        (
            "aws-third.kelir.test",
            &["203.0.113.7", "2001:db8::7", "fd00:ec2::254"],
            "fd00:ec2::254",
        ),
        (
            "mapped.kelir.test",
            &["::ffff:100.100.100.200"],
            "100.100.100.200",
        ),
        (
            "compatible.kelir.test",
            &["203.0.113.7", "::192.0.0.192"],
            "c000:c0",
        ),
        (
            "first.kelir.test",
            &["100.100.100.200", "203.0.113.7"],
            "100.100.100.200",
        ),
        // A private answer the list opens, then the address no list opens.
        (
            "private-then-metadata.kelir.test",
            &["fd00:ec2::253", "fd00:ec2::254"],
            "fd00:ec2::25",
        ),
    ];

    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = everything_and_each_metadata_address();
        for (name, addresses, _) in names {
            config.integration_dns_overrides.insert(
                name.to_owned(),
                addresses
                    .iter()
                    .map(|address| address.parse().expect("an address"))
                    .collect(),
            );
        }
    })
    .await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-zone-3f6b");

    for (name, _, address) in names {
        // The lookup is by name, whatever case the URL writes it in.
        let base = format!("http://{}", name.to_uppercase());
        let target = target(&app, &token, &base, "GET", "/latest/meta-data").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let message = refused_as(
            &app,
            &token,
            &target,
            "a cloud metadata address",
            &secret,
            name,
        )
        .await;

        assert!(
            !message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{name}: {message}"
        );
        let row = log_rows(&app, target.endpoint).await[0].to_string();
        for text in [message, row] {
            assert!(
                !text.contains(address),
                "{name}: no address is shown: {text}"
            );
        }
    }
}

#[tokio::test]
async fn the_test_seam_does_not_open_a_metadata_address() {
    // `integration_allow_loopback` opens loopback for the mock and nothing
    // else: a metadata address is refused under it, by literal and by name,
    // and the mock on loopback is still reached.
    let app = TestApp::spawn_with(|config| {
        config.integration_allow_loopback = true;
        config.integration_dns_overrides.insert(
            "seam.kelir.test".to_owned(),
            vec![
                "127.0.0.1".parse().expect("an address"),
                "192.0.0.192".parse().expect("an address"),
            ],
        );
    })
    .await;
    let token = app.administrator_token().await;
    let mock = Mock::start().await;
    let (reference, secret) = plant("kelir-planted-seam-a41c");

    for base in [
        "http://100.100.100.200".to_owned(),
        "http://192.0.0.192".to_owned(),
        "http://[fd00:ec2::254]".to_owned(),
        "http://[::100.100.100.200]".to_owned(),
        format!("http://seam.kelir.test:{}", mock.address.port()),
    ] {
        let target = target(&app, &token, &base, "GET", "/echo").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        refused_as(
            &app,
            &token,
            &target,
            "a cloud metadata address",
            &secret,
            &base,
        )
        .await;
    }
    assert!(mock.seen().is_empty(), "{:?}", mock.seen());

    let open = target(&app, &token, &mock.base_url(), "GET", "/echo").await;
    bearer(&app, &token, open.system, &reference).await;
    let response = call(&app, &token, &open).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    assert_eq!(mock.seen().len(), 1);
}

#[tokio::test]
async fn the_addresses_beside_a_metadata_address_are_judged_by_their_range() {
    // The three are addresses, not ranges. With nothing listed, the IPv4
    // neighbours are public and go out, and the rest of AWS's
    // `fd00:ec2::/32` is unique-local: refused as private, the setting
    // offered.
    let app = TestApp::spawn().await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-neighbour-c27e");

    for base in [
        "http://100.100.100.199",
        "http://100.100.100.201",
        "http://192.0.0.191",
        "http://192.0.0.193",
    ] {
        let response = call_to(&app, &token, &reference, base).await;

        assert_ne!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base} is public: {}",
            response.body
        );
    }

    let aws_neighbours = [
        "http://[fd00:ec2::253]",
        "http://[fd00:ec2::255]",
        "http://[fd00:ec2::23]",
        "http://[fd00:ec2::fe]",
        "http://[fd00:ec2:0:0:1::254]",
    ];
    for base in aws_neighbours {
        let target = target(&app, &token, base, "GET", "/x").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let message = refused_as(&app, &token, &target, "a private address", &secret, base).await;

        assert!(
            message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{base}: a listed range opens it, so the setting is offered: {message}"
        );
    }

    // Installation §7.1's example opens every one of them, and not the
    // metadata address among them.
    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = vec!["fd00::/8".parse().expect("a CIDR")];
    })
    .await;
    let token = app.administrator_token().await;

    for base in aws_neighbours {
        let response = call_to(&app, &token, &reference, base).await;

        assert_ne!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base} is listed: {}",
            response.body
        );
    }

    let target = target(&app, &token, "http://[fd00:ec2::254]", "GET", "/x").await;
    set_timeout(&app, target.system, 1).await;
    bearer(&app, &token, target.system, &reference).await;
    refused_as(
        &app,
        &token,
        &target,
        "a cloud metadata address",
        &secret,
        "fd00:ec2::254 under fd00::/8",
    )
    .await;
}

#[tokio::test]
async fn the_ipv4_compatible_range_ends_where_it_is_written_to() {
    // `::/96` less `::` and `::1`. Inside it an address is the IPv4 address
    // it carries; one bit above it, an IPv6 address like any other.
    let app = TestApp::spawn_with(|config| {
        config.integration_allowed_cidrs = [
            "10.255.255.0/24",
            // Written in the forms the guard unwraps: neither opens the IPv4
            // range it spells, and `::/0` holds no IPv4 address in any form.
            "::10.255.254.0/120",
            "::ffff:10.255.254.0/120",
            "::/0",
        ]
        .iter()
        .map(|cidr| cidr.parse().expect("a CIDR"))
        .collect();
    })
    .await;
    let token = app.administrator_token().await;
    let (reference, secret) = plant("kelir-planted-edge-e90f");

    for (base, class) in [
        ("http://[::]", "an unspecified address"),
        ("http://[::1]", "a loopback address"),
        ("http://[::0.0.0.1]", "a loopback address"),
        // The lowest address the range carries is 0.0.0.2, in 0.0.0.0/8.
        ("http://[::2]", "an unspecified address"),
        ("http://[::0.0.255.255]", "an unspecified address"),
        ("http://[::0.255.255.255]", "an unspecified address"),
        // And the highest is 255.255.255.255.
        ("http://[::ffff:ffff]", "a multicast or broadcast address"),
        ("http://[::224.0.0.1]", "a multicast or broadcast address"),
        ("http://[::127.255.255.255]", "a loopback address"),
        ("http://[::169.254.0.1]", "a link-local address"),
    ] {
        let target = target(&app, &token, base, "GET", "/x").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let message = refused_as(&app, &token, &target, class, &secret, base).await;

        assert!(
            !message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{base}: {message}"
        );
    }

    // A private address in either IPv6 form is refused unless its IPv4 range
    // is listed, and a list entry written in an IPv6 form lists nothing.
    for base in [
        "http://[::10.255.254.1]",
        "http://[::ffff:10.255.254.1]",
        "http://10.255.254.1",
    ] {
        let target = target(&app, &token, base, "GET", "/x").await;
        set_timeout(&app, target.system, 1).await;
        bearer(&app, &token, target.system, &reference).await;

        let message = refused_as(&app, &token, &target, "a private address", &secret, base).await;

        assert!(
            message.contains("KELIR_INTEGRATION_ALLOWED_CIDRS"),
            "{base}: {message}"
        );
    }

    for base in [
        // Listed by its IPv4 range, in both IPv6 forms.
        "http://[::10.255.255.1]",
        "http://[::ffff:10.255.255.1]",
        // One bit above the /96, carrying loopback, a metadata address and a
        // private address: none is an IPv4 address.
        "http://[::1:7f00:1]",
        "http://[::1:6464:64c8]",
        "http://[::1:aff:fe01]",
    ] {
        let response = call_to(&app, &token, &reference, base).await;

        assert_ne!(
            response.error_code(),
            Some("EGRESS_REFUSED"),
            "{base}: {}",
            response.body
        );
    }
}
