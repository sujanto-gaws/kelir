/**
 * The dashboard's payload (`/api/v1/dashboard/summary`, FR-RPT-001).
 *
 * **One object rather than one per card**, which is
 * [ADR-0039](../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md)
 * (**D-78**): the dashboard is one screen with one contract, so FR-RPT-002's
 * pending-task rows and FR-RPT-003's recent documents become fields here rather
 * than a second call beside this one. The page makes one request on sign-in,
 * and that matters because it is the screen every session loads.
 */

import type { DocumentSummary } from './document'
import type { InboxTask } from './workflow'

/** What is waiting for the caller: the counts, and the top of the queue. */
export interface DashboardSummary {
  /** Tasks assigned to the caller, or offered to a role they hold, still open. */
  tasksWaiting: number
  /**
   * Those of them that are past their due date.
   *
   * **A subset of `tasksWaiting`, never a separate population** — so a card may
   * read *3 waiting, 1 late* and mean three tasks. Adding the two would be
   * wrong.
   *
   * **The server's answer.** It is computed against the clock that stamped the
   * due date, in the statement that read the row. A browser comparing a due
   * date to its own clock would be a second opinion, and a task late on one
   * machine and not on another is FR-TASK-007's named unreproducible bug.
   */
  tasksOverdue: number
  /** Documents the caller raised and has not sent yet — `DRAFT` and no other status. */
  draftDocuments: number
  /**
   * The first few tasks behind `tasksWaiting` (FR-RPT-002, #432).
   *
   * **`InboxTask`, the same row `/api/v1/tasks` serves** — not a dashboard
   * shape. One type means the widget and the inbox cannot disagree about what a
   * task row *is*, and the field that would drift first is `isOverdue`: a
   * second shape is where somebody recomputes it from `dueAt` against the
   * browser's clock, which is the bug FR-TASK-007 names as unreproducible.
   *
   * **Its length is not the count.** The server sends at most five; `tasksWaiting`
   * is the whole queue, which is what lets the card say *3 of 12*. A component
   * reading `pendingTasks.length` as the number waiting would be wrong by
   * exactly the rows the person cannot see.
   *
   * **Empty means nothing is waiting** — and the screen owes the reader that
   * sentence in words, because an empty card is indistinguishable from one that
   * failed to load (#432 AC4).
   *
   * The rows arrive in the order the inbox opens on, so they are the *top of
   * your inbox* rather than a second opinion about which work matters most.
   */
  pendingTasks: InboxTask[]
  /**
   * The documents the caller touched most recently (FR-RPT-003, #433).
   *
   * **`DocumentSummary` and one field**, not a widget shape — the same row the
   * document list renders, so a field added to a document row arrives on both
   * screens or neither. The same argument `pendingTasks` makes for carrying
   * `InboxTask`.
   *
   * **What *touched* means is the server's, and it is written down**: you
   * touched a document when you raised or changed it, said something on it,
   * attached something to it, or moved its workflow. Opening an attachment is
   * deliberately not one of them — that records looking, and counting it would
   * make this card *what you looked at*.
   *
   * **No count beside it**, unlike `pendingTasks` and its `tasksWaiting`: a
   * queue has a size somebody needs, and *how many documents have you ever
   * touched* is not a question anybody asks. So this array is everything the
   * widget claims to carry, and the document list is one click away for the
   * rest.
   *
   * **Empty means the caller has touched nothing** — and the screen owes the
   * reader that sentence in words, because an empty card is indistinguishable
   * from one that failed to load (#433 AC5).
   */
  recentDocuments: RecentlyTouchedDocument[]
}

/**
 * A document on the dashboard, with when this caller last touched it.
 *
 * `lastTouchedAt` is **not** on `DocumentSummary` and must not move there: it
 * is a fact about this caller's relationship to the row rather than about the
 * document, so two people who both edited one document have two different
 * answers and a field on the document could hold only one.
 */
export interface RecentlyTouchedDocument extends DocumentSummary {
  /**
   * When this caller last touched it — `max` over their own touch events.
   *
   * **The value the server ordered by**, so the sequence and the dates a client
   * renders cannot disagree. Re-sorting on `updatedAt` would be a second
   * opinion about what *recent* means, and `updatedAt` moves when *anybody*
   * changes the document rather than when this caller did.
   */
  lastTouchedAt: string
}
