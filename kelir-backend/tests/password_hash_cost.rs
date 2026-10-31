//! **The Argon2id cost is written out, and every path that stores a password
//! pays it** (NFR-SEC-001, [#779](https://github.com/sujanto-gaws/kelir/issues/779)
//! AC3).
//!
//! `hash_password` takes `Argon2::default()`, so the cost is whatever the
//! crate's default is on the day. `Cargo.toml` asks for `argon2 = "0.5"`: a
//! release of that line, or a move to the next, could lower it in a lock-file
//! change that no test reads. `stores_an_argon2id_phc_string` checks the
//! algorithm's name and nothing after it.
//!
//! **The numbers are written out rather than read from the crate**, as
//! `auth_lockout.rs` writes out five and fifteen: a test that reads
//! `Params::DEFAULT_M_COST` passes whatever it is changed to. Raising the cost
//! is a decision, so it reddens this file on purpose, and the change that makes
//! it updates the numbers, `DUMMY_HASH` and this sentence together.
//!
//! **Each writing path is read back from the column**, and each must have
//! written a new hash, so a path that stopped writing cannot pass on the hash
//! before it. Six sites call `hash_password`: the bootstrap, a user created, a
//! password set by an administrator, a password changed, a password reset, and
//! a tenant's first administrator. A site that took another hasher would pass
//! every test that only reads `hash_password`.
//!
//! # Seen to fail under mutation
//!
//! Each mutation was made alone, this file run, and the mutation reverted.
//! **Seen red, 2026-10-31**, in `kelir-rust-tools:1.89`.
//!
//! | Mutation | Reddened, of 3 tests |
//! |---|---|
//! | `hash_password` at `Params::new(4096, 1, 1, None)`: the cost a crate default could fall to | 2: `a_fresh_hash_carries_the_cost_written_here`, `every_path_that_stores_a_password_stores_that_cost` |
//! | `hash_password` asks for `Algorithm::Argon2i` at the same cost | 2: the same two |
//! | Only `m` lowered, to 9728 | 2: the same two |
//! | Only `t` lowered, to 1 | 2: the same two |
//! | Only `p` moved, to 2 | 2: the same two |
//! | The administrator's set-password path alone hashes at `m=4096,t=1,p=1` (`identity/service.rs`) | 1: `every_path_that_stores_a_password_stores_that_cost`. `a_fresh_hash_…` stays green, which is why the walk is here |
//! | `DUMMY_HASH` written at `t=1` | 1: `the_unknown_user_hash_costs_what_a_real_one_does` |

mod common;

use argon2::password_hash::PasswordHash;
use argon2::Params;
use axum::http::{Method, StatusCode};
use serde_json::json;
use uuid::Uuid;

use common::{TestApp, ADMIN_PASSWORD, ADMIN_USERNAME};
use kelir_backend::modules::auth::password::hash_password;

/// 19 MiB, 2 passes, 1 lane: `argon2` 0.5.3's default, and the first of OWASP's
/// listed Argon2id configurations.
const MEMORY_KIB: u32 = 19_456;
const PASSES: u32 = 2;
const LANES: u32 = 1;
const OUTPUT_BYTES: usize = 32;
/// Argon2 version 1.3, as a PHC `v=`.
const VERSION: u32 = 19;

/// Whether a stored PHC string is Argon2id at exactly the written-out cost.
fn assert_the_cost(path: &str, stored: &str) {
    let parsed =
        PasswordHash::new(stored).unwrap_or_else(|error| panic!("{path}: not PHC: {error}"));
    let params = Params::try_from(&parsed)
        .unwrap_or_else(|error| panic!("{path}: no Argon2 parameters: {error}"));

    assert_eq!(parsed.algorithm.as_str(), "argon2id", "{path}: {stored}");
    assert_eq!(parsed.version, Some(VERSION), "{path}: {stored}");
    assert_eq!(
        (params.m_cost(), params.t_cost(), params.p_cost()),
        (MEMORY_KIB, PASSES, LANES),
        "{path}: m, t, p in {stored}"
    );
    assert_eq!(
        parsed.hash.map(|output| output.len()),
        Some(OUTPUT_BYTES),
        "{path}: {stored}"
    );
}

async fn stored_hash(app: &TestApp, user: Uuid) -> String {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .expect("read the stored hash")
}

async fn user_named(app: &TestApp, username: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&app.pool)
        .await
        .expect("find the user")
}

/// The stored hash, once it has been seen to be a new one at the cost.
async fn holds_a_new_hash(app: &TestApp, path: &str, user: Uuid, before: &str) -> String {
    let stored = stored_hash(app, user).await;
    assert_ne!(stored, before, "{path}: nothing new was stored");
    assert_the_cost(path, &stored);
    stored
}

#[test]
fn a_fresh_hash_carries_the_cost_written_here() {
    let hash = hash_password("whatever").expect("hashes");

    assert_the_cost("hash_password", &hash);
}

/// `sign_in` verifies this when the user does not exist, so that an unknown
/// name costs what a wrong password does. It is a literal, so a raised cost
/// leaves it behind and makes the two answers tell apart by time, with no test
/// red: the constant is private, so its source is what can be read.
#[test]
fn the_unknown_user_hash_costs_what_a_real_one_does() {
    let source = include_str!("../src/modules/auth/service.rs");
    let declared = source
        .split("const DUMMY_HASH: &str =")
        .nth(1)
        .expect("the constant is declared");
    let literal = declared
        .split('"')
        .nth(1)
        .expect("the constant is a literal");

    assert_the_cost("DUMMY_HASH", literal);
}

#[tokio::test]
async fn every_path_that_stores_a_password_stores_that_cost() {
    let app = TestApp::spawn().await;

    // The bootstrap: the harness runs the real one.
    let administrator = user_named(&app, ADMIN_USERNAME).await;
    assert_the_cost("the bootstrap", &stored_hash(&app, administrator).await);
    let token = app.sign_in(ADMIN_USERNAME, ADMIN_PASSWORD).await;

    let created = app
        .post(
            "/api/v1/identity/users",
            Some(&token),
            json!({
                "username": "hash.cost",
                "email": "hash.cost@example.test",
                "displayName": "Hash Cost",
                "password": "a-first-long-password",
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let user = Uuid::parse_str(created.data()["id"].as_str().expect("an id")).expect("a uuid");
    let hash = stored_hash(&app, user).await;
    assert_the_cost("a user created", &hash);

    let set = app
        .post(
            &format!("/api/v1/identity/users/{user}/password"),
            Some(&token),
            json!({ "password": "a-second-long-password" }),
        )
        .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);
    let hash = holds_a_new_hash(&app, "a password an administrator set", user, &hash).await;

    let own = app.sign_in("hash.cost", "a-second-long-password").await;
    let changed = app
        .post(
            "/api/v1/auth/change-password",
            Some(&own),
            json!({
                "currentPassword": "a-second-long-password",
                "newPassword": "a-third-long-password",
            }),
        )
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    let hash = holds_a_new_hash(&app, "a password changed", user, &hash).await;

    let asked = app
        .post(
            "/api/v1/auth/forgot-password",
            None,
            json!({ "username": "hash.cost" }),
        )
        .await;
    assert_eq!(asked.status, StatusCode::ACCEPTED, "{}", asked.body);
    let mail = app.mail_delivered(1).await;
    let body = &mail.last().expect("a link was sent").body;
    let token_at = body.find("token=").expect("the body carries a link") + "token=".len();
    let reset_token: String = body[token_at..]
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .collect();
    let reset = app
        .post(
            "/api/v1/auth/reset-password",
            None,
            json!({ "token": reset_token, "newPassword": "a-fourth-long-password" }),
        )
        .await;
    assert_eq!(reset.status, StatusCode::NO_CONTENT, "{}", reset.body);
    holds_a_new_hash(&app, "a password reset", user, &hash).await;

    // A tenant's first administrator is only reachable on a multi-tenant
    // deployment, and the reset above only on a single-tenant one.
    let app = TestApp::spawn_with(|config| config.multi_tenant = true).await;
    let token = app
        .sign_in_to("SYSTEM", ADMIN_USERNAME, ADMIN_PASSWORD)
        .await;
    let tenant = app
        .send(
            Method::POST,
            "/api/v1/organization/tenants",
            Some(&token),
            Some(json!({
                "tenantCode": "HASHCOST",
                "name": "Hash Cost Limited",
                "administrator": {
                    "username": "hash.cost.admin",
                    "email": "hash.cost.admin@example.test",
                    "displayName": "Tenant Administrator",
                    "password": "a-fifth-long-password",
                },
            })),
        )
        .await;
    assert_eq!(tenant.status, StatusCode::CREATED, "{}", tenant.body);
    let first_administrator = user_named(&app, "hash.cost.admin").await;
    assert_the_cost(
        "a tenant's first administrator",
        &stored_hash(&app, first_administrator).await,
    );
}
