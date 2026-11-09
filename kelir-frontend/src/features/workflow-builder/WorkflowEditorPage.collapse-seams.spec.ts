import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowEditorPage from './WorkflowEditorPage.vue'
import {
  installFakeBackend,
  pageBody,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { press, tabStops, tabTo } from '@/lib/testing/keyboard'
import { useAuthStore } from '@/stores/auth'
import type { JwssDefinition } from '@/types/workflow'

/**
 * The editor's list above its threshold, at its seams (#800, row 38's
 * `test-engineer` campaign, AC-38-3 to AC-38-6).
 *
 * `WorkflowEditorPage.collapse.spec.ts` is the builder's suite. This one holds
 * what the campaign's mutants got past it: a refusal at a transition alone, a
 * transition's detail while its state is closed, an unattached transition, the
 * open state kept by position across save, publish and deprecate, undo and
 * redo on an opened card, the toggle's accessible name and keyboard reach, the
 * header's count at or under the threshold, and the 300-state payload.
 *
 * **The threshold is written here and not imported**, as in the builder's suite.
 */
const THRESHOLD = { states: 40, transitions: 112 } as const

const ID = '0199a1a0-0000-7000-8000-00000000c801'
const blank = { template: '<div />' }

const CONDITION = { '==': [{ var: 'document.status' }, 'PENDING_APPROVAL'] }

/** The builder's chain: transition `at` leaves state `at % (states - 1)`. */
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

type Status = 'DRAFT' | 'ACTIVE' | 'DEPRECATED'

function record(definition: JwssDefinition, status: Status = 'DRAFT') {
  return {
    id: ID,
    workflowKey: definition.workflowKey,
    name: definition.name,
    description: null,
    version: 1,
    jwssVersion: '1.0.0',
    status,
    initialState: definition.initialState,
    definition,
    publishedAt: null,
    publishedBy: null,
    createdAt: '2026-11-09T00:00:00Z',
    updatedAt: '2026-11-09T00:00:00Z',
  }
}

describe(
  'WorkflowEditorPage, the list above its threshold at its seams (#800)',
  { timeout: 60_000 },
  () => {
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
            'workflow:definition:deprecate',
            'document-type:read',
          ],
        },
      })

      backend = installFakeBackend((request: RecordedRequest) => {
        if (request.url === '/document-types') {
          return { status: 200, body: pageBody([], { pageSize: 100 }) }
        }

        if (request.method !== 'get' && refuse) {
          const reply = refuse

          refuse = null

          return reply
        }

        if (request.method === 'put') {
          stored = record((request.body as { definition: JwssDefinition }).definition)
        }

        if (request.url.endsWith('/publication')) {
          stored = { ...stored, status: 'ACTIVE' }
        }

        if (request.url.endsWith('/deprecation')) {
          stored = { ...stored, status: 'DEPRECATED' }
        }

        // A stored copy, so the page never holds the object the fake keeps.
        return { status: 200, body: { success: true, data: structuredClone(stored) } }
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

    async function render(
      definition: JwssDefinition,
      { attach = false, status = 'DRAFT' as Status } = {},
    ): Promise<VueWrapper> {
      stored = record(structuredClone(definition), status)

      await router.push(`/admin/workflows/${ID}`)
      await router.isReady()

      wrapper = mount(WorkflowEditorPage, {
        global: { plugins: [router] },
        attachTo: attach ? document.body : undefined,
      })
      await settle()

      return wrapper
    }

    function toggleOf(page: VueWrapper, index: number) {
      return page.get(`[data-testid="open-state-${index}"]`)
    }

    function isExpanded(page: VueWrapper, index: number): boolean {
      return toggleOf(page, index).attributes('aria-expanded') === 'true'
    }

    async function openState(page: VueWrapper, index: number): Promise<void> {
      if (!isExpanded(page, index)) {
        await toggleOf(page, index).trigger('click')
        await settle()
      }
    }

    async function closeState(page: VueWrapper, index: number): Promise<void> {
      if (isExpanded(page, index)) {
        await toggleOf(page, index).trigger('click')
        await settle()
      }
    }

    async function click(page: VueWrapper, testId: string): Promise<void> {
      await page.get(`[data-testid="${testId}"]`).trigger('click')
      await settle()
    }

    function writes(): RecordedRequest[] {
      return backend.requests.filter((request) => request.method !== 'get')
    }

    function lastWrite(): JwssDefinition {
      const sent = writes().filter((request) => request.method === 'put')

      return (sent[sent.length - 1].body as { definition: JwssDefinition }).definition
    }

    /** The body region a toggle controls, read through its `aria-controls`. */
    function regionOf(page: VueWrapper, index: number): HTMLElement | null {
      const id = toggleOf(page, index).attributes('aria-controls') as string
      const found = page.find(`[id="${id}"]`)

      return found.exists() ? (found.element as HTMLElement) : null
    }

    describe('a refusal stays reachable (AC-38-4)', () => {
      it('opens the state a transition-only refusal is about, and only that state, with no focus move', async () => {
        const page = await render(chain(300, 892), { attach: true })
        const save = page.get('[data-testid="save-workflow"]').element as HTMLButtonElement

        // Transition 549 leaves state 250 (549 % 299) and enters state 251.
        refuse = validationReply([
          'definition.transitions.549.allowedBy',
          '`GONE` is not a live role.',
        ])
        save.focus()
        save.click()
        await settle()

        expect(isExpanded(page, 250)).toBe(true)
        // Not the state the transition enters, nor any other.
        expect(isExpanded(page, 251)).toBe(false)
        expect(page.findAll('[data-testid^="open-state-"][aria-expanded="true"]')).toHaveLength(1)
        expect(
          page.get('[data-testid="state-250"]').get('[data-testid="transition-549"]').text(),
        ).toContain('is not a live role')
        expect(page.get('[data-testid="state-refusals-250"]').text()).toBe('1 refusal')
        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
        expect(document.activeElement).toBe(save)
      })

      it('lists a transition’s refusal as unplaced while the state that draws it is closed', async () => {
        const page = await render(chain(300, 892))

        refuse = validationReply([
          'definition.transitions.549.allowedBy',
          '`GONE` is not a live role.',
        ])
        await click(page, 'save-workflow')
        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)

        await closeState(page, 250)

        expect(page.get('[data-testid="unplaced-errors"]').text()).toContain('is not a live role')
        // The header still says it, as text.
        expect(page.get('[data-testid="state-refusals-250"]').text()).toBe('1 refusal')

        await openState(page, 250)

        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
      })

      it('draws an unattached transition’s refusal in the unattached table, opening nothing', async () => {
        const definition = chain(300, 892)

        definition.transitions[891] = { ...definition.transitions[891], from: 'GHOST' }

        const page = await render(definition)

        refuse = validationReply([
          'definition.transitions.891.allowedBy',
          '`GONE` is not a live role.',
        ])
        await click(page, 'save-workflow')

        expect(page.get('[data-testid="unattached-transitions"]').text()).toContain(
          'is not a live role',
        )
        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
        expect(page.findAll('[data-testid^="open-state-"][aria-expanded="true"]')).toHaveLength(0)
      })

      it('opens the state an S7 message at the transitions list names in backticks', async () => {
        const page = await render(chain(300, 892))

        refuse = validationReply([
          'definition.transitions',
          'State `STEP_77` has more than one fallback transition for APPROVE.',
        ])
        await click(page, 'save-workflow')

        expect(isExpanded(page, 77)).toBe(true)
        expect(page.get('[data-testid="state-77"]').text()).toContain('more than one fallback')
        expect(page.get('[data-testid="state-refusals-77"]').text()).toBe('1 refusal')
      })

      // DEFECT found by the campaign, red on #933's head: the auto-open
      // watches `fieldErrors`, which `clearField` also replaces, so the first
      // edit of any refused field re-opens every other refused state the
      // author has closed. AC-38-4 opens a state on a refused save or publish.
      it('does not reopen a closed refused state when another refused field is edited', async () => {
        const page = await render(chain(300, 892))

        refuse = validationReply(
          ['definition.states.250.name', 'A state needs a name.'],
          ['definition.states.7.name', 'A state needs a name.'],
        )
        await click(page, 'save-workflow')
        await closeState(page, 250)
        await page.get('[data-testid="state-name-7"]').setValue('Fixed')
        await settle()

        expect(isExpanded(page, 250)).toBe(false)
      })

      it('opens the state a refused publish names', async () => {
        const page = await render(chain(300, 892))

        refuse = validationReply(['definition.states.180.name', 'A state needs a name.'])
        await click(page, 'publish-workflow')

        expect(isExpanded(page, 180)).toBe(true)
        expect(page.get('[data-testid="state-180"]').text()).toContain('A state needs a name.')
        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
      })

      it('says no count and folds nothing at the threshold itself, where every body shows', async () => {
        const page = await render(chain(THRESHOLD.states, THRESHOLD.transitions), { attach: true })

        // Transition 39 leaves state 0 (39 % 39).
        refuse = validationReply(
          ['definition.states.5.name', 'A state needs a name.'],
          ['definition.transitions.39.allowedBy', '`GONE` is not a live role.'],
        )
        await click(page, 'save-workflow')

        expect(page.find('[data-testid^="state-refusals-"]').exists()).toBe(false)
        expect(page.find('[data-testid^="open-state-"]').exists()).toBe(false)
        expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
        // Drawn and shown: not merely mounted under a hidden region.
        const name = page.get('[data-testid="state-name-5"]').element as HTMLElement

        expect(name.closest('[hidden]')).toBeNull()
        expect(page.get('[data-testid="state-5"]').text()).toContain('A state needs a name.')
      })
    })

    describe('the toggle (AC-38-5)', () => {
      it('names the state with its Final badge and its refusals', async () => {
        const page = await render(chain(300, 892))

        expect(toggleOf(page, 299).attributes('aria-label')).toBe('State STEP_299, Step 300, Final')
        expect(toggleOf(page, 250).attributes('aria-label')).toBe('State STEP_250, Step 251')

        refuse = validationReply(
          ['definition.states.250.name', 'A state needs a name.'],
          ['definition.transitions.549.allowedBy', '`GONE` is not a live role.'],
        )
        await click(page, 'save-workflow')

        expect(toggleOf(page, 250).attributes('aria-label')).toBe(
          'State STEP_250, Step 251, 2 refusals',
        )
      })

      it('is reached by Tab; a closed body’s controls are not, once drawn or never drawn', async () => {
        const page = await render(chain(41, 112), { attach: true })

        ;(page.get('[data-testid="workflow-key"]').element as HTMLElement).focus()
        const reached = tabTo((element) => element.dataset.testid === 'open-state-2')

        expect(reached.tagName).toBe('BUTTON')

        press('Enter')
        await settle()

        const code = page.get('[data-testid="state-code-2"]').element as HTMLElement

        expect(isExpanded(page, 2)).toBe(true)
        expect(document.activeElement).toBe(reached)
        expect(tabStops()).toContain(code)
        // Never drawn: state 3's body has no control at all.
        expect(page.find('[data-testid="state-code-3"]').exists()).toBe(false)

        press(' ')
        await settle()

        // Closed, the body is still mounted, and out of the tab order.
        expect(document.activeElement).toBe(reached)
        expect(page.find('[data-testid="state-code-2"]').exists()).toBe(true)
        expect(tabStops()).not.toContain(code)
        expect(regionOf(page, 2)?.hidden).toBe(true)
      })

      it('keeps every label’s `for` and every `aria-describedby` inside an opened card resolving', async () => {
        const page = await render(chain(300, 892))

        refuse = validationReply(['definition.states.250.name', 'A state needs a name.'])
        await click(page, 'save-workflow')

        const card = page.get('[data-testid="state-250"]').element
        const labels = Array.from(card.querySelectorAll('label[for]'))
        const described = Array.from(card.querySelectorAll('[aria-describedby]'))

        expect(labels.length).toBeGreaterThan(0)
        for (const label of labels) {
          const target = card.querySelector(`[id="${label.getAttribute('for')}"]`)

          expect(target, label.textContent ?? '').not.toBeNull()
        }
        // The refused name field points at its message.
        expect(described.length).toBeGreaterThan(0)
        for (const element of described) {
          for (const id of (element.getAttribute('aria-describedby') ?? '').split(/\s+/)) {
            expect(card.querySelector(`[id="${id}"]`), id).not.toBeNull()
          }
        }
      })

      it('controls a region that exists for every state, open, closed, or never opened', async () => {
        const page = await render(chain(41, 112), { attach: true })

        await openState(page, 4)
        await openState(page, 9)
        await closeState(page, 9)

        const ids = new Set<string>()

        for (let at = 0; at < 41; at += 1) {
          const region = regionOf(page, at)

          expect(region, `state ${at}`).not.toBeNull()
          expect(page.get(`[data-testid="state-${at}"]`).element.contains(region)).toBe(true)
          expect(region?.hidden, `state ${at}`).toBe(at !== 4)
          ids.add(toggleOf(page, at).attributes('aria-controls') as string)
        }
        expect(ids.size).toBe(41)
      })
    })

    describe('an open state across a save, a publish and a deprecation (AC-38-6)', () => {
      it('stays open by its position after a save; a closed one stays closed and mounted', async () => {
        const page = await render(chain(300, 892))

        await openState(page, 250)
        await openState(page, 3)
        await closeState(page, 3)
        await page.get('[data-testid="state-name-250"]').setValue('Renamed 251')
        await click(page, 'save-workflow')

        expect(page.get('[data-testid="notice"]').text()).toContain('Saved.')
        expect(isExpanded(page, 250)).toBe(true)
        expect((page.get('[data-testid="state-name-250"]').element as HTMLInputElement).value).toBe(
          'Renamed 251',
        )
        expect(regionOf(page, 250)?.hidden).toBe(false)
        expect(isExpanded(page, 3)).toBe(false)
        expect(page.find('[data-testid="state-code-3"]').exists()).toBe(true)
        expect(page.findAll('[data-testid^="open-state-"][aria-expanded="true"]')).toHaveLength(1)
      })

      it('stays open by its position after a publish', async () => {
        const page = await render(chain(300, 892))

        await openState(page, 250)
        await click(page, 'publish-workflow')

        expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
        expect(isExpanded(page, 250)).toBe(true)
        expect(page.find('[data-testid="state-name-250"]').exists()).toBe(true)
      })

      it('stays open by its position after a deprecation', async () => {
        const page = await render(chain(300, 892), { status: 'ACTIVE' })

        await openState(page, 250)
        await click(page, 'deprecate-workflow')
        await click(page, 'confirm-action')

        expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
        expect(isExpanded(page, 250)).toBe(true)
        expect(page.find('[data-testid="state-name-250"]').exists()).toBe(true)
      })
    })

    describe('the draft is the draft (AC-38-3, AC-38-6)', () => {
      it('saves an unopened 300-state draft deep-equal to the one loaded; one edit changes only its state', async () => {
        const definition = chain(300, 892)
        const page = await render(definition)

        await openState(page, 17)
        await closeState(page, 17)
        await click(page, 'save-workflow')

        expect(lastWrite()).toEqual(definition)

        await openState(page, 250)
        await page.get('[data-testid="state-name-250"]').setValue('Renamed 251')
        await click(page, 'save-workflow')

        const expected = structuredClone(definition)

        expected.states[250] = { ...expected.states[250], name: 'Renamed 251' }
        expect(lastWrite()).toEqual(expected)
      })

      it('undoes and redoes an edit on an opened card, which stays open', async () => {
        const page = await render(chain(300, 892))

        await openState(page, 250)
        await page.get('[data-testid="state-name-250"]').setValue('Renamed 251')
        await click(page, 'undo')

        expect(isExpanded(page, 250)).toBe(true)
        expect((page.get('[data-testid="state-name-250"]').element as HTMLInputElement).value).toBe(
          'Step 251',
        )

        await click(page, 'redo')

        expect(isExpanded(page, 250)).toBe(true)
        expect((page.get('[data-testid="state-name-250"]').element as HTMLInputElement).value).toBe(
          'Renamed 251',
        )
      })

      it('keeps the opened state open as it moves: a state above is removed, then the removal undone', async () => {
        const page = await render(chain(300, 892))

        await openState(page, 250)
        await click(page, 'remove-state-200')

        // STEP_250 is now at 249, and still open; what is now at 250 is not.
        expect(page.get('[data-testid="state-249"]').text()).toContain('STEP_250')
        expect(isExpanded(page, 249)).toBe(true)
        expect(isExpanded(page, 250)).toBe(false)

        await click(page, 'undo')

        expect(isExpanded(page, 250)).toBe(true)
        expect(isExpanded(page, 249)).toBe(false)
        expect((page.get('[data-testid="state-code-250"]').element as HTMLInputElement).value).toBe(
          'STEP_250',
        )
      })
    })

    describe('the threshold is fixed at load, by transitions alone too (AC-38-3)', () => {
      it('draws everything still when an edit takes the threshold’s transitions one over', async () => {
        const page = await render(chain(THRESHOLD.states, THRESHOLD.transitions))

        await click(page, 'add-transition-1')

        expect(page.findAll('[data-testid^="transition-"][data-testid$="-detail"]')).toHaveLength(
          THRESHOLD.transitions + 1,
        )
        expect(page.find('[data-testid^="open-state-"]').exists()).toBe(false)
      })

      it('stays collapsed when an edit takes a draft one over by transitions back to the threshold', async () => {
        const page = await render(chain(THRESHOLD.states, THRESHOLD.transitions + 1))

        await openState(page, 1)
        await page.get('[aria-label="Remove Transition 2"]').trigger('click')
        await settle()

        expect(page.findAll('[data-testid^="open-state-"]')).toHaveLength(THRESHOLD.states)
        expect(page.find('[data-testid="state-code-2"]').exists()).toBe(false)
      })
    })
  },
)
