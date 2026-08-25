//! RAD metadata use cases (FR-RAD-002, FR-RAD-003).
//!
//! Two rules hold across every function here and are stated once:
//!
//! - **The permission is required before anything is read.** `caller.require`
//!   is the first line of each use case, not a check somewhere below the query
//!   that answers it — a 404 that only a permitted caller could have received
//!   is itself a disclosure.
//! - **A write is read back before it is audited** (#135). The record then says
//!   what the row holds rather than what the request asked for, and the two
//!   differ on every write here: keys are trimmed, order is assigned, and a
//!   default fills a field the caller omitted.

pub mod form;
pub mod list;
