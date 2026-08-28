//! Workflow use cases.
//!
//! [`engine`] is the one place a process moves; everything else here is a way in
//! to it. [`assignment`] is the one place "who is this task for" is answered.

pub mod assignment;
pub mod definition;
pub mod engine;
pub mod inbox;
pub mod instance;
pub mod task;
