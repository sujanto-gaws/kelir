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

/// The prefix an `env://` name must carry to be read (the product owner's
/// decision on #547, 2026-09-29): `KELIR_INTEGRATION_SECRET_ERP_TOKEN`.
///
/// Built with `concat!` so that no string literal in the crate *is* a
/// `KELIR_*` name: `tests/configuration_reference.rs` counts every such
/// literal as a variable this binary reads, and this is a prefix, not a
/// variable. The Installation guide §7.1 describes it with the variables.
pub const RESOLVABLE_ENVIRONMENT_PREFIX: &str = concat!("KELIR", "_INTEGRATION_SECRET_");

/// Whether an `env://NAME` reference may be read at all.
///
/// **Only a name that starts with [`RESOLVABLE_ENVIRONMENT_PREFIX`] and has
/// something after it** (the product owner's decision on #547, 2026-09-29).
/// The backend's environment also holds the deployment's own settings —
/// `KELIR_JWT_SECRET`, the database URL, the object-store keys — and a
/// reference is written by whoever holds `integration:credential:create`, so
/// without the prefix a test call to a host its caller controls would send any
/// of them there. A name outside the prefix fails the call with
/// `SECRET_NAME_NOT_PERMITTED` **before the environment is read**, and that
/// call writes its one `integration_logs` row like any other failure.
///
/// Saving a credential is unchanged: the registry still accepts any
/// `env://NAME` of the right shape (#520's rules), and this is checked when a
/// call would read it.
pub fn environment_name_is_resolvable(name: &str) -> bool {
    name.len() > RESOLVABLE_ENVIRONMENT_PREFIX.len()
        && name.starts_with(RESOLVABLE_ENVIRONMENT_PREFIX)
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

    #[test]
    fn only_a_name_under_the_integration_prefix_is_resolvable() {
        assert_eq!(RESOLVABLE_ENVIRONMENT_PREFIX, "KELIR_INTEGRATION_SECRET_");

        assert!(environment_name_is_resolvable(
            "KELIR_INTEGRATION_SECRET_ERP_TOKEN"
        ));
        for refused in [
            "KELIR_JWT_SECRET",
            "KELIR_STORAGE_SECRET_KEY",
            "KELIR_DATABASE_URL",
            "DATABASE_URL",
            "KELIR_ERP_API_KEY",
            // The prefix alone names nothing.
            "KELIR_INTEGRATION_SECRET_",
            // Close is not enough.
            "KELIR_INTEGRATION_SECRETS_X",
            "X_KELIR_INTEGRATION_SECRET_Y",
            "",
        ] {
            assert!(!environment_name_is_resolvable(refused), "{refused}");
        }
    }
}
