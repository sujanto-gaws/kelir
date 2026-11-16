//! `test-engineer`'s campaign against the signing secret's floor ([#837],
//! NFR-SEC-007).
//!
//! `config.rs`'s unit tests are the builder's: `from_source` at 31 and 32
//! bytes, in bytes of UTF-8, and development and test exempt. **This file is
//! everything around them**, and each test names what it pins:
//!
//! | Claim | Test |
//! |---|---|
//! | The real binary refuses at the floor, names the key and the minimum, never the value; a secret that begins or ends with whitespace is refused as such; the placeholder and the empty value keep their own messages | *the binary refuses a short signing secret and nothing else* |
//! | `deploy.sh` counts bytes, as the backend does, at the same boundary, and refuses surrounding whitespace | *deploy sh counts bytes at the same floor as the backend* |
//! | Every literal secret shipped for staging or production is at least 32 bytes | *every shipped deployment secret clears the floor* |
//! | A refresh token survives a rotation, and the old access token does not | *a refresh token outlives a signing secret rotation across a restart* |
//!
//! **The binary is started with a cleared environment.** CI's test job sets
//! `KELIR_STORAGE_ACCESS_KEY` and `KELIR_STORAGE_SECRET_KEY`, and a secret
//! that clears the floor is told apart from one that does not by the next
//! refusal, a storage key left at its default: inherited, those would let it
//! through to the database.
//!
//! [#837]: https://github.com/sujanto-gaws/kelir/issues/837

mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::{TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use kelir_backend::modules::auth::token::verify_access_token;
use serde_json::{json, Value};
use tokio::time::Instant;

/// How long the binary may take to refuse, or to listen.
const PATIENCE: Duration = Duration::from_secs(20);

/// The floor, as the backend and `deploy.sh` both state it.
const FLOOR: usize = 32;

// ---------------------------------------------------------------------------
// The binary's refusals
// ---------------------------------------------------------------------------

/// What startup is expected to say about a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    /// Refused as shorter than the floor.
    Short,
    /// Refused for beginning or ending with whitespace.
    Padded,
    /// Refused as the development placeholder.
    Placeholder,
    /// Refused as unset.
    Missing,
    /// Past the secret: refused next for the storage key nobody set.
    PassedOn,
}

/// The binary, in `app_env`, with `secret` (or none) and nothing else the
/// environment might carry. Returns its exit code and everything it printed.
async fn start_refused(app_env: &str, secret: Option<&str>) -> (Option<i32>, String) {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_kelir-backend"));
    command
        .env_clear()
        .env("KELIR_APP_ENV", app_env)
        .env("KELIR_BIND_ADDRESS", "127.0.0.1:0")
        .env("KELIR_LOG", "info")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(secret) = secret {
        command.env("KELIR_JWT_SECRET", secret);
    }

    let child = command.spawn().expect("the kelir-backend binary starts");
    let output = tokio::time::timeout(PATIENCE, child.wait_with_output())
        .await
        .unwrap_or_else(|_| panic!("{app_env}: the binary was still starting after {PATIENCE:?}"))
        .expect("the binary's output");

    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), log)
}

/// **The binary refuses a short signing secret, and nothing else** (#837):
/// the built `kelir-backend`, in staging and in production, exits 1 at the
/// floor and passes a secret at it on to the next check.
///
/// - **At the boundary**: 31 bytes refused, 32 passed on, and the same in
///   multibyte, where 31 characters can be 32 bytes.
/// - **Surrounding whitespace is refused**, whatever the length (the product
///   owner, 2026-11-16). `deploy.sh`'s sourcing, compose's `.env` parser and
///   the backend can each see different bytes there, and padding would carry
///   a placeholder past the floor: `change-me` and 23 spaces is 32 bytes.
///   Whitespace alone is not a secret at any length: it is refused as unset.
/// - **Ordering**: a placeholder, which is shorter than the floor, is refused
///   as the placeholder, and an empty or absent value as required.
///
/// The refusal names `KELIR_JWT_SECRET` and the minimum, and no refusal prints
/// the secret.
#[tokio::test]
async fn the_binary_refuses_a_short_signing_secret_and_nothing_else() {
    let short = format!("{:-<31}", "probe-short-secret");
    let exact = format!("{:-<32}", "probe-exact-secret");
    let multibyte_short = format!("{}a", "é".repeat(15));
    let multibyte_exact = format!("{}é", "a".repeat(30));
    let leading_space = format!(" {short}");
    let trailing_space = format!("{short} ");
    let trailing_newline = format!("{short}\n");
    let padded_to_32 = format!(" {:-<30} ", "probe-padded-secret");
    let padded_to_31 = format!(" {:-<29} ", "probe-padded-secret");
    let only_spaces = " ".repeat(FLOOR);
    let padded_placeholder = format!("change-me{}", " ".repeat(23));

    let cases: Vec<(&str, Option<&str>, Expected)> = vec![
        ("31 bytes", Some(short.as_str()), Expected::Short),
        ("32 bytes", Some(exact.as_str()), Expected::PassedOn),
        (
            "31 bytes in 16 characters",
            Some(multibyte_short.as_str()),
            Expected::Short,
        ),
        (
            "32 bytes in 31 characters",
            Some(multibyte_exact.as_str()),
            Expected::PassedOn,
        ),
        (
            "a space, then 31 bytes",
            Some(leading_space.as_str()),
            Expected::Padded,
        ),
        (
            "31 bytes, then a space",
            Some(trailing_space.as_str()),
            Expected::Padded,
        ),
        (
            "31 bytes, then a newline",
            Some(trailing_newline.as_str()),
            Expected::Padded,
        ),
        (
            "30 bytes inside two spaces",
            Some(padded_to_32.as_str()),
            Expected::Padded,
        ),
        (
            "29 bytes inside two spaces",
            Some(padded_to_31.as_str()),
            Expected::Padded,
        ),
        (
            "change-me and 23 spaces",
            Some(padded_placeholder.as_str()),
            Expected::Padded,
        ),
        (
            "a space, then change-me",
            Some(" change-me"),
            Expected::Padded,
        ),
        ("change-me", Some("change-me"), Expected::Placeholder),
        ("test-secret", Some("test-secret"), Expected::Placeholder),
        ("empty", Some(""), Expected::Missing),
        ("32 spaces", Some(only_spaces.as_str()), Expected::Missing),
        ("unset", None, Expected::Missing),
    ];

    // The fixtures are the lengths their names say.
    assert_eq!(short.len(), 31);
    assert_eq!(exact.len(), 32);
    assert_eq!(multibyte_short.len(), 31);
    assert_eq!(
        (multibyte_exact.len(), multibyte_exact.chars().count()),
        (32, 31)
    );
    assert_eq!((padded_to_32.len(), padded_to_32.trim().len()), (32, 30));
    assert_eq!((padded_to_31.len(), padded_to_31.trim().len()), (31, 29));
    assert_eq!(padded_placeholder.len(), 32);

    let mut wrong = Vec::new();
    for app_env in ["staging", "production"] {
        for &(name, secret, expected) in &cases {
            let (code, log) = start_refused(app_env, secret).await;
            let said = |text: &str| log.contains(text);

            let as_expected = code == Some(1)
                && match expected {
                    Expected::Short => {
                        said("KELIR_JWT_SECRET is invalid")
                            && said("at least 32 bytes")
                            && said("openssl rand -base64 48")
                    }
                    Expected::Padded => {
                        said("KELIR_JWT_SECRET is invalid")
                            && said("must not begin or end with whitespace")
                            && !said("32 bytes")
                    }
                    Expected::Placeholder => {
                        said("KELIR_JWT_SECRET is invalid")
                            && said("development placeholder")
                            && !said("32 bytes")
                    }
                    Expected::Missing => said("KELIR_JWT_SECRET is required"),
                    Expected::PassedOn => {
                        !said("KELIR_JWT_SECRET") && said("KELIR_STORAGE_ACCESS_KEY")
                    }
                };
            // Never the value, nor the readable part of a padded one. A
            // placeholder is a word the message may use for itself.
            let leaked = secret
                .map(str::trim)
                .filter(|value| !value.is_empty() && expected != Expected::Placeholder)
                .is_some_and(|value| log.contains(value));

            if !as_expected || leaked {
                wrong.push(format!(
                    "{app_env}, {name}: expected {expected:?}, exit {code:?}, \
                     leaked the secret: {leaked}\n{log}"
                ));
            }
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n---\n"));
}

// ---------------------------------------------------------------------------
// deploy.sh's check
// ---------------------------------------------------------------------------

/// The repository's root.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("kelir-backend sits in the repository")
        .to_path_buf()
}

/// `deploy.sh`'s signing secret checks, as the lines it runs: from the
/// assignment of `jwt_secret_bytes` to the first blank line, which must hold a
/// `die` naming `KELIR_JWT_SECRET`.
#[cfg(unix)]
fn deploy_floor_check() -> String {
    let path = repository_root().join("deploy/staging/deploy.sh");
    let script = std::fs::read_to_string(&path).expect("deploy.sh reads");
    let lines: Vec<&str> = script.lines().collect();

    let start = lines
        .iter()
        .position(|line| line.starts_with("jwt_secret_bytes="))
        .expect("deploy.sh assigns jwt_secret_bytes: the floor check moved or went");
    let end = lines[start..]
        .iter()
        .position(|line| line.trim().is_empty())
        .map_or(lines.len(), |offset| start + offset);
    let check = lines[start..end].join("\n");
    assert!(
        check.contains("die") && check.contains("KELIR_JWT_SECRET"),
        "deploy.sh's floor check ends in a die naming KELIR_JWT_SECRET:\n{check}"
    );
    check
}

/// Runs `script` in bash under a UTF-8 locale, with `KELIR_JWT_SECRET` set to
/// `secret` and nothing else. Returns its exit code and output.
#[cfg(unix)]
fn bash_with_secret(script: &str, secret: &str) -> (Option<i32>, String) {
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("KELIR_JWT_SECRET", secret)
        .stdin(Stdio::null())
        .output()
        .expect("bash runs");

    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), printed)
}

/// **`deploy.sh` counts bytes at the same floor as the backend** (#837): its
/// check, lifted out of the script and run as the script runs it, under
/// `set -euo pipefail` and a UTF-8 locale, refuses exactly what
/// `str::len() < 32` refuses. The multibyte 32-byte, 31-character secret is
/// the case a character count gets wrong. A secret that begins or ends with
/// whitespace is refused, at any length, as the backend refuses it.
///
/// **The locale is checked first**: unless bash counts that secret as 31
/// characters here, the multibyte cases could not tell the two counts apart.
#[cfg(unix)]
#[test]
fn deploy_sh_counts_bytes_at_the_same_floor_as_the_backend() {
    let multibyte_exact = format!("{}é", "a".repeat(30));
    let (_, characters) =
        bash_with_secret(r#"printf '%s' "${#KELIR_JWT_SECRET}""#, &multibyte_exact);
    assert_eq!(
        characters, "31",
        "bash does not count characters under C.UTF-8 here, so this test cannot \
         tell bytes from characters"
    );

    let check = deploy_floor_check();
    let script = format!(
        "set -euo pipefail\n\
         die() {{ printf 'error: %s\\n' \"$*\" >&2; exit 1; }}\n\
         {check}\n\
         printf 'accepted %s' \"${{jwt_secret_bytes}}\"\n"
    );

    let secrets = [
        format!("{:-<31}", "deploy-short-secret"),
        format!("{:-<32}", "deploy-exact-secret"),
        format!("{}a", "é".repeat(15)),
        multibyte_exact,
        "x".repeat(FLOOR * 2),
    ];
    let padded = [
        format!(" {:-<32}", "deploy-leading-space"),
        format!("{:-<32} ", "deploy-trailing-space"),
    ];

    let mut wrong = Vec::new();
    for secret in &secrets {
        let (code, printed) = bash_with_secret(&script, secret);
        let backend_accepts = secret.len() >= FLOOR;

        let agrees = if backend_accepts {
            code == Some(0) && printed == format!("accepted {}", secret.len())
        } else {
            code == Some(1)
                && printed.contains("KELIR_JWT_SECRET must be at least 32 bytes")
                && !printed.contains(secret.as_str())
        };
        if !agrees {
            wrong.push(format!(
                "{} bytes, {} characters: the backend accepts it: {backend_accepts}; \
                 deploy.sh exited {code:?}: {printed}",
                secret.len(),
                secret.chars().count()
            ));
        }
    }

    for secret in &padded {
        let (code, printed) = bash_with_secret(&script, secret);
        let refused = code == Some(1)
            && printed.contains("KELIR_JWT_SECRET must not begin or end with whitespace")
            && !printed.contains(secret.trim());
        if !refused {
            wrong.push(format!(
                "{:?}: deploy.sh exited {code:?}: {printed}",
                secret.chars().next()
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "{}\n\nthe check:\n{check}",
        wrong.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Every shipped secret
// ---------------------------------------------------------------------------

/// Files that ship a development placeholder, each with the line that keeps
/// it in development. Moving one to staging takes it off this list.
const DEVELOPMENT_ONLY: &[(&str, &str)] = &[
    (
        "deploy/docker/docker-compose.yml",
        "KELIR_APP_ENV=development",
    ),
    ("deploy/env/.env.example", "KELIR_APP_ENV=development"),
];

/// The text files under `dir`, recursively, skipping dependency trees.
fn text_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != "node_modules" && name != "target" {
                text_files(&path, found);
            }
        } else {
            found.push(path);
        }
    }
}

/// **Every shipped deployment secret clears the floor** (#837): a literal
/// `KELIR_JWT_SECRET=` value anywhere under `deploy/`, `load/`,
/// `.github/workflows/` and `kelir-backend/tests/`, and the harness's
/// `JWT_SECRET`, is at least 32 bytes, unless its file is one of the
/// [`DEVELOPMENT_ONLY`] ones and still says so. A value built at run time
/// (`${…}`) is not literal and is not checked; the load harness's
/// `secret(n)` is base64 of `n` random bytes, and is checked by its length.
///
/// **Not vacuous**: the scan must find the CI stack's, the encryption probe's,
/// the N-1 rehearsal's and the harness's.
#[test]
fn every_shipped_deployment_secret_clears_the_floor() {
    let root = repository_root();
    let mut files = Vec::new();
    for dir in ["deploy", "load", ".github/workflows", "kelir-backend/tests"] {
        text_files(&root.join(dir), &mut files);
    }
    files.sort();

    let assignment = regex::Regex::new(r#"KELIR_JWT_SECRET=["']?([^"'\s]*)"#).expect("regex");
    let fixture = regex::Regex::new(r#"JWT_SECRET: &str = "([^"]*)""#).expect("regex");
    let generated = regex::Regex::new(r"KELIR_JWT_SECRET: secret\((\d+)\)").expect("regex");

    let mut checked = Vec::new();
    let mut short = Vec::new();
    for path in &files {
        if path
            .file_name()
            .is_some_and(|name| name == "signing_secret_floor_campaign.rs")
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .expect("under the root")
            .to_string_lossy()
            .replace('\\', "/");
        let development = DEVELOPMENT_ONLY
            .iter()
            .find(|(file, _)| *file == relative)
            .is_some_and(|(_, marker)| text.contains(marker));

        for (number, line) in text.lines().enumerate() {
            let mut values: Vec<usize> = Vec::new();
            for captured in assignment.captures_iter(line) {
                let value = &captured[1];
                if value.is_empty() || value.contains('$') || development {
                    continue;
                }
                values.push(value.len());
            }
            for captured in fixture.captures_iter(line) {
                values.push(captured[1].len());
            }
            for captured in generated.captures_iter(line) {
                let bytes: usize = captured[1].parse().expect("a byte count");
                values.push(bytes.div_ceil(3) * 4);
            }

            for length in values {
                let at = format!("{relative}:{}", number + 1);
                if length < FLOOR {
                    short.push(format!("{at}: {length} bytes"));
                }
                checked.push(at);
            }
        }
    }

    assert!(
        short.is_empty(),
        "shipped secrets under {FLOOR} bytes: {short:#?}"
    );
    for expected in [
        ".github/workflows/ci.yml",
        "deploy/staging/encryption-at-rest.probe.sh",
        "deploy/staging/n-minus-one.rehearsal.sh",
        "kelir-backend/tests/common/mod.rs",
        "load/run.mjs",
    ] {
        assert!(
            checked.iter().any(|at| at.starts_with(expected)),
            "the scan found no secret in {expected}, so it no longer reads what it \
             claims to: {checked:#?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Rotation, through the binary
// ---------------------------------------------------------------------------

/// The built binary, in staging, over the harness's database with `secret`.
struct Staged {
    child: tokio::process::Child,
    base: String,
}

/// Starts the binary in staging over `app`'s database, already migrated,
/// narrowed and bootstrapped, and returns once `/health/live` answers.
async fn start_staging(app: &TestApp, secret: &str, client: &reqwest::Client) -> Staged {
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a loopback port")
        .local_addr()
        .expect("the bound address");
    let owner_url = common::with_database(&common::server_url(), &app.database_name);
    let serving_url = common::as_the_serving_role(&owner_url);

    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_kelir-backend"))
        .env_clear()
        .env("KELIR_APP_ENV", "staging")
        .env("KELIR_JWT_SECRET", secret)
        .env("KELIR_DATABASE_URL", &serving_url)
        .env("KELIR_DATABASE_MIGRATION_URL", &owner_url)
        .env("KELIR_BIND_ADDRESS", address.to_string())
        .env("KELIR_STORAGE_ACCESS_KEY", "rotation-probe-access-key")
        .env("KELIR_STORAGE_SECRET_KEY", "rotation-probe-secret-key")
        .env("KELIR_LOG", "warn")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("the kelir-backend binary starts");

    let mut staged = Staged {
        child,
        base: format!("http://{address}"),
    };
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = staged.child.try_wait().expect("the binary's status") {
            panic!("the binary exited in staging before it was listening: {status}");
        }
        let live = client
            .get(format!("{}/health/live", staged.base))
            .send()
            .await
            .is_ok_and(|response| response.status().as_u16() == 200);
        if live {
            return staged;
        }
        assert!(
            Instant::now() < deadline,
            "the binary was not listening in staging within {PATIENCE:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `method` `path` on `staged`, with `token` and `body`: the status and the
/// JSON body (`Null` if there is none).
async fn call(
    client: &reqwest::Client,
    staged: &Staged,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let url = format!("{}{path}", staged.base);
    let mut request = match &body {
        Some(body) => client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string()),
        None => client.get(url),
    };
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.expect("the binary answers");
    let status = response.status().as_u16();
    let text = response.text().await.expect("the body reads");
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// `data.<field>` of a response, as a string.
fn token_in(body: &Value, field: &str) -> String {
    body["data"][field]
        .as_str()
        .unwrap_or_else(|| panic!("data.{field} is a string: {body}"))
        .to_owned()
}

/// **A refresh token outlives a signing secret rotation across a restart**
/// (#837's upgrade note, Installation guide): the binary, in staging, signs
/// in with secret A; restarted with secret B over the same database, it
/// refuses the access token A signed and takes the refresh token, which is a
/// stored digest, for a new access token that B signed.
///
/// Both secrets clear the floor and carry no surrounding whitespace, which
/// staging refuses.
#[tokio::test]
async fn a_refresh_token_outlives_a_signing_secret_rotation_across_a_restart() {
    let app = TestApp::spawn().await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(PATIENCE)
        .build()
        .expect("an HTTP client");

    let secret_a = format!("{:-<32}", "rotation-secret-a");
    let secret_b = format!("{:=<48}", "rotation-secret-b");
    assert_eq!(secret_a.len(), 32);

    let mut first = start_staging(&app, &secret_a, &client).await;
    let (status, signed_in) = call(
        &client,
        &first,
        "/api/v1/auth/login",
        None,
        Some(json!({ "username": ADMIN_USERNAME, "password": ADMIN_PASSWORD })),
    )
    .await;
    assert_eq!(status, 200, "sign-in in staging: {signed_in}");
    let access_a = token_in(&signed_in, "accessToken");
    let refresh = token_in(&signed_in, "refreshToken");

    assert!(
        verify_access_token(&secret_a, &access_a).is_ok(),
        "the access token is not signed with the secret as configured"
    );
    let (status, me) = call(&client, &first, "/api/v1/auth/me", Some(&access_a), None).await;
    assert_eq!(
        status, 200,
        "the access token works before the rotation: {me}"
    );

    first.child.kill().await.expect("the first binary stops");

    let second = start_staging(&app, &secret_b, &client).await;

    let (status, refused) = call(&client, &second, "/api/v1/auth/me", Some(&access_a), None).await;
    assert_eq!(
        status, 401,
        "an access token signed with the old secret is accepted after the rotation: {refused}"
    );

    let (status, refreshed) = call(
        &client,
        &second,
        "/api/v1/auth/refresh",
        None,
        Some(json!({ "refreshToken": refresh })),
    )
    .await;
    assert_eq!(
        status, 200,
        "the refresh token did not survive the rotation: {refreshed}"
    );
    let access_b = token_in(&refreshed, "accessToken");
    assert!(
        verify_access_token(&secret_b, &access_b).is_ok(),
        "the refreshed access token is not signed with the new secret"
    );
    assert!(
        verify_access_token(&secret_a, &access_b).is_err(),
        "the refreshed access token verifies under the old secret"
    );

    let (status, me) = call(&client, &second, "/api/v1/auth/me", Some(&access_b), None).await;
    assert_eq!(status, 200, "the refreshed access token is refused: {me}");
}
