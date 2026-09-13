//! Reporting — the dashboard, and what a person sees when they sign in
//! (SRS §4.14, FR-RPT-*; [#431]).
//!
//! # This module exists because of a decision, not because of a table
//!
//! Kelir has two ways to put rows on a screen and
//! [architectures/06](../../../../docs/architectures/06.%20Building%20an%20ERP%20on%20Kelir.md)
//! §7 says they **do not converge**: a RAD list, whose rows are the documents of
//! every type that names it ([SDD] §8.2.4) and which a deployment builds by
//! configuration with no code; and a report written as an endpoint and a page.
//!
//! **[ADR-0039] (D-78) puts every FR-RPT widget on the second path**, and the
//! count is the argument: four of Phase 8's five widgets have rows a list cannot
//! carry — two over `workflow_tasks`, one an aggregate over statuses, and one
//! that counts rather than lists — while the fifth, FR-RPT-003, genuinely is a
//! page of documents and would render as a list definition today. **That fifth
//! is why the decision had to be taken rather than assumed.** A dashboard that
//! fetched its cards two ways would have two answers to *what may this viewer
//! see* on one screen.
//!
//! **FR-RPT-003 is now built, and it is a field on
//! [`domain::DashboardSummary`] rather than a list definition** ([#433]). It is
//! the row where the decision cost something, so it is worth being plain about
//! what was paid: a list definition would have rendered this widget with no
//! Rust at all. What the endpoint bought instead is one visibility rule on the
//! screen rather than two, and one shape for a client to render. **[#433]
//! records the trade rather than hiding it** — if the list path is ever taken
//! for a dashboard widget, that is the row it starts from and [ADR-0039] is
//! where the argument reopens.
//!
//! So this module has the ordinary shape — domain, repository, service,
//! handlers — and the dashboard has **one contract that the later widgets
//! extend rather than bypass**. FR-RPT-002 and FR-RPT-003 add their rows to
//! [`domain::DashboardSummary`]; they do not add endpoints beside it. **A second
//! dashboard endpoint is the defect this module's shape exists to prevent**, and
//! it arrives quietly, as *just this one widget, which is different*.
//!
//! [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
//! [SDD]: ../../../../docs/design/01.%20System%20Design%20Document.md
//!
//! # It owns no table, and it writes no statement of its own
//!
//! `0043_reporting.sql` creates nothing. The summary's numbers come from
//! `workflow_tasks` and `documents`, and this module reaches both **through
//! their owning modules' services** — coding standard §2.2, and here it is
//! load-bearing rather than tidy:
//!
//! | Number | Comes from | Under whose predicate |
//! |---|---|---|
//! | `tasksWaiting`, `tasksOverdue`, `pendingTasks` | [`workflow::service::inbox::waiting_work`] | `workflow::repository::inbox`'s `WHERE` — the one the inbox itself pages on |
//! | `draftDocuments` | [`document::service::list::count_own_drafts`] | `tenant_id`, `created_by`, `status = 'DRAFT'` |
//! | `recentDocuments` | [`document::service::list::recent_documents`] | `document::repository::list`'s own `WHERE` — `tenant_id` and `deleted_at IS NULL`, the two lines the document list opens with |
//!
//! **The first row is the one that matters.** *Whose task is this* is a rule
//! with four clauses — assignee, unassigned-plus-candidate-role, the role
//! grant's validity window, and the department scoping
//! [#225](https://github.com/sujanto-gaws/kelir/issues/225) added — and it is
//! written once, in the statement the inbox pages on. A dashboard counting its
//! own way would be a second answer to it, and
//! [#279](https://github.com/sujanto-gaws/kelir/issues/279) is this project's
//! record of what that costs: the one duplicated predicate in that file drifted
//! exactly where its own comment warned it would, and an inbox said 23 and
//! ended at 19. **The dashboard's number and the inbox's number cannot disagree,
//! because there is one of them.**
//!
//! **FR-RPT-002 ([#432]) put that to the test**, because its widget needed the
//! task *rows* and not only the count. It got them from the same statement —
//! `count(*) OVER ()` beside the page, so the number and the list are one
//! answer from one snapshot rather than two reads with a decision possibly
//! landing between them ([#432] AC5). **No fourth copy of *whose task is this*
//! was written**, which is what AC2 asks and what
//! `tests/reporting_dashboard.rs`'s divergence tests are there to keep true:
//! they assert that the widget and the inbox list the *same rows*, not merely
//! that the widget lists some.
//!
//! [#432]: https://github.com/sujanto-gaws/kelir/issues/432
//!
//! **FR-RPT-003 ([#433]) tested the same rule on the other table and it held
//! the same way.** The recent-documents widget takes the document list's own
//! `WHERE` — `tenant_id` and `deleted_at IS NULL`, the rule that file states at
//! its top — rather than restating it, so a document the list refuses and the
//! widget shows is a defect neither can have separately ([#433] AC3). What the
//! widget adds is a join and no filter: `activity_events` narrowed to the
//! caller's own touch events, which is a **different** predicate over the same
//! rows rather than a second copy of the visibility rule.
//!
//! **`activity_events` is the only record of who did what to which document**,
//! and that is why the widget reads it rather than `documents.created_by`. That
//! column answers who *raised* a row and nothing else, so a widget built on it
//! would miss every document the caller commented on or decided — two of the
//! four verbs [`activity::domain::TOUCH_EVENT_TYPES`] names. The definition
//! lives there, in the module that owns the vocabulary, beside the other
//! function that classifies it.
//!
//! [#433]: https://github.com/sujanto-gaws/kelir/issues/433
//! [`activity::domain::TOUCH_EVENT_TYPES`]: crate::modules::activity::domain::TOUCH_EVENT_TYPES
//! [`document::service::list::recent_documents`]: crate::modules::document::service::list::recent_documents
//!
//! # One permission, and the invariant that makes one enough
//!
//! [`DASHBOARD_READ`] is the only thing [`service::dashboard_summary`] requires.
//! Not `workflow:task:read`, not `document:read`.
//!
//! **The invariant: everything this module serves is about the caller's own
//! work.** Tasks assigned to them or offered to a role they hold; documents they
//! raised themselves; documents they themselves acted on. Nothing on the
//! dashboard describes another person's work, another department's workload, or
//! the tenant's population — so there is no second surface's data here for a
//! second permission to be protecting.
//!
//! **It says *everything* rather than *every number* because FR-RPT-002 put
//! rows on the dashboard**, and the distinction is worth being exact about. The
//! line the invariant draws is **whose work**, not *rows versus counts*: five
//! task rows the caller is being asked to decide are as much their own work as
//! a count of them was, and a task's own holder is the last party its name
//! needs keeping from. [ADR-0039] had already taken this — FR-RPT-002 "inherits
//! FR-RPT-001's endpoint, its permission and its card" — and requiring
//! `workflow:task:read` for the rows would have made the count and the list
//! disagree by permission, which is the one thing [#432] AC5 forbids.
//!
//! **FR-RPT-003 is the plainest case of that line rather than the hardest.**
//! The caller is the *actor* on every event that put a document in
//! `recentDocuments` — they raised it, edited it, commented on it or decided it
//! — so there is no title in that list a viewer is learning for the first time.
//! `document:read` in front of it would be a check standing between somebody
//! and a record of their own actions, and it would refuse a person a list of
//! their own work on the one screen built to show it.
//!
//! The `document:read` **route** still holds, which is what makes the rows
//! links only where a link would work — the same courtesy the task rows get,
//! and the same non-control.
//!
//! **What the frontend does with that** is narrower and is a courtesy rather
//! than a rule: the widget's rows link into the task screen, that route holds
//! `workflow:task:read`, so a viewer without it sees the rows and not the
//! links. The server is not asked to know this; a page that offered a link to
//! `/forbidden` would merely be impolite.
//!
//! **The alternative was requiring the underlying reads too, and one migration
//! away is what that costs when it is wrong.** `activity:read` gated a timeline
//! whose every fact was already behind the document's own read; **D-45** found
//! it serving an attachment's file name to a caller holding neither
//! `attachment:read` nor `comment:read`, **D-47** took it out of the check, and
//! `0041_activity_read_dropped.sql` took the row out of the catalogue a release
//! later ([#301](https://github.com/sujanto-gaws/kelir/issues/301)). **A
//! permission whose only job is to duplicate one already checked outlives the
//! check and then guards nothing.** The mirror-image mistake is the one
//! available here, and it would have been worse in a way that shows: a person
//! holding `reporting:dashboard:read` and nothing else would have been refused
//! a count of their own drafts.
//!
//! What [`DASHBOARD_READ`] does gate is the **surface**. A deployment that does
//! not want somebody on the dashboard has one grant to withhold, and that is a
//! real capability rather than a duplicated one.
//!
//! ## The boundary, named before Sprint 18 meets it
//!
//! **This is not a general reporting permission**, and two of the requirements
//! already on the roadmap cross the line it draws:
//!
//! - **FR-RPT-004** — a status summary over the tenant's documents. Those are
//!   not the caller's own rows, and counting them is the document population,
//!   which `document:read` gates. **It cannot be served by widening
//!   [`document::service::list::recent_documents`] either**, which is worth
//!   saying now that a function on this path returns document rows rather than a
//!   number: the widening that would serve it is dropping `actor_user_id`, and
//!   that one deletion turns *the caller's own work* into *the tenant's*.
//! - **FR-RPT-007** — workload by department. That is other people's queues,
//!   which `workflow:task:read` gates.
//!
//! Neither can ship behind [`DASHBOARD_READ`] alone, and neither can be served
//! by widening [`workflow::service::inbox::waiting_work`],
//! [`document::service::list::count_own_drafts`] or
//! [`document::service::list::recent_documents`] — all three of which say so in
//! their own doc comments, where an author reaching for them will be standing.
//!
//! [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
//! [`workflow::service::inbox::waiting_work`]: crate::modules::workflow::service::inbox::waiting_work
//! [`document::service::list::count_own_drafts`]: crate::modules::document::service::list::count_own_drafts
//!
//! # What is not here
//!
//! **FR-RPT-004 and FR-RPT-005 are Sprint 18** (**D-77**). FR-RPT-003 was this
//! module's last Sprint 17 row and is built: it extended
//! [`domain::DashboardSummary`] the way FR-RPT-002 did, a field beside the
//! others rather than an endpoint beside this one.
//!
//! **A count of the documents the caller has ever touched.**
//! [`RECENT_DOCUMENTS_SHOWN`] has no `recentDocumentsTotal` beside it, unlike
//! `pendingTasks` and its `tasksWaiting`. A queue's size is a thing somebody
//! needs — *3 of 12* is the difference between nothing waiting and a backlog —
//! and *how many documents have you ever touched* is not a question anybody
//! asks. It would be a count over the caller's whole history, on the screen
//! every session loads, to caption a card.
//!
//! **Search over the recent-documents widget.** The document list has search
//! ([#171](https://github.com/sujanto-gaws/kelir/issues/171)) and the card
//! links to it. **A configurable widget** is further out than that and is the
//! same refusal from the other end: the moment *recent documents* takes a
//! definition, the row has quietly become the RAD list path [ADR-0039]
//! rejected.
//!
//! **Deciding a task from the dashboard.** The widget links to the task;
//! approve and reject live where
//! [#182](https://github.com/sujanto-gaws/kelir/issues/182) built them. A
//! decision surface on a summary card would be a second place for the
//! `requiresComment` rule to be got wrong, and JWSS §4.1 makes that rule a
//! property of the edge the engine actually chooses — which a card has no way
//! to know.
//!
//! **A second filter on the pending-task query.** FR-RPT-005's overdue widget
//! is exactly that and is Sprint 18's; `workflow_tasks.due_at` and its index
//! have existed since [#235](https://github.com/sujanto-gaws/kelir/pull/235).
//! That is a reason to leave this query fit to carry one more filter, and not a
//! reason to write it now.
//!
//! **Any charting dependency.** The tree has none, and the first surface that
//! needs one is FR-RPT-004, where the choice gets its own decision rather than
//! arriving as a transitive dependency of a card.
//!
//! **Export.** FR-RPT-008 (CSV, Excel) is unscheduled in the
//! [Product Backlog](../../../../projects/planning/02.%20Product%20Backlog.md).
//!
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

pub mod domain;
pub mod handlers;
pub mod service;

/// Reading the dashboard summary (FR-RPT-001, [#431]).
///
/// **One permission for the whole dashboard**, seeded by `0043_reporting.sql`.
/// The module doc above is the argument for why it is one and not three, and
/// for the invariant — *every number is the caller's own work* — that a widget
/// added later has to keep holding or take a second grant.
///
/// [#431]: https://github.com/sujanto-gaws/kelir/issues/431
pub const DASHBOARD_READ: &str = "reporting:dashboard:read";

/// How many pending tasks the widget carries (FR-RPT-002, [#432]).
///
/// **A card, not a queue.** The inbox is one click away and pages properly;
/// what this screen answers is *is there anything, and what is at the top of
/// it*. Five rows fit a card at the narrowest width the dashboard lays out at
/// without the grid becoming a list of lists, and a person whose answer is
/// *more than five* has `tasksWaiting` to tell them so in one number.
///
/// **It is this module's decision rather than the workflow module's**, which is
/// why it lives here and is passed in: how much room a card has is a property
/// of the dashboard, and `waiting_work` should not have an opinion about it.
///
/// [#432]: https://github.com/sujanto-gaws/kelir/issues/432
pub const PENDING_TASKS_SHOWN: i64 = 5;

/// How many recent documents the widget carries (FR-RPT-003, [#433]).
///
/// **Five, matching [`PENDING_TASKS_SHOWN`], and they are separate constants on
/// purpose.** The two cards sit side by side and a grid whose rows are five and
/// seven deep reads as a layout accident rather than a decision — but they are
/// not one constant, because *how much room a card has* is a per-card property
/// and a later widget with a different shape should not have to change this
/// one's cap to get its own.
///
/// **There is no count beside this list**, and that is the difference from the
/// pending-task widget rather than an omission. `tasksWaiting` exists because a
/// queue has a size somebody needs — *3 of 12* is the difference between
/// nothing waiting and a backlog. **"How many documents have you ever touched"
/// is not a question anybody asks**, and a number that answered it would be a
/// count over the caller's whole history, on a screen every session loads, to
/// caption a card. So the widget shows the top five and links to the document
/// list, which pages properly.
///
/// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
pub const RECENT_DOCUMENTS_SHOWN: i64 = 5;
