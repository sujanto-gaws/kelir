import { inject, type ComputedRef, type InjectionKey, type Ref } from 'vue'

import type { LogicVariable } from '@/components/logic-builder/logicTree'

import type { WorkflowDraft } from './useWorkflowDraft'

/**
 * What the editor page shares with every state card and transition row (#426).
 *
 * Provided once rather than threaded through as a dozen props per row (coding
 * standard §3.2): a row is addressed by its index, and reads the draft, the
 * server's messages and whether it may be edited from here.
 */
export interface WorkflowEditorContext {
  draft: WorkflowDraft
  /** `useFormErrors().fieldErrors`, by path in the request body. */
  fieldErrors: Ref<Record<string, string>>
  readOnly: ComputedRef<boolean>
  /** What a condition may read: the context the engine builds. */
  variables: ComputedRef<LogicVariable[]>
  /** After an edit to one field: its message no longer describes it. */
  clearField(path: string): void
  /**
   * After an edit that adds, removes or moves a row: the messages address rows
   * by position, and the positions no longer mean what they did.
   */
  resetErrors(): void
  /** A logic builder's validity, by the row's key and the field it edits. */
  setValidity(key: string, valid: boolean): void
  /**
   * Whether a state's body is drawn only once it is opened (#800): the
   * revision was above `WORKFLOW_LIST_THRESHOLD` when it loaded. Fixed until
   * another revision loads, so no edit unmounts an open body.
   */
  collapsible: Ref<boolean>
  /** Whether a state's body shows, by the state's row key. Always, when not collapsible. */
  isOpen(key: string): boolean
  /** Whether its body is drawn. Once opened it stays drawn, and closing only hides it. */
  isDrawn(key: string): boolean
  /** Opens or closes a state. Not an edit: no history, no dirty flag. */
  setOpen(key: string, open: boolean): void
}

export const WORKFLOW_EDITOR: InjectionKey<WorkflowEditorContext> = Symbol('workflow-editor')

export function useWorkflowEditorContext(): WorkflowEditorContext {
  const context = inject(WORKFLOW_EDITOR)

  if (!context) {
    throw new Error('A workflow editor row is rendered outside WorkflowEditorPage')
  }

  return context
}
