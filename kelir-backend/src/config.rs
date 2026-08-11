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
    pub frontend_url: String,
}

/// Secrets shipped in `.env.example` and the compose stack. Never valid in
/// production.
const PLACEHOLDER_SECRETS: &[&str] = &["change-me", "changeme", "secret", "test-secret"];

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

    /// Production must not run on development defaults for anything sensitive.
    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
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
        if app_env.is_production() && PLACEHOLDER_SECRETS.contains(&jwt_secret.as_str()) {
            return Err(ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                reason: "the development placeholder cannot be used in production".to_owned(),
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
            frontend_url: optional("KELIR_FRONTEND_URL", "http://localhost:5173"),
        })
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
    fn allows_the_placeholder_secret_outside_production() {
        // Local development and CI rely on the placeholder working.
        for env in ["development", "test", "staging"] {
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

        assert!(config.app_env.is_production());
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
