//! Storage for workflow definitions, their projections, running instances and
//! the tasks they generate.
//!
//! **Private to this module** (coding standard §2.2). [`super::super::task_inbox`]
//! reads `workflow_tasks` through [`super::service`], not through these
//! functions — `inbox` is here rather than there for that reason, and says so.

pub mod definition;
pub mod inbox;
pub mod instance;
pub mod projection;
pub mod reference;
pub mod task;
