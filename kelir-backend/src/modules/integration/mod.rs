//! Integration core: the external system registry (FR-INT-001, #520;
//! architectures/03 §2.1, §3.1, §4).
//!
//! **What this module holds today is configuration, not traffic.** An
//! administrator registers the systems Kelir talks to, the endpoints each one
//! exposes and **where** each one's secrets live. Nothing here calls out, and
//! nothing here receives: outbound and inbound REST, webhooks, logs, retry,
//! sync and file exchange are FR-INT-002 to FR-INT-010 (#520 AC-9). The five
//! tables those will write were created by `0046_integration.sql` beside the
//! three this module reads, and no route here touches them (#520 AC-3).
//!
//! # A secret is never a value in this module
//!
//! `integration_credentials.secret_reference` is a pointer —
//! `vault://kelir/erp/api-key`, `env://KELIR_ERP_API_KEY` — and **no function
//! here resolves one**. There is no field, response, OpenAPI schema or log line
//! that carries a resolved secret because there is nothing to resolve it with.
//! What the API checks about a reference's shape is in
//! [`domain::credential::validate_secret_reference`], and it says there what it
//! cannot check.
//!
//! # Three resources and eight permissions (#520, the product owner's answers)
//!
//! * **External systems** — `:create`, `:read`, `:update`, and `:deactivate`
//!   gated apart from `:update`. **There is no delete**: a delete would have to
//!   decide what happens to the `master_data_source_references` rows whose key
//!   points at the system, and that is its own decision.
//! * **Endpoints** are part of a system's configuration and have **no
//!   permissions of their own**: [`EXTERNAL_SYSTEM_READ`] lists them and
//!   [`EXTERNAL_SYSTEM_UPDATE`] creates and edits them. An endpoint is retired
//!   by setting its `status` to `INACTIVE` through that same edit — the
//!   system's own deactivate-not-delete rule applied one level down, without a
//!   ninth permission for it.
//! * **Credentials** have their own four, and [`CREDENTIAL_READ`] is separate
//!   from [`EXTERNAL_SYSTEM_READ`] so that seeing a system does not mean seeing
//!   where its secrets live.

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

/// Register an external system.
pub const EXTERNAL_SYSTEM_CREATE: &str = "integration:external-system:create";
/// Read external systems **and their endpoints**.
pub const EXTERNAL_SYSTEM_READ: &str = "integration:external-system:read";
/// Edit an external system **and create or edit its endpoints**.
pub const EXTERNAL_SYSTEM_UPDATE: &str = "integration:external-system:update";
/// Take an external system out of service.
///
/// Gated apart from [`EXTERNAL_SYSTEM_UPDATE`] on `workflow:definition:publish`'s
/// precedent: who may correct a base URL and who may stop every integration
/// that runs through a system are different questions. For the same reason an
/// edit cannot set `status` to `INACTIVE` — that would be this permission
/// reached through the other one.
pub const EXTERNAL_SYSTEM_DEACTIVATE: &str = "integration:external-system:deactivate";

pub const CREDENTIAL_CREATE: &str = "integration:credential:create";
/// Read credential references — **where** a system's secrets live, never the
/// secrets.
pub const CREDENTIAL_READ: &str = "integration:credential:read";
pub const CREDENTIAL_UPDATE: &str = "integration:credential:update";
pub const CREDENTIAL_DELETE: &str = "integration:credential:delete";
