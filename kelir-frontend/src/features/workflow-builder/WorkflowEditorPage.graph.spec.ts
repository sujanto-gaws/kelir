import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Router } from 'vue-router'

import type { FakeBackendHandle, FakeReply, RecordedRequest } from '@/lib/testing/fake-backend'
import type { JwssDefinition } from '@/types/workflow'

/**
 * The editor's graph view (#687, **D-95** A): a tab beside the list, loaded
 * off the first-load path, showing the draft and writing nothing back.
 *
 * **A file of its own because the graph is a module mock, and a module mock is
 * per file**, as `DashboardPage.chart-failure.spec.ts` is for the chart. The
 * graph is replaced by a stub that records the definition it is handed; what
 * it draws is `WorkflowGraph.spec.ts`'s.
 *
 * **Every test loads the page afresh** (`vi.resetModules`, then `vi.doMock`),
 * so the graph's module is imported again, and the factory below counts each
 * import. That is how *lazy* is observed: a page that imported the graph
 * statically, or loaded it before the tab was opened, imports it at mount, and
 * the count says so. (`vi.mock`'s factory runs once per file, whatever
 * `resetModules` does, so it could not count.) Whether the libraries stay out
 * of the first-load chunks is `scripts/check-bundle-split.mjs`'s, against the
 * build.
 */

const graph = {
  /** How many times the graph's module was imported. */
  imports: 0,
  /** Whether the next import rejects, as a chunk rotated away by a deploy does. */
  fail: false,
}

function mockGraph(): void {
  vi.doMock('./WorkflowGraph.vue', async () => {
    graph.imports += 1

    if (graph.fail) {
      throw new Error('the graph chunk is gone')
    }

    const { defineComponent, h } = await import('vue')

    return {
      // `defineAsyncComponent` unwraps `default` only from something marked as
      // a module, as the dashboard's chart mock says.
      __esModule: true,
      default: defineComponent({
        name: 'WorkflowGraph',
        props: { definition: { type: Object, required: true } },
        setup: (props) => () =>
          h('div', { 'data-testid': 'graph-stub' }, (props.definition as JwssDefinition).name),
      }),
    }
  })
}

const ID = '0199a1a0-0000-7000-8000-00000000wf01'
const blank = { template: '<div />' }
const EVERY_PERMISSION = [
  'workflow:definition:read',
  'workflow:definition:create',
  'workflow:definition:update',
  'workflow:definition:publish',
]

function jwss(): JwssDefinition {
  return {
    workflowKey: 'purchase_approval',
    version: '1.0.0',
    name: 'Purchase approval',
    initialState: 'MANAGER_APPROVAL',
    states: [
      {
        code: 'MANAGER_APPROVAL',
        name: 'Manager approval',
        mapsToDocumentStatus: 'PENDING_APPROVAL',
        task: {
          taskDefinitionKey: 'manager_approval',
          taskName: 'Approve the purchase',
          assignment: { assigneeType: 'ROLE', roleCode: 'APPROVER' },
        },
      },
      { code: 'COMPLETED', name: 'Completed', mapsToDocumentStatus: 'COMPLETED', isFinal: true },
      { code: 'REJECTED', name: 'Rejected', mapsToDocumentStatus: 'REJECTED', isFinal: true },
    ],
    transitions: [
      {
        from: 'MANAGER_APPROVAL',
        to: 'COMPLETED',
        action: 'APPROVE',
        allowedBy: 'ROLE:APPROVER',
        condition: { '==': [{ var: 'document.status' }, 'PENDING_APPROVAL'] },
      },
      {
        from: 'MANAGER_APPROVAL',
        to: 'REJECTED',
        action: 'REJECT',
        allowedBy: 'ROLE:APPROVER',
        requiresComment: true,
      },
    ],
  }
}

function record(definition: JwssDefinition = jwss()) {
  return {
    id: ID,
    workflowKey: 'purchase_approval',
    name: definition.name,
    description: null,
    version: 1,
    jwssVersion: '1.0.0',
    status: 'DRAFT',
    initialState: definition.initialState,
    definition,
    publishedAt: null,
    publishedBy: null,
    createdAt: '2026-10-10T00:00:00Z',
    updatedAt: '2026-10-10T00:00:00Z',
  }
}

/** The keys of every object anywhere in a value. */
function keysIn(value: unknown, into = new Set<string>()): Set<string> {
  if (value !== null && typeof value === 'object') {
    for (const [key, field] of Object.entries(value)) {
      if (!Array.isArray(value)) {
        into.add(key)
      }

      keysIn(field, into)
    }
  }

  return into
}

describe('WorkflowEditorPage, the graph view', () => {
  let backend: FakeBackendHandle
  let router: Router
  let wrapper: VueWrapper | null
  let stored: ReturnType<typeof record>
  /** A refusal the next write answers instead of succeeding. */
  let refuse: FakeReply | null
  /** The refusal the refusal test scripts, built from the fresh registry. */
  let refusal: FakeReply
  let keyboard: typeof import('@/lib/testing/keyboard')
  let violations: (document: unknown) => string[]

  beforeEach(async () => {
    vi.resetModules()
    mockGraph()
    graph.imports = 0
    graph.fail = false
    wrapper = null
    stored = record()
    refuse = null

    const { createPinia, setActivePinia } = await import('pinia')
    const { createMemoryHistory, createRouter } = await import('vue-router')
    const { installFakeBackend, validationReply } = await import('@/lib/testing/fake-backend')
    const { useAuthStore } = await import('@/stores/auth')

    keyboard = await import('@/lib/testing/keyboard')
    violations = (await import('@/lib/testing/jwss-meta-schema')).jwssViolations

    setActivePinia(createPinia())
    useAuthStore().$patch({
      user: {
        id: '0199a1a0-0000-7000-8000-0000000000u1',
        username: 'admin',
        displayName: 'Administrator',
        email: 'admin@example.test',
        roles: [],
        permissions: EVERY_PERMISSION,
      },
    })

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'put') {
        if (refuse) {
          const reply = refuse

          refuse = null

          return reply
        }

        stored = record((request.body as { definition: JwssDefinition }).definition)
      }

      return { status: 200, body: { success: true, data: stored } }
    })

    // Kept for the refusal test; made here so it comes from the fresh registry.
    refusal = validationReply(['definition.states.0.name', 'A state needs a name.'])

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/admin/workflows/new', name: 'admin-workflow-new', component: blank },
        { path: '/admin/workflows/:id', name: 'admin-workflow-editor', component: blank },
      ],
    })
  })

  afterEach(() => {
    wrapper?.unmount()
    backend.restore()
    document.body.innerHTML = ''
  })

  async function settle(): Promise<void> {
    for (let round = 0; round < 6; round += 1) {
      await flushPromises()
    }
  }

  async function render(attach = false): Promise<VueWrapper> {
    const { default: WorkflowEditorPage } = await import('./WorkflowEditorPage.vue')

    await router.push(`/admin/workflows/${ID}`)
    await router.isReady()

    wrapper = mount(WorkflowEditorPage, {
      global: { plugins: [router] },
      attachTo: attach ? document.body : undefined,
    })
    await settle()

    return wrapper
  }

  async function openGraph(page: VueWrapper): Promise<void> {
    await page.get('[data-testid="view-graph"]').trigger('click')
    await settle()
  }

  function listPanel(page: VueWrapper): HTMLElement {
    return page.get('#workflow-view-list').element as HTMLElement
  }

  function shownDefinition(page: VueWrapper): JwssDefinition {
    return page.getComponent({ name: 'WorkflowGraph' }).props('definition') as JwssDefinition
  }

  describe('off the first-load path', () => {
    it('does not import the graph until the Graph tab is opened, and then once', async () => {
      const page = await render()

      expect(graph.imports).toBe(0)
      expect(page.find('[data-testid="graph-stub"]').exists()).toBe(false)

      await openGraph(page)

      expect(graph.imports).toBe(1)
      expect(page.find('[data-testid="graph-stub"]').exists()).toBe(true)

      // Back to the list and to the graph again: the module is already here.
      await page.get('[data-testid="view-list"]').trigger('click')
      await openGraph(page)

      expect(graph.imports).toBe(1)
    })

    it('says so, and keeps the list, when the graph cannot be loaded', async () => {
      graph.fail = true

      const page = await render()

      await openGraph(page)

      expect(page.get('[data-testid="workflow-graph-error"]').text()).toBe(
        'The graph could not be drawn. The list holds the same workflow.',
      )

      await page.get('[data-testid="view-list"]').trigger('click')

      expect(listPanel(page).hidden).toBe(false)
      expect(page.findAll('[data-testid^="state-"][aria-label]')).toHaveLength(3)
    })
  })

  describe('a view beside the list', () => {
    it('opens on the list, with the graph tab not selected', async () => {
      const page = await render()

      expect(page.get('[data-testid="view-list"]').attributes('aria-selected')).toBe('true')
      expect(page.get('[data-testid="view-graph"]').attributes('aria-selected')).toBe('false')
      expect(listPanel(page).hidden).toBe(false)
    })

    it('shows the graph in place of the list, keeping the list mounted and hidden', async () => {
      const page = await render()

      await openGraph(page)

      expect(page.get('[data-testid="view-graph"]').attributes('aria-selected')).toBe('true')
      expect(page.get('#workflow-view-graph').attributes('hidden')).toBeUndefined()
      // Hidden, not removed: an operand being typed in a logic builder survives.
      expect(listPanel(page).hidden).toBe(true)
      expect(page.findAll('[data-testid^="state-"][aria-label]')).toHaveLength(3)
    })

    it('ties each tab to its panel for assistive technology', async () => {
      const page = await render()

      for (const view of ['list', 'graph']) {
        const tab = page.get(`[data-testid="view-${view}"]`)

        expect(tab.attributes('role')).toBe('tab')
        expect(tab.attributes('aria-controls')).toBe(`workflow-view-${view}`)
        expect(page.get(`#workflow-view-${view}`).attributes('aria-labelledby')).toBe(
          tab.attributes('id'),
        )
      }
    })

    it('is reached and switched by keyboard alone', async () => {
      const page = await render(true)

      keyboard.tabTo((element) => element.dataset.testid === 'view-graph')
      keyboard.press('Enter')
      await settle()

      expect(page.find('[data-testid="graph-stub"]').exists()).toBe(true)

      keyboard.tabTo((element) => element.dataset.testid === 'view-list')
      keyboard.press('Enter')
      await settle()

      expect(listPanel(page).hidden).toBe(false)
      expect(page.find('[data-testid="graph-stub"]').exists()).toBe(false)
    })

    it('is offered on a published revision too, which the list shows read-only', async () => {
      stored = { ...record(), status: 'ACTIVE' }

      const page = await render()

      await openGraph(page)

      expect(page.find('[data-testid="graph-stub"]').exists()).toBe(true)
    })
  })

  describe('the current draft, laid out again when it changes', () => {
    it('hands the graph the draft itself, unsaved edits included', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Purchase approval, edited')
      await openGraph(page)

      expect(shownDefinition(page).name).toBe('Purchase approval, edited')
      expect(shownDefinition(page).states).toHaveLength(3)
    })

    it('follows an undo while the graph is shown', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Edited')
      await openGraph(page)
      expect(shownDefinition(page).name).toBe('Edited')

      await page.get('[data-testid="undo"]').trigger('click')
      await settle()

      expect(shownDefinition(page).name).toBe('Purchase approval')
    })

    it('follows what the server stored after a save while the graph is shown', async () => {
      const page = await render()

      await openGraph(page)
      // The server answers with a definition it changed: the graph shows that.
      backend.restore()
      backend = (await import('@/lib/testing/fake-backend')).installFakeBackend(() => ({
        status: 200,
        body: { success: true, data: record({ ...jwss(), name: 'As stored' }) },
      }))
      await page.get('[data-testid="save-workflow"]').trigger('click')
      await settle()

      expect(shownDefinition(page).name).toBe('As stored')
    })
  })

  describe('no layout is written back', () => {
    it('saves exactly the definition it loaded after the graph was drawn: no position, no key added', async () => {
      const page = await render()

      await openGraph(page)
      await page.get('[data-testid="view-list"]').trigger('click')
      await page.get('[data-testid="save-workflow"]').trigger('click')
      await settle()

      const sent = backend.requests.find((request) => request.method === 'put')!.body as {
        definition: JwssDefinition
      }

      expect(sent.definition).toEqual(jwss())
      expect(keysIn(sent.definition)).toEqual(keysIn(jwss()))
      expect(violations(sent.definition)).toEqual([])
    })

    it('leaves nothing to save: opening the graph does not make the draft dirty', async () => {
      const page = await render()

      await openGraph(page)

      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()
    })
  })

  describe('a refusal is drawn where it happened', () => {
    it('brings the list back when a save from the graph view is refused at a state', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Edited')
      await openGraph(page)
      refuse = refusal
      await page.get('[data-testid="save-workflow"]').trigger('click')
      await settle()

      expect(listPanel(page).hidden).toBe(false)
      expect(page.get('[data-testid="view-list"]').attributes('aria-selected')).toBe('true')
      expect(page.get('[data-testid="state-0"]').text()).toContain('A state needs a name.')
    })
  })
})
