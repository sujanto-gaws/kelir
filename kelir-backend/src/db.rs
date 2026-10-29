use std::fmt;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::{ConnectOptions, PgPool};

use crate::config::AppConfig;

/// The reserved system tenant (database schema §1.5). Platform-level rows that
/// belong to no customer tenant carry this id.
///
/// Not used to scope queries. Runtime code resolves its tenant through
/// `modules::organization::service` (FR-IDM-009), so that a deployment which
/// renames or repoints its default tenant is not silently overridden by a
/// hardcoded id. What this constant is still for is pinning the UUID that
/// `0001_core.sql` seeds, so a migration that changed it would fail the test
/// below rather than orphan every platform-level row.
#[allow(
    dead_code,
    reason = "pins the seeded system-tenant UUID against 0001_core.sql; asserted by test"
)]
pub const SYSTEM_TENANT_ID: uuid::Uuid = uuid::uuid!("00000000-0000-0000-0000-000000000001");

/// Opens the connection pool.
///
/// The pool is created lazily: `connect_lazy` does not dial the database, so
/// the API can start and serve `/health/live` while PostgreSQL is still coming
/// up. Readiness is what reports the real state (`/health/ready`).
///
/// With neither TLS setting: the rule [`connect_options`] applies holds here
/// too, so a non-local URL gets `require` from this entry as well. The binary
/// opens its pool through [`create_pool_with_tls`].
pub fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    create_pool_with_max_connections(database_url, DEFAULT_MAX_CONNECTIONS)
}

/// The pool size one running instance takes.
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// [`create_pool`], with the ceiling chosen by the caller.
///
/// Exists for the integration harness, which spawns one application — and so
/// one pool — per test, all against the same PostgreSQL. At the default
/// ceiling, a runner with enough cores to run twenty tests at once asks for two
/// hundred connections from a server whose own default limit is a hundred, and
/// the failure lands as an acquire timeout in whichever test was unlucky. The
/// production path keeps the default; nothing else about the pool differs, so
/// the harness still exercises the same connection options the binary uses.
pub fn create_pool_with_max_connections(
    database_url: &str,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    let options = connect_options(database_url, DatabaseTlsSettings::default())
        .map_err(ConnectOptionsError::into_sqlx)?;

    Ok(lazy_pool(options, max_connections))
}

/// The pool the binary opens: [`create_pool`], with the operator's
/// `KELIR_DATABASE_SSL_MODE` and `KELIR_DATABASE_SSL_ROOT_CERT`.
///
/// A refusal here is known from configuration alone, so it comes before any
/// dial and before migrations run.
pub fn create_pool_with_tls(
    database_url: &str,
    tls: DatabaseTlsSettings<'_>,
) -> Result<PgPool, ConnectOptionsError> {
    Ok(lazy_pool(
        connect_options(database_url, tls)?,
        DEFAULT_MAX_CONNECTIONS,
    ))
}

fn lazy_pool(options: PgConnectOptions, max_connections: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy_with(options)
}

/// The two settings that decide how the database hop is protected (#803).
///
/// Borrowed rather than owned so a probe can pass literals, and the binary
/// passes its [`AppConfig`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DatabaseTlsSettings<'a> {
    /// `KELIR_DATABASE_SSL_MODE`: `require` or `verify-full`, or unset.
    pub ssl_mode: Option<&'a str>,
    /// `KELIR_DATABASE_SSL_ROOT_CERT`: the PEM CA file `verify-full` checks the
    /// server's certificate against.
    pub ssl_root_cert: Option<&'a str>,
}

impl<'a> From<&'a AppConfig> for DatabaseTlsSettings<'a> {
    fn from(config: &'a AppConfig) -> Self {
        Self {
            ssl_mode: config.database_ssl_mode.as_deref(),
            ssl_root_cert: config.database_ssl_root_cert.as_deref(),
        }
    }
}

/// Hosts whose database hop never leaves the machine or the compose network,
/// compared ASCII case-insensitively after trimming, and exactly.
///
/// `host.docker.internal` is the local deployment's own PostgreSQL, and
/// `postgres` is the developer compose stack's service. Not shared with the
/// SMTP rule in `mail.rs`, whose list is shorter and case-sensitive, and which
/// #803 leaves alone.
const LOCAL_DATABASE_HOSTS: [&str; 6] = [
    "localhost",
    "127.0.0.1",
    "::1",
    "[::1]",
    "host.docker.internal",
    "postgres",
];

/// Whether `host` is one of [`LOCAL_DATABASE_HOSTS`] or a Unix-socket
/// directory. Anything else is not local, a Docker bridge IP included: the
/// rule fails closed.
fn is_local_database_host(host: &str) -> bool {
    let host = host.trim();

    host.starts_with('/')
        || LOCAL_DATABASE_HOSTS
            .iter()
            .any(|local| host.eq_ignore_ascii_case(local))
}

/// Builds the connection options from the URL and the two TLS settings
/// (NFR-SEC-010, **D-119** C, #803).
///
/// **The database hop leaves the host encrypted or not at all.** Until #803
/// every connection was forced to `prefer`, which falls back to plaintext
/// without a word and overrode any `sslmode` the operator put in the URL. Now:
///
/// - **A local host** (see [`LOCAL_DATABASE_HOSTS`]) dials with the URL's
///   `sslmode` as given, `prefer` when it has none.
/// - **Any other host** dials with the URL's `sslmode`, `require` when it has
///   none, and a URL that asks for less than `require` is refused here,
///   before anything is dialled.
/// - **`KELIR_DATABASE_SSL_MODE`**, when set, raises the mode to itself on any
///   host. It never lowers one: a URL's `verify-full` stays `verify-full`.
///
/// A server that offers no TLS is known only when it is dialled, so that half
/// of the rule is sqlx's: `require` and stronger refuse to continue in
/// plaintext. [`ping`] and [`run_migrations`] name the setting in that error.
///
/// **`PGSSLMODE` is not read.** sqlx seeds an absent `sslmode` from it, which
/// would make the default depend on a variable no Kelir text documents; the
/// mode is always set here from the URL and the setting alone, as the forced
/// `prefer` before it was. The host is the one the connection will actually
/// dial, `hostaddr` and `host` query parameters included.
pub fn connect_options(
    database_url: &str,
    tls: DatabaseTlsSettings<'_>,
) -> Result<PgConnectOptions, ConnectOptionsError> {
    let options: PgConnectOptions = database_url.parse().map_err(ConnectOptionsError::Url)?;

    let local = options.get_socket().is_some() || is_local_database_host(options.get_host());
    let requested = url_ssl_mode(database_url);
    let setting = tls.ssl_mode.map(parse_ssl_mode_setting).transpose()?;

    if let Some(weak) =
        requested.filter(|mode| !local && strength(*mode) < strength(PgSslMode::Require))
    {
        return Err(ConnectOptionsError::WeakSslMode {
            mode: mode_name(weak),
        });
    }

    let floor = if local {
        PgSslMode::Prefer
    } else {
        PgSslMode::Require
    };
    let base = requested.unwrap_or(floor);
    let mode = match setting {
        Some(setting) if strength(setting) > strength(base) => setting,
        _ => base,
    };

    let mut options = options
        // Statement logging at debug would echo parameter values, which can
        // include personal data (coding standard §2.7).
        .log_statements(tracing::log::LevelFilter::Trace)
        .ssl_mode(mode);

    if matches!(setting, Some(PgSslMode::VerifyFull)) && tls.ssl_root_cert.is_none() {
        return Err(ConnectOptionsError::RootCertificateMissing);
    }

    if let Some(path) = tls.ssl_root_cert {
        // A CA that nothing checks against is a setting the operator believes
        // is protecting them; say so rather than carry it silently.
        if strength(mode) < strength(PgSslMode::VerifyCa) {
            return Err(ConnectOptionsError::RootCertificateUnchecked {
                mode: mode_name(mode),
            });
        }

        check_root_certificate(path)?;
        // By path, not inlined: sqlx reads it at each connect, so a rotated
        // CA is picked up by the next connection.
        options = options.ssl_root_cert(path);
    }

    Ok(options)
}

/// The `sslmode` the URL itself gives, the last one winning as in sqlx.
///
/// Read from the URL rather than from the parsed options, because those
/// cannot tell an `sslmode` the URL gave from sqlx's `PGSSLMODE` default.
fn url_ssl_mode(database_url: &str) -> Option<PgSslMode> {
    let url = url::Url::parse(database_url).ok()?;

    url.query_pairs()
        .filter(|(key, _)| key == "sslmode" || key == "ssl-mode")
        .last()
        .and_then(|(_, value)| value.parse().ok())
}

fn parse_ssl_mode_setting(value: &str) -> Result<PgSslMode, ConnectOptionsError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "require" => Ok(PgSslMode::Require),
        "verify-full" => Ok(PgSslMode::VerifyFull),
        _ => Err(ConnectOptionsError::UnknownSslMode {
            value: value.to_owned(),
        }),
    }
}

/// libpq's order, weakest first. `prefer` and below may run in plaintext.
fn strength(mode: PgSslMode) -> u8 {
    match mode {
        PgSslMode::Disable => 0,
        PgSslMode::Allow => 1,
        PgSslMode::Prefer => 2,
        PgSslMode::Require => 3,
        PgSslMode::VerifyCa => 4,
        PgSslMode::VerifyFull => 5,
    }
}

fn mode_name(mode: PgSslMode) -> &'static str {
    match mode {
        PgSslMode::Disable => "disable",
        PgSslMode::Allow => "allow",
        PgSslMode::Prefer => "prefer",
        PgSslMode::Require => "require",
        PgSslMode::VerifyCa => "verify-ca",
        PgSslMode::VerifyFull => "verify-full",
    }
}

/// Refuses a CA file that cannot be read or holds no PEM certificate, at
/// startup rather than at the first connection.
fn check_root_certificate(path: &str) -> Result<(), ConnectOptionsError> {
    let unreadable = |reason: String| ConnectOptionsError::RootCertificateUnreadable {
        path: path.to_owned(),
        reason,
    };

    let contents = std::fs::read(path).map_err(|error| unreadable(error.to_string()))?;

    if !contents
        .windows(PEM_CERTIFICATE_MARKER.len())
        .any(|window| window == PEM_CERTIFICATE_MARKER)
    {
        return Err(unreadable("it holds no PEM certificate".to_owned()));
    }

    Ok(())
}

const PEM_CERTIFICATE_MARKER: &[u8] = b"-----BEGIN CERTIFICATE-----";

/// Why the database's connection options were refused.
///
/// **No variant carries the URL or its password** (coding standard §2.7): the
/// URL is the one place the database password lives.
#[derive(Debug)]
pub enum ConnectOptionsError {
    /// `KELIR_DATABASE_URL` does not parse. sqlx's text names the part, not
    /// the value.
    Url(sqlx::Error),
    /// `KELIR_DATABASE_SSL_MODE` is neither `require` nor `verify-full`.
    UnknownSslMode { value: String },
    /// The URL asks a non-local host for a mode that may run in plaintext.
    WeakSslMode { mode: &'static str },
    /// `verify-full` was asked for with no CA to verify against.
    RootCertificateMissing,
    /// The CA file cannot be read, or holds no certificate.
    RootCertificateUnreadable { path: String, reason: String },
    /// A CA was given, and the effective mode checks no certificate.
    RootCertificateUnchecked { mode: &'static str },
}

impl ConnectOptionsError {
    /// For the entries whose signature predates #803 and returns sqlx's error.
    fn into_sqlx(self) -> sqlx::Error {
        match self {
            Self::Url(error) => error,
            other => sqlx::Error::Configuration(Box::new(other)),
        }
    }
}

impl fmt::Display for ConnectOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "KELIR_DATABASE_URL is invalid: {error}"),
            Self::UnknownSslMode { value } => write!(
                f,
                "KELIR_DATABASE_SSL_MODE is invalid: expected `require` or `verify-full`; \
                 found '{value}'"
            ),
            Self::WeakSslMode { mode } => write!(
                f,
                "KELIR_DATABASE_URL asks for sslmode={mode}, which may run in plaintext, and \
                 its host is not local: a database that is not local is reached over TLS \
                 only (KELIR_DATABASE_SSL_MODE). Remove sslmode from the URL or raise it to \
                 `require` or `verify-full`"
            ),
            Self::RootCertificateMissing => write!(
                f,
                "KELIR_DATABASE_SSL_MODE is `verify-full`, which checks the server's \
                 certificate, but KELIR_DATABASE_SSL_ROOT_CERT names no CA file to check it \
                 against"
            ),
            Self::RootCertificateUnreadable { path, reason } => write!(
                f,
                "KELIR_DATABASE_SSL_ROOT_CERT names '{path}', which cannot be used: {reason}. \
                 The certificate check KELIR_DATABASE_SSL_MODE asks for needs a readable \
                 PEM CA file there"
            ),
            Self::RootCertificateUnchecked { mode } => write!(
                f,
                "KELIR_DATABASE_SSL_ROOT_CERT is set, but the database connection's mode is \
                 `{mode}`, which checks no certificate: set KELIR_DATABASE_SSL_MODE to \
                 `verify-full`, or unset KELIR_DATABASE_SSL_ROOT_CERT"
            ),
        }
    }
}

impl std::error::Error for ConnectOptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Url(error) => Some(error),
            _ => None,
        }
    }
}

/// Names the Kelir settings in a TLS failure.
///
/// sqlx's own text names no setting, so an operator reading the log would not
/// know which knob decided it. sqlx 0.8.6 reports TLS failures in two
/// variants, and each keeps its variant here so callers that match on it
/// still do:
///
/// - [`sqlx::Error::Tls`]: the server said it offers no TLS (*server does not
///   support TLS*), or the CA file could not be loaded.
/// - [`sqlx::Error::Io`]: rustls refused the handshake itself — a certificate
///   from an unknown issuer, one not valid for the host's name, or bytes that
///   are not TLS. sqlx returns the handshake's `io::Error` as it is, and
///   rustls builds that error as `InvalidData` around its own.
fn name_the_tls_settings(error: sqlx::Error) -> sqlx::Error {
    const SETTINGS: &str = "The database is reached over TLS as the host and \
         KELIR_DATABASE_SSL_MODE decide: turn TLS on at the server, check \
         KELIR_DATABASE_SSL_ROOT_CERT, or reach it through a local host";

    match error {
        sqlx::Error::Tls(source) => sqlx::Error::Tls(format!("{source}. {SETTINGS}").into()),
        // **Matched by shape, not by type.** Downcasting to `rustls::Error`
        // would take rustls as a direct dependency, which changes Cargo.lock.
        // The shape is narrow enough here: the socket is a TCP or Unix stream
        // whose own failures carry an OS error, not an inner one, and sqlx's
        // protocol faults are `Error::Protocol`. `InvalidData` with an error
        // inside is what rustls builds (`ConnectionCommon::complete_io`).
        sqlx::Error::Io(source)
            if source.kind() == std::io::ErrorKind::InvalidData && source.get_ref().is_some() =>
        {
            sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{source}. {SETTINGS}"),
            ))
        }
        other => other,
    }
}

/// Applies any migrations the database has not yet seen.
///
/// SQLx records applied migrations and their checksums in `_sqlx_migrations`,
/// so an edited migration fails loudly rather than silently diverging — which
/// is the mechanism behind "migrations are immutable once merged"
/// (coding standard §2.5). That check is untouched by the tolerance below:
/// every migration this binary *does* know is still verified by checksum.
///
/// # Migrations from a newer release are tolerated (#76)
///
/// By default SQLx refuses to start when the database holds an applied
/// migration the binary cannot resolve. That default makes application
/// rollback impossible: the previous image, redeployed against a database the
/// newer image has already migrated, dies with "migration N was previously
/// applied but is missing in the resolved migrations" — which is exactly what
/// the `v0.2.0` rollback rehearsal hit.
///
/// The [release process](../../docs/standards/04.%20Release%20Process.md) §6
/// requires the opposite: rollback is "always possible because images are
/// immutable", and every migration must be backward-compatible with the
/// previous release. Additive DDL satisfies that rule and the migrator still
/// refused, so the rule was being met in the columns and broken in the
/// bookkeeping.
///
/// What is given up is detection of a migration file that has gone missing
/// from the directory. That is an acceptable trade because migrations are
/// forward-only and immutable once merged, so a file disappearing is a
/// mistake caught in review, while the failure it was preventing here is a
/// deployment that cannot be rolled back.
///
/// The binary's first connection is made here, so a server that refuses TLS
/// is reported here, with the setting named (#803).
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.set_ignore_missing(true);
    migrator.run(pool).await.map_err(|error| match error {
        sqlx::migrate::MigrateError::Execute(error) => {
            sqlx::migrate::MigrateError::Execute(name_the_tls_settings(error))
        }
        other => other,
    })
}

/// Cheap liveness probe for the database, used by `/health/ready`, whose log
/// line carries the TLS settings' names when TLS is what failed.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1")
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(name_the_tls_settings)
}

/// A server that answers PostgreSQL's TLS request with *no*, as one with
/// `ssl=off` does, and a URL that reaches it by a host that is not local.
///
/// `127.0.0.2` is loopback on Linux and not in [`LOCAL_DATABASE_HOSTS`], so
/// the rule applies to it without a second machine. The real-server check is
/// the row's probe (#803 AC8); this one pins the error text.
#[cfg(test)]
pub(crate) async fn server_refusing_tls() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.2:0")
        .await
        .expect("127.0.0.2 is loopback");
    let port = listener.local_addr().expect("bound").port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            // SSLRequest is eight bytes: a length and the request code.
            let mut request = [0u8; 8];
            if socket.read_exact(&mut request).await.is_ok() {
                let _ = socket.write_all(b"N").await;
            }
        }
    });

    format!("postgres://kelir:s3cret-pw@127.0.0.2:{port}/kelir")
}

/// [`server_refusing_tls`]'s opposite: it says *yes* to TLS, then answers the
/// ClientHello with bytes that are not TLS, so rustls itself refuses the
/// handshake — the path a certificate refusal takes through sqlx, without a
/// certificate.
#[cfg(test)]
async fn server_breaking_the_handshake() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.2:0")
        .await
        .expect("127.0.0.2 is loopback");
    let port = listener.local_addr().expect("bound").port();

    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0u8; 8];
                if socket.read_exact(&mut request).await.is_err()
                    || socket.write_all(b"S").await.is_err()
                {
                    return;
                }
                let mut client_hello = [0u8; 1024];
                if socket.read(&mut client_hello).await.unwrap_or(0) > 0 {
                    let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
                }
                // Held open until the client gives up, so the refusal is
                // rustls's and not a closed socket's.
                let _ = socket.read(&mut client_hello).await;
            });
        }
    });

    format!("postgres://kelir:s3cret-pw@127.0.0.2:{port}/kelir")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A password that must never appear in a refusal.
    const PASSWORD: &str = "s3cret-pw";

    fn options(url: &str, tls: DatabaseTlsSettings<'_>) -> PgConnectOptions {
        connect_options(url, tls).unwrap_or_else(|error| panic!("{url} builds options: {error}"))
    }

    fn refusal(url: &str, tls: DatabaseTlsSettings<'_>) -> String {
        let message = connect_options(url, tls)
            .expect_err("these settings are refused")
            .to_string();

        assert!(
            !message.contains(PASSWORD) && !message.contains(url),
            "a refusal echoes the URL or its password: {message}"
        );

        message
    }

    fn mode(url: &str, tls: DatabaseTlsSettings<'_>) -> &'static str {
        mode_name(options(url, tls).get_ssl_mode())
    }

    fn url_at(host: &str) -> String {
        format!("postgres://kelir:{PASSWORD}@{host}:5432/kelir")
    }

    /// A PEM file of its own for each test, since they run in parallel.
    fn pem_file(contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("kelir-db-ca-{}.pem", uuid::Uuid::now_v7()));
        std::fs::write(&path, contents).expect("the temp dir is writable");
        path
    }

    const PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";

    const fn tls<'a>(
        ssl_mode: Option<&'a str>,
        ssl_root_cert: Option<&'a str>,
    ) -> DatabaseTlsSettings<'a> {
        DatabaseTlsSettings {
            ssl_mode,
            ssl_root_cert,
        }
    }

    const NO_SETTINGS: DatabaseTlsSettings<'static> = tls(None, None);

    #[test]
    fn local_hosts_keep_prefer_without_an_sslmode() {
        for url in [
            url_at("localhost"),
            url_at("127.0.0.1"),
            url_at("[::1]"),
            url_at("LOCALHOST"),
            url_at("host.docker.internal"),
            url_at("postgres"),
            // `::1` cannot stand bare in a URL's authority, so it arrives as
            // the `host` parameter, which wins over the authority.
            format!("{}?host=::1", url_at("db.example.com")),
            // And `hostaddr` at a loopback address is local, whatever the name.
            format!("{}?hostaddr=127.0.0.1", url_at("db.example.com")),
            // Both spellings of a socket directory sqlx accepts.
            format!("postgres://kelir:{PASSWORD}@%2Fvar%2Frun%2Fpostgresql/kelir"),
            format!("{}?host=/var/run/postgresql", url_at("db.example.com")),
        ] {
            assert_eq!(mode(&url, NO_SETTINGS), "prefer", "{url}");
        }
    }

    #[test]
    fn a_name_that_only_contains_a_local_one_is_not_local() {
        for host in [
            "postgres.example.com",
            "localhost.evil.example",
            "host.docker.internal.example",
            "db.example.com",
            "10.0.0.5",
            // The Docker bridge gateway: the same machine, but reached across
            // a network the rule cannot see the edges of.
            "172.17.0.1",
            "127.0.0.2",
        ] {
            assert_eq!(mode(&url_at(host), NO_SETTINGS), "require", "{host}");
        }

        // Hostaddr names the address dialled, so it is what is judged.
        assert_eq!(
            mode(
                &format!("{}?hostaddr=10.0.0.5", url_at("localhost")),
                NO_SETTINGS
            ),
            "require"
        );
    }

    #[test]
    fn the_local_list_is_exact_case_insensitive_and_trimmed() {
        assert!(is_local_database_host(" Host.Docker.Internal "));
        assert!(is_local_database_host("/tmp"));
        assert!(!is_local_database_host("localhost."));
        assert!(!is_local_database_host("postgres-replica"));
        assert!(!is_local_database_host(""));
    }

    #[test]
    fn a_weak_url_sslmode_at_a_non_local_host_is_refused_before_any_dial() {
        for weak in ["disable", "allow", "prefer"] {
            let url = format!("{}?sslmode={weak}", url_at("db.example.com"));

            // `connect_options` is not async and opens no socket, so a
            // refusal from it is a refusal without a dial.
            let message = refusal(&url, NO_SETTINGS);

            assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
            assert!(message.contains(&format!("sslmode={weak}")), "{message}");

            // A stricter setting does not excuse it: the URL says something
            // the operator should correct, not something to overrule quietly.
            let message = refusal(&url, tls(Some("require"), None));
            assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
        }
    }

    #[tokio::test]
    async fn the_entries_refuse_what_connect_options_refuses() {
        let url = format!("{}?sslmode=disable", url_at("db.example.com"));

        let error = create_pool_with_tls(&url, NO_SETTINGS).expect_err("refused");
        assert!(error.to_string().contains("KELIR_DATABASE_SSL_MODE"));

        let error = create_pool(&url).expect_err("refused");
        assert!(matches!(error, sqlx::Error::Configuration(_)));
        assert!(error.to_string().contains("KELIR_DATABASE_SSL_MODE"));
    }

    #[test]
    fn verify_full_with_a_readable_ca_carries_that_ca() {
        let ca = pem_file(PEM);
        let ca_path = ca.to_str().expect("utf-8 temp path");

        let options = options(
            &url_at("db.example.com"),
            tls(Some("verify-full"), Some(ca_path)),
        );

        assert!(matches!(options.get_ssl_mode(), PgSslMode::VerifyFull));
        // sqlx has no getter for the root certificate; its Debug names it.
        assert!(
            format!("{options:?}").contains(&format!("ssl_root_cert: Some(File({ca:?}))")),
            "the CA is not the one given"
        );

        let _ = std::fs::remove_file(ca);
    }

    #[test]
    fn verify_full_without_a_usable_ca_is_refused_naming_both_settings() {
        let verify_full = |ssl_root_cert| tls(Some("verify-full"), ssl_root_cert);
        let url = url_at("db.example.com");

        let message = refusal(&url, verify_full(None));
        assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
        assert!(
            message.contains("KELIR_DATABASE_SSL_ROOT_CERT"),
            "{message}"
        );

        let missing = std::env::temp_dir().join("kelir-db-ca-that-does-not-exist.pem");
        let not_pem = pem_file("not a certificate");
        // A directory exists and cannot be read as a file.
        let directory = std::env::temp_dir();

        for path in [&missing, &not_pem, &directory] {
            let path = path.to_str().expect("utf-8 temp path");
            let message = refusal(&url, verify_full(Some(path)));

            assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
            assert!(
                message.contains("KELIR_DATABASE_SSL_ROOT_CERT"),
                "{message}"
            );
            assert!(message.contains(path), "{message}");
        }

        let _ = std::fs::remove_file(not_pem);
    }

    #[test]
    fn an_unknown_mode_setting_is_refused_listing_the_two_it_takes() {
        for value in ["verify-ca", "disable", "prefer", "on", "required"] {
            let message = refusal(&url_at("localhost"), tls(Some(value), None));

            assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
            assert!(message.contains("`require`"), "{message}");
            assert!(message.contains("`verify-full`"), "{message}");
        }

        // Case and surrounding space are an operator's typing, not a value.
        assert_eq!(
            mode(&url_at("db.example.com"), tls(Some(" Require "), None),),
            "require"
        );
    }

    #[test]
    fn a_setting_given_with_a_local_host_is_applied() {
        let require = tls(Some("require"), None);

        assert_eq!(mode(&url_at("localhost"), require), "require");
        assert_eq!(mode(&url_at("host.docker.internal"), require), "require");
        assert_eq!(
            mode(&format!("{}?sslmode=disable", url_at("postgres")), require),
            "require"
        );

        let ca = pem_file(PEM);
        assert_eq!(
            mode(&url_at("localhost"), tls(Some("verify-full"), ca.to_str()),),
            "verify-full"
        );
        let _ = std::fs::remove_file(ca);
    }

    #[test]
    fn the_setting_raises_the_url_mode_and_never_lowers_it() {
        let require = tls(Some("require"), None);

        for (host, url_mode) in [
            ("db.example.com", "verify-full"),
            ("db.example.com", "verify-ca"),
            ("localhost", "verify-full"),
        ] {
            let url = format!("{}?sslmode={url_mode}", url_at(host));
            assert_eq!(mode(&url, require), url_mode, "{url}");
        }
    }

    #[test]
    fn the_last_sslmode_wins_in_either_spelling_as_in_sqlx() {
        let at = |query: &str| format!("{}?{query}", url_at("db.example.com"));

        // What sqlx would dial is what the rule judges: a weak mode after a
        // strong one is the one that counts, under either key sqlx reads.
        for query in [
            "sslmode=require&sslmode=disable",
            "ssl-mode=prefer",
            "sslmode=verify-full&ssl-mode=allow",
            "sslmode=DISABLE",
        ] {
            let message = refusal(&at(query), NO_SETTINGS);
            assert!(
                message.contains("KELIR_DATABASE_SSL_MODE"),
                "{query}: {message}"
            );
        }

        for (query, expected) in [
            ("sslmode=disable&sslmode=require", "require"),
            ("ssl-mode=verify-ca", "verify-ca"),
            ("ssl-mode=prefer&sslmode=verify-full", "verify-full"),
        ] {
            let url = at(query);
            let sqlx_reads: PgConnectOptions = url.parse().expect("sqlx parses it");

            assert_eq!(mode(&url, NO_SETTINGS), expected, "{query}");
            assert_eq!(mode_name(sqlx_reads.get_ssl_mode()), expected, "{query}");
        }
    }

    #[test]
    fn the_binary_passes_both_settings_from_its_configuration() {
        let config = AppConfig {
            database_ssl_mode: Some("verify-full".to_owned()),
            database_ssl_root_cert: Some("/etc/kelir/db-tls/ca.pem".to_owned()),
            ..AppConfig::test_default()
        };

        let settings = DatabaseTlsSettings::from(&config);

        assert_eq!(settings.ssl_mode, Some("verify-full"));
        assert_eq!(settings.ssl_root_cert, Some("/etc/kelir/db-tls/ca.pem"));
    }

    #[test]
    fn a_ca_the_url_asks_for_is_checked_at_startup_too() {
        let missing = std::env::temp_dir().join("kelir-db-ca-that-does-not-exist.pem");
        let missing = missing.to_str().expect("utf-8 temp path");

        // No setting: the URL's own certificate check is what reads the file.
        for url_mode in ["verify-ca", "verify-full"] {
            let url = format!("{}?sslmode={url_mode}", url_at("db.example.com"));
            let message = refusal(&url, tls(None, Some(missing)));

            assert!(
                message.contains("KELIR_DATABASE_SSL_ROOT_CERT"),
                "{message}"
            );
            assert!(message.contains(missing), "{message}");
        }
    }

    #[test]
    fn a_ca_that_nothing_would_check_is_refused() {
        let ca = pem_file(PEM);

        for (url, ssl_mode) in [
            (url_at("db.example.com"), Some("require")),
            (url_at("db.example.com"), None),
            (url_at("localhost"), None),
        ] {
            let message = refusal(
                &url,
                DatabaseTlsSettings {
                    ssl_mode,
                    ssl_root_cert: ca.to_str(),
                },
            );

            assert!(
                message.contains("KELIR_DATABASE_SSL_ROOT_CERT"),
                "{message}"
            );
            assert!(message.contains("KELIR_DATABASE_SSL_MODE"), "{message}");
        }

        // The URL's own certificate check uses it.
        assert_eq!(
            mode(
                &format!("{}?sslmode=verify-ca", url_at("db.example.com")),
                tls(None, ca.to_str()),
            ),
            "verify-ca"
        );

        let _ = std::fs::remove_file(ca);
    }

    #[test]
    fn a_tls_failure_names_the_settings_and_other_failures_are_untouched() {
        let wrapped = name_the_tls_settings(sqlx::Error::Tls("server does not support TLS".into()));

        assert!(matches!(wrapped, sqlx::Error::Tls(_)));
        // Display for `main`'s exit line, Debug for readiness's log line.
        for text in [wrapped.to_string(), format!("{wrapped:?}")] {
            assert!(text.contains("KELIR_DATABASE_SSL_MODE"), "{text}");
            assert!(text.contains("server does not support TLS"), "{text}");
        }

        let untouched = name_the_tls_settings(sqlx::Error::PoolTimedOut);
        assert!(matches!(untouched, sqlx::Error::PoolTimedOut));
    }

    #[tokio::test]
    async fn a_handshake_rustls_refuses_names_both_settings() {
        // A certificate refusal travels this way too: sqlx 0.8.6 returns the
        // handshake's io::Error as `Error::Io`, not `Error::Tls`.
        let url = server_breaking_the_handshake().await;
        let pool = create_pool(&url).expect("the configuration alone is acceptable");

        let error = ping(&pool).await.expect_err("no handshake, no connection");
        assert!(matches!(error, sqlx::Error::Io(_)), "{error:?}");
        for text in [error.to_string(), format!("{error:?}")] {
            assert!(text.contains("KELIR_DATABASE_SSL_MODE"), "{text}");
            assert!(text.contains("KELIR_DATABASE_SSL_ROOT_CERT"), "{text}");
        }

        let error = run_migrations(&pool)
            .await
            .expect_err("migrations need a connection");
        assert!(
            error.to_string().contains("KELIR_DATABASE_SSL_ROOT_CERT"),
            "{error}"
        );
    }

    #[test]
    fn an_io_error_carrying_a_tls_refusal_is_named_and_a_plain_one_is_not() {
        // The shape rustls gives a refused certificate: InvalidData, with
        // rustls's error inside.
        let refused = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid peer certificate: UnknownIssuer",
        );
        let wrapped = name_the_tls_settings(sqlx::Error::Io(refused));

        let sqlx::Error::Io(inner) = &wrapped else {
            panic!("Io stays Io: {wrapped:?}");
        };
        assert_eq!(inner.kind(), std::io::ErrorKind::InvalidData);
        for text in [wrapped.to_string(), format!("{wrapped:?}")] {
            assert!(text.contains("UnknownIssuer"), "{text}");
            assert!(text.contains("KELIR_DATABASE_SSL_MODE"), "{text}");
            assert!(text.contains("KELIR_DATABASE_SSL_ROOT_CERT"), "{text}");
        }

        for plain in [
            std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"),
            // InvalidData with nothing inside is not rustls's shape.
            std::io::Error::from(std::io::ErrorKind::InvalidData),
        ] {
            let before = plain.to_string();
            let untouched = name_the_tls_settings(sqlx::Error::Io(plain));

            assert!(
                !untouched.to_string().contains("KELIR_DATABASE_SSL_MODE"),
                "{untouched}"
            );
            assert!(untouched.to_string().contains(&before), "{untouched}");
        }
    }

    #[tokio::test]
    async fn a_server_without_tls_is_refused_at_the_first_connection_naming_the_setting() {
        let url = server_refusing_tls().await;
        let pool = create_pool(&url).expect("the configuration alone is acceptable");

        let error = ping(&pool).await.expect_err("no TLS, no connection");
        assert!(matches!(error, sqlx::Error::Tls(_)), "{error:?}");
        assert!(
            format!("{error:?}").contains("KELIR_DATABASE_SSL_MODE"),
            "{error:?}"
        );

        let error = run_migrations(&pool)
            .await
            .expect_err("migrations need a connection");
        assert!(
            error.to_string().contains("KELIR_DATABASE_SSL_MODE"),
            "{error}"
        );
    }

    // Pool construction registers with the Tokio reactor even when lazy, so
    // these need a runtime despite never reaching the network.
    #[tokio::test]
    async fn builds_a_pool_without_contacting_the_database() {
        // connect_lazy must not dial; if it did, this test would need a server.
        let pool = create_pool("postgres://postgres:postgres@localhost:5432/kelir")
            .expect("a valid url builds a pool");

        assert!(pool.options().get_max_connections() >= 1);
    }

    #[tokio::test]
    async fn rejects_a_malformed_url() {
        for url in ["", "not-a-url", "://x", "postgres://u:p@h:notaport/d"] {
            assert!(
                create_pool(url).is_err(),
                "{url:?} should not produce a pool"
            );
        }
    }

    #[tokio::test]
    async fn does_not_validate_the_url_scheme() {
        // Documenting a sharp edge rather than asserting what we wish were true:
        // PgConnectOptions ignores the scheme, so a mistyped KELIR_DATABASE_URL
        // is not caught at startup. It surfaces as a readiness failure instead.
        assert!(create_pool("mysql://root@localhost/kelir").is_ok());
    }

    /// The mode a pool built by [`create_pool`] would dial with.
    fn pool_ssl_mode(url: &str) -> &'static str {
        mode_name(
            create_pool(url)
                .expect("the url builds a pool")
                .connect_options()
                .get_ssl_mode(),
        )
    }

    #[tokio::test]
    async fn a_non_local_host_gets_at_least_require() {
        for url in [
            "postgres://u:p@db.example.com/kelir",
            "postgres://u:p@10.0.0.5/kelir",
        ] {
            let mode = pool_ssl_mode(url);

            assert!(
                ["require", "verify-ca", "verify-full"].contains(&mode),
                "{url} dials with {mode}, which can fall back to plaintext"
            );
        }
    }

    #[tokio::test]
    async fn a_url_sslmode_is_honoured_on_a_local_host() {
        let base = "postgres://u:p@localhost/kelir?sslmode=";

        assert_eq!(pool_ssl_mode(&format!("{base}require")), "require");
        assert_eq!(pool_ssl_mode(&format!("{base}disable")), "disable");
        assert_eq!(pool_ssl_mode(&format!("{base}verify-full")), "verify-full");
    }

    #[tokio::test]
    async fn a_url_sslmode_is_honoured_on_a_non_local_host() {
        let base = "postgres://u:p@db.example.com/kelir?sslmode=";

        assert_eq!(pool_ssl_mode(&format!("{base}require")), "require");
        assert_eq!(pool_ssl_mode(&format!("{base}verify-ca")), "verify-ca");
        assert_eq!(pool_ssl_mode(&format!("{base}verify-full")), "verify-full");
    }

    #[test]
    fn system_tenant_id_matches_the_migration() {
        // The constant and 0001_core.sql must agree; drift here would silently
        // orphan every platform-level row.
        assert_eq!(
            SYSTEM_TENANT_ID.to_string(),
            "00000000-0000-0000-0000-000000000001"
        );
    }
}
