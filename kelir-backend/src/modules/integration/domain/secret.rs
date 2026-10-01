//! A resolved secret, what a reference says to resolve, and the one header a
//! secret becomes (FR-INT-002, #547; ADR-0043 §2).
//!
//! # A resolved secret has no way out of this type except [`Secret::expose`]
//!
//! [`Secret`] does not derive `Debug`, `Display`, `Serialize`, `Clone` or
//! `ToSchema`, and its hand-written `Debug` prints `Secret([REDACTED])`. So a
//! `tracing` field, a `{:?}` in an error, a response DTO or an OpenAPI schema
//! cannot carry one by accident; each would have to call `expose()`, and every
//! call of it is a line a reviewer can find. Its memory is zeroed on drop.
//!
//! **What this does not cover**, said here so nobody believes otherwise: the
//! header value the HTTP client sends is its own copy, owned by the client
//! until the request is dropped, and `std::env::var` hands the value over in a
//! `String` the standard library allocated. Zeroing is hygiene on the copy this
//! module owns, not a guarantee about the process.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use zeroize::Zeroizing;

use super::external_system::AuthType;

/// A secret value, resolved for one call.
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// The value. **Every call of this is a place the secret can leave**; there
    /// are two outside tests, the header below and the redaction list.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// A `secret_reference`, read for resolution.
///
/// The shape was checked at save (`credential::is_secret_reference`); a stored
/// value that no longer parses is [`SecretReferenceError::Malformed`] rather
/// than a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretReference<'a> {
    /// `env://NAME`.
    Environment { name: &'a str },
    /// `vault://path[#field]`.
    Vault {
        path: &'a str,
        field: Option<&'a str>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretReferenceError {
    Malformed,
}

impl<'a> SecretReference<'a> {
    pub fn parse(value: &'a str) -> Result<Self, SecretReferenceError> {
        let value = value.trim();

        if !super::credential::is_secret_reference(value) {
            return Err(SecretReferenceError::Malformed);
        }

        match value.split_once("://") {
            Some(("env", name)) => Ok(Self::Environment { name }),
            Some(("vault", rest)) => Ok(match rest.split_once('#') {
                Some((path, field)) => Self::Vault {
                    path,
                    field: Some(field),
                },
                None => Self::Vault {
                    path: rest,
                    field: None,
                },
            }),
            _ => Err(SecretReferenceError::Malformed),
        }
    }
}

/// The prefix every `env://` name must carry to be read (the product owner's
/// decision on #547, 2026-09-29), before its tenant's code (#618).
///
/// Built with `concat!` so that no string literal in the crate *is* a
/// `KELIR_*` name: `tests/configuration_reference.rs` counts every such
/// literal as a variable this binary reads, and this is a prefix, not a
/// variable. The Installation guide §7.1 describes it with the variables.
pub const RESOLVABLE_ENVIRONMENT_PREFIX: &str = concat!("KELIR", "_INTEGRATION_SECRET_");

/// Between a tenant's code and the secret's own name. Two underscores, because
/// a code may contain one.
pub const NAMESPACE_SEPARATOR: &str = "__";

/// A tenant code as it is spelled in a variable name (#618): upper case, with
/// `-`, which no environment variable name may hold, written as `_`. So
/// `TNT-001` is `TNT_001`.
pub fn namespace_segment(tenant_code: &str) -> String {
    tenant_code.to_ascii_uppercase().replace('-', "_")
}

/// The prefix of every name a tenant's calls may read:
/// `KELIR_INTEGRATION_SECRET_<CODE>__`. This, and no other tenant's, is what
/// `SECRET_NAME_NOT_PERMITTED` names.
pub fn tenant_prefix(tenant_code: &str) -> String {
    format!(
        "{RESOLVABLE_ENVIRONMENT_PREFIX}{}{NAMESPACE_SEPARATOR}",
        namespace_segment(tenant_code)
    )
}

/// Whether `name` is spelled as one of `tenant_code`'s names: its prefix,
/// then one or more of `A–Z 0–9 _`. Exact, and case-sensitive.
fn is_spelled_in(name: &str, tenant_code: &str) -> bool {
    name.strip_prefix(tenant_prefix(tenant_code).as_str())
        .is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

/// The tenants a resolution is judged against: the caller's code, and every
/// other live tenant's. Built from one read of `tenants` per call, before the
/// environment is read; the code comes from the caller's `tenant_id` and never
/// from the request, the token or the reference.
#[derive(Debug, Clone)]
pub struct TenantNamespaces {
    caller_code: String,
    other_codes: Vec<String>,
}

impl TenantNamespaces {
    /// `None` when the caller's tenant is not among `live` — a tenant deleted
    /// between the request's admission and this read, since a deleted
    /// tenant's token is refused before any route (#650). Nothing is resolved
    /// for it.
    pub fn for_caller(caller: uuid::Uuid, live: Vec<(uuid::Uuid, String)>) -> Option<Self> {
        let mut caller_code = None;
        let mut other_codes = Vec::with_capacity(live.len());
        for (id, code) in live {
            if id == caller {
                caller_code = Some(code);
            } else {
                other_codes.push(code);
            }
        }

        caller_code.map(|caller_code| Self {
            caller_code,
            other_codes,
        })
    }

    /// The caller's own prefix, for the refusal's message.
    pub fn caller_prefix(&self) -> String {
        tenant_prefix(&self.caller_code)
    }

    /// Whether an `env://NAME` reference may be read for this caller.
    ///
    /// **Three gates, in order, and all before the environment is read:**
    ///
    /// 1. `NAME` starts with [`RESOLVABLE_ENVIRONMENT_PREFIX`] and has
    ///    something after it (#547). The backend's environment also holds the
    ///    deployment's own settings — `KELIR_JWT_SECRET`, the database URL,
    ///    the object-store keys — and a reference is written by whoever holds
    ///    `integration:credential:create`.
    /// 2. `NAME` is one of the caller's own names, `<prefix><CODE>__<NAME>`
    ///    ([`tenant_prefix`]). Record 20's P1 (#618): with the first gate
    ///    alone, a tenant created through the route granted itself
    ///    `integration:credential:create` and was sent the system tenant's
    ///    token. The same rule holds with one tenant (A1).
    /// 3. **No other live tenant's names include it.** `A-B` and `A_B` both
    ///    map to `A_B`, and `ACME`'s `..._ACME__X__T` is also `ACME__X`'s. Such
    ///    a name belongs to nobody, so it is refused for every tenant it
    ///    matches: ambiguity fails closed.
    ///
    /// A refusal is `SECRET_NAME_NOT_PERMITTED`, the same bytes whether the
    /// variable is set or not, and its call writes its one `integration_logs`
    /// row like any other failure. Saving a credential is unchanged: the
    /// registry accepts any `env://NAME` of the right shape (#520's rules),
    /// and this is checked when a call would read it.
    pub fn admits(&self, name: &str) -> bool {
        name.len() > RESOLVABLE_ENVIRONMENT_PREFIX.len()
            && name.starts_with(RESOLVABLE_ENVIRONMENT_PREFIX)
            && is_spelled_in(name, &self.caller_code)
            && !self
                .other_codes
                .iter()
                .any(|other| is_spelled_in(name, other))
    }
}

/// The credential types a test call can attach (the product owner's answer 3).
/// Every other type is refused, named, before its secret is resolved.
pub fn is_supported(credential_type: AuthType) -> bool {
    matches!(credential_type, AuthType::BearerToken | AuthType::BasicAuth)
}

/// Why a secret could not become a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    /// A type [`is_supported`] refuses.
    NotSupported(AuthType),
    /// A `BASIC_AUTH` secret without a `:` between user and password.
    BasicWithoutSeparator,
    /// A character a header value cannot carry — a line break, a control
    /// character, anything outside visible ASCII and space. Named without the
    /// character, which is part of the secret.
    NotHeaderSafe,
    /// An empty secret, or an empty user in a `BASIC_AUTH` pair.
    Empty,
}

/// The `Authorization` value a credential of `credential_type` sends.
///
/// * `BEARER_TOKEN` — `Bearer <secret>`;
/// * `BASIC_AUTH` — the secret is `user:password`, sent as
///   `Basic <base64(user:password)>` (RFC 7617). The split is at the first `:`,
///   so a password may itself contain one.
pub fn authorization_value(
    credential_type: AuthType,
    secret: &Secret,
) -> Result<Secret, HeaderError> {
    let value = secret.expose();

    if value.is_empty() {
        return Err(HeaderError::Empty);
    }

    match credential_type {
        AuthType::BearerToken => {
            if !header_safe(value) {
                return Err(HeaderError::NotHeaderSafe);
            }
            Ok(Secret::new(format!("Bearer {value}")))
        }
        AuthType::BasicAuth => {
            let (user, _password) = value
                .split_once(':')
                .ok_or(HeaderError::BasicWithoutSeparator)?;
            if user.is_empty() {
                return Err(HeaderError::Empty);
            }
            // Base64 of anything is header-safe, but a control character in a
            // credential is a mistake worth naming rather than encoding.
            if value.chars().any(char::is_control) {
                return Err(HeaderError::NotHeaderSafe);
            }
            Ok(Secret::new(format!("Basic {}", STANDARD.encode(value))))
        }
        other => Err(HeaderError::NotSupported(other)),
    }
}

/// The shortest value whose own spellings are listed on their own: a
/// `BASIC_AUTH` password shorter than this is not, and no value shorter than
/// this has its encoded forms listed.
pub const MIN_REDACTED_CHARACTERS: usize = 4;

/// What must not appear in anything a call returns or stores: the secret, the
/// header it became, the base64 of a `BASIC_AUTH` pair, and its password on
/// its own — **and each of those values spelled as a system might echo it**
/// (the product owner's decision on #547): for the secret (a bearer token, or
/// a Basic `user:password` pair) and for a Basic password alone,
///
/// * base64, standard and URL-safe, each with and without padding;
/// * percent-encoded, with every character outside RFC 3986's unreserved set
///   escaped (`encodeURIComponent`), with every byte escaped, and as a form
///   field (space as `+`) — each in upper- and lower-case hex;
/// * JSON `\u`-escaped, every character (`\u0073\u0065…`, as a JSONP
///   callback spells a string), upper- and lower-case hex.
///
/// A spelling mixed from these — half a token escaped, half not — is not
/// listed; neither is the base64 of a longer text the secret is part of.
///
/// **A value shorter than [`MIN_REDACTED_CHARACTERS`] has no encoded forms
/// listed, and a password that short is not listed on its own**: redacting
/// every `a` in a response body would destroy the body to protect a value no
/// policy should allow. It is still redacted inside the pair and the header.
pub fn redactions(credential_type: AuthType, secret: &Secret) -> Vec<Secret> {
    let value = secret.expose();
    let mut needles = vec![Secret::new(value.to_owned())];
    let mut sources = vec![value];

    if let Ok(header) = authorization_value(credential_type, secret) {
        if let Some((_, encoded)) = header.expose().split_once(' ') {
            needles.push(Secret::new(encoded.to_owned()));
        }
        needles.push(header);
    }

    if credential_type == AuthType::BasicAuth {
        if let Some((_, password)) = value.split_once(':') {
            if password.chars().count() >= MIN_REDACTED_CHARACTERS {
                needles.push(Secret::new(password.to_owned()));
                sources.push(password);
            }
        }
    }

    for source in sources {
        if source.chars().count() >= MIN_REDACTED_CHARACTERS {
            needles.extend(encoded_forms(source).into_iter().map(Secret::new));
        }
    }

    needles.retain(|needle| !needle.expose().is_empty());
    // Longest first, so a pair is replaced whole before its password is looked
    // for inside what is left; then once each.
    needles.sort_by(|a, b| {
        b.expose()
            .len()
            .cmp(&a.expose().len())
            .then_with(|| a.expose().cmp(b.expose()))
    });
    needles.dedup_by(|a, b| a.expose() == b.expose());
    needles
}

/// The spellings of `value` [`redactions`] lists besides the value itself.
fn encoded_forms(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut forms = vec![
        STANDARD.encode(bytes),
        STANDARD_NO_PAD.encode(bytes),
        URL_SAFE.encode(bytes),
        URL_SAFE_NO_PAD.encode(bytes),
    ];

    for upper in [true, false] {
        forms.push(percent_encoded(value, upper, Percent::Component));
        forms.push(percent_encoded(value, upper, Percent::Form));
        forms.push(percent_encoded(value, upper, Percent::EveryByte));
        forms.push(unicode_escaped(value, upper));
    }

    forms
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Percent {
    /// Everything but RFC 3986's unreserved characters escaped.
    Component,
    /// As [`Percent::Component`], with a space as `+`
    /// (`application/x-www-form-urlencoded`).
    Form,
    /// Every byte escaped.
    EveryByte,
}

fn percent_encoded(value: &str, upper: bool, style: Percent) -> String {
    let mut encoded = String::with_capacity(value.len() * 3);

    for byte in value.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');

        if style != Percent::EveryByte && unreserved {
            encoded.push(char::from(byte));
        } else if style == Percent::Form && byte == b' ' {
            encoded.push('+');
        } else if upper {
            encoded.push_str(&format!("%{byte:02X}"));
        } else {
            encoded.push_str(&format!("%{byte:02x}"));
        }
    }

    encoded
}

/// Every UTF-16 unit as a JSON `\uXXXX` escape.
fn unicode_escaped(value: &str, upper: bool) -> String {
    value
        .encode_utf16()
        .map(|unit| {
            if upper {
                format!("\\u{unit:04X}")
            } else {
                format!("\\u{unit:04x}")
            }
        })
        .collect()
}

fn header_safe(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t' || (0x21..=0x7e).contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANTED: &str = "kelir-planted-secret-7f3a";

    #[test]
    fn debug_never_prints_the_value() {
        let secret = Secret::new(PLANTED.to_owned());

        let printed = format!("{secret:?} {:?}", Some(&secret));

        assert!(!printed.contains(PLANTED), "{printed}");
        assert!(printed.contains("[REDACTED]"));
    }

    #[test]
    fn an_environment_reference_names_its_variable() {
        assert_eq!(
            SecretReference::parse("env://KELIR_ERP_API_KEY"),
            Ok(SecretReference::Environment {
                name: "KELIR_ERP_API_KEY"
            })
        );
    }

    #[test]
    fn a_vault_reference_names_its_path_and_field() {
        assert_eq!(
            SecretReference::parse("vault://kelir/erp/oauth#client_secret"),
            Ok(SecretReference::Vault {
                path: "kelir/erp/oauth",
                field: Some("client_secret")
            })
        );
        assert_eq!(
            SecretReference::parse("vault://kelir/erp"),
            Ok(SecretReference::Vault {
                path: "kelir/erp",
                field: None
            })
        );
    }

    #[test]
    fn a_stored_value_that_is_not_a_reference_is_malformed() {
        for bad in [
            "sk_live_abc",
            "env://lower",
            "file:///etc/passwd",
            "",
            "vault://",
        ] {
            assert_eq!(
                SecretReference::parse(bad),
                Err(SecretReferenceError::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_bearer_token_is_sent_after_bearer() {
        let header = authorization_value(AuthType::BearerToken, &Secret::new("abc.def".to_owned()))
            .expect("builds");

        assert_eq!(header.expose(), "Bearer abc.def");
    }

    #[test]
    fn a_basic_pair_is_base64_of_user_colon_password() {
        let header = authorization_value(
            AuthType::BasicAuth,
            &Secret::new("svc-kelir:pa:ss word".to_owned()),
        )
        .expect("builds");

        // base64("svc-kelir:pa:ss word")
        assert_eq!(header.expose(), "Basic c3ZjLWtlbGlyOnBhOnNzIHdvcmQ=");
    }

    #[test]
    fn a_basic_secret_without_a_colon_is_refused_by_name() {
        assert!(matches!(
            authorization_value(
                AuthType::BasicAuth,
                &Secret::new("justapassword".to_owned())
            ),
            Err(HeaderError::BasicWithoutSeparator)
        ));
        assert!(matches!(
            authorization_value(AuthType::BasicAuth, &Secret::new(":password".to_owned())),
            Err(HeaderError::Empty)
        ));
    }

    #[test]
    fn a_bearer_token_with_a_line_break_is_refused() {
        assert!(matches!(
            authorization_value(
                AuthType::BearerToken,
                &Secret::new("abc\r\nX-Injected: 1".to_owned())
            ),
            Err(HeaderError::NotHeaderSafe)
        ));
        assert!(matches!(
            authorization_value(AuthType::BearerToken, &Secret::new(String::new())),
            Err(HeaderError::Empty)
        ));
    }

    #[test]
    fn only_bearer_and_basic_are_built() {
        for kind in AuthType::ALL {
            let built = authorization_value(*kind, &Secret::new("u:p".to_owned()));

            if is_supported(*kind) {
                assert!(built.is_ok(), "{kind:?}");
            } else {
                assert!(
                    matches!(built, Err(HeaderError::NotSupported(refused)) if refused == *kind),
                    "{kind:?}"
                );
            }
        }
    }

    #[test]
    fn the_redaction_list_covers_every_form_the_secret_takes() {
        let secret = Secret::new("svc:correct-horse".to_owned());
        let needles: Vec<String> = redactions(AuthType::BasicAuth, &secret)
            .iter()
            .map(|needle| needle.expose().to_owned())
            .collect();

        assert!(needles.contains(&"svc:correct-horse".to_owned()));
        assert!(needles.contains(&"correct-horse".to_owned()));
        assert!(needles.contains(&STANDARD.encode("svc:correct-horse")));
        assert!(needles.contains(&format!("Basic {}", STANDARD.encode("svc:correct-horse"))));
        assert!(
            needles
                .windows(2)
                .all(|pair| pair[0].len() >= pair[1].len()),
            "longest first: {needles:?}"
        );
    }

    fn needles_of(credential_type: AuthType, value: &str) -> Vec<String> {
        redactions(credential_type, &Secret::new(value.to_owned()))
            .iter()
            .map(|needle| needle.expose().to_owned())
            .collect()
    }

    #[test]
    fn a_bearer_token_is_listed_in_every_encoding_it_may_be_echoed_in() {
        // `/`, `+` and `?` make the base64 alphabets and the percent forms
        // differ from each other and from the token.
        let token = "tok/en+?>> 9";
        let needles = needles_of(AuthType::BearerToken, token);

        for form in [
            token.to_owned(),
            format!("Bearer {token}"),
            STANDARD.encode(token),
            STANDARD_NO_PAD.encode(token),
            URL_SAFE.encode(token),
            URL_SAFE_NO_PAD.encode(token),
            "tok%2Fen%2B%3F%3E%3E%209".to_owned(),
            "tok%2fen%2b%3f%3e%3e%209".to_owned(),
            "tok%2Fen%2B%3F%3E%3E+9".to_owned(),
            "%74%6F%6B%2F%65%6E%2B%3F%3E%3E%20%39".to_owned(),
            "\\u0074\\u006f\\u006b\\u002f\\u0065\\u006e\\u002b\\u003f\\u003e\\u003e\\u0020\\u0039"
                .to_owned(),
            "\\u0074\\u006F\\u006B\\u002F\\u0065\\u006E\\u002B\\u003F\\u003E\\u003E\\u0020\\u0039"
                .to_owned(),
        ] {
            assert!(needles.contains(&form), "{form} missing from {needles:?}");
        }

        // The URL-safe and standard alphabets do differ here, so both are
        // really listed and not one twice.
        assert_ne!(STANDARD.encode(token), URL_SAFE.encode(token));
        let mut unique = needles.clone();
        unique.dedup();
        assert_eq!(unique, needles, "each needle once");
    }

    #[test]
    fn a_basic_password_alone_is_listed_encoded_too() {
        let needles = needles_of(AuthType::BasicAuth, "svc:pass/word");

        for form in [
            STANDARD.encode("pass/word"),
            URL_SAFE_NO_PAD.encode("pass/word"),
            "pass%2Fword".to_owned(),
            STANDARD.encode("svc:pass/word"),
            URL_SAFE_NO_PAD.encode("svc:pass/word"),
            "svc%3Apass%2Fword".to_owned(),
        ] {
            assert!(needles.contains(&form), "{form} missing from {needles:?}");
        }
    }

    #[test]
    fn a_value_too_short_to_list_alone_has_no_encoded_forms() {
        let needles = needles_of(AuthType::BearerToken, "abc");

        assert!(needles.contains(&"abc".to_owned()));
        assert!(!needles.contains(&STANDARD.encode("abc")), "{needles:?}");
        assert!(!needles.contains(&"\\u0061\\u0062\\u0063".to_owned()));

        let needles = needles_of(AuthType::BasicAuth, "svc:a");
        assert!(
            !needles.contains(&STANDARD_NO_PAD.encode("a")),
            "{needles:?}"
        );
    }

    #[test]
    fn a_short_password_is_redacted_only_inside_the_pair() {
        let needles: Vec<String> =
            redactions(AuthType::BasicAuth, &Secret::new("svc:a".to_owned()))
                .iter()
                .map(|needle| needle.expose().to_owned())
                .collect();

        assert!(!needles.contains(&"a".to_owned()), "{needles:?}");
        assert!(needles.contains(&"svc:a".to_owned()));
    }

    /// The caller's namespaces with `others` as the other live tenants.
    fn namespaces(caller: &str, others: &[&str]) -> TenantNamespaces {
        let caller_id = uuid::Uuid::now_v7();
        let mut live = vec![(caller_id, caller.to_owned())];
        live.extend(
            others
                .iter()
                .map(|code| (uuid::Uuid::now_v7(), (*code).to_owned())),
        );
        TenantNamespaces::for_caller(caller_id, live).expect("the caller is live")
    }

    #[test]
    fn only_a_name_under_the_integration_prefix_is_resolvable() {
        assert_eq!(RESOLVABLE_ENVIRONMENT_PREFIX, "KELIR_INTEGRATION_SECRET_");
        let system = namespaces("SYSTEM", &[]);

        assert!(system.admits("KELIR_INTEGRATION_SECRET_SYSTEM__ERP_TOKEN"));
        for refused in [
            "KELIR_JWT_SECRET",
            "KELIR_STORAGE_SECRET_KEY",
            "KELIR_DATABASE_URL",
            "DATABASE_URL",
            "KELIR_ERP_API_KEY",
            // The prefix alone names nothing.
            "KELIR_INTEGRATION_SECRET_",
            // Close is not enough.
            "KELIR_INTEGRATION_SECRETS_SYSTEM__X",
            "X_KELIR_INTEGRATION_SECRET_SYSTEM__Y",
            "",
        ] {
            assert!(!system.admits(refused), "{refused}");
        }
    }

    #[test]
    fn a_name_under_the_bare_prefix_is_refused_with_one_tenant_too() {
        // A1: #547's names, `KELIR_INTEGRATION_SECRET_ERP_TOKEN`, name no
        // tenant, and a single-tenant deployment reads them no more than any.
        let system = namespaces("SYSTEM", &[]);

        assert!(!system.admits("KELIR_INTEGRATION_SECRET_ERP_TOKEN"));
        assert!(!system.admits("KELIR_INTEGRATION_SECRET_SYSTEM_ERP_TOKEN"));
        assert!(system.admits("KELIR_INTEGRATION_SECRET_SYSTEM__ERP_TOKEN"));
    }

    #[test]
    fn a_tenant_prefix_is_its_code_upper_case_with_a_hyphen_as_an_underscore() {
        assert_eq!(tenant_prefix("SYSTEM"), "KELIR_INTEGRATION_SECRET_SYSTEM__");
        assert_eq!(
            tenant_prefix("TNT-001"),
            "KELIR_INTEGRATION_SECRET_TNT_001__"
        );
        // A code is stored upper case; one that is not is still spelled so.
        assert_eq!(
            tenant_prefix("tnt-001"),
            "KELIR_INTEGRATION_SECRET_TNT_001__"
        );
        assert_eq!(namespace_segment("A-B_C-"), "A_B_C_");
    }

    #[test]
    fn a_hyphenated_code_reads_its_names_with_an_underscore() {
        let tenant = namespaces("TNT-001", &[]);

        assert!(tenant.admits("KELIR_INTEGRATION_SECRET_TNT_001__ERP_TOKEN"));
        // A hyphen is not legal in a variable name, so nothing is spelled so.
        assert!(!tenant.admits("KELIR_INTEGRATION_SECRET_TNT-001__ERP_TOKEN"));
    }

    #[test]
    fn a_name_is_matched_exactly_and_in_upper_case() {
        let tenant = namespaces("ACME", &[]);

        assert!(tenant.admits("KELIR_INTEGRATION_SECRET_ACME__ERP_TOKEN_2"));
        for refused in [
            // The code in another case.
            "KELIR_INTEGRATION_SECRET_acme__ERP_TOKEN",
            "KELIR_INTEGRATION_SECRET_Acme__ERP_TOKEN",
            // The name in another case.
            "KELIR_INTEGRATION_SECRET_ACME__erp_token",
            "KELIR_INTEGRATION_SECRET_ACME__ERP_Token",
            // The prefix in another case.
            "kelir_integration_secret_ACME__ERP_TOKEN",
            // A character outside A-Z 0-9 _.
            "KELIR_INTEGRATION_SECRET_ACME__ERP-TOKEN",
            "KELIR_INTEGRATION_SECRET_ACME__ERP.TOKEN",
            "KELIR_INTEGRATION_SECRET_ACME__ERP TOKEN",
            "KELIR_INTEGRATION_SECRET_ACME__ÉRP",
        ] {
            assert!(!tenant.admits(refused), "{refused}");
        }

        // A caller whose code arrived in lower case reads the upper-case names.
        assert!(namespaces("acme", &[]).admits("KELIR_INTEGRATION_SECRET_ACME__ERP_TOKEN"));
    }

    #[test]
    fn the_separator_is_two_underscores_and_the_name_after_it_is_not_empty() {
        let tenant = namespaces("ACME", &[]);

        for refused in [
            // No separator.
            "KELIR_INTEGRATION_SECRET_ACMEERP_TOKEN",
            // One underscore.
            "KELIR_INTEGRATION_SECRET_ACME_ERP_TOKEN",
            // An empty name.
            "KELIR_INTEGRATION_SECRET_ACME__",
            "KELIR_INTEGRATION_SECRET_ACME",
        ] {
            assert!(!tenant.admits(refused), "{refused}");
        }
        // Underscores after the separator are the name's own.
        assert!(tenant.admits("KELIR_INTEGRATION_SECRET_ACME___"));
        assert!(tenant.admits("KELIR_INTEGRATION_SECRET_ACME__X__Y"));
    }

    #[test]
    fn acme_acme2_and_acme_ltd_each_read_only_their_own() {
        // Each alone, so what refuses is the separator, not the ambiguity rule.
        let acme = namespaces("ACME", &[]);
        let acme2 = namespaces("ACME2", &[]);
        let acme_ltd = namespaces("ACME_LTD", &[]);

        let of_acme = "KELIR_INTEGRATION_SECRET_ACME__TOKEN";
        let of_acme2 = "KELIR_INTEGRATION_SECRET_ACME2__TOKEN";
        let of_acme_ltd = "KELIR_INTEGRATION_SECRET_ACME_LTD__TOKEN";

        assert!(acme.admits(of_acme));
        assert!(!acme.admits(of_acme2));
        assert!(!acme.admits(of_acme_ltd));

        assert!(!acme2.admits(of_acme));
        assert!(acme2.admits(of_acme2));
        assert!(!acme2.admits(of_acme_ltd));

        assert!(!acme_ltd.admits(of_acme));
        assert!(!acme_ltd.admits(of_acme2));
        assert!(acme_ltd.admits(of_acme_ltd));

        // And together, each still reads its own.
        let together = namespaces("ACME", &["ACME2", "ACME_LTD"]);
        assert!(together.admits(of_acme));
        assert!(!together.admits(of_acme2));
        assert!(!together.admits(of_acme_ltd));
    }

    #[test]
    fn another_tenants_name_is_refused() {
        let tenant = namespaces("TNT-001", &["SYSTEM"]);

        assert!(!tenant.admits("KELIR_INTEGRATION_SECRET_SYSTEM__ERP_TOKEN"));
        assert!(!namespaces("SYSTEM", &["TNT-001"])
            .admits("KELIR_INTEGRATION_SECRET_TNT_001__ERP_TOKEN"));
        assert_eq!(tenant.caller_prefix(), "KELIR_INTEGRATION_SECRET_TNT_001__");
    }

    #[test]
    fn a_name_two_live_tenants_map_to_is_refused_for_both() {
        // Same mapped code.
        let name = "KELIR_INTEGRATION_SECRET_A_B__TOKEN";
        assert!(namespaces("A-B", &[]).admits(name), "alone, it is A-B's");
        assert!(!namespaces("A-B", &["A_B"]).admits(name));
        assert!(!namespaces("A_B", &["A-B"]).admits(name));

        // A code holding the separator.
        let nested = "KELIR_INTEGRATION_SECRET_ACME__X__TOKEN";
        assert!(!namespaces("ACME", &["ACME__X"]).admits(nested));
        assert!(!namespaces("ACME__X", &["ACME"]).admits(nested));
        // ...and a code ending in an underscore.
        let trailing = "KELIR_INTEGRATION_SECRET_ACME___TOKEN";
        assert!(!namespaces("ACME", &["ACME_"]).admits(trailing));
        assert!(!namespaces("ACME_", &["ACME"]).admits(trailing));

        // The refusal is of the name: ACME's names ACME__X's prefix does not
        // reach are still ACME's.
        assert!(namespaces("ACME", &["ACME__X"]).admits("KELIR_INTEGRATION_SECRET_ACME__TOKEN"));
        assert!(namespaces("ACME", &["ACME__X"]).admits("KELIR_INTEGRATION_SECRET_ACME__X"));
    }

    #[test]
    fn a_caller_whose_tenant_is_not_live_has_no_namespace() {
        let live = vec![(uuid::Uuid::now_v7(), "SYSTEM".to_owned())];

        assert!(TenantNamespaces::for_caller(uuid::Uuid::now_v7(), live).is_none());
    }
}
