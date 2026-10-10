import type { LogicVariable } from '@/components/logic-builder/logicTree'
import type { DocumentStatus } from '@/types/document'
import type {
  AssigneeType,
  JwssDefinition,
  JwssState,
  JwssTaskPriority,
  JwssTaskType,
  TransitionAction,
} from '@/types/workflow'

/**
 * The workflow editor's vocabularies, each in one typed registry (#426, **D-95**).
 *
 * kopiflowvue wires a node type through one registry that the compiler holds
 * complete; this is that shape over JWSS's own vocabularies rather than BPMN's.
 * **Each table is a `Record` over its union**, so a value added to the type
 * without a row here breaks the build instead of rendering as a blank option.
 *
 * **Nothing here validates.** What the editor offers is shaped by these
 * tables, and whether a definition is runnable is the server's save-time
 * verdict (ADR-0019) — the editor shows it, it does not compute a second one.
 */

interface Labelled {
  label: string
}

/** What the platform calls each status a state may map onto (JWSS §3). */
export const DOCUMENT_STATUS_LABELS: Record<DocumentStatus, string> = {
  DRAFT: 'Draft',
  SUBMITTED: 'Submitted',
  IN_REVIEW: 'In review',
  PENDING_APPROVAL: 'Pending approval',
  APPROVED: 'Approved',
  REJECTED: 'Rejected',
  RETURNED: 'Returned',
  COMPLETED: 'Completed',
  ARCHIVED: 'Archived',
  CANCELLED: 'Cancelled',
}

interface ActionEntry extends Labelled {
  /**
   * What fires an edge with this action in this release, said beside the
   * choice so an author does not build a route nothing will ever take.
   * Informative: the engine is the authority (JWSS §5.3, `service/inbox.rs`).
   */
  firedBy: string
}

/** JWSS §4's `action` vocabulary, in the order an author usually wants it. */
export const TRANSITION_ACTIONS: Record<TransitionAction, ActionEntry> = {
  APPROVE: { label: 'Approve', firedBy: 'A decision on the state’s task.' },
  REJECT: { label: 'Reject', firedBy: 'A decision on the state’s task.' },
  RETURN: { label: 'Return', firedBy: 'A decision on the state’s task.' },
  RESUBMIT: { label: 'Resubmit', firedBy: 'The document’s owner submitting it again.' },
  AUTO: {
    label: 'Automatic',
    firedBy: 'The engine, with no caller, so it takes no “allowed by” and no comment (S5, S12).',
  },
  SUBMIT: {
    label: 'Submit',
    firedBy: 'Nothing: a first submit enters the initial state without a transition.',
  },
  CANCEL: { label: 'Cancel', firedBy: 'Nothing in this release fires it.' },
  COMPLETE: { label: 'Complete', firedBy: 'Nothing in this release fires it.' },
  DELEGATE: {
    label: 'Delegate',
    firedBy: 'Nothing: a delegation hands a task over without a transition.',
  },
  ESCALATE: { label: 'Escalate', firedBy: 'Nothing in this release fires it.' },
}

interface AssigneeEntry extends Labelled {
  /**
   * Whether Kelir resolves it. The two it does not are refused at save, by the
   * server (JWSS §5.3); they are shown, not offered, so a stored one still reads.
   */
  resolved: boolean
}

/** JWSS §5.1's `assigneeType` vocabulary. */
export const ASSIGNEE_TYPES: Record<AssigneeType, AssigneeEntry> = {
  ROLE: { label: 'A role', resolved: true },
  DEPARTMENT_ROLE: { label: 'A role, in a department', resolved: true },
  OWNER: { label: 'The document’s owner', resolved: true },
  USER: { label: 'One user', resolved: true },
  MANAGER_OF_OWNER: { label: 'The owner’s manager — not resolved by Kelir', resolved: false },
  EXPRESSION: { label: 'An expression — not resolved by Kelir', resolved: false },
}

/** JWSS §3.1's `taskType` vocabulary. Which of them the engine performs is the server's to say. */
export const TASK_TYPES: Record<JwssTaskType, Labelled> = {
  APPROVAL_TASK: { label: 'Approval' },
  REVIEW_TASK: { label: 'Review' },
  USER_TASK: { label: 'User task' },
  DATA_ENTRY_TASK: { label: 'Data entry' },
  SIGNATURE_TASK: { label: 'Signature' },
  SERVICE_TASK: { label: 'Service (automatic)' },
}

export const TASK_PRIORITIES: Record<JwssTaskPriority, Labelled> = {
  LOW: { label: 'Low' },
  NORMAL: { label: 'Normal' },
  HIGH: { label: 'High' },
  URGENT: { label: 'Urgent' },
}

/** The kinds of state an author adds: a JWSS state is one object, and these are its usual shapes. */
export type StateKind = 'task' | 'wait' | 'final'

interface StateKindEntry extends Labelled {
  create(code: string): JwssState
}

/** A task key from a state code: `MANAGER_APPROVAL` is `manager_approval`. */
export function taskKeyFor(code: string): string {
  return code.toLowerCase()
}

export const STATE_KINDS: Record<StateKind, StateKindEntry> = {
  task: {
    label: 'State with a task',
    create: (code) => ({
      code,
      name: 'New approval',
      mapsToDocumentStatus: 'PENDING_APPROVAL',
      task: {
        taskDefinitionKey: taskKeyFor(code),
        taskName: 'New approval',
        assignment: { assigneeType: 'ROLE', roleCode: '' },
      },
    }),
  },
  wait: {
    // JWSS §10's `RETURNED`: the document is with its author, in nobody's inbox.
    label: 'State with no task',
    create: (code) => ({ code, name: 'Returned', mapsToDocumentStatus: 'RETURNED' }),
  },
  final: {
    label: 'Final state',
    create: (code) => ({
      code,
      name: 'Completed',
      mapsToDocumentStatus: 'COMPLETED',
      isFinal: true,
    }),
  },
}

/** The JWSS specification version the editor writes for a new workflow (`version`, §2). */
export const JWSS_VERSION = '1.0.0'

/**
 * What a new workflow starts from: one approval, approved or rejected.
 *
 * **It is not publishable as it stands**, and that is deliberate: the key,
 * the name and the deciding role are blank, and only the author knows them.
 * A save is refused at the key and the name; a blank role saves, and the
 * publish refuses it as `ROLE_NOT_LIVE` at each role field (**D-111**).
 */
export function starterDefinition(): JwssDefinition {
  const approval = STATE_KINDS.task.create('PENDING_APPROVAL')
  const decider = { assigneeType: 'ROLE' as const, roleCode: '' }

  return {
    workflowKey: '',
    version: JWSS_VERSION,
    name: '',
    initialState: 'PENDING_APPROVAL',
    states: [
      { ...approval, name: 'Pending approval' },
      STATE_KINDS.final.create('COMPLETED'),
      {
        code: 'REJECTED',
        name: 'Rejected',
        mapsToDocumentStatus: 'REJECTED',
        isFinal: true,
      },
    ],
    transitions: [
      { from: 'PENDING_APPROVAL', to: 'COMPLETED', action: 'APPROVE', allowedBy: decider },
      {
        from: 'PENDING_APPROVAL',
        to: 'REJECTED',
        action: 'REJECT',
        allowedBy: { ...decider },
        // JWSS §4.1: a refusal is the edge that usually needs a reason.
        requiresComment: true,
      },
    ],
  }
}

/**
 * What a condition may read: the context the engine **actually builds**
 * (`workflow/service/engine.rs`, `EvaluationContext::as_json` and
 * `document_facts`), not the wider one JWSS §6.1 sketches.
 *
 * §6.1 also lists `document.amount`, `document.documentTypeKey`, `actor.roles`
 * and `actor.departmentId`, and the engine builds none of them. A condition on
 * one reads `null`, and a comparison against `null` is silently false, so the
 * route would take the fallback and report nothing. Offering them would put
 * that mistake one click away. `formData` is every field of the document's
 * form, which this editor cannot enumerate — a workflow is bound to a document
 * type, not a form — so a `formData.<key>` path is typed, and counts as
 * offered through {@link CONDITION_FREE_PREFIXES}.
 */
export function conditionVariables(definition: JwssDefinition): LogicVariable[] {
  return [
    { path: 'document.status', label: 'Document status', type: 'text' },
    { path: 'document.documentTypeId', label: 'Document type id', type: 'text' },
    { path: 'document.documentNumber', label: 'Document number', type: 'text' },
    { path: 'actor.userId', label: 'Acting user id', type: 'text' },
    ...(definition.variables ?? []).map((variable) => ({
      path: `variables.${variable.key}`,
      label: `Variable ${variable.key}`,
      type: variable.dataType.toLowerCase(),
    })),
  ]
}

/**
 * The parts of the condition context the engine builds and the editor cannot
 * list: a typed path under one is offered, and draws no warning.
 */
export const CONDITION_FREE_PREFIXES: readonly string[] = ['formData.']

/** `[{value, label}]` from a registry, for a `Select`. */
export function optionsOf<Key extends string>(
  registry: Record<Key, Labelled>,
): { value: Key; label: string }[] {
  return (Object.keys(registry) as Key[]).map((value) => ({
    value,
    label: registry[value].label,
  }))
}
