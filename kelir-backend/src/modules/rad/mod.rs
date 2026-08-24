//! RAD — the metadata layer that makes a document type configurable rather
//! than coded (FR-RAD-*).
//!
//! Phase 4 carries the front half of it (decision **D-2**): the JSON Logic
//! evaluator here, the metadata tables, and the storage APIs over form and list
//! definitions. The builder UIs and the rule engines around this evaluator stay
//! in Sprints 14–16.

pub mod evaluator;
