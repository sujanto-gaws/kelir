import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import DocumentTypeListPage from './DocumentTypeListPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The document type builder (#341).
 *
 * **What is asserted here is the client's half.** Which types exist, whether a
 * code is taken and whether a form may be bound are the backend's — and
 * `kelir-backend/tests/document_types.rs` is what holds them. A spec that
 * checked them against a stub would be asserting that a fake returned what it
 * was told to.
 *
 * What the client owns is the part the backend cannot see: an action is offered
 * only to somebody who holds its permission, the dialog is populated from the
 * *whole* type rather than from the list row, and a refusal is shown rather
 * than swallowed.
 */

function summary(overrides: Record<string, unknown> = {}) {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000t1',
    typeCode: 'PURCHASE_REQUISITION',
    name: 'Purchase requisition',
    category: 'PROCUREMENT',
    formId: '0199a1a0-0000-7000-8000-0000000000f1',
    status: 'ACTIVE',
    createdAt: '2026-09-06T00:00:00Z',
    updatedAt: '2026-09-06T00:00:00Z',
    ...overrides,
  }
}

function principal(permissions: string[]): CurrentUser {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000u1',
    username: 'admin',
    displayName: 'Administrator',
    email: 'admin@example.test',
    roles: [],
    permissions,
  }
}

const EVERY_PERMISSION = [
  'document-type:read',
  'document-type:create',
  'document-type:update',
  'document-type:delete',
]

describe('DocumentTypeListPage', () => {
  let backend: FakeBackendHandle
  let rows: unknown[]
  /** What a `GET /document-types/{id}` answers. */
  let detail: { status: number; body: unknown }

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [summary()]
    detail = {
      status: 200,
      body: {
        success: true,
        data: {
          ...summary(),
          description: null,
          listId: null,
          defaultSecurityLevel: 'INTERNAL',
          retentionPolicyId: null,
          targetEntityType: null,
          workflows: [],
        },
      },
    }

    backend = installFakeBackend((request: RecordedRequest) => {
      if (/\/document-types\/[^/]+$/.test(request.url)) {
        return detail
      }

      if (
        request.url.includes('/rad/forms') ||
        request.url.includes('/rad/lists') ||
        request.url.includes('/workflow/definitions')
      ) {
        return {
          status: 200,
          body: { success: true, data: [], meta: { page: 1, pageSize: 20, total: 0 } },
        }
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })
  })

  afterEach(() => backend.restore())

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

    const wrapper = mount(DocumentTypeListPage)

    for (let round = 0; round < 5; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('lists the types the API returned', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="type-PURCHASE_REQUISITION"]').exists()).toBe(true)
  })

  /**
   * A type with no form is legal — a type is configured before its form exists
   * as often as after — and saying so beats a blank cell.
   */
  it('says a type has no form rather than showing an empty cell', async () => {
    rows = [summary({ formId: null })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="type-PURCHASE_REQUISITION"]').text()).toContain('No form')
  })

  // -- AC3: gated the way the other admin screens are ---------------------

  it('offers no action a caller cannot perform', async () => {
    const wrapper = await render(['document-type:read'])

    expect(wrapper.find('[data-testid="new-document-type"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="edit-PURCHASE_REQUISITION"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="delete-PURCHASE_REQUISITION"]').exists()).toBe(false)
  })

  it('offers each action to a caller who holds its own permission', async () => {
    // The paired assertion: without it, a screen that offered nothing to
    // anybody would pass the test above.
    const wrapper = await render()

    expect(wrapper.find('[data-testid="new-document-type"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="edit-PURCHASE_REQUISITION"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="delete-PURCHASE_REQUISITION"]').exists()).toBe(true)
  })

  it('offers the numbering rule to a caller who may update, and not otherwise', async () => {
    expect((await render()).find('[data-testid="numbering-PURCHASE_REQUISITION"]').exists()).toBe(
      true,
    )
    expect(
      (await render(['document-type:read', 'document-type:delete']))
        .find('[data-testid="numbering-PURCHASE_REQUISITION"]')
        .exists(),
    ).toBe(false)
  })

  // -- The dialog is populated from the whole type ------------------------

  /**
   * **The list returns a summary and the dialog needs the whole type.** The
   * bindings and the workflows are not on the row, so opening the dialog
   * fetches — one populated from the summary would silently drop every binding
   * it could not see.
   */
  it('fetches the whole type before opening the edit dialog', async () => {
    const wrapper = await render()

    await wrapper.get('[data-testid="edit-PURCHASE_REQUISITION"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(backend.requests.map((request) => request.url)).toContain(
      '/document-types/0199a1a0-0000-7000-8000-0000000000t1',
    )
    expect(wrapper.find('[data-testid="document-type-dialog"]').exists()).toBe(true)
  })

  it('shows the refusal rather than opening a dialog it could not fill', async () => {
    detail = { status: 403, body: errorBody('FORBIDDEN', 'Forbidden', []) }

    const wrapper = await render()

    await wrapper.get('[data-testid="edit-PURCHASE_REQUISITION"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(wrapper.find('[data-testid="editing-error"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="document-type-dialog"]').exists()).toBe(false)
  })

  it('says the list is empty rather than showing nothing', async () => {
    rows = []

    const wrapper = await render()

    expect(wrapper.text()).toContain('No document types yet')
  })

  // -- The governed-entity chooser ----------------------------------------

  /**
   * **`document_types.target_entity_type` is free text with no `CHECK`**, so a
   * row really can hold a value this build does not implement — the backend's
   * `GovernedEntity::from_db` refuses one rather than guessing, and a type
   * carrying it governs nothing.
   *
   * The chooser has to say the same thing. Showing the unknown value as the
   * selection would claim a governance that does not happen, and saving the
   * dialog unchanged would then write it back.
   */
  it('shows a governed entity it does not implement as no governance at all', async () => {
    detail = {
      status: 200,
      body: {
        success: true,
        data: {
          ...summary(),
          description: null,
          listId: null,
          defaultSecurityLevel: 'INTERNAL',
          retentionPolicyId: null,
          targetEntityType: 'SOMETHING_A_PLUGIN_WROTE',
          workflows: [],
        },
      },
    }

    const wrapper = await render()

    await wrapper.get('[data-testid="edit-PURCHASE_REQUISITION"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect((wrapper.find('[data-testid="type-governs"]').element as HTMLSelectElement).value).toBe(
      '',
    )
  })

  /** The paired assertion: one it does implement is selected. */
  it('shows a governed entity it does implement as the selection', async () => {
    detail = {
      status: 200,
      body: {
        success: true,
        data: {
          ...summary(),
          description: null,
          listId: null,
          defaultSecurityLevel: 'INTERNAL',
          retentionPolicyId: null,
          targetEntityType: 'PARTY',
          workflows: [],
        },
      },
    }

    const wrapper = await render()

    await wrapper.get('[data-testid="edit-PURCHASE_REQUISITION"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect((wrapper.find('[data-testid="type-governs"]').element as HTMLSelectElement).value).toBe(
      'PARTY',
    )
  })
})
