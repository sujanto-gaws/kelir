//! What the dashboard summary carries (FR-RPT-001; [#431]).
//!
//! [#431]: https://github.com/sujanto-gaws/kelir/issues/431

use serde::Serialize;
use utoipa::ToSchema;

use crate::modules::document::domain::RecentlyTouchedDocument;
use crate::modules::workflow::service::inbox::InboxTask;

/// The dashboard, as one payload.
///
/// # Why one object rather than a card's worth of endpoints
///
/// [ADR-0039] makes the dashboard **one screen with one contract**, and this
/// struct is that contract. FR-RPT-002's pending-task rows and FR-RPT-003's
/// recent documents are fields added here; they are not endpoints added beside
/// this one. **A dashboard that fetches four widgets four ways is four surfaces
/// to secure, four shapes to page, and two answers to *what may this viewer
/// see*** — which is the rejected alternative in that record, stated as a cost
/// rather than as a preference.
///
/// It also means the page makes **one** request on sign-in. That is the screen
/// every authenticated person loads, so the number of round trips it costs is
/// the number every session pays (NFR-PERF-002).
///
/// # Every field is about the caller
///
/// The invariant [`super::DASHBOARD_READ`] rests on, and it is a property of
/// this struct rather than of the handler: **no field here describes another
/// person's work, another department's load, or the tenant's population.** A
/// field that did would need the permission of the surface it came from, and
/// the module doc names the two roadmap requirements that will reach that line
/// first.
///
/// [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    /// Tasks waiting for the caller — assigned to them, or offered to a role
    /// they hold.
    ///
    /// **The inbox's own number**, counted under the statement the inbox pages
    /// on rather than under a second predicate that could disagree with it. A
    /// person who reads *3 waiting* here and opens a queue of four has found a
    /// bug nobody can reproduce, and the way to make that impossible is to have
    /// one count rather than two that are checked against each other.
    pub tasks_waiting: i64,
    /// Those of them that are past their due date.
    ///
    /// **A subset of [`Self::tasks_waiting`], never a separate population** —
    /// `InboxScope`'s documented shape is `overdue ⊂ open`. So a card may read
    /// *3 waiting, 1 late* and mean three tasks, and a client that added them
    /// would be wrong rather than merely odd.
    ///
    /// **Late is the database's opinion**, computed against the same clock that
    /// stamped `due_at`, for the reason `workflow::repository::inbox` gives at
    /// length: a browser subtracting a due date from its own clock is a second
    /// opinion, and a task late on one machine and not on another is FR-TASK-007's
    /// named unreproducible bug report.
    pub tasks_overdue: i64,
    /// Documents the caller raised and has not sent yet.
    ///
    /// **`DRAFT` and no other status**, which makes this *what you have not
    /// finished* rather than *what you have ever touched*. The second question
    /// is FR-RPT-003's, and it is a list rather than a number.
    ///
    /// A document raised by the system belongs to nobody and is counted for
    /// nobody — `documents.created_by` is nullable, and a null author matches no
    /// caller.
    pub draft_documents: i64,
    /// The first few tasks behind [`Self::tasks_waiting`] (FR-RPT-002, [#432]).
    ///
    /// # Why this is the inbox's own row type
    ///
    /// It is [`InboxTask`] — the same struct `GET /api/v1/tasks` serves, not a
    /// trimmed copy of it. **A widget-shaped row would be a second description
    /// of a task**, and the field that drifts first is `isOverdue`: the inbox
    /// gets it from the database in the statement that read the row, and a
    /// second shape is where somebody helpfully recomputes it from `dueAt`.
    /// FR-TASK-007 names that as the bug nobody can reproduce, and the cheapest
    /// way not to have it twice is not to have two rows.
    ///
    /// It also means the dashboard and the inbox render from one TypeScript
    /// type, so a field added to a task row arrives on both screens or neither.
    ///
    /// **The cost, stated:** an open task's `action`, `decisionComment` and
    /// `completedAt` are always `null` here, because a task that is waiting has
    /// not been decided. Three null fields on five rows is the price of one
    /// shape, and it is the right way round — a payload carrying nulls is
    /// legible, and two row types that disagree about *late* are not.
    ///
    /// # How many, and which
    ///
    /// [`super::PENDING_TASKS_SHOWN`] of them, in the order the inbox opens on,
    /// so the widget is **the top of your inbox** rather than a fresh opinion
    /// about which work matters most. `tasksWaiting` is the whole queue, so a
    /// card can say *3 of 12* — and a client must not read this array's length
    /// as the count, which is exactly what the two fields being separate is for.
    ///
    /// **Empty means nothing is waiting**, and the screen has to say so in
    /// words: an empty card is indistinguishable from one that failed to load
    /// ([#432] AC4).
    ///
    /// [#432]: https://github.com/sujanto-gaws/kelir/issues/432
    pub pending_tasks: Vec<InboxTask>,
    /// The documents the caller touched most recently (FR-RPT-003, [#433]).
    ///
    /// # The row the ADR's rejected alternative would have served
    ///
    /// **These really are documents.** [SDD] §8.2.4 defines a RAD list's rows
    /// as the documents of every type that names it, and a list definition
    /// would render this widget today with no code at all —
    /// [architectures/06](../../../../docs/architectures/06.%20Building%20an%20ERP%20on%20Kelir.md)
    /// §7 says that path is the right tool for its own job, and this field is
    /// its best case in the phase.
    ///
    /// **It is a field here anyway, and [ADR-0039] is the reason**: the
    /// dashboard is one screen and should have one contract. Four widgets
    /// fetched four ways is four surfaces to secure, four shapes to page, and
    /// two answers to *what may this viewer see*. So this field is the one the
    /// decision cost something to take, and [#433] records the trade rather than
    /// hiding it — if the list path is ever taken for a dashboard widget, this
    /// is the row it starts from, and the ADR is where the argument reopens.
    ///
    /// # What "touched" means
    ///
    /// `activity::domain::TOUCH_EVENT_TYPES`, written down before the query
    /// existed ([#433] AC2): **you touched a document when you raised or
    /// changed it, said something on it, attached something to it, or moved its
    /// workflow.** Sixteen event types in four groups, and
    /// `Attachment.Downloaded` is deliberately not one of them — it records
    /// *looking*, and counting it would make this *what you looked at*, a
    /// different card with a different privacy question.
    ///
    /// **Ordered by the caller's most recent touch, newest first**, and
    /// `lastTouchedAt` on each row is the value that ordering used — so the
    /// sequence and the dates a client renders cannot disagree.
    ///
    /// # No count beside it
    ///
    /// Unlike [`Self::pending_tasks`], which has `tasksWaiting` for the whole
    /// queue. *How many documents have you ever touched* is not a question
    /// anybody asks, so a client may read this array as everything the widget
    /// has — there is nothing hidden behind it to be honest about, and the
    /// document list is one click away for the rest.
    ///
    /// **Empty means the caller has touched nothing**, and the screen has to
    /// say so in words rather than rendering an empty card ([#433] AC5): a card
    /// with nothing in it is indistinguishable from one that failed to load.
    ///
    /// [ADR-0039]: ../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md
    /// [SDD]: ../../../../docs/design/01.%20System%20Design%20Document.md
    /// [#433]: https://github.com/sujanto-gaws/kelir/issues/433
    pub recent_documents: Vec<RecentlyTouchedDocument>,
}
