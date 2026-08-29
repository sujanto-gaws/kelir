/**
 * Wire types for the workflow surface and the task inbox (FR-WF-*, FR-TASK-*).
 *
 * These mirror `kelir-backend/src/modules/workflow/domain/` and
 * `modules/workflow/service/inbox.rs`. **The state codes are not an enum**, and
 * that is the whole design: a workflow's states are declared by its own
 * definition, so a client that enumerated them would be a client that only works
 * for the workflows somebody thought of. What the payloads carry instead is the
 * definition's own display name beside every code, so a screen renders
 * `MANAGER_APPROVAL` as "Manager approval" without holding a table of its own.
 */

import type { DocumentStatus } from './document'

/** Where a running process is (`workflow_instances.status`). */
export type InstanceStatus =
  'STARTED' | 'RUNNING' | 'SUSPENDED' | 'COMPLETED' | 'CANCELLED' | 'FAILED'

/** How a process ended. */
export type InstanceOutcome = 'APPROVED' | 'REJECTED' | 'RETURNED' | 'CANCELLED'

/** Where one task is in its own life (`workflow_tasks.status`). */
export type TaskStatus =
  'CREATED' | 'ASSIGNED' | 'IN_PROGRESS' | 'COMPLETED' | 'DELEGATED' | 'ESCALATED' | 'CANCELLED'

/**
 * How a task reached the person looking at it.
 *
 * **Not derived from a null assignee.** An unclaimed queue item and work that is
 * already mine are different situations and need different words on the screen;
 * the backend answers the question once so two components cannot answer it
 * differently.
 */
export type TaskAssignment = 'MINE' | 'ROLE'

/**
 * What this release can actually do to a task.
 *
 * **`RESUBMIT` is not here, and that is the shape rather than a gap.** A return
 * is taken on a *task* by the approver holding it; a resubmission is taken on
 * the *document* by its owner, from a state that declares no task at all. It
 * goes through `POST /documents/{id}/submission` — the same button the first
 * submit used — so there is no task id for it to name.
 */
export type DecisionAction = 'APPROVE' | 'REJECT' | 'RETURN'

/**
 * How wide the inbox is asked to be.
 *
 * **One axis, three points**: `all ⊃ open ⊃ overdue`. A task that is late is by
 * definition still open — a finished one is not late, it is done — so this is a
 * narrowing rather than a second filter beside `open`. Offering it as a separate
 * flag would let a screen ask for *completed and overdue*, which is a question
 * with no answer, and would need two controls to express one choice.
 */
export type InboxScope = 'open' | 'overdue' | 'all'

export interface WorkflowVariable {
  key: string
  dataType: string
  value: unknown
}

export interface WorkflowInstance {
  id: string
  instanceRef: string
  documentId: string
  workflowDefinitionId: string
  workflowKey: string
  workflowName: string
  /** The revision this approval is running — pinned when it started. */
  definitionVersion: number
  status: InstanceStatus
  currentState: string
  /** The definition's own name for `currentState`. */
  currentStateName: string
  outcome: InstanceOutcome | null
  businessKey: string | null
  startedBy: string | null
  startedAt: string
  completedAt: string | null
  variables: WorkflowVariable[]
}

export interface WorkflowTask {
  id: string
  taskRef: string
  workflowInstanceId: string
  documentId: string
  taskDefinitionKey: string
  taskName: string
  taskType: string
  status: TaskStatus
  assigneeUserId: string | null
  candidateRoleId: string | null
  candidateRoleCode: string | null
  candidateDepartmentId: string | null
  /**
   * Whose authority the assignee is exercising (#184).
   *
   * Set when a delegation window routed this task past the person the
   * definition named, and when its holder handed it over.
   */
  delegatedFromUserId: string | null
  priority: string
  dueAt: string | null
  action: DecisionAction | null
  completedBy: string | null
  completedAt: string | null
  createdAt: string
}

/** The process deciding a document, with every task it has generated. */
export interface DocumentWorkflow {
  instance: WorkflowInstance
  tasks: WorkflowTask[]
}

/** One row of somebody's inbox. */
export interface InboxTask {
  id: string
  taskRef: string
  taskName: string
  taskType: string
  status: TaskStatus
  priority: string
  dueAt: string | null
  assignment: TaskAssignment
  /**
   * Whether this task is late (FR-TASK-007, #185).
   *
   * **The server's answer, and the only one this client has.** `dueAt` is beside
   * it so a screen can say *when*; this says *whether*, computed by the database
   * against the clock that stamped the deadline. A component comparing `dueAt`
   * to `Date.now()` would be a second opinion, and a task late on one machine
   * and not on another is a bug report nobody can reproduce — which is the
   * failure #185 AC4 names.
   *
   * `false` for a task with no deadline, and for one already decided: the
   * indicator says what needs doing now, and a task finished after its date
   * passed is done rather than late.
   */
  isOverdue: boolean
  candidateRoleCode: string | null
  /**
   * Whose work this is, when the holder is standing in for somebody (#184).
   *
   * **A field beside `assignment`, not a third value of it.** A delegated task
   * is unambiguously mine — it is assigned to me and I am the one who has to
   * decide it. What is different is whose approval it is, which is a second
   * sentence on the row rather than a different answer to *is this mine*.
   */
  delegatedFromUserId: string | null
  delegatedFromDisplayName: string | null
  workflowInstanceId: string
  workflowName: string
  currentState: string
  documentId: string
  documentRef: string
  documentNumber: string | null
  documentTitle: string
  createdAt: string
}

/** One thing the holder of a task may do, and where it leads. */
export interface AvailableDecision {
  action: string
  toState: string
  toStateName: string
  /**
   * Whether this release can perform it.
   *
   * A definition may declare a transition this release cannot fire, and a
   * screen that drew a button for it would produce a 422 from a control the
   * product itself put there. The flag is what lets the screen *show* the edge
   * without offering it. `RETURN` was the original example and left the list
   * when #183 built it, which is the flag working rather than a reason to
   * remove it.
   *
   * **`DELEGATE` stays outside this list even now that #184 has built
   * delegation**, and that is not an oversight. Handing a task to somebody else
   * does not move the process, so it is not one of the decisions this list
   * describes; it has its own control on the task screen and its own route. A
   * `DELEGATE` edge in a definition is still fired by nothing.
   */
  supported: boolean
  /**
   * Whether the definition requires a reason with this decision (JWSS §4.1).
   *
   * **Read, never derived.** A screen that decided for itself which actions
   * need a comment — *rejections do* — would be a second rule, and the two would
   * disagree the first time a workflow marked an `APPROVE`. Where they
   * disagreed, this screen would either refuse a decision the server would have
   * taken or send one the server refuses from a button the product drew. #182
   * AC4 is that both ends agree, and the way they agree is that the server owns
   * the rule and this field carries it.
   */
  requiresComment: boolean
}

/** One task, with everything its holder needs to decide it responsibly. */
export interface TaskDetail extends InboxTask {
  workflowKey: string
  currentStateName: string
  decisions: AvailableDecision[]
}

/** What a decision answers with. */
export interface DecisionResult {
  taskId: string
  workflowInstanceId: string
  documentId: string
  action: DecisionAction
  previousState: string
  currentState: string
  documentStatus: DocumentStatus
}

/** What a person calls a task's status. */
export const TASK_STATUS_LABELS: Record<TaskStatus, string> = {
  CREATED: 'Waiting',
  ASSIGNED: 'In your queue',
  IN_PROGRESS: 'In progress',
  COMPLETED: 'Done',
  // Nothing writes it. A delegated task stays `ASSIGNED` — it is an open task
  // somebody else now holds, and `DELEGATED` would take it out of the open-task
  // index and out of the inbox. The label is here because the vocabulary is.
  DELEGATED: 'Delegated',
  ESCALATED: 'Escalated',
  CANCELLED: 'Cancelled',
}

/**
 * One transition, as the document workspace renders it (FR-WF-012; #181).
 *
 * **`occurredAt` rather than `createdAt`**, mirroring the backend: the row is
 * written in the transition's own transaction, so the write and the event are
 * the same instant, and naming the field for the event is what stops a reader
 * treating the list as a log of writes.
 *
 * `actorUsername` is `null` for an engine action and for a user since deleted,
 * which is why the screen falls back to words rather than to a blank.
 */
export interface WorkflowHistoryEntry {
  id: string
  /** `null` on the first entry: the initial state came from nowhere. */
  fromState: string | null
  toState: string
  /** `null` when nothing named an action — the start. */
  action: string | null
  taskId: string | null
  /** The reason given with the decision (FR-TASK-006, #182). */
  comment: string | null
  actorUserId: string | null
  actorUsername: string | null
  /**
   * Whose authority the actor was exercising (#184 AC4).
   *
   * `null` on every entry nobody was standing in for. A delegated approval that
   * showed only the delegate would lose the accountability delegation exists to
   * preserve, so the screen renders both names when this is set.
   */
  onBehalfOfUserId: string | null
  onBehalfOfUsername: string | null
  /**
   * Why this branch and not the other one (FR-WF-015, #186 AC5).
   *
   * Every transition condition the engine evaluated to choose this edge, in the
   * order it evaluated them, each with its outcome. Edges after the winner were
   * never evaluated and are absent rather than `false`.
   *
   * `null` on every entry where nothing was evaluated — the instance's first
   * state, and every action leaving one unconditioned edge, which is most of
   * them.
   */
  routing: RoutingStep[] | null
  occurredAt: string
}

/**
 * One condition the engine evaluated while choosing a transition.
 *
 * `condition` is the JSON Logic expression as the definition wrote it. It is on
 * the wire and **deliberately not rendered**: the workspace's history is read by
 * the person deciding the document, and an expression is not what answers their
 * question — *which branch was considered, and did it apply* is. The rule itself
 * lives in the workflow definition, where somebody who needs to change it is
 * going anyway.
 */
export interface RoutingStep {
  /** The state that edge would have led to. */
  to: string
  condition: unknown
  outcome: boolean
}

/**
 * What a person calls each decision, and how firmly the button should read.
 *
 * **`RETURN` is not destructive and must not look it.** Reject ends the request;
 * return sends it back to be corrected and keeps its number, its history and its
 * place in the queue. A button styled like the terminal one would misdescribe
 * the safer of the two at the moment somebody is choosing between them.
 *
 * "Send back" rather than "Return", because *return* on its own does not say
 * which direction: the target state's name follows it on the button.
 */
export const DECISION_LABELS: Record<DecisionAction, string> = {
  APPROVE: 'Approve',
  REJECT: 'Reject',
  RETURN: 'Send back',
}

export const DECISION_VARIANTS: Record<DecisionAction, 'default' | 'destructive' | 'outline'> = {
  APPROVE: 'default',
  REJECT: 'destructive',
  RETURN: 'outline',
}

/** What a person calls where a process is. */
export const INSTANCE_STATUS_LABELS: Record<InstanceStatus, string> = {
  STARTED: 'Starting',
  RUNNING: 'In progress',
  SUSPENDED: 'Paused',
  COMPLETED: 'Finished',
  CANCELLED: 'Cancelled',
  FAILED: 'Failed',
}
