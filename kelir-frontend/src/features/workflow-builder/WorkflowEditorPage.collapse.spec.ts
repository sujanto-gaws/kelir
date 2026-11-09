import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowEditorPage from './WorkflowEditorPage.vue'
import {
  installFakeBackend,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { press } from '@/lib/testing/keyboard'
import { useAuthStore } from '@/stores/auth'
import type { JwssDefinition } from '@/types/workflow'

/**
 * The editor's list above its threshold (#800, AC-38-3 to AC-38-5).
 *
 * Above 40 states or 112 transitions, either count alone and strictly above,
 * the list draws each state's header and draws its body (its fields, its task
 * and its transitions) only once the state is opened. Once drawn, a body stays
 * mounted, and closing the state only hides it. At or under the threshold the
 * list is drawn as it always was.
 *
 * **The threshold is written here and not imported**, as the end-to-end spec
 * writes the graph's ceiling: a threshold moved in the product fails the
 * boundary tests below instead of moving them with it.
 */
const THRESHOLD = { states: 40, transitions: 112 } as const

const ID = '0199a1a0-0000-7000-8000-00000000c800'
const blank = { template: '<div />' }

const CONDITION = { '==': [{ var: 'document.status' }, 'PENDING_APPROVAL'] }

/**
 * A chain of `states` states and `transitions` transitions. State 0 is the
 * initial state and has a task, the last state is final, and transition `at`
 * leaves state `at % (states - 1)`. Transition 0 carries a condition.
 */
function chain(states: number, transitions: number): JwssDefinition {
  const codes = Array.from({ length: states }, (_, at) => `STEP_${at}`)

  return {
    workflowKey: 'large_chain',
    version: '1.0.0',
    name: `${states} states`,
    initialState: codes[0],
    states: codes.map((code, at) => ({
      code,
      name: `Step ${at + 1}`,
      mapsToDocumentStatus:
        at === states - 1 ? ('COMPLETED' as const) : ('PENDING_APPROVAL' as const),
      ...(at === 0
        ? {
            task: {
              taskDefinitionKey: 'step_0',
              taskName: 'Approve at step 1',
              assignment: { assigneeType: 'ROLE' as const, roleCode: 'APPROVER' },
            },
          }
        : {}),
      ...(at === states - 1 ? { isFinal: true } : {}),
    })),
    transitions: Array.from({ length: transitions }, (_, at) => ({
      from: codes[at % (states - 1)],
      to: codes[(at % (states - 1)) + 1],
      action: 'APPROVE' as const,
      allowedBy: 'ROLE:APPROVER',
      ...(at === 0 ? { condition: CONDITION } : {}),
    })),
  }
}

function record(definition: JwssDefinition) {
  return {
    id: ID,
    workflowKey: definition.workflowKey,
    name: definition.name,
    description: null,
    version: 1,
    jwssVersion: '1.0.0',
    status: 'DRAFT',
    initialState: definition.initialState,
    definition,
    publishedAt: null,
    publishedBy: null,
    createdAt: '2026-11-09T00:00:00Z',
    updatedAt: '2026-11-09T00:00:00Z',
  }
}

// 300 headers, or 40 states drawn whole, render slower in jsdom than the
// default 5 s allows once the whole suite runs beside them.
describe('WorkflowEditorPage, the list above its threshold (#800)', { timeout: 30_000 }, () => {
  let backend: FakeBackendHandle
  let router: Router
  let wrapper: VueWrapper | null
  let stored: ReturnType<typeof record>
  let refuse: FakeReply | null

  beforeEach(() => {
    setActivePinia(createPinia())
    wrapper = null
    refuse = null
    stored = record(chain(3, 2))

    useAuthStore().$patch({
      user: {
        id: '0199a1a0-0000-7000-8000-0000000000u1',
        username: 'admin',
        displayName: 'Administrator',
        email: 'admin@example.test',
        roles: [],
        permissions: [
          'workflow:definition:read',
          'workflow:definition:create',
          'workflow:definition:update',
          'workflow:definition:publish',
        ],
      },
    })

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method !== 'get' && refuse) {
        const reply = refuse

        refuse = null

        return reply
      }

      if (request.method === 'put') {
        stored = record((request.body as { definition: JwssDefinition }).definition)
      }

      return { status: 200, body: { success: true, data: stored } }
    })

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

  async function render(definition: JwssDefinition, attach = false): Promise<VueWrapper> {
    stored = record(definition)

    await router.push(`/admin/workflows/${ID}`)
    await router.isReady()

    wrapper = mount(WorkflowEditorPage, {
      global: { plugins: [router] },
      attachTo: attach ? document.body : undefined,
    })
    await settle()

    return wrapper
  }

  /** The one way a test reaches into a body above the threshold: it opens the state first. */
  async function openState(page: VueWrapper, index: number): Promise<void> {
    const toggle = page.get(`[data-testid="open-state-${index}"]`)

    if (toggle.attributes('aria-expanded') !== 'true') {
      await toggle.trigger('click')
      await settle()
    }
  }

  function lastWrite(): JwssDefinition {
    const sent = backend.requests.filter((request) => request.method !== 'get')

    return (sent[sent.length - 1].body as { definition: JwssDefinition }).definition
  }

  async function save(page: VueWrapper): Promise<void> {
    await page.get('[data-testid="save-workflow"]').trigger('click')
    await settle()
  }

  describe('what is drawn (AC-38-3)', () => {
    it.each([
      ['31 states, the largest real draft', 31, 87],
      ['the threshold itself', THRESHOLD.states, THRESHOLD.transitions],
    ])(
      'draws every body at %s',
      async (_, states, transitions) => {
        const page = await render(chain(states, transitions))

        for (let at = 0; at < states; at += 1) {
          expect(page.find(`[data-testid="state-code-${at}"]`).exists(), `state ${at}`).toBe(true)
        }
        expect(page.findAll('[data-testid^="transition-"][data-testid$="-detail"]')).toHaveLength(
          transitions,
        )
        expect(page.find('[data-testid^="open-state-"]').exists()).toBe(false)
        // Every body of the threshold's 112 transitions, in jsdom: slower than the default 5 s.
      },
      30_000,
    )

    it.each([
      ['by states alone', THRESHOLD.states + 1, THRESHOLD.transitions],
      ['by transitions alone', THRESHOLD.states, THRESHOLD.transitions + 1],
    ])('draws only the headers one over the threshold %s', async (_, states, transitions) => {
      const page = await render(chain(states, transitions))
      const last = states - 1

      // Every header: the code, the badges, and Make initial, Move and Remove.
      expect(page.findAll('[data-testid^="open-state-"]')).toHaveLength(states)
      expect(page.get('[data-testid="state-0"]').text()).toContain('STEP_0')
      expect(page.get('[data-testid="state-0"]').text()).toContain('Initial')
      expect(page.get(`[data-testid="state-${last}"]`).text()).toContain('Final')
      expect(page.find('[data-testid="make-initial-0"]').exists()).toBe(false)
      expect(page.find(`[data-testid="make-initial-${last}"]`).exists()).toBe(true)
      expect(page.find(`[data-testid="remove-state-${last}"]`).exists()).toBe(true)
      expect(page.find(`[aria-label="Move State STEP_${last} up"]`).exists()).toBe(true)

      // No field, task panel or transition.
      expect(page.find('[data-testid^="state-code-"]').exists()).toBe(false)
      expect(page.find('[data-testid^="state-has-task-"]').exists()).toBe(false)
      expect(page.find('[data-testid^="transition-"]').exists()).toBe(false)
      expect(page.find('[data-testid^="add-transition-"]').exists()).toBe(false)
    })

    it('draws a state’s fields, task and transitions once it is opened', async () => {
      const page = await render(chain(41, 112))

      await openState(page, 0)

      const card = page.get('[data-testid="state-0"]')

      expect(card.find('[data-testid="state-code-0"]').exists()).toBe(true)
      expect(card.find('[data-testid="state-has-task-0"]').exists()).toBe(true)
      expect(card.find('[data-testid="transition-0"]').exists()).toBe(true)
      expect(page.find('[data-testid="state-code-1"]').exists()).toBe(false)
    })

    it('opens and closes a state without touching the draft', async () => {
      const definition = chain(41, 112)
      const page = await render(definition)

      await openState(page, 3)
      await page.get('[data-testid="open-state-3"]').trigger('click')
      await settle()
      await openState(page, 3)

      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')

      await save(page)

      expect(lastWrite()).toEqual(definition)
    })

    it('keeps a body mounted once opened: an unfilled operand survives a close and a reopen', async () => {
      const page = await render(chain(41, 112))

      await openState(page, 0)
      await page
        .get('[aria-label="Transition 1 condition, operand 2: kind"]')
        .setValue('comparison')
      expect(page.get('[data-testid="unfilled-expressions"]').text()).toContain('1 expression')

      await page.get('[data-testid="open-state-0"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="unfilled-expressions"]').text()).toContain('1 expression')
      expect(page.find('[data-testid="transition-0-detail"]').exists()).toBe(true)

      await openState(page, 0)

      expect(page.get('[data-testid="transition-0-detail"]').text()).toContain(
        'Choose what this operand is.',
      )
    })

    it('opens a state the author has just added, with its body drawn, and no other', async () => {
      const page = await render(chain(41, 112))

      await page.get('[data-testid="add-state-wait"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="open-state-41"]').attributes('aria-expanded')).toBe('true')
      expect(page.find('[data-testid="state-code-41"]').exists()).toBe(true)
      expect(page.findAll('[data-testid^="state-code-"]')).toHaveLength(1)
    })

    it('mounts no logic builder for a state that was never opened', async () => {
      const page = await render(chain(41, 112))

      expect(page.findAllComponents({ name: 'JsonLogicBuilder' })).toHaveLength(0)

      await openState(page, 1)

      const builders = page.findAllComponents({ name: 'JsonLogicBuilder' })

      expect(builders.length).toBeGreaterThan(0)
      for (const builder of builders) {
        expect(page.get('[data-testid="state-1"]').element.contains(builder.element)).toBe(true)
      }
    })

    it('fixes the choice when the revision loads: crossing the threshold while editing changes nothing', async () => {
      const under = await render(chain(THRESHOLD.states, THRESHOLD.transitions))

      await under.get('[data-testid="add-state-wait"]').trigger('click')
      await settle()

      expect(under.findAll('[data-testid^="state-code-"]')).toHaveLength(THRESHOLD.states + 1)
      expect(under.find('[data-testid^="open-state-"]').exists()).toBe(false)

      under.unmount()
      wrapper = null

      const over = await render(chain(THRESHOLD.states + 1, THRESHOLD.transitions))

      await over.get(`[data-testid="remove-state-${THRESHOLD.states}"]`).trigger('click')
      await settle()

      expect(over.findAll('[data-testid^="open-state-"]')).toHaveLength(THRESHOLD.states)
      expect(over.find('[data-testid^="state-code-"]').exists()).toBe(false)
    })
  })

  describe('a refusal stays reachable (AC-38-4)', () => {
    it('opens a collapsed state the refusal names, draws it where it is drawn today, and counts it', async () => {
      const page = await render(chain(300, 892), true)
      const save = page.get('[data-testid="save-workflow"]').element as HTMLButtonElement

      // Transition 549 leaves state 250 (549 % 299).
      refuse = validationReply(
        ['definition.states.250.name', 'A state needs a name.'],
        ['definition.transitions.549.allowedBy', '`GONE` is not a live role.'],
      )
      save.focus()
      save.click()
      await settle()

      const card = page.get('[data-testid="state-250"]')

      expect(card.get('[data-testid="open-state-250"]').attributes('aria-expanded')).toBe('true')
      expect(card.text()).toContain('A state needs a name.')
      expect(card.get('[data-testid="transition-549"]').text()).toContain('is not a live role')
      expect(card.get('[data-testid="state-refusals-250"]').text()).toBe('2 refusals')
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
      // Opened by the refusal, and focus did not move.
      expect(document.activeElement).toBe(save)
      expect(page.get('[data-testid="open-state-249"]').attributes('aria-expanded')).toBe('false')
    })

    it('opens the state an S6 message names in backticks', async () => {
      const page = await render(chain(300, 892))

      refuse = validationReply(['definition.states', 'State `STEP_120` cannot be reached.'])
      await save(page)

      expect(page.get('[data-testid="open-state-120"]').attributes('aria-expanded')).toBe('true')
      expect(page.get('[data-testid="state-120"]').text()).toContain('cannot be reached')
    })

    it('lists a refusal as unplaced while its state is closed', async () => {
      const page = await render(chain(300, 892))

      refuse = validationReply(['definition.states.7.name', 'A state needs a name.'])
      await save(page)
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)

      await page.get('[data-testid="open-state-7"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="unplaced-errors"]').text()).toContain('A state needs a name.')
      expect(page.get('[data-testid="state-refusals-7"]').text()).toBe('1 refusal')
    })
  })

  describe('accessibility (AC-38-5)', () => {
    it('toggles with a real button in the heading, named for the state, which controls a region that always exists', async () => {
      const page = await render(chain(41, 112), true)
      const toggle = page.get('[data-testid="open-state-0"]')
      const region = () => document.getElementById(toggle.attributes('aria-controls') as string)

      expect(toggle.element.tagName).toBe('BUTTON')
      expect(toggle.element.closest('h4')).not.toBeNull()
      expect(toggle.attributes('aria-label')).toBe('State STEP_0, Step 1, Initial')
      expect(toggle.attributes('aria-expanded')).toBe('false')
      expect(region()).not.toBeNull()
      expect(region()?.hidden).toBe(true)

      ;(toggle.element as HTMLButtonElement).focus()
      press('Enter')
      await settle()

      expect(toggle.attributes('aria-expanded')).toBe('true')
      expect(region()?.hidden).toBe(false)
      expect(document.activeElement).toBe(toggle.element)

      press(' ')
      await settle()

      expect(toggle.attributes('aria-expanded')).toBe('false')
      expect(region()?.hidden).toBe(true)
      expect(document.activeElement).toBe(toggle.element)
    })
  })
})
