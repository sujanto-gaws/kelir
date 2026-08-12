//! Authentication: password verification, token issuance, session lifecycle.
//!
//! Architecture 01 §18.1 recommends JWT access tokens with rotating refresh
//! tokens, and the schema provides `refresh_tokens`, so that is what this
//! implements. Access tokens are short-lived and carry the caller's permissions;
//! refresh tokens are opaque, stored only as a digest, and rotate on use.

pub mod bootstrap;
pub mod handlers;
pub mod password;
pub mod service;
pub mod token;
