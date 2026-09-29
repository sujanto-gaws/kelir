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
//! # One function resolves a reference, for one call
//!
//! `integration_credentials.secret_reference` is a pointer —
//! `vault://kelir/erp/api-key`, `env://KELIR_ERP_API_KEY`. **Only
//! [`outbound::resolve_secret`] resolves one**, for an administrator's test
//! call (FR-INT-002, #547; ADR-0043), and the value it returns is a
//! [`domain::secret::Secret`]: no `Debug`, `Display` or `Serialize` that could
//! carry it into a response, a log line, an `integration_logs` row or an audit
//! row. `env://NAME` resolves when `NAME` starts with
//! `KELIR_INTEGRATION_SECRET_`, and fails with `SECRET_NAME_NOT_PERMITTED`
//! otherwise; `vault://` fails the call with `SECRET_BACKEND_NOT_CONFIGURED`. What the API checks about a reference's
//! shape at save is in [`domain::credential::validate_secret_reference`].
//!
//! # One route calls out
//!
//! `POST …/external-systems/{id}/endpoints/{endpointId}/test-call`, under
//! [`ENDPOINT_CALL`], is the only traffic this module makes. It runs in the
//! request, behind the egress guard in [`domain::egress`], within the system's
//! timeout, and writes exactly one `integration_logs` row
//! ([`service::test_call`]).
//!
//! # Three resources and eight permissions (#520, the product owner's answers)
//!
//! * **External systems** — `:create`, `:read`, `:update`, and `:deactivate`
//!   gated apart from `:update`; `:deactivate` opens both `deactivate` and
//!   `activate`. **There is no delete**: a delete would have to
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
pub mod outbound;
pub mod repository;
pub mod service;

/// Register an external system.
pub const EXTERNAL_SYSTEM_CREATE: &str = "integration:external-system:create";
/// Read external systems **and their endpoints**.
pub const EXTERNAL_SYSTEM_READ: &str = "integration:external-system:read";
/// Edit an external system **and create or edit its endpoints**.
pub const EXTERNAL_SYSTEM_UPDATE: &str = "integration:external-system:update";
/// Take an external system out of service, **and put it back**.
///
/// Read as *change whether it is active*: turning a system on and turning it
/// off are one permission (#520, the product owner's decision of 2026-09-25),
/// so it gates `POST {id}/deactivate` and `POST {id}/activate` both.
///
/// Gated apart from [`EXTERNAL_SYSTEM_UPDATE`] on `workflow:definition:publish`'s
/// precedent: who may correct a base URL and who may stop every integration
/// that runs through a system are different questions. For the same reason an
/// edit cannot move `status` into or out of `INACTIVE` — that would be this
/// permission reached through the other one. `ACTIVE` ↔ `MAINTENANCE` stays an
/// edit: it says the system is being worked on, not whether it is in service.
pub const EXTERNAL_SYSTEM_DEACTIVATE: &str = "integration:external-system:deactivate";

pub const CREDENTIAL_CREATE: &str = "integration:credential:create";
/// Read credential references — **where** a system's secrets live.
pub const CREDENTIAL_READ: &str = "integration:credential:read";
pub const CREDENTIAL_UPDATE: &str = "integration:credential:update";
pub const CREDENTIAL_DELETE: &str = "integration:credential:delete";

/// Make a test call to an endpoint (FR-INT-002, #547) — **a real request** to
/// the system, with its credential attached.
///
/// Its own permission, apart from `:external-system:*` and `:credential:*`:
/// reading a system, editing it and seeing where its secrets live are each a
/// different question from sending a request under its credential.
pub const ENDPOINT_CALL: &str = "integration:endpoint:call";
