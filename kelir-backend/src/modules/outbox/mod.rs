//! The transactional outbox (architectures/01 §20; [#519]; ADR-0041).
//!
//! **A business write and the event that records it share one transaction**,
//! and something after the commit delivers the event. That is the whole
//! pattern, and this module is both halves of it:
//!
//! - [`write`] inserts an Event Envelope Schema envelope into `outbox_events`,
//!   in the caller's transaction. A transition that rolls back takes its event
//!   with it, and one that commits cannot lose it.
//! - [`worker`] claims what is due, dispatches it by `eventType` to the
//!   consumers this build has, and records how that went on the row.
//!
//! # One producer and one consumer, and a table for more
//!
//! **Every committed workflow transition writes a `Workflow.Transitioned`
//! event**, decided or automatic, whether or not anything is registered to
//! care (ADR-0041 §2, its option A). The engine knows at commit that a
//! transition happened; it does not know, and should not have to, which
//! consumers will read it. The one consumer this build has is the
//! `after_workflow_transition` chain — JWSS `actions` and the registry's after
//! entries — resolved when the event is delivered rather than when it was
//! written. A webhook subscription (FR-INT-005) is a second consumer of the
//! same rows, not a second write in the engine.
//!
//! **A new event is written through [`write`], inside the business
//! transaction** — ADR-0041 §5's one review check. A consumer that published by
//! calling another consumer directly would be the synchronous in-transaction
//! call architectures/01 §12.5 forbids, with a queue's name on it.
//!
//! # What this is not
//!
//! ADR-0041 §4 draws three boundaries, and they are restated where somebody
//! extending this will look:
//!
//! - **Notifications stay on their own table**, delivered by
//!   `notification::worker` under ADR-0034. The circuit breaker's report is an
//!   in-app notification written through `notification::service::notify`.
//! - **ERP posting does not move here** (ADR-0037 §6): it runs inside the
//!   document transaction.
//! - **`external_systems.retry_policy_json` is not this schedule.** That
//!   governs calls to one external system (FR-INT-007); [`domain::retry_delay`]
//!   governs in-process dispatch.
//!
//! [#519]: https://github.com/sujanto-gaws/kelir/issues/519

pub mod domain;
pub mod repository;
pub mod service;
pub mod worker;

pub use domain::{Actor, AggregateType, Delivery, NewEvent, WORKFLOW_TRANSITIONED};
pub use service::write;
