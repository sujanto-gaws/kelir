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
  candidateRoleCode: string | null
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
   * A definition may declare `DELEGATE` — FR-WF-009 is #184 — and a screen that
   * drew a button for it would produce a 422 from a control the product itself
   * put there. The flag is what lets the screen *show* the edge without
   * offering it. `RETURN` was the original example and left the list when #183
   * built it, which is the flag working rather than a reason to remove it.
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
  occurredAt: string
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
