import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowListPage from './WorkflowListPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The workflow list (#426 AC1, AC5): every revision a row, each action offered
 * only to a caller who holds its permission, and a published revision's only
 * way forward a new revision.
 *
 * Seen red, 2026-10-09: *New revision* offered on a draft row (*offers a new
 * revision of a published revision only…*).
 */

const blank = { template: '<div />' }

function summary(overrides: Record<string, unknown> = {}) {
  return {
    id: '0199a1a0-0000-7000-8000-00000000wf01',
    workflowKey: 'purchase_approval',
    name: 'Purchase approval',
    version: 1,
    jwssVersion: '1.0.0',
    status: 'DRAFT',
    initialState: 'MANAGER_APPROVAL',
    createdAt: '2026-10-09T00:00:00Z',
    updatedAt: '2026-10-09T00:00:00Z',
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

const EVERY_PERMISSION = ['workflow:definition:read', 'workflow:definition:create']

describe('WorkflowListPage', () => {
  let backend: FakeBackendHandle
  let rows: unknown[]
  let reply: FakeReply | null
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [summary()]
    reply = null

    backend = installFakeBackend((request: RecordedRequest) => {
      if (reply) {
        return reply
      }

      if (request.method === 'post') {
        return {
          status: 201,
          body: { success: true, data: summary({ id: 'next-revision', version: 2 }) },
        }
      }

      return {
        status: 200,
        body: { success: true, data: rows, meta: { page: 1, pageSize: 20, total: rows.length } },
      }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/workflows', name: 'admin-workflows', component: blank },
        { path: '/admin/workflows/new', name: 'admin-workflow-new', component: blank },
        { path: '/admin/workflows/:id', name: 'admin-workflow-editor', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

    await router.push('/admin/workflows')
    await router.isReady()

    const wrapper = mount(WorkflowListPage, { global: { plugins: [router] } })

    for (let round = 0; round < 5; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('lists every revision as its own row', async () => {
    rows = [summary({ status: 'ACTIVE' }), summary({ id: 'second', version: 2 })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="workflow-purchase_approval-1"]').text()).toContain('ACTIVE')
    expect(wrapper.find('[data-testid="workflow-purchase_approval-2"]').text()).toContain('DRAFT')
  })

  it('says so when there are none, and when the list cannot be read', async () => {
    rows = []

    const empty = await render()

    expect(empty.text()).toContain('No workflows yet.')

    reply = { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:read') }

    const refused = await render()

    expect(refused.get('[data-testid="workflows-error"]').text()).toContain(
      'Missing workflow:definition:read',
    )
  })

  it('opens a revision in the editor', async () => {
    const wrapper = await render()

    await wrapper.get('[data-testid="open-purchase_approval-1"]').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('admin-workflow-editor')
    expect(router.currentRoute.value.params.id).toBe('0199a1a0-0000-7000-8000-00000000wf01')
  })

  it('starts a new workflow for a caller who may create one, and for no one else', async () => {
    const creator = await render()

    await creator.get('[data-testid="new-workflow"]').trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('admin-workflow-new')

    const reader = await render(['workflow:definition:read'])

    expect(reader.find('[data-testid="new-workflow"]').exists()).toBe(false)
    expect(reader.find('[data-testid="open-purchase_approval-1"]').exists()).toBe(true)
  })

  it('offers a new revision of a published revision only, and opens it (AC5)', async () => {
    rows = [summary({ status: 'ACTIVE' }), summary({ id: 'draft-two', version: 2 })]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="revise-purchase_approval-2"]').exists()).toBe(false)

    await wrapper.get('[data-testid="revise-purchase_approval-1"]').trigger('click')
    await flushPromises()
    await flushPromises()

    const post = backend.requests.find((request) => request.method === 'post')

    expect(post?.url).toBe('/workflow/definitions/0199a1a0-0000-7000-8000-00000000wf01/revisions')
    expect(router.currentRoute.value.params.id).toBe('next-revision')
  })

  it('offers no new revision to a caller who cannot create one', async () => {
    rows = [summary({ status: 'ACTIVE' })]

    const wrapper = await render(['workflow:definition:read'])

    expect(wrapper.find('[data-testid="revise-purchase_approval-1"]').exists()).toBe(false)
  })

  it('shows why a new revision was refused', async () => {
    rows = [summary({ status: 'ACTIVE' })]

    const wrapper = await render()

    reply = { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:create') }
    await wrapper.get('[data-testid="revise-purchase_approval-1"]').trigger('click')
    await flushPromises()
    await flushPromises()

    expect(wrapper.get('[data-testid="revision-error"]').text()).toContain(
      'Missing workflow:definition:create',
    )
    expect(router.currentRoute.value.name).toBe('admin-workflows')
  })

  it('offers no deprecate action until #573’s route exists', async () => {
    rows = [summary({ status: 'ACTIVE' })]

    const wrapper = await render()

    expect(wrapper.text()).not.toMatch(/Deprecate\b/)
  })

  // --- The test-engineer campaign, 2026-10-10 ----------------------------------

  it('offers a new revision of a deprecated revision too, and no deprecate on any row', async () => {
    // AC5: a published revision's way forward is a new revision, whether it is
    // ACTIVE or DEPRECATED; the editor's notice says the same of a deprecated one.
    rows = [
      summary({ status: 'ACTIVE' }),
      summary({ id: 'old', version: 2, status: 'DEPRECATED' }),
      summary({ id: 'draft', version: 3 }),
    ]

    const wrapper = await render()

    expect(wrapper.find('[data-testid="revise-purchase_approval-1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="revise-purchase_approval-2"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="revise-purchase_approval-3"]').exists()).toBe(false)

    for (const control of wrapper.findAll('button, a')) {
      expect(control.text()).not.toMatch(/deprecat|retire/i)
    }

    expect(backend.requests.some((request) => /deprecat/i.test(request.url))).toBe(false)
  })

  it('pages through the server, not through what it already has', async () => {
    reply = {
      status: 200,
      body: { success: true, data: [summary()], meta: { page: 1, pageSize: 20, total: 45 } },
    }

    const wrapper = await render()

    await wrapper.get('[data-testid="next-page"]').trigger('click')
    await flushPromises()

    const reads = backend.requests.filter((request) => request.url === '/workflow/definitions')

    expect(reads).toHaveLength(2)
    expect(reads[1].params).toMatchObject({ page: 2 })
  })
})
