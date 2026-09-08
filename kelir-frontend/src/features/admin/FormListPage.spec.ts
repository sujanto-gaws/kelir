import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import FormListPage from './FormListPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The form list (#373).
 *
 * **What is asserted here is the client's half.** Whether a key is taken,
 * whether a draft may be published and whether a form a document type binds may
 * be deleted are the backend's, and `kelir-backend/tests/rad_forms.rs` holds
 * them. A spec that checked them against a fake would assert that a stub
 * returned what it was told to.
 *
 * What the client owns: an action is offered only to somebody holding its
 * permission, *New revision* is offered only where editing is impossible, and a
 * refusal is shown rather than swallowed.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Two mutations, run 2026-09-08, both red:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | *New revision* offered on a draft as well as a published form | *offers a new revision only for a published form* |
 * | The delete refusal replaced by a generic *Could not delete* | *shows the refusal when a form cannot be deleted* |
 */

const blank = { template: '<div />' }

function summary(overrides: Record<string, unknown> = {}) {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000f1',
    formKey: 'purchase_requisition',
    title: 'Purchase requisition',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'DRAFT',
    entityId: null,
    createdAt: '2026-09-08T00:00:00Z',
    updatedAt: '2026-09-08T00:00:00Z',
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
  'rad:form:read',
  'rad:form:create',
  'rad:form:update',
  'rad:form:delete',
  'rad:form:publish',
]

describe('FormListPage', () => {
  let backend: FakeBackendHandle
  let rows: unknown[]
  let deleteResponse: { status: number; body: unknown }
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [summary()]
    deleteResponse = { status: 204, body: null }

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'delete') {
        return deleteResponse
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/forms', name: 'admin-forms', component: blank },
        { path: '/admin/forms/:id', name: 'admin-form-builder', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

    await router.push('/admin/forms')
    await router.isReady()

    const wrapper = mount(FormListPage, { global: { plugins: [router] } })

    for (let round = 0; round < 5; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('lists the forms this tenant has', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="form-purchase_requisition"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Purchase requisition')
  })

  it('offers each action only to a caller holding its permission', async () => {
    const wrapper = await render(['rad:form:read'])

    expect(wrapper.find('[data-testid="new-form"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="delete-purchase_requisition"]').exists()).toBe(false)
    // Opening needs only the read that opened the screen: a published revision
    // is worth reading without holding update.
    expect(wrapper.find('[data-testid="open-purchase_requisition"]').exists()).toBe(true)
  })

  /**
   * **The row that says what ADR-0027 means.** A draft is edited in place; a
   * published revision is not, because documents pinned it — so the only way
   * forward is a new revision, and it is offered exactly where editing is
   * impossible rather than everywhere.
   */
  it('offers a new revision only for a published form', async () => {
    const draft = await render()

    expect(draft.find('[data-testid="revise-purchase_requisition"]').exists()).toBe(false)

    rows = [summary({ status: 'PUBLISHED' })]

    const published = await render()

    expect(published.find('[data-testid="revise-purchase_requisition"]').exists()).toBe(true)
  })

  it('shows the refusal when a form cannot be deleted', async () => {
    deleteResponse = {
      status: 409,
      body: errorBody('FORM_IN_USE', 'PURCHASE_REQUISITION binds revision 1 of this form'),
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="delete-purchase_requisition"]').trigger('click')
    await flushPromises()
    await wrapper.find('[data-testid="confirm-action"]').trigger('click')
    await flushPromises()

    // The backend's own words: *which* type binds it is the thing the person
    // needs, and "could not delete" is not that.
    expect(wrapper.text()).toContain('binds revision 1 of this form')
  })
})
