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
}
