import { ref, type Ref } from 'vue'

import { toApiError } from '@/api/client'
import { listDocumentTypes } from '@/api/document-types'
import { useAuthStore } from '@/stores/auth'
import type { DocumentTypeSummary } from '@/types/document-type'

/** The revision a *Deprecate* was asked for. */
export interface DeprecationTarget {
  id: string
  workflowKey: string
  version: number
}

/**
 * What the warning knows about the document types still bound to the revision.
 *
 * - `checking`: the list is being read, and the action waits for it.
 * - `listed`: the first page of them, which may be empty, and how many in all.
 * - `unchecked`: they could not be read, and `reason` says why. The action is
 *   still offered: the binding is the document type's, not the revision's, and
 *   the warning then says what deprecating does without naming the types.
 */
export type BoundTypes =
  | { kind: 'checking' }
  | { kind: 'listed'; types: DocumentTypeSummary[]; total: number }
  | { kind: 'unchecked'; reason: string }

/** The list's page cap (`response::MAX_PAGE_SIZE`): the first hundred are named. */
export const BOUND_TYPES_PAGE_SIZE = 100

const NOT_PERMITTED =
  'You cannot read document types, so whether any is still bound to this revision could not be checked.'

export interface WorkflowDeprecation {
  isOpen: Ref<boolean>
  target: Ref<DeprecationTarget | null>
  bound: Ref<BoundTypes>
  /** Opens the warning for a revision, and reads the types bound to it. */
  ask(revision: DeprecationTarget): void
  /** Closes the warning and hands back the revision it was for, the caller to deprecate it. */
  confirm(): DeprecationTarget | null
  /** Closes the warning and does nothing else. */
  cancel(): void
}

/**
 * The *Deprecate* action's warning, shared by the editor header and the list
 * rows (#713, plan 19 row 6b; **D-101** B, **D-108**).
 *
 * **Deprecating a revision a document type is still bound to may block that
 * type's submissions until somebody rebinds it** (#187's rule, D-101's dated
 * note of 2026-10-10): the binding stays, and a submit it routes is refused
 * with `WORKFLOW_NOT_PUBLISHED`. *May* (corrected 2026-10-10: this said
 * *blocks*): a document follows the first binding in force by priority, so a
 * type another binding routes first, or whose binding has lapsed, is not
 * blocked. The list names every live binding whatever its dates, erring on
 * the side of too many. The route does not warn, and the permissions are
 * split, so a `workflow:definition:deprecate` holder can halt what only a
 * `document-type:update` holder can fix. So before it proceeds, the action
 * lists those types, read in one call through the list's
 * `workflowDefinitionId` filter (#713).
 *
 * **A caller without `document-type:read` is told the check could not be
 * made**, and may still go on or cancel. A refusal the list gives anyway (403),
 * or any other failure, is told the same way, with the server's words.
 *
 * **The deprecation itself is the caller's to run**, under its own in-flight
 * state: {@link WorkflowDeprecation.confirm} closes the warning first, so
 * nothing in the dialog is left operable while the request is out.
 */
export function useWorkflowDeprecation(): WorkflowDeprecation {
  const auth = useAuthStore()

  const isOpen = ref(false)
  const target = ref<DeprecationTarget | null>(null)
  const bound = ref<BoundTypes>({ kind: 'checking' })

  // A read that comes back after the warning was reopened for another revision
  // is not that revision's answer.
  let asked = 0

  async function readBound(revisionId: string, ticket: number): Promise<void> {
    let next: BoundTypes

    try {
      const page = await listDocumentTypes({
        workflowDefinitionId: revisionId,
        pageSize: BOUND_TYPES_PAGE_SIZE,
      })

      next = { kind: 'listed', types: page.items, total: page.meta.total }
    } catch (failure) {
      const error = toApiError(failure)

      next = {
        kind: 'unchecked',
        reason: error.isForbidden
          ? NOT_PERMITTED
          : `Whether any document type is still bound to this revision could not be checked: ${error.message}`,
      }
    }

    if (ticket === asked) {
      bound.value = next
    }
  }

  function ask(revision: DeprecationTarget): void {
    asked += 1
    target.value = { id: revision.id, workflowKey: revision.workflowKey, version: revision.version }
    isOpen.value = true

    if (!auth.can('document-type:read')) {
      bound.value = { kind: 'unchecked', reason: NOT_PERMITTED }

      return
    }

    bound.value = { kind: 'checking' }
    void readBound(revision.id, asked)
  }

  function confirm(): DeprecationTarget | null {
    if (!isOpen.value || bound.value.kind === 'checking') {
      return null
    }

    isOpen.value = false

    return target.value
  }

  function cancel(): void {
    isOpen.value = false
  }

  return { isOpen, target, bound, ask, confirm, cancel }
}
