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

/** What this release can actually do to a task. */
export type DecisionAction = 'APPROVE' | 'REJECT'

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
   * A definition may declare `RETURN` — FR-WF-008 is Sprint 11 — and a screen
   * that drew a button for it would produce a 422 from a control the product
   * itself put there. The flag is what lets the screen *show* the edge without
   * offering it.
   */
  supported: boolean
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

/** What a person calls where a process is. */
export const INSTANCE_STATUS_LABELS: Record<InstanceStatus, string> = {
  STARTED: 'Starting',
  RUNNING: 'In progress',
  SUSPENDED: 'Paused',
  COMPLETED: 'Finished',
  CANCELLED: 'Cancelled',
  FAILED: 'Failed',
}
