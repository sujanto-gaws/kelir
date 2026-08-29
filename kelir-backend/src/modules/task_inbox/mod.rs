//! The task inbox — what is waiting for the person looking at it (FR-TASK-001,
//! 002, 003; [#179]).
//!
//! # A view module: it owns no storage and invents no permission
//!
//! Its one statement lives in [`super::workflow::repository::inbox`] and it
//! reaches it through [`super::workflow::service::inbox`], because coding
//! standard §2.2 keeps a repository private to its module and `workflow_tasks`
//! is the workflow module's table. A second module writing its own SQL against
//! those rows would be a second implementation of the visibility rule, and the
//! two would drift.
//!
//! **`GET /api/v1/tasks` requires `workflow:task:read`** — the same permission
//! the workflow module's own task read requires, because it reads the same rows.
//! A `task:read` beside it would let a deployment grant the inbox without
//! granting the task, which is the gap [Database Schema](../../../../docs/design/02.%20Database%20Schema.md)
//! §5.13 refused to create for `rad:lookup:read` one module over.
//!
//! # Then why is it a module at all, rather than three more workflow routes?
//!
//! FR-TASK-* is its own SRS area (§4.9) with nine requirements, seven of them
//! still to build, and the inbox is a **surface over** the workflow engine
//! rather than a part of it — the same relationship `rad::lookup` has to master
//! data. The module boundary is where that stays visible; when FR-TASK-007's
//! overdue indicator and FR-TASK-009's completed view land, they land here and
//! the engine does not grow a screen.
//!
//! **Acting on a task is not here.** `POST /workflow/tasks/{id}/decision` is the
//! API ([#177]), and the buttons that call it are FR-TASK-004/005 in Sprint 11
//! ([#182]). This module surfaces tasks.
//!
//! That sentence was tested by FR-TASK-008. The [System Design Document] planned
//! task delegation as `POST /tasks/{id}/delegate` — here, under this prefix —
//! and [#184] put it at `POST /workflow/tasks/{id}/delegation` instead, beside
//! claim and decision. Two things you can do to a task under one prefix and a
//! third under another would be a difference with no reason behind it, and this
//! module would have had to grow a service that writes. The design document was
//! corrected rather than this boundary bent.
//!
//! What *did* arrive here is a field: an inbox row now says whose work it is
//! when its holder is standing in for somebody, because the queue is where that
//! has to be readable.
//!
//! [System Design Document]: ../../../../docs/design/01.%20System%20Design%20Document.md
//! [#184]: https://github.com/sujanto-gaws/kelir/issues/184
//!
//! [#177]: https://github.com/sujanto-gaws/kelir/issues/177
//! [#179]: https://github.com/sujanto-gaws/kelir/issues/179
//! [#182]: https://github.com/sujanto-gaws/kelir/issues/182

pub mod domain;
pub mod handlers;
pub mod service;
