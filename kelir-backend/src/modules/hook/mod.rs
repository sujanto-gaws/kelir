//! The document lifecycle hook chain (LHCS 1.0.0, architectures/01 §12;
//! FR-WF-005, [#339]).
//!
//! **The chain has been specified since the founding architecture and built by
//! nothing.** `document_lifecycle_hooks` and `document_hook_executions` have
//! been in the schema since `0015_document.sql` with no reader;
//! `workflow::service::engine`'s own module doc said so where it would have
//! invoked them, and used the words *there is no chain*. This module is the
//! chain.
//!
//! It arrives with system tasks because that is what needed it: a
//! `SERVICE_TASK` is *a step the product performs rather than a person*, and
//! until there was somewhere for the step to live, the type could only ever
//! have meant *a task nobody is coming to do*.
//!
//! # Both halves, and where each runs
//!
//! **`before_*` runs inside the caller's transaction**, and a `REJECT` rolls it
//! back ([`service::run_before_chain`]).
//!
//! **`after_*` runs from the outbox**, which is architectures/01 §12.5's
//! *published to the outbox in the same transaction, executed by the worker*,
//! and arrived with it (ADR-0041; the half ADR-0036 left for it). The workflow
//! engine writes a `Workflow.Transitioned` event beside every transition it
//! commits; `outbox::worker` delivers it to
//! `workflow::service::after_hooks`, which resolves the chain — the edge's
//! JWSS `actions` from the pinned revision, merged with the registry's
//! `after_workflow_transition` entries — and [`service::run_after_chain`] runs
//! it. So JWSS `actions` now fire, after commit, on decided and automatic
//! transitions alike.
//!
//! What an after-handler does is report success or failure (LHCS §5.2), and
//! the log records it as `CONTINUE` or `ERROR`. A failure makes the delivery
//! retryable, and repeated failure opens a **circuit breaker derived from the
//! execution log itself**: five `ERROR`s in a row for one (tenant, hook,
//! handler) switch the handler off, tell the tenant's administrators once, and
//! allow one trial ten minutes after the last failure ([`domain::breaker_is_open`]).
//!
//! **A handler declares its kind** ([`domain::HandlerKind`]). A before-only
//! handler — `set_form_field`, `reject_when` — named in `actions` is refused at
//! publish: after commit its `MODIFY` would read as a write and its `REJECT` as
//! a veto, and be neither.
//!
//! **Plugin handlers do not resolve.** §2 makes an unknown plugin an ERROR at
//! registration and a *disabled* one a warning; there are no plugins, so every
//! `plugin:` reference is refused at publish naming the plugin. That is the
//! honest answer while `plugins` has no rows — the alternative is accepting a
//! reference that can never run, which is the shape this whole issue is about.
//!
//! # The stages that fire
//!
//! Two: `before_workflow_transition` and `after_workflow_transition`, both at
//! [`Stage::Transition`]. Every other name in the §12.3 catalogue is a valid
//! registration and fires nothing. That is a smaller claim than the catalogue
//! makes, and the next stage plugs in beside these two.
//!
//! # What a handler may not do
//!
//! §12.5's rules that this module enforces rather than documents: a handler
//! returns a result rather than writing; it may not write `documents.status`,
//! because it never receives a handle to write anything; and it has a time
//! budget. A before-handler that overruns is treated as a `REJECT` with
//! `HOOK_TIMEOUT`; an after-handler that overruns is an `ERROR`, and its
//! delivery is retried.
//!
//! What is **not** enforced is isolation — *a plugin hook panic must never
//! abort a core transaction it did not veto*. Every handler here is core Rust
//! compiled into this binary, so a panic is this process's panic and catching
//! it would be catching our own bug. The rule lands when a plugin runtime does.
//!
//! [#339]: https://github.com/sujanto-gaws/kelir/issues/339

pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;

pub use domain::{
    HandlerKind, HandlerReference, HookResult, Invocation, Registration, Rejection, Source, Stage,
};

/// The before-hook a JWSS `guards` entry registers (architectures/01 §12.3).
pub const BEFORE_WORKFLOW_TRANSITION: &str = "before_workflow_transition";

/// The after-hook a JWSS `actions` entry registers, delivered from the outbox
/// (ADR-0041).
pub const AFTER_WORKFLOW_TRANSITION: &str = "after_workflow_transition";
