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

/** What is waiting for the caller, as counts. */
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
}
