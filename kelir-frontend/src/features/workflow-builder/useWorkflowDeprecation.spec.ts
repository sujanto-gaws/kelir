import { flushPromises } from '@vue/test-utils'
import type { AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { useWorkflowDeprecation } from './useWorkflowDeprecation'
import { apiClient } from '@/api/client'
import {
  installFakeBackend,
  pageBody,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'

/**
 * The *Deprecate* warning's own rules (#713), past what a page can reach: the
 * pages disable their confirm button while the read is out, and open one
 * warning at a time, so neither the guard in `confirm()` nor the discarding of
 * a stale read is visible through them.
 *
 * Seen to fail (coding standard §2.9), 2026-10-10: twenty-six mutations over
 * row 6b's code, run against the two page specs and `src/api/`. Twenty-four
 * were red there; the two that survived are killed here.
 *
 * | # | Mutation | Reddened |
 * |---|---|---|
 * | M01 | Editor offers Deprecate without the permission | `WorkflowEditorPage.spec.ts` *leaves nothing but New revision operable on an ACTIVE revision…*, *becomes read-only the moment it is published…* and four more |
 * | M02 | Editor offers it on any published status | *…not on a DEPRECATED revision*, *says a deprecated revision is not bindable…* |
 * | M03 | List offers it on any published row | `WorkflowListPage.spec.ts` *offers Deprecate only on an ACTIVE row…* and one more |
 * | M04 | List offers it without the permission | *offers Deprecate only on an ACTIVE row, and only to a holder…* |
 * | M05 | The read sends no revision filter | *warns before it deprecates, naming the document types…*, the list's twin |
 * | M06 | The bound types are not named | the same two |
 * | M07 | Confirm not held while the read is out | *holds the confirm button, not Cancel…* |
 * | M08 | The read made without `document-type:read` | *says it could not check without document-type:read…* (both pages) |
 * | M09 | A 403 on the read told as a failure | *says it could not check when the read is refused…* |
 * | M10 | `confirm()` goes on while the read is out | **Survived the pages** (the button is disabled); *hands back nothing while the bound types are being read* here |
 * | M11 | Editor's deprecate does not set `isSaving` | *leaves no control operable while the deprecation is in flight*, *…while a 409's re-read is in flight* |
 * | M12 | No re-read after a 409 | *reads the revision again when the route answers 409…* (both cases) |
 * | M13 | A re-read after any deprecate refusal | *reads nothing again after a 403…*, *…after a 404…* |
 * | M14 | Success keeps the stored record | *turns the screen read-only and says it is deprecated…* and three more |
 * | M15 | List not read again after success | *warns with the document types bound to that row's revision…* |
 * | M16 | List read again after any refusal | the list's *reads nothing again after a 403/404* |
 * | M17 | List not read again after a 409 | the list's *reads the list again when the route answers 409…* |
 * | M18 | List row not held while its deprecation is out | *holds the row's actions while its deprecation is in flight* |
 * | M19 | Cancel does not close the warning | *does nothing when cancelled* (both pages) |
 * | M20 | The count past the first hundred not said | *names the first hundred and says how many more there are* |
 * | M21 | A stale read lands on the warning | **Survived the pages**; *keeps a read that comes back late off another revision's warning* here |
 * | M22 | No "none bound" sentence | *says no document type is bound when none is…* and the list's twin |
 * | M23 | `confirm()` leaves the warning open in flight | *leaves no control operable while the deprecation is in flight* and more |
 * | M24 | The client posts to `/publication` | `api/workflow.spec.ts` *deprecates at #711's route…*, and the list's deprecate tests |
 * | M25 | `ConfirmDialog` ignores `confirmDisabled` | *holds the confirm button, not Cancel…* |
 * | M26 | `ConfirmDialog` drops its slot | every test naming the bound types |
 */

type Adapter = (config: InternalAxiosRequestConfig) => Promise<AxiosResponse>

function type(id: string, name: string) {
  return {
    id,
    typeCode: id.toUpperCase(),
    name,
    category: null,
    formId: null,
    status: 'ACTIVE',
    createdAt: '2026-10-09T00:00:00Z',
    updatedAt: '2026-10-09T00:00:00Z',
  }
}

describe('useWorkflowDeprecation', () => {
  let backend: FakeBackendHandle

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()
    useAuthStore().$patch({
      user: {
        id: 'u1',
        username: 'admin',
        displayName: 'Administrator',
        email: 'admin@example.test',
        roles: [],
        permissions: ['workflow:definition:deprecate', 'document-type:read'],
      },
    })

    backend = installFakeBackend((request: RecordedRequest) => {
      const revision = request.params.workflowDefinitionId

      return {
        status: 200,
        body: pageBody(
          revision === 'first' ? [type('a', 'First’s type')] : [type('b', 'Second’s type')],
        ),
      }
    })
  })

  afterEach(() => backend.restore())

  /** Holds every bound-types read for `revision` until the returned function is called. */
  function holdReadsOf(revision: string): () => Promise<void> {
    const fake = apiClient.defaults.adapter as Adapter
    let release: () => void = () => undefined
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    apiClient.defaults.adapter = (async (config: InternalAxiosRequestConfig) => {
      if (
        (config.params as Record<string, unknown> | undefined)?.workflowDefinitionId === revision
      ) {
        await gate
      }

      return fake(config)
    }) as Adapter

    return async () => {
      release()
      await flushPromises()
      apiClient.defaults.adapter = fake
    }
  }

  it('hands back nothing while the bound types are being read, and the revision once they are', async () => {
    const deprecation = useWorkflowDeprecation()
    const release = holdReadsOf('first')

    deprecation.ask({ id: 'first', workflowKey: 'purchase_approval', version: 1 })

    expect(deprecation.bound.value.kind).toBe('checking')
    expect(deprecation.confirm()).toBeNull()
    expect(deprecation.isOpen.value).toBe(true)

    await release()

    expect(deprecation.bound.value.kind).toBe('listed')
    expect(deprecation.confirm()).toEqual({
      id: 'first',
      workflowKey: 'purchase_approval',
      version: 1,
    })
    expect(deprecation.isOpen.value).toBe(false)
  })

  it('keeps a read that comes back late off another revision’s warning', async () => {
    const deprecation = useWorkflowDeprecation()
    const release = holdReadsOf('first')

    deprecation.ask({ id: 'first', workflowKey: 'purchase_approval', version: 1 })
    deprecation.cancel()
    deprecation.ask({ id: 'second', workflowKey: 'purchase_approval', version: 2 })
    await flushPromises()

    await release()

    const bound = deprecation.bound.value

    expect(bound.kind === 'listed' && bound.types.map((listed) => listed.name)).toEqual([
      'Second’s type',
    ])
    expect(deprecation.target.value?.id).toBe('second')
  })

  it('closes and hands back nothing when cancelled', async () => {
    const deprecation = useWorkflowDeprecation()

    deprecation.ask({ id: 'first', workflowKey: 'purchase_approval', version: 1 })
    await flushPromises()
    deprecation.cancel()

    expect(deprecation.isOpen.value).toBe(false)
    expect(deprecation.confirm()).toBeNull()
  })
})
