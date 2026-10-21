//! Authentication: password verification, token issuance, session lifecycle.
//!
//! Architecture 01 §18.1 recommends JWT access tokens with rotating refresh
//! tokens, and the schema provides `refresh_tokens`, so that is what this
//! implements. Access tokens are short-lived and carry the caller's permissions;
//! refresh tokens are opaque, stored only as a digest, and rotate on use.
//!
//! A revoked refresh token presented again is refused by why it was revoked:
//! a rotation, a sign-out or the reuse sweep is replay evidence, audited and
//! ending every session of the user, and an administrator's revoke is refused
//! with the same 401 and nothing else (#710). Of two refreshes of one token,
//! the one whose rotation `UPDATE` changes the row renews (#751). SDD §11.1
//! is the operative statement.

pub mod bootstrap;
pub mod handlers;
pub mod password;
pub mod reset;
pub mod service;
pub mod token;
