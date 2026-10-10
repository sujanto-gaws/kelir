import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import type { AxiosResponse, InternalAxiosRequestConfig } from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowListPage from './WorkflowListPage.vue'
import { apiClient } from '@/api/client'
import {
  errorBody,
  installFakeBackend,
  pageBody,
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

/** The above, the deprecation (**D-108**), and the read its warning makes (#713). */
const DEPRECATOR = [...EVERY_PERMISSION, 'workflow:definition:deprecate', 'document-type:read']

const BOUND_TYPE = {
  id: 'dt-1',
  typeCode: 'PURCHASE_REQUEST',
  name: 'Purchase request',
  category: null,
  formId: null,
  status: 'ACTIVE',
  createdAt: '2026-10-09T00:00:00Z',
  updatedAt: '2026-10-09T00:00:00Z',
}

describe('WorkflowListPage', () => {
  let backend: FakeBackendHandle
  let rows: unknown[]
  let reply: FakeReply | null
  /** What the deprecation answers instead of succeeding, when set (#713). */
  let deprecateReply: FakeReply | null
  /** The document types the list's `workflowDefinitionId` filter answers. */
  let boundTypes: unknown[]
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    rows = [summary()]
    reply = null
    deprecateReply = null
    boundTypes = []

    backend = installFakeBackend((request: RecordedRequest) => {
      if (reply) {
        return reply
      }

      if (request.url === '/document-types') {
        return { status: 200, body: pageBody(boundTypes, { pageSize: 100 }) }
      }

      if (request.url.endsWith('/deprecation')) {
        if (deprecateReply) {
          return deprecateReply
        }

        const id = request.url.split('/')[3]

        rows = rows.map((row) =>
          (row as { id: string }).id === id ? { ...(row as object), status: 'DEPRECATED' } : row,
        )

        return { status: 200, body: { success: true, data: summary({ id, status: 'DEPRECATED' }) } }
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

  it('offers Deprecate on an ACTIVE row to a holder of workflow:definition:deprecate', async () => {
    // Inverted 2026-10-10 (#713): this asserted no Deprecate until #573’s
    // route existed. The route is #711’s, and the action is on the rows.
    rows = [summary({ status: 'ACTIVE' })]

    const wrapper = await render(DEPRECATOR)

    expect(wrapper.get('[data-testid="deprecate-purchase_approval-1"]').text()).toMatch(
      /Deprecate\b/,
    )
  })

  // --- The test-engineer campaign, 2026-10-10 ----------------------------------

  it('offers a new revision of a deprecated revision too, and Deprecate on the ACTIVE row only', async () => {
    // AC5: a published revision's way forward is a new revision, whether it is
    // ACTIVE or DEPRECATED; the editor's notice says the same of a deprecated one.
    // Inverted 2026-10-10 (#713): this asserted no deprecate on any row.
    rows = [
      summary({ status: 'ACTIVE' }),
      summary({ id: 'old', version: 2, status: 'DEPRECATED' }),
      summary({ id: 'draft', version: 3 }),
    ]

    const wrapper = await render(DEPRECATOR)

    expect(wrapper.find('[data-testid="revise-purchase_approval-1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="revise-purchase_approval-2"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="revise-purchase_approval-3"]').exists()).toBe(false)

    const deprecating = wrapper
      .findAll('button, a')
      .filter((control) => /deprecat|retire/i.test(control.text()))

    expect(deprecating.map((control) => control.attributes('data-testid'))).toEqual([
      'deprecate-purchase_approval-1',
    ])
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

  // --- Deprecating a revision (#713, plan 19 row 6b; D-101 B, D-108) ---------

  describe('deprecating a revision (#713)', () => {
    function confirmButton(wrapper: VueWrapper) {
      return wrapper.get('[data-testid="confirm-action"]')
    }

    function listReads(): RecordedRequest[] {
      return backend.requests.filter((request) => request.url === '/workflow/definitions')
    }

    async function ask(wrapper: VueWrapper): Promise<void> {
      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await flushPromises()
      await flushPromises()
    }

    async function confirm(wrapper: VueWrapper): Promise<void> {
      await confirmButton(wrapper).trigger('click')

      for (let round = 0; round < 4; round += 1) {
        await flushPromises()
      }
    }

    it('offers Deprecate only on an ACTIVE row, and only to a holder of deprecate', async () => {
      rows = [
        summary({ status: 'ACTIVE' }),
        summary({ id: 'draft', version: 2 }),
        summary({ id: 'old', version: 3, status: 'DEPRECATED' }),
      ]

      const holder = await render(DEPRECATOR)

      expect(holder.find('[data-testid="deprecate-purchase_approval-1"]').exists()).toBe(true)
      expect(holder.find('[data-testid="deprecate-purchase_approval-2"]').exists()).toBe(false)
      expect(holder.find('[data-testid="deprecate-purchase_approval-3"]').exists()).toBe(false)

      const other = await render(EVERY_PERMISSION)

      expect(other.find('[data-testid="deprecate-purchase_approval-1"]').exists()).toBe(false)
    })

    it('warns with the document types bound to that row’s revision, and deprecates once confirmed', async () => {
      rows = [summary({ status: 'ACTIVE' })]
      boundTypes = [BOUND_TYPE]

      const wrapper = await render(DEPRECATOR)

      await ask(wrapper)

      const read = backend.requests.find((request) => request.url === '/document-types')

      expect(read?.params).toEqual({ workflowDefinitionId: summary().id, pageSize: 100 })
      expect(wrapper.findAll('[data-testid="bound-type"]').map((item) => item.text())).toEqual([
        'Purchase request (PURCHASE_REQUEST)',
      ])
      expect(backend.requests.some((request) => request.method === 'post')).toBe(false)

      await confirm(wrapper)

      expect(
        backend.requests.filter((request) => request.method === 'post').map((r) => r.url),
      ).toEqual([`/workflow/definitions/${summary().id}/deprecation`])
      // Read again after it: the row says what it now is, and offers no Deprecate.
      expect(listReads()).toHaveLength(2)
      expect(wrapper.get('[data-testid="workflow-purchase_approval-1"]').text()).toContain(
        'DEPRECATED',
      )
      expect(wrapper.find('[data-testid="deprecate-purchase_approval-1"]').exists()).toBe(false)
      expect(wrapper.get('[data-testid="deprecation-notice"]').text()).toBe(
        'Revision 1 of purchase_approval is deprecated.',
      )
    })

    it('says when no document type is bound, and that it could not check without the read', async () => {
      rows = [summary({ status: 'ACTIVE' })]

      const reader = await render(DEPRECATOR)

      await ask(reader)

      expect(reader.find('[data-testid="bound-types-none"]').exists()).toBe(true)

      const blind = await render(
        DEPRECATOR.filter((permission) => permission !== 'document-type:read'),
      )

      backend.requests.length = 0
      await ask(blind)

      expect(blind.get('[data-testid="bound-types-unchecked"]').text()).toContain(
        'could not be checked',
      )
      expect(backend.requests).toEqual([])
      expect(confirmButton(blind).attributes('disabled')).toBeUndefined()
    })

    it('does nothing when cancelled', async () => {
      rows = [summary({ status: 'ACTIVE' })]

      const wrapper = await render(DEPRECATOR)

      await ask(wrapper)
      await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Cancel')!
        .trigger('click')
      await flushPromises()

      expect(wrapper.find('[data-testid="confirm-action"]').exists()).toBe(false)
      expect(backend.requests.some((request) => request.method === 'post')).toBe(false)
      expect(listReads()).toHaveLength(1)
    })

    it.each([
      ['already deprecated', 'revision 1 of `purchase_approval` is already deprecated'],
      [
        'a draft',
        'revision 1 of `purchase_approval` is a draft, and only a published revision can be deprecated; nothing routes to a draft, so delete it instead',
      ],
    ])('reads the list again when the route answers 409, %s', async (_case, message) => {
      rows = [summary({ status: 'ACTIVE' })]

      const wrapper = await render(DEPRECATOR)

      await ask(wrapper)
      rows = [summary({ status: 'DEPRECATED' })]
      deprecateReply = { status: 409, body: errorBody('CONFLICT', message) }
      await confirm(wrapper)

      expect(wrapper.get('[data-testid="deprecation-error"]').text()).toBe(message)
      expect(listReads()).toHaveLength(2)
      expect(wrapper.get('[data-testid="workflow-purchase_approval-1"]').text()).toContain(
        'DEPRECATED',
      )
    })

    it.each([
      ['403', 403, 'FORBIDDEN', 'Missing workflow:definition:deprecate'],
      ['404', 404, 'NOT_FOUND', 'Workflow definition not found'],
    ])('reads nothing again after a %s, and says why', async (_case, status, code, message) => {
      rows = [summary({ status: 'ACTIVE' })]

      const wrapper = await render(DEPRECATOR)

      await ask(wrapper)
      deprecateReply = { status, body: errorBody(code, message) }
      await confirm(wrapper)

      expect(wrapper.get('[data-testid="deprecation-error"]').text()).toBe(message)
      expect(listReads()).toHaveLength(1)
      expect(wrapper.get('[data-testid="workflow-purchase_approval-1"]').text()).toContain('ACTIVE')
    })

    it('holds the row’s actions while its deprecation is in flight', async () => {
      rows = [summary({ status: 'ACTIVE' })]

      const wrapper = await render(DEPRECATOR)

      await ask(wrapper)

      type Adapter = (config: InternalAxiosRequestConfig) => Promise<AxiosResponse>

      const fake = apiClient.defaults.adapter as Adapter
      let release: () => void = () => undefined
      const gate = new Promise<void>((resolve) => {
        release = resolve
      })

      apiClient.defaults.adapter = (async (config: InternalAxiosRequestConfig) => {
        if ((config.url ?? '').endsWith('/deprecation')) {
          await gate
        }

        return fake(config)
      }) as Adapter

      try {
        await confirmButton(wrapper).trigger('click')
        await flushPromises()

        expect(wrapper.find('[data-testid="confirm-action"]').exists()).toBe(false)
        expect(
          wrapper.get('[data-testid="deprecate-purchase_approval-1"]').attributes('disabled'),
        ).toBeDefined()
        expect(
          wrapper.get('[data-testid="revise-purchase_approval-1"]').attributes('disabled'),
        ).toBeDefined()

        await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
        await wrapper.get('[data-testid="revise-purchase_approval-1"]').trigger('click')
        await flushPromises()

        expect(wrapper.find('[data-testid="confirm-action"]').exists()).toBe(false)

        release()

        for (let round = 0; round < 4; round += 1) {
          await flushPromises()
        }
      } finally {
        apiClient.defaults.adapter = fake
      }

      expect(backend.requests.filter((request) => request.method === 'post')).toHaveLength(1)
      expect(wrapper.find('[data-testid="deprecate-purchase_approval-1"]').exists()).toBe(false)
    })
  })

  // --- The test-engineer campaign on #713, 2026-10-10 --------------------------
  //
  // Two rows at once, a stale read, and the no-read path carried through to
  // the request. Seen to fail: the ticket dropped from `readBound` (*shows the
  // reopened row’s types…*), and `busy()` holding every row while any one is
  // deprecating (*leaves another row’s actions free…*). One defect was pinned
  // with `it.fails`, and is fixed (2026-10-10): see *holds a second row’s
  // actions…*.

  describe('deprecating, adversarially (#713)', () => {
    type Adapter = (config: InternalAxiosRequestConfig) => Promise<AxiosResponse>

    const FIRST = summary({ status: 'ACTIVE' })
    const SECOND = summary({ id: 'second', version: 2, status: 'ACTIVE' })

    function namedType(id: string, name: string) {
      return { ...BOUND_TYPE, id, typeCode: id.toUpperCase(), name }
    }

    /** Holds the first request `matches` accepts until the returned function is called. */
    function hold(matches: (config: InternalAxiosRequestConfig) => boolean): () => Promise<void> {
      const fake = apiClient.defaults.adapter as Adapter
      let release: () => void = () => undefined
      const gate = new Promise<void>((resolve) => {
        release = resolve
      })

      apiClient.defaults.adapter = (async (config: InternalAxiosRequestConfig) => {
        if (!matches(config)) {
          return fake(config)
        }

        apiClient.defaults.adapter = fake
        await gate

        return fake(config)
      }) as Adapter

      return async () => {
        release()

        for (let round = 0; round < 4; round += 1) {
          await flushPromises()
        }
      }
    }

    function deprecationOf(id: string) {
      return (config: InternalAxiosRequestConfig) =>
        (config.url ?? '') === `/workflow/definitions/${id}/deprecation`
    }

    function readFor(id: string) {
      return (config: InternalAxiosRequestConfig) =>
        (config.params as Record<string, unknown> | undefined)?.workflowDefinitionId === id
    }

    function disabled(wrapper: VueWrapper, testid: string): boolean {
      return wrapper.get(`[data-testid="${testid}"]`).attributes('disabled') !== undefined
    }

    async function settle(): Promise<void> {
      for (let round = 0; round < 4; round += 1) {
        await flushPromises()
      }
    }

    function posts(): string[] {
      return backend.requests
        .filter((request) => request.method === 'post')
        .map((request) => request.url)
    }

    it('leaves another row’s actions free while one row’s deprecation is in flight', async () => {
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      const release = hold(deprecationOf(FIRST.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(disabled(wrapper, 'deprecate-purchase_approval-1')).toBe(true)
      expect(disabled(wrapper, 'revise-purchase_approval-1')).toBe(true)
      expect(disabled(wrapper, 'deprecate-purchase_approval-2')).toBe(false)
      expect(disabled(wrapper, 'revise-purchase_approval-2')).toBe(false)

      // The other row's warning opens and reads its own revision's types.
      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()

      expect(
        backend.requests
          .filter((request) => request.url === '/document-types')
          .map((r) => r.params),
      ).toEqual([
        { workflowDefinitionId: FIRST.id, pageSize: 100 },
        { workflowDefinitionId: SECOND.id, pageSize: 100 },
      ])

      await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Cancel')!
        .trigger('click')
      await release()

      expect(posts()).toEqual([`/workflow/definitions/${FIRST.id}/deprecation`])
      expect(wrapper.get('[data-testid="workflow-purchase_approval-1"]').text()).toContain(
        'DEPRECATED',
      )
      expect(wrapper.get('[data-testid="workflow-purchase_approval-2"]').text()).toContain('ACTIVE')
    })

    // DEFECT (test-engineer, 2026-10-10, PR #715): `deprecatingId` held one
    // row. Deprecate row 1, then row 2 while row 1 is still out: when row 1
    // returned, its `finally` cleared the id, and row 2's actions came back
    // while row 2's own deprecation was still in flight. Fixed the same day:
    // the list holds every in-flight row in a set, and this is a plain `it`.
    it('holds a second row’s actions until its own deprecation returns', async () => {
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      const releaseFirst = hold(deprecationOf(FIRST.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()

      const releaseSecond = hold(deprecationOf(SECOND.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      try {
        expect(disabled(wrapper, 'deprecate-purchase_approval-2')).toBe(true)

        await releaseFirst()

        // The fake backend records a request once it is answered: row 2's is
        // still held.
        expect(posts()).toEqual([`/workflow/definitions/${FIRST.id}/deprecation`])
        expect(wrapper.get('[data-testid="workflow-purchase_approval-2"]').text()).toContain(
          'ACTIVE',
        )
        // Row 2 is still out; its actions must still be held.
        expect(disabled(wrapper, 'deprecate-purchase_approval-2')).toBe(true)
        expect(disabled(wrapper, 'revise-purchase_approval-2')).toBe(true)
      } finally {
        await releaseSecond()
      }
    })

    it('shows the latest answer when two deprecations overlap, and frees each row on its own', async () => {
      // PR #715's campaign: two overlapping deprecations' notice and error
      // could stand together, each about a different row. The latest answer
      // replaces the one before; both name their revision, and the rows read
      // again show what became of the other.
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      const releaseFirst = hold(deprecationOf(FIRST.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()
      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()

      const releaseSecond = hold(deprecationOf(SECOND.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      try {
        await releaseFirst()

        expect(wrapper.get('[data-testid="deprecation-notice"]').text()).toBe(
          'Revision 1 of purchase_approval is deprecated.',
        )
        expect(wrapper.find('[data-testid="deprecation-error"]').exists()).toBe(false)

        deprecateReply = {
          status: 409,
          body: errorBody('CONFLICT', 'revision 2 of `purchase_approval` is already deprecated'),
        }
      } finally {
        await releaseSecond()
      }

      expect(wrapper.get('[data-testid="deprecation-error"]').text()).toBe(
        'revision 2 of `purchase_approval` is already deprecated',
      )
      expect(wrapper.find('[data-testid="deprecation-notice"]').exists()).toBe(false)
      expect(wrapper.get('[data-testid="workflow-purchase_approval-1"]').text()).toContain(
        'DEPRECATED',
      )
      expect(disabled(wrapper, 'revise-purchase_approval-2')).toBe(false)
    })

    it('shows a later success over an earlier refusal', async () => {
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      const releaseFirst = hold(deprecationOf(FIRST.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()
      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()

      const releaseSecond = hold(deprecationOf(SECOND.id))

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      try {
        deprecateReply = {
          status: 403,
          body: errorBody('FORBIDDEN', 'Missing workflow:definition:deprecate'),
        }
        await releaseFirst()

        expect(wrapper.get('[data-testid="deprecation-error"]').text()).toBe(
          'Missing workflow:definition:deprecate',
        )

        deprecateReply = null
      } finally {
        await releaseSecond()
      }

      expect(wrapper.get('[data-testid="deprecation-notice"]').text()).toBe(
        'Revision 2 of purchase_approval is deprecated.',
      )
      expect(wrapper.find('[data-testid="deprecation-error"]').exists()).toBe(false)
    })

    it('shows the reopened row’s types, not a late answer for the row first asked', async () => {
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)
      const releaseFirst = hold(readFor(FIRST.id))

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      expect(wrapper.find('[data-testid="bound-types-checking"]').exists()).toBe(true)

      await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Cancel')!
        .trigger('click')

      boundTypes = [namedType('dt-second', 'Bound to the second')]
      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()

      // The first row's read answers last, and with its own types.
      boundTypes = [namedType('dt-first', 'Bound to the first')]
      await releaseFirst()

      expect(wrapper.get('[role="dialog"]').text()).toContain(
        'Deprecate revision 2 of purchase_approval?',
      )
      expect(wrapper.findAll('[data-testid="bound-type"]').map((item) => item.text())).toEqual([
        'Bound to the second (DT-SECOND)',
      ])

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(posts()).toEqual([`/workflow/definitions/${SECOND.id}/deprecation`])
    })

    it('deprecates from the warning that could not check, once confirmed', async () => {
      rows = [FIRST]

      const wrapper = await render(
        DEPRECATOR.filter((permission) => permission !== 'document-type:read'),
      )

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()

      expect(wrapper.find('[data-testid="bound-types-unchecked"]').exists()).toBe(true)

      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(backend.requests.some((request) => request.url === '/document-types')).toBe(false)
      expect(posts()).toEqual([`/workflow/definitions/${FIRST.id}/deprecation`])
      expect(wrapper.get('[data-testid="deprecation-notice"]').text()).toBe(
        'Revision 1 of purchase_approval is deprecated.',
      )
    })

    it('clears a refusal when the next deprecation succeeds', async () => {
      rows = [FIRST, SECOND]

      const wrapper = await render(DEPRECATOR)

      await wrapper.get('[data-testid="deprecate-purchase_approval-1"]').trigger('click')
      await settle()
      deprecateReply = {
        status: 409,
        body: errorBody('CONFLICT', 'revision 1 of `purchase_approval` is already deprecated'),
      }
      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(wrapper.find('[data-testid="deprecation-error"]').exists()).toBe(true)

      deprecateReply = null
      await wrapper.get('[data-testid="deprecate-purchase_approval-2"]').trigger('click')
      await settle()
      await wrapper.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(wrapper.find('[data-testid="deprecation-error"]').exists()).toBe(false)
      expect(wrapper.get('[data-testid="deprecation-notice"]').text()).toBe(
        'Revision 2 of purchase_approval is deprecated.',
      )
    })
  })
})
