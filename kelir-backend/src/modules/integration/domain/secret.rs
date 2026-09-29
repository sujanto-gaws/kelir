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

use base64::engine::general_purpose::STANDARD;
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

/// What must not appear in anything a call returns or stores: the secret, the
/// header it became, the base64 of a `BASIC_AUTH` pair, and its password on
/// its own.
///
/// **A password shorter than four characters is not listed on its own**:
/// redacting every `a` in a response body would destroy the body to protect a
/// value no policy should allow. It is still redacted inside the pair and the
/// header.
pub fn redactions(credential_type: AuthType, secret: &Secret) -> Vec<Secret> {
    let value = secret.expose();
    let mut needles = vec![Secret::new(value.to_owned())];

    if let Ok(header) = authorization_value(credential_type, secret) {
        if let Some((_, encoded)) = header.expose().split_once(' ') {
            needles.push(Secret::new(encoded.to_owned()));
        }
        needles.push(header);
    }

    if credential_type == AuthType::BasicAuth {
        if let Some((_, password)) = value.split_once(':') {
            if password.chars().count() >= 4 {
                needles.push(Secret::new(password.to_owned()));
            }
        }
    }

    needles.retain(|needle| !needle.expose().is_empty());
    // Longest first, so a pair is replaced whole before its password is looked
    // for inside what is left.
    needles.sort_by_key(|needle| std::cmp::Reverse(needle.expose().len()));
    needles
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
