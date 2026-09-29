//! An administrator's test call to one endpoint (FR-INT-002, #547; ADR-0043):
//! its answer, the failures it names, and the pure rules the service applies —
//! which credential, which URL, and what of a body may be shown or stored.

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use super::egress::AddressClass;
use super::external_system::AuthType;
use super::secret::Secret;
use super::{HttpMethod, IntegrationCredential};
use crate::error::AppError;

/// How much of a response body is shown, and stored, in characters.
pub const PREVIEW_CHARACTERS: usize = 2048;

/// How much of a response body is read at all, in bytes. A body longer than
/// this is cut here and the rest is never read, so a large download costs the
/// server at most this much memory.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// What replaces a masked value or a redacted secret.
pub const REDACTED: &str = "[REDACTED]";

/// `integration_logs.status` for a test call: the two terminal values of §12.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TestCallStatus {
    /// The system answered with a `2xx`.
    Success,
    /// The system answered with anything else. A call that got no answer is
    /// not a `FAILED` result but an error response; its log row is `FAILED`.
    Failed,
}

impl TestCallStatus {
    pub fn for_status_code(code: u16) -> Self {
        if (200..300).contains(&code) {
            Self::Success
        } else {
            Self::Failed
        }
    }

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Failed => "FAILED",
        }
    }
}

/// A test call's answer, for a call the system answered.
///
/// **No headers**, the system's or the call's: the request's `Authorization`
/// is the secret, and a response header can echo it. The body preview has the
/// secret redacted in every form it was sent in, and sensitive JSON keys
/// masked.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestCallResponse {
    /// The `integration_logs` row this call wrote.
    pub log_id: Uuid,
    pub method: HttpMethod,
    /// The system's `baseUrl` joined with the endpoint's `path` — what was
    /// called.
    pub url: String,
    /// `SUCCESS` for a `2xx`, `FAILED` for anything else.
    pub status: TestCallStatus,
    /// The system's HTTP status. A `3xx` is returned as it came: redirects are
    /// not followed.
    pub status_code: u16,
    pub duration_ms: i64,
    /// The start of the response body, at most 2048 characters, masked.
    pub body_preview: String,
    /// Whether `bodyPreview` is shorter than the body.
    pub body_truncated: bool,
}

/// Why a test call did not produce an answer — each a code a client branches on.
///
/// Named for the failure (coding standard §2.3). **No variant carries the
/// secret or the resolved address**: every message here is returned to the
/// caller and written to `integration_logs.error_message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestCallError {
    SystemNotActive,
    EndpointNotActive,
    BaseUrlMissing,
    TargetUrlInvalid,
    NoUsableCredential,
    AmbiguousCredential { count: usize },
    CredentialTypeNotSupported(AuthType),
    SecretReferenceMalformed,
    SecretNameNotPermitted,
    SecretBackendNotConfigured,
    SecretNotFound { name: String },
    SecretMalformed(&'static str),
    HostNotResolved,
    EgressRefused(AddressClass),
    UpstreamTimeout { seconds: i32 },
    UpstreamUnreachable,
}

impl TestCallError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SystemNotActive => "EXTERNAL_SYSTEM_NOT_ACTIVE",
            Self::EndpointNotActive => "ENDPOINT_NOT_ACTIVE",
            Self::BaseUrlMissing => "BASE_URL_MISSING",
            Self::TargetUrlInvalid => "TARGET_URL_INVALID",
            Self::NoUsableCredential => "NO_USABLE_CREDENTIAL",
            Self::AmbiguousCredential { .. } => "AMBIGUOUS_CREDENTIAL",
            Self::CredentialTypeNotSupported(_) => "CREDENTIAL_TYPE_NOT_SUPPORTED",
            Self::SecretReferenceMalformed => "SECRET_REFERENCE_MALFORMED",
            Self::SecretNameNotPermitted => "SECRET_NAME_NOT_PERMITTED",
            Self::SecretBackendNotConfigured => "SECRET_BACKEND_NOT_CONFIGURED",
            Self::SecretNotFound { .. } => "SECRET_NOT_FOUND",
            Self::SecretMalformed(_) => "SECRET_MALFORMED",
            Self::HostNotResolved => "HOST_NOT_RESOLVED",
            Self::EgressRefused(_) => "EGRESS_REFUSED",
            Self::UpstreamTimeout { .. } => "UPSTREAM_TIMEOUT",
            Self::UpstreamUnreachable => "UPSTREAM_UNREACHABLE",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::SystemNotActive => {
                "The external system is not ACTIVE, so its endpoints are not called".to_owned()
            }
            Self::EndpointNotActive => "The endpoint is not ACTIVE".to_owned(),
            Self::BaseUrlMissing => {
                "The external system has no baseUrl, so the endpoint has nowhere to be called"
                    .to_owned()
            }
            Self::TargetUrlInvalid => {
                "The system's baseUrl and the endpoint's path do not make a URL on that system"
                    .to_owned()
            }
            Self::NoUsableCredential => "The external system has no credential that is active \
                 and valid today; a test call needs exactly one"
                .to_owned(),
            Self::AmbiguousCredential { count } => format!(
                "The external system has {count} credentials that are active and valid today; \
                 a test call needs exactly one, so deactivate or date the others"
            ),
            Self::CredentialTypeNotSupported(kind) => format!(
                "A test call attaches BEARER_TOKEN and BASIC_AUTH credentials only; {} is not built",
                kind.as_db()
            ),
            Self::SecretReferenceMalformed => {
                "The credential's secretReference is not a reference a resolver can follow"
                    .to_owned()
            }
            Self::SecretNameNotPermitted => format!(
                "The credential's env:// reference names a variable outside {}*, which is the                  only part of the environment a test call reads",
                super::secret::RESOLVABLE_ENVIRONMENT_PREFIX
            ),
            Self::SecretBackendNotConfigured => "The credential is a vault:// reference, and no \
                 Vault backend is configured in this release; use an env:// reference"
                .to_owned(),
            Self::SecretNotFound { name } => {
                format!("The environment variable {name} is not set, or is empty")
            }
            Self::SecretMalformed(why) => format!("The resolved secret {why}"),
            Self::HostNotResolved => "The system's host name did not resolve".to_owned(),
            Self::EgressRefused(class) => format!(
                "The system's host resolves to {class}, which a test call may not reach{}",
                if *class == AddressClass::Private {
                    " unless KELIR_INTEGRATION_ALLOWED_CIDRS lists its range"
                } else {
                    ""
                }
            ),
            Self::UpstreamTimeout { seconds } => {
                format!("The system did not answer within its timeout of {seconds} seconds")
            }
            Self::UpstreamUnreachable => {
                "The system could not be reached: the connection failed or was closed".to_owned()
            }
        }
    }

    /// The response for this failure. `log_id` is the row that recorded it,
    /// named in the message so the administrator can find it.
    pub fn into_app_error(self, log_id: Uuid) -> AppError {
        let message = format!("{} (integration log {log_id})", self.message());

        match self {
            Self::UpstreamTimeout { .. } => AppError::Upstream {
                code: self.code(),
                message,
                timed_out: true,
            },
            Self::UpstreamUnreachable => AppError::Upstream {
                code: self.code(),
                message,
                timed_out: false,
            },
            _ => AppError::CodedUnprocessable {
                code: self.code(),
                message,
            },
        }
    }
}

/// **The credential a test call uses: the one that is active and valid today.**
///
/// A credential qualifies when `isActive` is true and `today` lies inside its
/// window — `validFrom` absent or on or before today, `validTo` absent or on or
/// after today (both bounds inclusive; `today` is the UTC date). Soft-deleted
/// credentials are not passed in. **Exactly one must qualify**: none is
/// [`TestCallError::NoUsableCredential`], several is
/// [`TestCallError::AmbiguousCredential`] — a test that picked one of two
/// would prove whichever it picked, and the administrator would not know which.
///
/// The system's own `authType` is not consulted: a credential says its type,
/// and that type is what is attached.
pub fn choose_credential(
    credentials: &[IntegrationCredential],
    today: NaiveDate,
) -> Result<&IntegrationCredential, TestCallError> {
    let usable: Vec<&IntegrationCredential> = credentials
        .iter()
        .filter(|credential| {
            credential.is_active
                && credential.valid_from.is_none_or(|from| from <= today)
                && credential.valid_to.is_none_or(|to| to >= today)
        })
        .collect();

    match usable.as_slice() {
        [one] => Ok(one),
        [] => Err(TestCallError::NoUsableCredential),
        several => Err(TestCallError::AmbiguousCredential {
            count: several.len(),
        }),
    }
}

/// The URL a test call goes to: `base_url` with `path` appended.
///
/// **Appended, not resolved**: `https://erp.example.com/api/v2` and `/orders`
/// make `…/api/v2/orders`, where RFC 3986 resolution would drop `api/v2`. The
/// result must keep the base's scheme, host and port, and be `http` or `https`
/// without user information — which the base's own validation already makes
/// true, and which is checked again here because the row may predate it.
pub fn target_url(base_url: &str, path: &str) -> Result<url::Url, TestCallError> {
    let base = url::Url::parse(base_url.trim()).map_err(|_| TestCallError::TargetUrlInvalid)?;
    let joined = format!("{}{}", base_url.trim().trim_end_matches('/'), path.trim());
    let target = url::Url::parse(&joined).map_err(|_| TestCallError::TargetUrlInvalid)?;

    let same_place = target.scheme() == base.scheme()
        && target.host() == base.host()
        && target.port_or_known_default() == base.port_or_known_default();

    if !matches!(target.scheme(), "http" | "https")
        || !same_place
        || target.host().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err(TestCallError::TargetUrlInvalid);
    }

    Ok(target)
}

/// A response body as it may be shown and stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyPreview {
    pub text: String,
    pub truncated: bool,
}

/// Masks and cuts a body.
///
/// 1. **JSON is masked by key**: the value of any key naming a password, token,
///    secret, key, credential or account number — architectures/03 rule 3's
///    list — becomes [`REDACTED`], at any depth.
/// 2. **Every form of the secret is redacted**, JSON or not: `redactions` is
///    [`super::secret::redactions`]' list, and a system that echoes the header
///    it was sent (many test endpoints do) has it replaced here.
/// 3. The result is cut to [`PREVIEW_CHARACTERS`]. `read_was_cut` says the body
///    was already longer than [`MAX_BODY_BYTES`] when it was read.
pub fn preview(body: &[u8], redactions: &[Secret], read_was_cut: bool) -> BodyPreview {
    let text = match serde_json::from_slice::<Value>(body) {
        Ok(value) if !read_was_cut => {
            let masked = mask_json(value, redactions);
            serde_json::to_string(&masked).unwrap_or_default()
        }
        _ => redact(&String::from_utf8_lossy(body), redactions),
    };

    // Redact the serialized form too: JSON escaping can spell a secret with a
    // `"` or `\` in it differently from the value the walk above compared.
    let text = redact(&text, redactions);

    let mut characters = text.char_indices();
    match characters.nth(PREVIEW_CHARACTERS) {
        Some((cut, _)) => BodyPreview {
            text: text[..cut].to_owned(),
            truncated: true,
        },
        None => BodyPreview {
            text,
            truncated: read_was_cut,
        },
    }
}

/// Replaces every occurrence of every needle in `text`.
pub fn redact(text: &str, redactions: &[Secret]) -> String {
    let mut text = text.to_owned();

    for needle in redactions {
        let needle = needle.expose();
        if !needle.is_empty() && text.contains(needle) {
            text = text.replace(needle, REDACTED);
        }
        // The JSON-escaped spelling, for a secret with a quote or a backslash.
        if let Ok(quoted) = serde_json::to_string(needle) {
            let escaped = &quoted[1..quoted.len() - 1];
            if escaped != needle && !escaped.is_empty() && text.contains(escaped) {
                text = text.replace(escaped, REDACTED);
            }
        }
    }

    text
}

fn mask_json(value: Value, redactions: &[Secret]) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    let value = if is_sensitive_key(&key) {
                        Value::String(REDACTED.to_owned())
                    } else {
                        mask_json(value, redactions)
                    };
                    (redact(&key, redactions), value)
                })
                .collect::<Map<String, Value>>(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| mask_json(item, redactions))
                .collect(),
        ),
        Value::String(text) => Value::String(redact(&text, redactions)),
        other => other,
    }
}

/// Whether a JSON key names a value that is masked whatever it holds.
///
/// Compared with `_`, `-` and case removed, so `api_key`, `apiKey` and
/// `API-KEY` are one key. Substrings, so `refresh_token` and `clientSecret`
/// match: a key masked that did not need to be costs a preview a value, and a
/// key missed costs a secret.
pub fn is_sensitive_key(key: &str) -> bool {
    const SENSITIVE: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "authorization",
        "apikey",
        "privatekey",
        "credential",
        "cookie",
        "creditcard",
        "cardnumber",
        "cvv",
        "bankaccount",
        "accountnumber",
        "iban",
    ];

    let normalized: String = key
        .chars()
        .filter(|character| !matches!(character, '_' | '-' | ' ' | '.'))
        .flat_map(char::to_lowercase)
        .collect();

    SENSITIVE.iter().any(|word| normalized.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn credential(
        active: bool,
        from: Option<(i32, u32, u32)>,
        to: Option<(i32, u32, u32)>,
    ) -> IntegrationCredential {
        let date = |(y, m, d): (i32, u32, u32)| NaiveDate::from_ymd_opt(y, m, d).expect("a date");
        let at = Utc
            .with_ymd_and_hms(2026, 9, 1, 0, 0, 0)
            .single()
            .expect("a time");

        IntegrationCredential {
            id: Uuid::now_v7(),
            external_system_id: Uuid::nil(),
            credential_type: AuthType::BearerToken,
            secret_reference: "env://X".to_owned(),
            valid_from: from.map(date),
            valid_to: to.map(date),
            is_active: active,
            created_at: at,
            updated_at: at,
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).expect("a date")
    }

    #[test]
    fn the_one_active_credential_valid_today_is_chosen() {
        let all = [
            credential(false, None, None),
            credential(true, None, Some((2026, 9, 28))),
            credential(true, Some((2026, 9, 30)), None),
            credential(true, Some((2026, 9, 29)), Some((2026, 9, 29))),
        ];

        let chosen = choose_credential(&all, today()).expect("one qualifies");

        assert_eq!(chosen.id, all[3].id, "the one-day window that is today");
    }

    #[test]
    fn none_qualifying_and_several_qualifying_are_named_apart() {
        assert_eq!(
            choose_credential(&[credential(false, None, None)], today()).map(|c| c.id),
            Err(TestCallError::NoUsableCredential)
        );
        assert_eq!(
            choose_credential(&[], today()).map(|c| c.id),
            Err(TestCallError::NoUsableCredential)
        );
        assert_eq!(
            choose_credential(
                &[credential(true, None, None), credential(true, None, None)],
                today()
            )
            .map(|c| c.id),
            Err(TestCallError::AmbiguousCredential { count: 2 })
        );
    }

    #[test]
    fn the_path_is_appended_to_the_base_and_not_resolved_against_it() {
        let url =
            target_url("https://erp.example.com/api/v2/", "/orders?expand=lines").expect("a URL");

        assert_eq!(
            url.as_str(),
            "https://erp.example.com/api/v2/orders?expand=lines"
        );
        assert_eq!(
            target_url("http://10.0.0.5:8080", "/")
                .expect("a URL")
                .as_str(),
            "http://10.0.0.5:8080/"
        );
    }

    #[test]
    fn a_path_that_would_leave_the_base_host_is_refused() {
        // A stored path the endpoint validation would refuse today, as a row
        // written before that validation might carry.
        for path in ["@evil.example.com/x", ".evil.example.com/x", ":9999/x"] {
            assert_eq!(
                target_url("https://erp.example.com", path),
                Err(TestCallError::TargetUrlInvalid),
                "{path}"
            );
        }
        assert_eq!(
            target_url("ftp://erp.example.com", "/x"),
            Err(TestCallError::TargetUrlInvalid)
        );
    }

    #[test]
    fn sensitive_keys_are_masked_at_any_depth() {
        let body = br#"{"id":1,"access_token":"abc","nested":[{"clientSecret":"s","note":"ok"}],"Password":"p"}"#;

        let shown = preview(body, &[], false);

        let value: Value = serde_json::from_str(&shown.text).expect("still JSON");
        assert_eq!(value["id"], 1);
        assert_eq!(value["access_token"], REDACTED);
        assert_eq!(value["nested"][0]["clientSecret"], REDACTED);
        assert_eq!(value["nested"][0]["note"], "ok");
        assert_eq!(value["Password"], REDACTED);
        assert!(!shown.truncated);
    }

    #[test]
    fn an_echoed_secret_is_redacted_in_json_and_in_text() {
        let needles = [Secret::new("planted-7f3a".to_owned())];

        let json = preview(br#"{"echo":"Bearer planted-7f3a"}"#, &needles, false);
        assert!(!json.text.contains("planted-7f3a"), "{}", json.text);
        assert!(json.text.contains(REDACTED));

        let text = preview(b"you sent planted-7f3a", &needles, false);
        assert_eq!(text.text, format!("you sent {REDACTED}"));
    }

    #[test]
    fn a_secret_with_a_quote_is_redacted_in_its_escaped_form() {
        let needles = [Secret::new(r#"ab"cd"#.to_owned())];

        let shown = preview(br#"{"echo":"x ab\"cd y"}"#, &needles, false);

        assert!(!shown.text.contains(r#"ab\"cd"#), "{}", shown.text);
        assert!(!shown.text.contains(r#"ab"cd"#), "{}", shown.text);
    }

    #[test]
    fn a_long_body_is_cut_on_a_character_boundary() {
        let body = "é".repeat(PREVIEW_CHARACTERS + 10);

        let shown = preview(body.as_bytes(), &[], false);

        assert!(shown.truncated);
        assert_eq!(shown.text.chars().count(), PREVIEW_CHARACTERS);
    }

    #[test]
    fn a_body_cut_at_read_says_so_even_when_short_after_masking() {
        let shown = preview(b"{\"partial\": ", &[], true);

        assert!(shown.truncated);
    }

    #[test]
    fn key_matching_ignores_case_and_separators() {
        for key in [
            "api_key",
            "apiKey",
            "API-KEY",
            "refresh_token",
            "Authorization",
            "iban",
        ] {
            assert!(is_sensitive_key(key), "{key}");
        }
        for key in ["id", "name", "status", "amount"] {
            assert!(!is_sensitive_key(key), "{key}");
        }
    }

    #[test]
    fn a_2xx_is_success_and_everything_else_is_failed() {
        assert_eq!(
            TestCallStatus::for_status_code(200),
            TestCallStatus::Success
        );
        assert_eq!(
            TestCallStatus::for_status_code(204),
            TestCallStatus::Success
        );
        assert_eq!(TestCallStatus::for_status_code(302), TestCallStatus::Failed);
        assert_eq!(TestCallStatus::for_status_code(500), TestCallStatus::Failed);
    }

    #[test]
    fn every_failure_has_a_distinct_code() {
        let all = [
            TestCallError::SystemNotActive,
            TestCallError::EndpointNotActive,
            TestCallError::BaseUrlMissing,
            TestCallError::TargetUrlInvalid,
            TestCallError::NoUsableCredential,
            TestCallError::AmbiguousCredential { count: 2 },
            TestCallError::CredentialTypeNotSupported(AuthType::ApiKey),
            TestCallError::SecretReferenceMalformed,
            TestCallError::SecretNameNotPermitted,
            TestCallError::SecretBackendNotConfigured,
            TestCallError::SecretNotFound {
                name: "X".to_owned(),
            },
            TestCallError::SecretMalformed("x"),
            TestCallError::HostNotResolved,
            TestCallError::EgressRefused(AddressClass::Loopback),
            TestCallError::UpstreamTimeout { seconds: 1 },
            TestCallError::UpstreamUnreachable,
        ];

        let codes: std::collections::BTreeSet<&str> = all.iter().map(TestCallError::code).collect();
        assert_eq!(codes.len(), all.len());
    }
}
