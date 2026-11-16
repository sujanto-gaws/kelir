use std::collections::HashMap;
use std::env::{self, VarError};
use std::fmt;
use std::net::IpAddr;

use crate::utils::cidr::{self, Cidr};

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
/// Secrets never carry a default that works where it matters (coding standard
/// §2.8): a missing `KELIR_JWT_SECRET` fails startup rather than silently
/// running on a placeholder, and the object-store credentials fall back only in
/// `development` and `test` — staging and production refuse to start on them
/// ([#317](https://github.com/sujanto-gaws/kelir/issues/317)). Everything else
/// falls back to a development-friendly value everywhere.
///
/// **Its `Debug` prints no secret** (see the impl below): both database URLs
/// carry a password, and the owner's stays in this struct for the process's
/// life (ADR-0051 §4).
#[derive(Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub app_env: AppEnv,
    pub bind_address: String,
    /// `KELIR_DATABASE_URL`: **the serving role's URL** (ADR-0051 §2), the
    /// role every request, worker and the administrator bootstrap use. It owns
    /// nothing and may not change `audit_events`.
    pub database_url: String,
    /// `KELIR_DATABASE_MIGRATION_URL`: the schema owner's URL, which migrates
    /// at startup through one connection of its own, named `kelir-migrate`,
    /// that is closed before the server binds (ADR-0051 §2, #991).
    ///
    /// **`None` is the development shape**: one URL migrates and serves, as
    /// every release before `v1.0.0` did. `staging` and `production` refuse to
    /// start without it, as they refuse a missing `KELIR_JWT_SECRET`.
    pub database_migration_url: Option<String>,
    /// `KELIR_DATABASE_SSL_MODE` and `KELIR_DATABASE_SSL_ROOT_CERT` (#803),
    /// as given, or `None` when unset or blank. Checked where the pool is
    /// built (`db::connect_options`), which is the one place that knows the
    /// host they apply to.
    pub database_ssl_mode: Option<String>,
    pub database_ssl_root_cert: Option<String>,
    pub jwt_secret: String,
    pub storage_driver: String,
    /// Where object storage is, and what it is called there (FR-ATT-001, #244).
    ///
    /// The defaults are the compose stack's MinIO, so a developer who has run
    /// `deploy/docker` has working attachments without setting anything. **The
    /// bucket is not created by this process**: the deployment provisions it and
    /// the application holds credentials that can put and get objects in one,
    /// and abort an upload of its own, which is the privilege it should have
    /// rather than the one that is convenient.
    ///
    /// **The two credentials have defaults on a developer's machine and none
    /// anywhere else** ([#317]). Staging and production refuse to start with
    /// them unset, because MinIO's own well-known credentials reaching a
    /// deployment nobody typed them into is the failure this pair had until
    /// Sprint 13: the guard existed, in a compose file, and a deployment that
    /// did not use that file got no guard at all.
    ///
    /// [#317]: https://github.com/sujanto-gaws/kelir/issues/317
    pub storage_endpoint: String,
    pub storage_bucket: String,
    pub storage_access_key: String,
    pub storage_secret_key: String,
    pub storage_region: String,
    /// The largest file this deployment accepts, in bytes (FR-ATT-004, #245).
    ///
    /// **Enforced on the request body before it is read**, not on what has
    /// already been accepted: a limit applied to bytes in hand is a limit on the
    /// disk. 25 MB by default, which is the figure the Sprint 12 plan measures
    /// the scanner against — the two want to agree, because a file this accepts
    /// and the scanner cannot hold is a file stuck at `PENDING` for ever.
    pub storage_max_upload_bytes: usize,
    /// What may be stored, by **content** rather than by the name a caller typed
    /// (FR-ATT-005, #245 AC4).
    ///
    /// Empty means *refuse everything*, deliberately: the failure direction for
    /// a misconfigured allow-list is to store nothing, not to store anything.
    pub storage_allowed_mime_types: Vec<String>,
    /// Where the virus scanner listens (FR-ATT-001, FR-ATT-002; #246).
    ///
    /// **There is no switch that turns scanning off**, and that is the design.
    /// An unreachable scanner leaves every attachment `PENDING`, which leaves
    /// every attachment undownloadable — the failure direction #246 AC7 asks
    /// for. A flag saying *skip the scan* would be a flag saying *serve
    /// unscanned bytes*, and no deployment should be one setting away from that.
    ///
    /// The default is the compose stack's service name, so a developer who has
    /// run `deploy/docker` has working scanning without setting anything.
    pub clamav_host: String,
    pub clamav_port: u16,
    /// How often the worker looks for attachments nobody has scanned.
    ///
    /// **A poll rather than a queue**, because the queue this would use is the
    /// outbox and the outbox is Phase 9 (D-79). The cost of the choice is latency
    /// bounded by this number; the benefit is that a scan lost to a restart is
    /// picked up again, which a spawned task would not be.
    pub clamav_poll_seconds: u64,
    /// How often the sender looks for notifications nobody has delivered yet
    /// (FR-NTF-004, [#257](https://github.com/sujanto-gaws/kelir/issues/257)).
    ///
    /// **A poll for `clamav_poll_seconds`' reason**, and one more: the queue is
    /// the notification row itself, written in the transaction of the thing it
    /// announces (#251 AC3), so a delivery lost to a restart is picked up by the
    /// next pass where a spawned task would be gone. The cost is latency bounded
    /// by this number — an email arrives within a few seconds of the approval
    /// that caused it, not within a few milliseconds.
    pub notification_poll_seconds: u64,
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
    /// Private networks an administrator's test call may reach
    /// (`KELIR_INTEGRATION_ALLOWED_CIDRS`; FR-INT-002, #547, the product
    /// owner's answer 2 of 2026-09-29).
    ///
    /// **Empty by default, and empty refuses every private address.** A private
    /// range (RFC 1918, IPv6 ULA) is reachable only inside a network listed
    /// here. Loopback, link-local, unspecified and multicast addresses, and
    /// three cloud metadata addresses outside link-local (#622), are
    /// refused whatever this says: listing `127.0.0.0/8` does not open it, see
    /// `integration::domain::egress`.
    pub integration_allowed_cidrs: Vec<Cidr>,
    /// **A test seam, and not a setting.** `true` lets the egress guard pass a
    /// loopback address, so an integration test can call a mock server on
    /// `127.0.0.1`.
    ///
    /// Nothing reads it from the environment: [`AppConfig::from_env`] always
    /// sets `false`, and there is no variable a deployment could set to change
    /// that. Only code that builds an `AppConfig` by hand — the test harness —
    /// can turn it on.
    pub integration_allow_loopback: bool,
    /// **A test seam, and not a setting.** Host names a test call's lookup
    /// answers from this table instead of the system resolver: a lowercase
    /// name, and the addresses it "resolves" to, in order. A name not in the
    /// table is looked up as usual, and the answer is judged by the egress
    /// guard either way.
    ///
    /// It exists so an integration test can make a name resolve to several
    /// addresses — one public and one private, or one that does not answer
    /// before one that does — which no real zone on a test machine will. Like
    /// [`AppConfig::integration_allow_loopback`], nothing reads it from the
    /// environment: [`AppConfig::from_env`] always leaves it empty.
    pub integration_dns_overrides: HashMap<String, Vec<IpAddr>>,
}

/// What a deployment accepts when it says nothing.
///
/// Five types, and the list is short on purpose: an allow-list is a statement
/// about what this product is for, and a long one is a statement that nobody
/// decided. A deployment that needs more says so in
/// `KELIR_STORAGE_ALLOWED_MIME_TYPES`.
const DEFAULT_ALLOWED_MIME_TYPES: &str = "application/pdf,image/png,image/jpeg,\
    application/vnd.openxmlformats-officedocument.wordprocessingml.document,\
    application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// A comma-separated allow-list, normalized once here so the comparison later is
/// a plain equality rather than a trim on every upload.
fn mime_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect()
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

/// Reads `KELIR_MULTI_TENANT`.
///
/// **This used to refuse to start when the flag was on, and decision D-18
/// removed the refusal.** The guard existed because tenancy was half-built:
/// `resolve_for_sign_in` demanded a tenant code, the login form had no field
/// for one, and a deployment that switched the flag on reached a state where
/// nobody could sign in at all (#67). Refusing at startup turned that into a
/// container that did not start and said which variable to unset — decision
/// **D-7**, 2026-08-20.
///
/// Both halves it was waiting for now exist: `GET /api/v1/deployment` tells the
/// login form which mode it is talking to, and `POST /auth/login` has carried
/// `tenantCode` since Sprint 4. Tenants are administrable (FR-ORG-001), and a
/// created tenant has an administrator who can sign in to it. So the flag is
/// honoured, and the only thing left here is parsing it.
///
/// It stays a flag rather than becoming always-on. With it off,
/// `resolve_for_sign_in` **ignores** a supplied code and resolves the
/// configured default, which is what keeps a single-tenant deployment's login
/// contract unchanged — and what stops a caller on such a deployment naming
/// somebody else's tenant.
fn multi_tenant(raw: &str) -> Result<bool, ConfigError> {
    flag(raw, "KELIR_MULTI_TENANT")
}

/// Secrets shipped in `.env.example` and the compose stack. Never valid in
/// production.
const PLACEHOLDER_SECRETS: &[&str] = &["change-me", "changeme", "secret", "test-secret"];

/// The shortest signing secret staging and production accept, in bytes of
/// UTF-8 (#837, NFR-SEC-007). Access tokens are HS256, and RFC 7518 §3.2 asks
/// for a key at least as long as the hash output. Bytes, because the bytes
/// are the HMAC key: `é` counts two.
const MIN_SIGNING_SECRET_BYTES: usize = 32;

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

/// What `Debug` prints in place of a secret.
const REDACTED: &str = "<redacted>";

/// **Names every setting and prints no secret.** A derived `Debug` printed
/// both database URLs with their passwords, the signing secret, the store's
/// pair and the bootstrap password into any `{:?}` of the configuration. A
/// secret that is set reads `<redacted>`; an optional one that is not reads
/// `None`, so the output still says which are configured.
impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let set = |value: &Option<String>| value.as_ref().map(|_| REDACTED);

        f.debug_struct("AppConfig")
            .field("app_name", &self.app_name)
            .field("app_env", &self.app_env)
            .field("bind_address", &self.bind_address)
            .field("database_url", &REDACTED)
            .field("database_migration_url", &set(&self.database_migration_url))
            .field("database_ssl_mode", &self.database_ssl_mode)
            .field("database_ssl_root_cert", &self.database_ssl_root_cert)
            .field("jwt_secret", &REDACTED)
            .field("storage_driver", &self.storage_driver)
            .field("storage_endpoint", &self.storage_endpoint)
            .field("storage_bucket", &self.storage_bucket)
            .field("storage_access_key", &REDACTED)
            .field("storage_secret_key", &REDACTED)
            .field("storage_region", &self.storage_region)
            .field("storage_max_upload_bytes", &self.storage_max_upload_bytes)
            .field(
                "storage_allowed_mime_types",
                &self.storage_allowed_mime_types,
            )
            .field("clamav_host", &self.clamav_host)
            .field("clamav_port", &self.clamav_port)
            .field("clamav_poll_seconds", &self.clamav_poll_seconds)
            .field("notification_poll_seconds", &self.notification_poll_seconds)
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("mail_from", &self.mail_from)
            .field("frontend_url", &self.frontend_url)
            .field("multi_tenant", &self.multi_tenant)
            .field("default_tenant_code", &self.default_tenant_code)
            .field(
                "bootstrap_admin",
                &self.bootstrap_admin.as_ref().map(|admin| &admin.username),
            )
            .field("trusted_proxy_hops", &self.trusted_proxy_hops)
            .field("integration_allowed_cidrs", &self.integration_allowed_cidrs)
            .field(
                "integration_allow_loopback",
                &self.integration_allow_loopback,
            )
            .field("integration_dns_overrides", &self.integration_dns_overrides)
            .finish()
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing {
        key: &'static str,
    },
    Invalid {
        key: &'static str,
        reason: String,
    },
    /// A credential with a development default was left at it, somewhere that
    /// default is not a choice anybody made ([#317]).
    ///
    /// Distinct from [`Self::Missing`] because the variable is not required —
    /// it has a default, and that default is correct on a developer's machine.
    /// What is wrong is *where* it was used, so the message says the
    /// environment as well as the key.
    ///
    /// [#317]: https://github.com/sujanto-gaws/kelir/issues/317
    Defaulted {
        key: &'static str,
        app_env: AppEnv,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { key } => write!(f, "{key} is required but was not set"),
            Self::Invalid { key, reason } => write!(f, "{key} is invalid: {reason}"),
            Self::Defaulted { key, app_env } => write!(
                f,
                "{key} was left at its development default, which {app_env} does not accept — \
                 set it to the credential this deployment actually uses"
            ),
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

        // For a setting whose absence means something of its own, which no
        // fallback string could stand for.
        let unset_if_blank = |key: &'static str| -> Option<String> {
            get(key).ok().filter(|value| !value.trim().is_empty())
        };

        let required = |key: &'static str| -> Result<String, ConfigError> {
            get(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ConfigError::Missing { key })
        };

        let app_env = AppEnv::parse(&optional("KELIR_APP_ENV", "development"))?;

        // **A default is a convenience, not a decision** ([#317]).
        //
        // `optional` cannot tell a value somebody set from a value nobody set:
        // by the time it returns, `minioadmin` from the environment and
        // `minioadmin` from the fallback are the same string. That is fine for
        // a bind address and wrong for a credential, because the whole question
        // about a credential is whether anybody chose it.
        //
        // So this is `optional` where the default is harmless and `required`
        // where it is not. **Not a list of known-bad strings**: a deployment
        // that sets `minioadmin` deliberately has made a choice this has no
        // business overruling, and a list would have to grow a row for every
        // credential's placeholder and would still miss the next one.
        //
        // [#317]: https://github.com/sujanto-gaws/kelir/issues/317
        let credential = |key: &'static str, fallback: &str| -> Result<String, ConfigError> {
            match get(key).ok().filter(|value| !value.trim().is_empty()) {
                Some(value) => Ok(value),
                None if app_env.requires_real_secrets() => {
                    Err(ConfigError::Defaulted { key, app_env })
                }
                None => Ok(fallback.to_owned()),
            }
        };

        let jwt_secret = required("KELIR_JWT_SECRET")?;

        // Surrounding whitespace is where `deploy.sh`'s sourcing, compose's
        // `.env` parser and this loader can read different bytes, and padding
        // would carry a placeholder past the floor (#837). Refused first, by
        // `char::is_whitespace`, so the message is about the padding.
        if app_env.requires_real_secrets()
            && (jwt_secret.starts_with(char::is_whitespace)
                || jwt_secret.ends_with(char::is_whitespace))
        {
            return Err(ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                reason: format!("must not begin or end with whitespace in {app_env}"),
            });
        }

        // `.env.example` and the compose stack ship a placeholder secret so
        // local development works out of the box. Shipping that placeholder to
        // production would make every token forgeable, so refuse to start.
        // Compared trimmed, so no padding gets one past this check.
        if app_env.requires_real_secrets() && PLACEHOLDER_SECRETS.contains(&jwt_secret.trim()) {
            return Err(ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                reason: format!("the development placeholder cannot be used in {app_env}"),
            });
        }

        // A secret short enough to guess forges every token as surely as the
        // placeholder does (#837). The value is the key, measured in bytes; the
        // message gives the minimum and never the value.
        if app_env.requires_real_secrets() && jwt_secret.len() < MIN_SIGNING_SECRET_BYTES {
            return Err(ConfigError::Invalid {
                key: "KELIR_JWT_SECRET",
                reason: format!(
                    "must be at least {MIN_SIGNING_SECRET_BYTES} bytes in {app_env}; \
                     generate one with `openssl rand -base64 48`"
                ),
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
            database_ssl_mode: unset_if_blank("KELIR_DATABASE_SSL_MODE"),
            database_ssl_root_cert: unset_if_blank("KELIR_DATABASE_SSL_ROOT_CERT"),
            jwt_secret,
            storage_driver: optional("KELIR_STORAGE_DRIVER", "local"),
            storage_endpoint: optional("KELIR_STORAGE_ENDPOINT", "http://localhost:9000"),
            storage_bucket: optional("KELIR_STORAGE_BUCKET", "kelir"),
            storage_access_key: credential("KELIR_STORAGE_ACCESS_KEY", "minioadmin")?,
            storage_secret_key: credential("KELIR_STORAGE_SECRET_KEY", "minioadmin")?,
            storage_region: optional("KELIR_STORAGE_REGION", "us-east-1"),
            storage_max_upload_bytes: {
                let raw = optional("KELIR_STORAGE_MAX_UPLOAD_BYTES", "26214400");

                raw.parse().map_err(|_| ConfigError::Invalid {
                    key: "KELIR_STORAGE_MAX_UPLOAD_BYTES",
                    reason: format!("expected a size in bytes; found '{raw}'"),
                })?
            },
            storage_allowed_mime_types: mime_list(&optional(
                "KELIR_STORAGE_ALLOWED_MIME_TYPES",
                DEFAULT_ALLOWED_MIME_TYPES,
            )),
            clamav_host: optional("KELIR_CLAMAV_HOST", "clamav"),
            clamav_port: {
                let raw = optional("KELIR_CLAMAV_PORT", "3310");

                raw.parse().map_err(|_| ConfigError::Invalid {
                    key: "KELIR_CLAMAV_PORT",
                    reason: format!("expected a port number; found '{raw}'"),
                })?
            },
            clamav_poll_seconds: {
                let raw = optional("KELIR_CLAMAV_POLL_SECONDS", "5");

                raw.parse().map_err(|_| ConfigError::Invalid {
                    key: "KELIR_CLAMAV_POLL_SECONDS",
                    reason: format!("expected a number of seconds; found '{raw}'"),
                })?
            },
            notification_poll_seconds: {
                let raw = optional("KELIR_NOTIFICATION_POLL_SECONDS", "5");

                raw.parse().map_err(|_| ConfigError::Invalid {
                    key: "KELIR_NOTIFICATION_POLL_SECONDS",
                    reason: format!("expected a number of seconds; found '{raw}'"),
                })?
            },
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
            integration_allowed_cidrs: {
                let raw = optional("KELIR_INTEGRATION_ALLOWED_CIDRS", "");

                cidr::parse_list(&raw).map_err(|error| ConfigError::Invalid {
                    key: "KELIR_INTEGRATION_ALLOWED_CIDRS",
                    reason: error.to_string(),
                })?
            },
            // Never from the environment: see the fields.
            integration_allow_loopback: false,
            integration_dns_overrides: HashMap::new(),
            // Last, so a refusal that names an older setting still comes first.
            database_migration_url: database_migration_url(&get, app_env)?,
        })
    }
}

/// `KELIR_DATABASE_MIGRATION_URL` (ADR-0051 §2).
///
/// **The single-URL shape is not a deployment's.** Where it is missing, the
/// serving URL would have to migrate, and a role that migrates owns the schema,
/// so `staging` and `production` refuse to start, naming the variable. A
/// developer's machine and the test harness keep one URL when they want it.
fn database_migration_url<F>(get: &F, app_env: AppEnv) -> Result<Option<String>, ConfigError>
where
    F: Fn(&'static str) -> Result<String, VarError>,
{
    let key = "KELIR_DATABASE_MIGRATION_URL";

    match get(key).ok().filter(|value| !value.trim().is_empty()) {
        Some(url) => Ok(Some(url)),
        None if app_env.requires_real_secrets() => Err(ConfigError::Missing { key }),
        None => Ok(None),
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
            database_migration_url: None,
            database_ssl_mode: None,
            database_ssl_root_cert: None,
            jwt_secret: "test-secret".to_owned(),
            storage_driver: "local".to_owned(),
            storage_endpoint: "http://localhost:9000".to_owned(),
            storage_bucket: "kelir".to_owned(),
            storage_access_key: "minioadmin".to_owned(),
            storage_secret_key: "minioadmin".to_owned(),
            storage_region: "us-east-1".to_owned(),
            storage_max_upload_bytes: 26_214_400,
            storage_allowed_mime_types: mime_list(DEFAULT_ALLOWED_MIME_TYPES),
            clamav_host: "localhost".to_owned(),
            clamav_port: 3310,
            clamav_poll_seconds: 5,
            notification_poll_seconds: 5,
            smtp_host: "localhost".to_owned(),
            smtp_port: 1025,
            mail_from: "no-reply@kelir.test".to_owned(),
            frontend_url: "http://localhost:5173".to_owned(),
            multi_tenant: false,
            default_tenant_code: "SYSTEM".to_owned(),
            bootstrap_admin: None,
            trusted_proxy_hops: 0,
            integration_allowed_cidrs: Vec::new(),
            integration_allow_loopback: false,
            integration_dns_overrides: HashMap::new(),
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
    fn the_integration_allow_list_is_empty_unless_set_and_parsed_when_it_is() {
        let unset =
            AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")])).expect("loads");
        assert!(unset.integration_allowed_cidrs.is_empty());

        let set = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_INTEGRATION_ALLOWED_CIDRS", "10.20.0.0/16, fd00::/8"),
        ]))
        .expect("loads");
        assert_eq!(set.integration_allowed_cidrs.len(), 2);
    }

    #[test]
    fn a_malformed_integration_allow_list_refuses_to_start() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_INTEGRATION_ALLOWED_CIDRS", "10.0.0.0/8,intranet"),
        ]))
        .expect_err("half an allow-list is not what the operator wrote");

        assert!(matches!(
            error,
            ConfigError::Invalid {
                key: "KELIR_INTEGRATION_ALLOWED_CIDRS",
                ..
            }
        ));
    }

    #[test]
    fn no_environment_variable_opens_loopback_to_a_test_call() {
        // The seam is for the harness, which builds its config by hand. Every
        // spelling a deployer might guess at leaves it off.
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_INTEGRATION_ALLOW_LOOPBACK", "true"),
            ("KELIR_INTEGRATION_ALLOWED_CIDRS", "127.0.0.0/8"),
            (
                "KELIR_INTEGRATION_DNS_OVERRIDES",
                "erp.example.com=127.0.0.1",
            ),
        ]))
        .expect("loads");

        assert!(!config.integration_allow_loopback);
        assert!(config.integration_dns_overrides.is_empty());
    }

    #[test]
    fn reads_the_database_tls_settings_and_discards_blank_ones() {
        let unset =
            AppConfig::from_source(source(&[("KELIR_JWT_SECRET", "s3cret")])).expect("loads");
        assert_eq!(unset.database_ssl_mode, None);
        assert_eq!(unset.database_ssl_root_cert, None);

        let set = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_DATABASE_SSL_MODE", "verify-full"),
            ("KELIR_DATABASE_SSL_ROOT_CERT", "/etc/kelir/db-tls/ca.pem"),
        ]))
        .expect("loads");
        assert_eq!(set.database_ssl_mode.as_deref(), Some("verify-full"));
        assert_eq!(
            set.database_ssl_root_cert.as_deref(),
            Some("/etc/kelir/db-tls/ca.pem")
        );

        // What the staging compose file passes when `.env` says nothing.
        let blank = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_DATABASE_SSL_MODE", ""),
            ("KELIR_DATABASE_SSL_ROOT_CERT", "  "),
        ]))
        .expect("loads");
        assert_eq!(blank.database_ssl_mode, None);
        assert_eq!(blank.database_ssl_root_cert, None);
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
            ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
            ("KELIR_APP_ENV", "staging"),
            ("KELIR_BOOTSTRAP_ADMIN_USERNAME", "admin"),
            ("KELIR_BOOTSTRAP_ADMIN_PASSWORD", "change-me"),
            ("KELIR_STORAGE_ACCESS_KEY", "a-deployment-key"),
            ("KELIR_STORAGE_SECRET_KEY", "a-deployment-secret"),
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

    // -----------------------------------------------------------------------
    // The object-store credentials (#317)
    //
    // Seen to fail (coding standard §2.9), three mutations run 2026-09-03:
    //
    // | Mutation | Reddened |
    // |---|---|
    // | The guard never fires — every environment falls back | *refuses a storage access key nobody set in staging*; *refuses a storage secret key nobody set in production* |
    // | The guard fires everywhere, a developer's machine included | sixteen, *the storage defaults still apply where they are meant to* among them — a guard that refuses too much is the failure that gets it reverted |
    // | Only the secret key guarded, the access key left as it was | *refuses a storage access key nobody set in staging* |
    // -----------------------------------------------------------------------

    /// **An object-store credential nobody set is not a credential** (#317
    /// AC1). Staging and production refuse to start, naming the variable, the
    /// way a missing `KELIR_JWT_SECRET` already did.
    ///
    /// The access key is checked first because it is read first; the test after
    /// this one supplies it so the secret key is what is left defaulted.
    #[test]
    fn refuses_a_storage_access_key_nobody_set_in_staging() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
            ("KELIR_APP_ENV", "staging"),
        ]))
        .expect_err("staging must not run on MinIO's own credentials");

        assert!(
            matches!(
                error,
                ConfigError::Defaulted {
                    key: "KELIR_STORAGE_ACCESS_KEY",
                    app_env: AppEnv::Staging,
                }
            ),
            "{error}"
        );
    }

    /// The other half, in production, with the access key supplied (#317 AC2).
    ///
    /// **An access key somebody set beside a secret key nobody set is the same
    /// misconfiguration half-done**, and is the shape a deployment reaches by
    /// copying one line out of an example file.
    #[test]
    fn refuses_a_storage_secret_key_nobody_set_in_production() {
        let error = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
            ("KELIR_APP_ENV", "production"),
            ("KELIR_STORAGE_ACCESS_KEY", "kelir-production"),
        ]))
        .expect_err("production must not run on a defaulted secret key");

        assert!(
            matches!(
                error,
                ConfigError::Defaulted {
                    key: "KELIR_STORAGE_SECRET_KEY",
                    app_env: AppEnv::Production,
                }
            ),
            "{error}"
        );
    }

    /// **A deployment that sets `minioadmin` has made a choice** (#317 AC4).
    ///
    /// The condition is *nobody set this*, not *this value is on a list of bad
    /// ones*. A list would have to grow a row for every credential's
    /// placeholder, would still miss the next one, and would overrule an
    /// operator who really is running MinIO with its defaults on a closed
    /// network — which is their call, not this loader's.
    #[test]
    fn a_credential_somebody_set_is_accepted_however_ordinary_it_looks() {
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
            ("KELIR_APP_ENV", "production"),
            ("KELIR_STORAGE_ACCESS_KEY", "minioadmin"),
            ("KELIR_STORAGE_SECRET_KEY", "minioadmin"),
            ("KELIR_DATABASE_MIGRATION_URL", OWNER_URL),
        ]))
        .expect("a value somebody set is a value somebody chose");

        assert_eq!(config.storage_secret_key, "minioadmin");
    }

    /// **Nothing changes on a developer's machine** (#317 AC3).
    ///
    /// Local development, the `deploy/docker` stack and CI all rely on the
    /// defaults working. A guard that made `cargo test` need credentials would
    /// have broken more than it fixed.
    #[test]
    fn the_storage_defaults_still_apply_where_they_are_meant_to() {
        for env in ["development", "test"] {
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "change-me"),
                ("KELIR_APP_ENV", env),
            ]))
            .unwrap_or_else(|error| panic!("{env} still loads without credentials: {error}"));

            assert_eq!(config.storage_access_key, "minioadmin");
            assert_eq!(config.storage_secret_key, "minioadmin");
        }
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
            ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
            ("KELIR_APP_ENV", "production"),
            ("KELIR_STORAGE_ACCESS_KEY", "a-deployment-key"),
            ("KELIR_STORAGE_SECRET_KEY", "a-deployment-secret"),
            ("KELIR_DATABASE_MIGRATION_URL", OWNER_URL),
        ]))
        .expect("a non-placeholder secret is accepted");

        assert_eq!(config.app_env, AppEnv::Production);
    }

    // -----------------------------------------------------------------------
    // The signing secret's floor (#837, NFR-SEC-007)
    //
    // Access tokens are HS256, and RFC 7518 §3.2 asks for a key at least as
    // long as the hash output: 32 bytes. The floor is counted in bytes of
    // UTF-8, not in characters, and holds where the placeholder is refused:
    // staging and production. Development and test keep `change-me`.
    // -----------------------------------------------------------------------

    /// A signing secret long enough for a deployment, for the tests that load
    /// `staging` or `production` and are about something else.
    const DEPLOYMENT_SECRET: &str = "a-deployment-signing-secret-at-least-32-bytes-long";

    /// Loads `env` with `secret` and everything else a deployment needs, so
    /// the signing secret is the only thing that can refuse.
    fn load_deployed(env: &str, secret: &str) -> Result<AppConfig, ConfigError> {
        AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", secret),
            ("KELIR_APP_ENV", env),
            ("KELIR_STORAGE_ACCESS_KEY", "a-deployment-key"),
            ("KELIR_STORAGE_SECRET_KEY", "a-deployment-secret"),
            ("KELIR_DATABASE_MIGRATION_URL", OWNER_URL),
        ]))
    }

    /// Asserts `error` refuses the signing secret, naming the setting and the
    /// minimum and not the value.
    fn assert_refused_as_short(error: &ConfigError, secret: &str, env: &str) {
        assert!(
            matches!(
                error,
                ConfigError::Invalid {
                    key: "KELIR_JWT_SECRET",
                    ..
                }
            ),
            "{env}: {error}"
        );
        let message = error.to_string();
        assert!(
            message.contains("KELIR_JWT_SECRET"),
            "{env}: the refusal names the setting: {message}"
        );
        assert!(
            message.contains("32 bytes"),
            "{env}: the refusal names the minimum: {message}"
        );
        assert!(
            !message.contains(secret),
            "{env}: the refusal never prints the secret: {message}"
        );
    }

    /// **31 bytes is refused where it is deployed**, as the placeholder is.
    #[test]
    fn refuses_a_31_byte_signing_secret_in_staging_and_production() {
        let secret = "s".repeat(31);
        for env in ["staging", "production"] {
            let error = load_deployed(env, &secret)
                .expect_err("a 31-byte secret is shorter than HS256's 32");

            assert_refused_as_short(&error, &secret, env);
        }
    }

    /// **32 bytes is enough**: the boundary itself is accepted.
    #[test]
    fn accepts_a_32_byte_signing_secret_in_staging_and_production() {
        let secret = "s".repeat(32);
        for env in ["staging", "production"] {
            let config = load_deployed(env, &secret)
                .unwrap_or_else(|error| panic!("{env} refused 32 bytes: {error}"));

            assert_eq!(config.jwt_secret, secret, "for {env}");
        }
    }

    /// **Bytes, not characters.** Fifteen `é` and an `a` are sixteen
    /// characters and 31 bytes: refused, though a character count would call
    /// it sixteen and refuse it too. The test after this one is the half a
    /// character count gets wrong.
    #[test]
    fn refuses_a_multibyte_secret_of_31_bytes() {
        let secret = format!("{}a", "é".repeat(15));
        assert_eq!(secret.len(), 31);
        for env in ["staging", "production"] {
            let error = load_deployed(env, &secret)
                .expect_err("31 bytes is short however many characters they hold");

            assert_refused_as_short(&error, &secret, env);
        }
    }

    /// Thirty `a` and an `é` are 31 characters and 32 bytes: accepted. A floor
    /// counted in characters refuses this one.
    #[test]
    fn accepts_a_multibyte_secret_of_32_bytes_and_31_characters() {
        let secret = format!("{}é", "a".repeat(30));
        assert_eq!((secret.len(), secret.chars().count()), (32, 31));
        for env in ["staging", "production"] {
            load_deployed(env, &secret)
                .unwrap_or_else(|error| panic!("{env} counted characters: {error}"));
        }
    }

    /// **A secret that begins or ends with whitespace is refused** where it is
    /// deployed, at any length (#837, the product owner, 2026-11-16).
    /// `deploy.sh`, compose and the backend can each read different bytes
    /// there, and padding carries a placeholder past the floor: `change-me`
    /// and 23 spaces is 32 bytes. `char::is_whitespace`, so a no-break space
    /// counts.
    #[test]
    fn refuses_a_signing_secret_that_begins_or_ends_with_whitespace() {
        let padded = [
            format!(" {DEPLOYMENT_SECRET}"),
            format!("{DEPLOYMENT_SECRET} "),
            format!("{DEPLOYMENT_SECRET}\n"),
            format!("\t{DEPLOYMENT_SECRET}"),
            format!("\u{a0}{DEPLOYMENT_SECRET}"),
            format!("change-me{}", " ".repeat(23)),
            " change-me".to_owned(),
        ];
        for env in ["staging", "production"] {
            for secret in &padded {
                let error = load_deployed(env, secret)
                    .expect_err("surrounding whitespace is refused, however long");

                assert!(
                    matches!(
                        error,
                        ConfigError::Invalid {
                            key: "KELIR_JWT_SECRET",
                            ..
                        }
                    ),
                    "{env}, {secret:?}: {error}"
                );
                let message = error.to_string();
                assert!(
                    message.contains("must not begin or end with whitespace"),
                    "{env}, {secret:?}: the refusal says why: {message}"
                );
                assert!(
                    !message.contains(secret.trim()),
                    "{env}: the refusal never prints the secret: {message}"
                );
            }
        }
    }

    /// **Development and test are unaffected**, as they are by the
    /// placeholder rule: a short secret, `change-me` and a padded one all
    /// still load.
    #[test]
    fn a_short_signing_secret_still_loads_on_a_developer_machine() {
        let short = "s".repeat(31);
        for env in ["development", "test"] {
            for secret in [short.as_str(), "change-me", "s3cret", " change-me "] {
                AppConfig::from_source(source(&[
                    ("KELIR_JWT_SECRET", secret),
                    ("KELIR_APP_ENV", env),
                ]))
                .unwrap_or_else(|error| panic!("{env} refused '{secret}': {error}"));
            }
        }
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
    fn starts_in_multi_tenant_mode_however_the_flag_is_spelled() {
        // Every spelling `flag` accepts, not only the literal "true". This was
        // the boot guard's test, inverted by **D-18**: the guard refused all
        // five spellings because per-request resolution had no client-side
        // half, and it now has one (`GET /api/v1/deployment` plus the
        // `tenantCode` login field), so the mode is a mode rather than a trap.
        //
        // Asserting every spelling still matters for the same reason it did
        // then: an implementation that honoured only the literal "true" would
        // read `KELIR_MULTI_TENANT=1` as single-tenant and quietly land every
        // sign-in in the default tenant.
        for raw in ["true", "1", "yes", "on", "TRUE"] {
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_MULTI_TENANT", raw),
            ]))
            .unwrap_or_else(|error| panic!("{raw} must start the deployment, got {error}"));

            assert!(config.multi_tenant, "for {raw}");
        }
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
            // **Staging and production need credentials and the owner's URL
            // to load at all** (#317, ADR-0051), so this loop supplies them
            // rather than testing four environments and asserting about two.
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
                ("KELIR_APP_ENV", raw),
                ("KELIR_STORAGE_ACCESS_KEY", "a-deployment-key"),
                ("KELIR_STORAGE_SECRET_KEY", "a-deployment-secret"),
                ("KELIR_DATABASE_MIGRATION_URL", OWNER_URL),
            ]))
            .expect("documented environment parses");

            assert_eq!(config.app_env, expected, "for {raw}");
        }
    }

    /// The schema owner's URL, for the tests that load `staging` or
    /// `production` and are about something else.
    const OWNER_URL: &str = "postgres://kelir:owner-pw@db.internal:5432/kelir";

    /// **One URL is the development shape** (ADR-0051 §2): unset, the serving
    /// URL migrates as well, on a developer's machine and in the harness.
    #[test]
    fn one_url_is_enough_on_a_developers_machine() {
        for env in ["development", "test"] {
            let config = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", "s3cret"),
                ("KELIR_APP_ENV", env),
            ]))
            .unwrap_or_else(|error| panic!("{env} refused one URL: {error}"));

            assert_eq!(config.database_migration_url, None, "for {env}");
        }
    }

    /// **Staging and production refuse to start with one URL**, naming the
    /// variable (ADR-0051 §2 and §5, the fourth bullet), as a missing
    /// `KELIR_JWT_SECRET` is refused.
    #[test]
    fn refuses_to_start_without_the_owners_url_where_it_is_deployed() {
        for env in ["staging", "production"] {
            let error = AppConfig::from_source(source(&[
                ("KELIR_JWT_SECRET", DEPLOYMENT_SECRET),
                ("KELIR_APP_ENV", env),
                ("KELIR_STORAGE_ACCESS_KEY", "a-deployment-key"),
                ("KELIR_STORAGE_SECRET_KEY", "a-deployment-secret"),
                ("KELIR_DATABASE_MIGRATION_URL", "  "),
            ]))
            .expect_err("one URL is not a deployment's shape");

            assert!(
                matches!(
                    error,
                    ConfigError::Missing {
                        key: "KELIR_DATABASE_MIGRATION_URL"
                    }
                ),
                "{env}: {error}"
            );
            assert!(
                error.to_string().contains("KELIR_DATABASE_MIGRATION_URL"),
                "the refusal names the variable: {error}"
            );
        }
    }

    /// **`{:?}` of the configuration prints no secret** (ADR-0051 §4): the
    /// owner's URL stays in the struct for the process's life.
    #[test]
    fn debug_prints_no_secret() {
        let mut config = AppConfig::test_default();
        config.database_url = "postgres://kelir_app:serving-pw@db:5432/kelir".to_owned();
        config.database_migration_url = Some("postgres://kelir:owner-pw@db:5432/kelir".to_owned());
        config.jwt_secret = "signing-secret".to_owned();
        config.storage_access_key = "store-access".to_owned();
        config.storage_secret_key = "store-secret".to_owned();
        config.bootstrap_admin = Some(BootstrapAdmin {
            username: "admin".to_owned(),
            email: "admin@localhost".to_owned(),
            password: "bootstrap-pw".to_owned(),
        });

        let printed = format!("{config:?}");

        for secret in [
            "serving-pw",
            "owner-pw",
            "signing-secret",
            "store-access",
            "store-secret",
            "bootstrap-pw",
        ] {
            assert!(!printed.contains(secret), "{secret} is printed: {printed}");
        }
        assert!(printed.contains("database_migration_url: Some(\"<redacted>\")"));
    }

    #[test]
    fn reads_the_owners_url_where_it_is_set() {
        let config = AppConfig::from_source(source(&[
            ("KELIR_JWT_SECRET", "s3cret"),
            ("KELIR_DATABASE_MIGRATION_URL", OWNER_URL),
        ]))
        .expect("loads");

        assert_eq!(config.database_migration_url.as_deref(), Some(OWNER_URL));
    }
}
