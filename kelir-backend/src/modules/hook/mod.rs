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
//! # What is built, and what is declared and is not
//!
//! **`before_*` is built. `after_*` is not, and the reason is not a schedule.**
//! Architectures/01 §12.5 specifies an after-hook as *published to the outbox in
//! the same transaction, executed by the worker* — and this product has no
//! outbox. Notifications are delivered by a worker over their own table
//! (`notification::worker`, and the note in `notification::service` that says a
//! general outbox is Phase 8), which is a queue for one subject rather than the
//! dispatcher §12.5 describes. Building after-hooks over a queue that does not
//! exist would mean either a second private queue or a synchronous call in the
//! caller's transaction, and the second is precisely what §12.5 forbids.
//!
//! So JWSS `actions` are **stored, validated, and not invoked** — exactly as
//! they were — and JWSS `guards` now run. That is a narrowing of the contract
//! and it is stated here rather than discovered: a definition author whose
//! `actions` never fire should be able to find out why in one place.
//!
//! **Plugin handlers do not resolve.** §2 makes an unknown plugin an ERROR at
//! registration and a *disabled* one a warning; there are no plugins, so every
//! `plugin:` reference is refused at publish naming the plugin. That is the
//! honest answer while `plugins` has no rows — the alternative is accepting a
//! reference that can never run, which is the shape this whole issue is about.
//!
//! # The stages that fire
//!
//! One: `before_workflow_transition`, at [`Stage::Transition`]. Every other
//! name in the §12.3 catalogue is a valid registration and fires nothing. That
//! is a smaller claim than the catalogue makes and a larger one than yesterday,
//! and [`service::resolve`] is where the next stage plugs in.
//!
//! # What a handler may not do
//!
//! §12.5's rules that this module enforces rather than documents: a handler
//! runs **inside the caller's transaction** and returns a result rather than
//! writing; it may not write `documents.status`, because it never receives a
//! handle to write anything; and it has a time budget, after which it is
//! treated as a `REJECT` with `HOOK_TIMEOUT`.
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
    HandlerReference, HookResult, Invocation, Registration, Rejection, Source, Stage,
};

/// The one hook name this build fires (architectures/01 §12.3).
pub const BEFORE_WORKFLOW_TRANSITION: &str = "before_workflow_transition";

/// The after-hook a JWSS `actions` entry registers, named so the validator can
/// enforce §3.2's kind constraint without a second literal.
pub const AFTER_WORKFLOW_TRANSITION: &str = "after_workflow_transition";
