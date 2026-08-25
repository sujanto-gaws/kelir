//! The Kelir backend, as a library.
//!
//! **Why this file exists.** Everything here used to be declared in `main.rs`.
//! A Rust integration test in `tests/` is a separate crate that links against
//! the package's *library* target, and a binary-only package has none — so no
//! test outside `src/` could construct the router, the state or the config, and
//! the only verification possible was a unit test inside the module it tests.
//! Exposing the same modules as a library is what makes `tests/` possible at
//! all; `main.rs` is now a thin binary over it, so the binary and the tests
//! drive identical code rather than two assemblies of it.
//!
//! Module layout and responsibilities are unchanged (coding standard §2.2).

pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod health;
pub mod mail;
pub mod middleware;
pub mod modules;
pub mod response;
pub mod router;
pub mod state;
pub mod utils;
