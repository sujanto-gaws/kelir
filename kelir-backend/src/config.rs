use std::env::{self, VarError};
use std::fmt;

/// Application configuration, loaded from `KELIR_*` environment variables.
///
/// The secret, storage, SMTP and frontend values are loaded and validated in
/// Phase 1 but not yet read: authentication uses the secret in Phase 2,
/// attachments the storage driver in Phase 6, notifications the SMTP host in
/// Phase 6. Validating them at startup means a misconfigured deployment fails
/// immediately rather than at first use.
#[allow(
    dead_code,
    reason = "fields are consumed by the modules that own them, from Phase 2 onward"
)]
///
/// Secrets never carry a default (coding standard §2.8): a missing
/// `KELIR_JWT_SECRET` fails startup rather than silently running on a
/// placeholder. Everything else falls back to a development-friendly value.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub app_env: AppEnv,
    pub bind_address: String,
    pub database_url: String,
    pub jwt_secret: String,
    pub storage_driver: String,
    pub smtp_host: String,
    /// The port `smtp_host` listens on. 1025 is mailpit's, which the compose
    /// stack runs; a relay is usually 587.
    pub smtp_port: u16,
    /// The `From` on the one transactional email the product sends
    /// (FR-AUTH-006). A deployment that leaves it at the default sends mail
    /// from an address nobody reads, which is correct for a reset link.
    pub mail_from: String,
    pub frontend_url: String,
    /// Whether this deployment serves more than one tenant (SRS §2, FR-IDM-009).
    ///
    /// Tenancy is a deployment property, not a per-request one. Off, every
    /// sign-in resolves [`AppConfig::default_tenant_code`] and the login
    /// contract is unchanged; on, the caller names its tenant and that name is
    /// looked up. A flag rather than always-on keeps single-tenant deployments —
    /// which is every deployment today — free of a field nobody would fill in.
    pub multi_tenant: bool,
    /// The tenant every sign-in resolves to when [`AppConfig::multi_tenant`] is
    /// off, and the tenant the first-run administrator is created in either way.
    pub default_tenant_code: String,
    /// Credentials for the first administrator, used only when the instance has
    /// no users at all. Absent means "do not create one".
    pub bootstrap_admin: Option<BootstrapAdmin>,
    /// How many reverse-proxy hops sit in front of this instance
    /// (`KELIR_TRUSTED_PROXY_HOPS`). See [`trusted_proxy_hops`].
    pub trusted_proxy_hops: usize,
}

/// Reverse-proxy hops the deployment puts in front of the backend.
///
/// **The default is zero: trust nothing.** An unconfigured deployment takes the
/// client address from the socket peer and ignores `X-Forwarded-For` entirely,
/// because that header is written by whoever is talking to us. Trusting it by
/// default would let any caller choose their own rate-limit bucket and their own
/// audit `ip_address` (NFR-SEC-008, FR-AUD-005).
///
/// Set it to the number of proxies the request really traverses before reaching
/// this process — `1` for the staging topology, where Caddy is the only hop. The
/// resolver then reads the address immediately to the left of those hops rather
/// than the leftmost element of the chain, so values a caller prepends are
/// skipped over (see `middleware::client_address`).
///
/// Counting hops assumes the backend is reachable *only* through those proxies,
/// which the deployment enforces by publishing no backend port (deployment §4.1).
/// A topology where that stops holding needs a CIDR allow-list instead.
fn trusted_proxy_hops<F>(get: &F) -> Result<usize, ConfigError>
where
    F: Fn(&'static str) -> Result<String, VarError>,
{
    const KEY: &str = "KELIR_TRUSTED_PROXY_HOPS";

    let Some(raw) = get("KELIR_TRUSTED_PROXY_HOPS")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(0);
    };

    let hops: usize = raw.parse().map_err(|_| ConfigError::Invalid {
        key: KEY,
        reason: format!("expected a whole number of proxy hops; found '{raw}'"),
    })?;

    // A silly value is a misconfiguration, and a large one would walk far enough
    // left in the chain to reach caller-supplied entries.
    if hops > MAX_TRUSTED_PROXY_HOPS {
        return Err(ConfigError::Invalid {
            key: KEY,
            reason: format!("at most {MAX_TRUSTED_PROXY_HOPS} hops are supported; found {hops}"),
        });
    }

    Ok(hops)
}

/// Upper bound on `KELIR_TRUSTED_PROXY_HOPS`. No real Kelir topology stacks more
/// than a load balancer, a CDN and an ingress.
const MAX_TRUSTED_PROXY_HOPS: usize = 8;

/// The first-run administrator (see `modules::auth::bootstrap`).
#[derive(Debug, Clone)]
pub struct BootstrapAdmin {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// Reads the first-run administrator, if configured.
///
/// All three values are required together: a username with no password is a
/// misconfiguration, and defaulting either would put a known credential on an
/// account that holds every permission.
fn bootstrap_admin<F>(get: &F, app_env: AppEnv) -> Result<Option<BootstrapAdmin>, ConfigError>
where
    F: Fn(&'static str) -> Result<String, VarError>,
{
    let read = |key: &'static str| {
        get(key)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };

    let username = read("KELIR_BOOTSTRAP_ADMIN_USERNAME");
    let password = read("KELIR_BOOTSTRAP_ADMIN_PASSWORD");
    let email = read("KELIR_BOOTSTRAP_ADMIN_EMAIL");

    match (username, password) {
        (None, None) => Ok(None),
        (Some(username), Some(password)) => {
            if app_env.requires_real_secrets() && PLACEHOLDER_SECRETS.contains(&password.as_str()) {
                return Err(ConfigError::Invalid {
                    key: "KELIR_BOOTSTRAP_ADMIN_PASSWORD",
                    reason: format!("the development placeholder cannot be used in {app_env}"),
                });
            }

            Ok(Some(BootstrapAdmin {
                email: email.unwrap_or_else(|| format!("{username}@localhost")),
                username,
                password,
            }))
        }
        (Some(_), None) => Err(ConfigError::Missing {
            key: "KELIR_BOOTSTRAP_ADMIN_PASSWORD",
        }),
        (None, Some(_)) => Err(ConfigError::Missing {
            key: "KELIR_BOOTSTRAP_ADMIN_USERNAME",
        }),
    }
}

/// Reads `KELIR_MULTI_TENANT`, and refuses to start when it is on.
///
/// **The flag is parsed but not honoured, on purpose.** Tenancy is half-built:
/// `tenant_id` is in the BASE column set and every identity read is scoped by
/// it, but nothing resolves a tenant per request. With the flag on,
/// `resolve_for_sign_in` demands a tenant code, the login form has no field for
/// one, and the deployment reaches a state where **nobody can sign in at all**
/// (#67) — discovered on first use, by a person who cannot fix it from the UI.
///
/// Refusing at startup turns that into a container that does not start and says
/// which variable to unset. It is the same trade the deployment already makes
/// for a placeholder `KELIR_JWT_SECRET` in production: a setting whose
/// consequence is discovered late is better refused early.
///
/// Decision **D-7** (2026-08-20) keeps a deployment single-tenant until
/// per-request resolution is designed, which is deferred past 1.0. The
/// resolution code stays and stays tested — this guard is on the deployment
/// path, not the code path — so removing it is all that stands between here and
/// multi-tenant mode once the missing half exists.
fn multi_tenant(raw: &str) -> Result<bool, ConfigError> {
    if flag(raw, "KELIR_MULTI_TENANT")? {
        return Err(ConfigError::Invalid {
            key: "KELIR_MULTI_TENANT",
            reason: "multi-tenant mode is not supported yet: no per-request tenant                      resolution exists, so sign-in would demand a tenant code that no                      client can send and nobody could sign in. Unset it or set it to                      false. See decision D-7 in projects/planning/02. Product Backlog.md"
                .to_owned(),
        });
    }

    Ok(false)
}

/// Secrets shipped in `.env.example` and the compose stack. Never valid in
/// production.
const PLACEHOLDER_SECRETS: &[&str] = &["change-me", "changeme", "secret", "test-secret"];

/// Parses a boolean deployment flag.
///
/// Refuses anything it does not recognise rather than treating it as `false`.
/// A flag that silently reads `KELIR_MULTI_TENANT=yes` as "off" would leave an
/// operator believing tenancy is enforced when every sign-in is landing in the
/// default tenant, which is the one failure mode this flag must not have.
fn flag(raw: &str, key: &'static str) -> Result<bool, ConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(ConfigError::Invalid {
            key,
            reason: format!(
                "expected a boolean (true/false, 1/0, yes/no, on/off); found '{other}'"
            ),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Test,
    Staging,
    Production,
}

impl AppEnv {
    fn parse(raw: &str) -> Result<Self, ConfigError> {
        match raw.to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "staging" => Ok(Self::Staging),
            "production" | "prod" => Ok(Self::Production),
            other => Err(ConfigError::Invalid {
                key: "KELIR_APP_ENV",
                reason: format!(
                    "expected one of development, test, staging, production; found '{other}'"
                ),
            }),
        }
    }

    /// Environments that are reachable from outside the developer's machine.
    ///
    /// Staging is internet-facing and holds real-shaped data, so it is held to
    /// the same secret rules as production — a forgeable token there is a real
    /// exposure, not a development convenience.
    pub fn requires_real_secrets(self) -> bool {
        matches!(self, Self::Staging | Self::Production)
    }
}

impl fmt::Display for AppEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Staging => "staging",
            Self::Production => "production",
        };
        f.write_str(name)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing { key: &'static str },
    Invalid { key: &'static str, reason: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { key } => write!(f, "{key} is required but was not set"),
            Self::Invalid { key, reason } => write!(f, "{key} is invalid: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl AppConfig {
    /// Reads configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(env::var)
    }

    /// Environment lookup is injected so the loader is testable without
    /// mutating process-global state, which races across parallel tests.
    fn from_source<F>(get: F) -> Result<Self, ConfigError>
    where
        F: Fn(&'static str) -> Result<String, VarError>,
    {
        let optional = |key: &'static str, fallback: &str| -> String {
            get(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };

        let required = |key: &'static str| -> Result<String, ConfigError> {
            get(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ConfigError::Missing { key })
        };

        let app_env = AppEnv::parse(&optional("KELIR_APP_ENV", "development"))?;
        let jwt_secret = required("KELIR_JWT_SECRET")?;

        // `.env.example` and the compose stack ship a placeholder secret so
        // local development works out of the box. Shipping that placeholder to
        // production would make every token forgeable, so refuse to start.
        if app_env.requires_real_secrets() && PLACEHOLDER_SECRETS.contains(&jwt_secret.as_str()) {
            return Err(ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                reason: format!("the development placeholder cannot be used in {app_env}"),
            });
        }

        Ok(Self {
            app_name: optional("KELIR_APP_NAME", "Kelir"),
            app_env,
            bind_address: optional("KELIR_BIND_ADDRESS", "0.0.0.0:8080"),
            database_url: optional(
                "KELIR_DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/kelir",
            ),
            jwt_secret,
            storage_driver: optional("KELIR_STORAGE_DRIVER", "local"),
            smtp_host: optional("KELIR_SMTP_HOST", "localhost"),
            smtp_port: {
                let raw = optional("KELIR_SMTP_PORT", "1025");

                raw.parse().map_err(|_| ConfigError::Invalid {
                    key: "KELIR_SMTP_PORT",
                    reason: format!("expected a port number; found '{raw}'"),
                })?
            },
            mail_from: optional("KELIR_MAIL_FROM", "no-reply@kelir.local"),
            frontend_url: optional("KELIR_FRONTEND_URL", "http://localhost:5173"),
            multi_tenant: multi_tenant(&optional("KELIR_MULTI_TENANT", "false"))?,
            // Uppercased on the way in so the configured value and the codes
            // callers send normalise identically (organization::domain).
            default_tenant_code: optional("KELIR_DEFAULT_TENANT_CODE", "SYSTEM")
                .trim()
                .to_ascii_uppercase(),
            bootstrap_admin: bootstrap_admin(&get, app_env)?,
            trusted_proxy_hops: trusted_proxy_hops(&get)?,
        })
    }
}

impl AppConfig {
    /// Configuration for tests, with the placeholder secret and a fixed
    /// frontend origin. Kept beside the real loader so the two cannot drift.
    #[cfg(test)]
    pub fn test_default() -> Self {
        Self {
            app_name: "Kelir".to_owned(),
            app_env: AppEnv::Test,
            bind_address: "127.0.0.1:0".to_owned(),
            database_url: "postgres://postgres:postgres@localhost:5432/kelir".to_owned(),
            jwt_secret: "test-secret".to_owned(),
            storage_driver: "local".to_owned(),
            smtp_host: "localhost".to_owned(),
            smtp_port: 1025,
            mail_from: "no-reply@kelir.test".to_owned(),
            frontend_url: "http://localhost:5173".to_owned(),
            multi_tenant: false,
            default_tenant_code: "SYSTEM".to_owned(),
            bootstrap_admin: None,
            trusted_proxy_hops: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn source(pairs: &[(&str, &str)]) -> impl Fn(&'static str) -> Result<String, VarError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();

        move |key: &'static str| map.get(key).cloned().ok_or(VarError::NotPresent)
    }

    #[test]
    fn applies_defaults_when_only_the_secret_is_set() {
        let config = AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")]))
            .expect("defaults cover every optional value");

        assert_eq!(config.app_name, "Kelir");
        assert_eq!(config.app_env, AppEnv::Development);
        assert_eq!(config.bind_address, "0.0.0.0:8080");
    }

    #[test]
    fn rejects_a_missing_secret() {
        let error = AppConfig::from_source(source(&[])).expect_err("the secret has no default");

        assert!(matches!(
            error,
            ConfigError::Missing {
                key: "KELIR_JWT_SECRET"
            }
        ));
    }

    #[test]
    fn treats_a_blank_value_as_absent() {
        // An empty env var is a common compose/CI accident and must not be
        // mistaken for a deliberately configured empty secret.
        let error = AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "   ")]))
            .expect_err("blank is not a secret");

        assert!(matches!(error, ConfigError::Missing { .. }));
    }

    #[test]
    fn rejects_an_unknown_environment() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_APP_ENV", "sandbox"),
        ]))
        .expect_err("sandbox is not a known environment");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_APP_ENV",
                ..
            }
        ));
    }

    #[test]
    fn no_bootstrap_admin_unless_configured() {
        let config =
            AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")])).expect("loads");

        assert!(config.bootstrap_admin.is_none());
    }

    #[test]
    fn a_bootstrap_username_without_a_password_is_a_misconfiguration() {
        // Defaulting the password would put a known credential on an account
        // holding every permission.
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_BOOTSTRAP_ADMIN_USERNAME", "admin"),
        ]))
        .expect_err("incomplete");

        assert!(matches!(
            error,
            ConfigError::Missing {
                key: "KELIR_BOOTSTRAP_ADMIN_PASSWORD"
            }
        ));
    }

    #[test]
    fn a_bootstrap_password_without_a_username_is_a_misconfiguration() {
        // The mirror of the case above. Defaulting the username would decide on
        // the operator's behalf which account the configured password unlocks.
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            (
                "KELIR_BOOTSTRAP_ADMIN_PASSWORD",
                "a-real-bootstrap-password",
            ),
        ]))
        .expect_err("incomplete");

        assert!(matches!(
            error,
            ConfigError::Missing {
                key: "KELIR_BOOTSTRAP_ADMIN_USERNAME"
            }
        ));
    }

    #[test]
    fn reads_a_complete_bootstrap_admin() {
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_BOOTSTRAP_ADMIN_USERNAME", "admin"),
            (
                "KELIR_BOOTSTRAP_ADMIN_PASSWORD",
                "a-real-bootstrap-password",
            ),
        ]))
        .expect("loads");

        let admin = config.bootstrap_admin.expect("present");
        assert_eq!(admin.username, "admin");
        assert_eq!(admin.email, "admin@localhost", "defaults when unset");
    }

    #[test]
    fn refuses_a_placeholder_bootstrap_password_in_staging() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "a-real-secret"),
            ("KELIR_APP_ENV", "staging"),
            ("KELIR_BOOTSTRAP_ADMIN_USERNAME", "admin"),
            ("KELIR_BOOTSTRAP_ADMIN_PASSWORD", "change-me"),
        ]))
        .expect_err("placeholder refused");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_BOOTSTRAP_ADMIN_PASSWORD",
                ..
            }
        ));
    }

    #[test]
    fn refuses_the_placeholder_secret_in_production() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "change-me"),
            ("KELIR_APP_ENV", "production"),
        ]))
        .expect_err("the compose placeholder must not reach production");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                ..
            }
        ));
    }

    #[test]
    fn refuses_the_placeholder_secret_in_staging() {
        // Staging is internet-facing; the same rule as production applies.
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "change-me"),
            ("KELIR_APP_ENV", "staging"),
        ]))
        .expect_err("staging must not run on the placeholder either");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                ..
            }
        ));
    }

    #[test]
    fn allows_the_placeholder_secret_only_on_a_developer_machine() {
        // Local development and CI rely on the placeholder working.
        for env in ["development", "test"] {
            AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "change-me"),
                ("KELIR_APP_ENV", env),
            ]))
            .unwrap_or_else(|_| panic!("{env} accepts the placeholder"));
        }
    }

    #[test]
    fn accepts_a_real_secret_in_production() {
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "a-real-deployment-secret"),
            ("KELIR_APP_ENV", "production"),
        ]))
        .expect("a non-placeholder secret is accepted");

        assert_eq!(config.app_env, AppEnv::Production);
    }

    #[test]
    fn is_single_tenant_unless_the_flag_is_set() {
        // FR-IDM-009: multi-tenancy is opt-in, so an existing deployment that
        // sets neither variable keeps today's behaviour exactly.
        let config =
            AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")])).expect("loads");

        assert!(!config.multi_tenant);
        assert_eq!(config.default_tenant_code, "SYSTEM");
    }

    #[test]
    fn reads_the_multi_tenant_flag_in_every_documented_spelling() {
        for raw in ["false", "0", "no", "off", "FALSE"] {
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_MULTI_TENANT", raw),
            ]))
            .unwrap_or_else(|_| panic!("{raw} parses"));

            assert!(!config.multi_tenant, "for {raw}");
        }
    }

    #[test]
    fn refuses_to_start_in_multi_tenant_mode_however_the_flag_is_spelled() {
        // Every spelling `flag` accepts, not only the literal "true": a guard
        // that matched one of them would let `KELIR_MULTI_TENANT=1` start a
        // deployment nobody can sign in to (#67), which is the whole failure
        // it exists to prevent.
        for raw in ["true", "1", "yes", "on", "TRUE"] {
            let error = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_MULTI_TENANT", raw),
            ]))
            .unwrap_err();

            let ConfigError::Invalid { key, reason } = error else {
                panic!("{raw} must be refused as invalid configuration");
            };

            assert_eq!(key, "KELIR_MULTI_TENANT");
            assert!(
                reason.contains("not supported yet"),
                "for {raw}, the operator is not told why: {reason}"
            );
            assert!(
                reason.contains("false"),
                "for {raw}, the operator is not told what to do instead: {reason}"
            );
        }
    }

    #[test]
    fn an_unrecognised_multi_tenant_value_is_refused_as_a_typo_not_as_the_guard() {
        // Both refusals carry the same key, and they mean different things: one
        // says "this deployment cannot run yet", the other says "that is not a
        // boolean". An operator who reads the wrong one edits the wrong thing.
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_MULTI_TENANT", "enabled"),
        ]))
        .unwrap_err();

        let ConfigError::Invalid { reason, .. } = error else {
            panic!("an unparseable flag is invalid configuration");
        };

        assert!(
            reason.contains("expected a boolean"),
            "a typo was reported as the tenancy guard: {reason}"
        );
    }

    #[test]
    fn refuses_a_multi_tenant_flag_it_does_not_understand() {
        // Reading an unrecognised value as "off" would leave an operator
        // believing tenancy is enforced while every sign-in lands in the
        // default tenant.
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_MULTI_TENANT", "enabled"),
        ]))
        .expect_err("only booleans are accepted");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_MULTI_TENANT",
                ..
            }
        ));
    }

    #[test]
    fn normalises_the_default_tenant_code() {
        // The configured code and the codes callers send must normalise the
        // same way, or the default tenant would fail to resolve against itself.
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_DEFAULT_TENANT_CODE", "  acme  "),
        ]))
        .expect("loads");

        assert_eq!(config.default_tenant_code, "ACME");
    }

    #[test]
    fn trusts_no_proxy_unless_one_is_configured() {
        // The whole point of the default: an unconfigured deployment must not
        // believe X-Forwarded-For, because the caller writes it.
        let config =
            AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")])).expect("loads");

        assert_eq!(config.trusted_proxy_hops, 0);
    }

    #[test]
    fn reads_the_configured_proxy_hop_count() {
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_TRUSTED_PROXY_HOPS", "1"),
        ]))
        .expect("loads");

        assert_eq!(config.trusted_proxy_hops, 1);
    }

    #[test]
    fn rejects_a_proxy_hop_count_that_is_not_a_number() {
        // Falling back to a default on a typo would silently change the trust
        // model, which is exactly the class of mistake this variable governs.
        for raw in ["one", "-1", "1.5", "1,2"] {
            let result = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_TRUSTED_PROXY_HOPS", raw),
            ]));

            assert!(
                matches!(
                    result,
                    Err(ConfigError::Invalid {
                        key: "KELIR_TRUSTED_PROXY_HOPS",
                        ..
                    })
                ),
                "{raw} is not a hop count"
            );
        }
    }

    #[test]
    fn rejects_an_implausible_proxy_hop_count() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_TRUSTED_PROXY_HOPS", "99"),
        ]))
        .expect_err("99 hops is a misconfiguration");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_TRUSTED_PROXY_HOPS",
                ..
            }
        ));
    }

    #[test]
    fn accepts_the_documented_environments() {
        for (raw, expected) in [
            ("development", AppEnv::Development),
            ("test", AppEnv::Test),
            ("staging", AppEnv::Staging),
            ("production", AppEnv::Production),
        ] {
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_APP_ENV", raw),
            ]))
            .expect("documented environment parses");

            assert_eq!(config.app_env, expected, "for {raw}");
        }
    }
}
