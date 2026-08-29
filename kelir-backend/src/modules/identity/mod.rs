//! Users, roles, permissions and their grants (SRS FR-IDM-001..009).
//!
//! The **delegation** slice (FR-IDM-006, [#184]) is the module's third: a
//! window that redirects one person's approvals to another for a stretch of
//! time. Its rows are `delegations` (Database Schema §3.8), which has existed
//! since `0002_identity.sql`; what arrived with #184 is a writer, and — the
//! point of **D-17** scheduling it beside FR-WF-009 and FR-TASK-008 — a reader.
//! The reader is in the workflow module, at the seam JWSS §5.1 puts it:
//! `delegation_repository::active_delegate_of` is the statement
//! `workflow::service::assignment` runs after a rule resolves.
//!
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184

pub mod delegation;
pub mod delegation_repository;
pub mod delegation_service;
pub mod domain;
pub mod handlers;
pub mod repository;
pub mod service;
