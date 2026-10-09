import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowEditorPage from './WorkflowEditorPage.vue'
import {
  errorBody,
  installFakeBackend,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { jwssViolations } from '@/lib/testing/jwss-meta-schema'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'
import type { JwssDefinition } from '@/types/workflow'

/**
 * The workflow editor (#426 AC1–AC5, and the two criteria row 6 inherits from
 * #695).
 *
 * **What is asserted is what only this screen can get wrong.** Whether a
 * definition is runnable is the backend's question, and
 * `kelir-backend/tests/workflow_*.rs` hold it; here the server's verdict is a
 * scripted 422 and the assertion is *where it is drawn*. What the screen
 * writes is checked against the meta-schema file itself (AC2).
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Nineteen mutations over row 6's code, run 2026-10-09, all nineteen red. Seen
 * red, 2026-10-09, in this file:
 *
 * | Mutation | Reddened |
 * |---|---|
 * | Transition rows keyed by their content, not their stable key | *keeps focus and unfilled operands…* |
 * | `replaceTransition` copies every transition it did not change | *keeps focus and unfilled operands…* |
 * | S6/S7 list details not moved to the state they name | *puts S6’s list-wide refusal on the state it names…* |
 * | Every path counted as placed | *lists what no input shows, and only that* |
 * | The `requiresComment` message not drawn at its checkbox | *…S12’s refusal lands on its checkbox* |
 * | The checkbox disabled on an `AUTO` edge | *…S12’s refusal lands on its checkbox* |
 * | A cleared `requiresComment` written as `false` | *writes the flag on the edge it was set on…* |
 * | `readOnly` ignores a published status | *opens read-only, says why, and offers a new revision* |
 * | Publish does not save unsaved edits first | *saves unsaved edits first…* |
 * | A condition is offered `document.amount` | *offers a condition only the variables the engine builds…* |
 * | A rename does not take its transitions | *renames a state and its transitions and the initial state follow* |
 * | A save keeps the local draft, not what was stored | *shows what the server stored after a save…* |
 * | An edit does not clear its field’s message; a row move keeps stale ones | *clears a field’s message when it is edited…* (two mutations) |
 *
 * The other six are in `useWorkflowDraft.spec.ts`, `workflowVerdict.spec.ts`,
 * `assignmentRule.spec.ts`, `jwssRegistry.spec.ts` and `WorkflowListPage.spec.ts`.
 */

const ID = '0199a1a0-0000-7000-8000-00000000wf01'
const NEXT_ID = '0199a1a0-0000-7000-8000-00000000wf02'
const blank = { template: '<div />' }

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
          // Stored and not executed (FR-WF-010): kept, with no field.
          escalation: { afterHours: 72, assignment: { assigneeType: 'ROLE', roleCode: 'HEAD' } },
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
        actions: [{ hook: 'after_workflow_transition', handler: 'core:continue_always' }],
      },
      {
        from: 'MANAGER_APPROVAL',
        to: 'REJECTED',
        action: 'REJECT',
        allowedBy: 'ROLE:APPROVER',
        requiresComment: true,
        condition: { '!=': [{ var: 'document.status' }, 'DRAFT'] },
      },
    ],
    variables: [{ key: 'threshold', dataType: 'NUMBER' }],
  }
}

function record(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    workflowKey: 'purchase_approval',
    name: 'Purchase approval',
    description: null,
    version: 1,
    jwssVersion: '1.0.0',
    status: 'DRAFT',
    initialState: 'MANAGER_APPROVAL',
    definition: jwss(),
    publishedAt: null,
    publishedBy: null,
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

const EVERY_PERMISSION = [
  'workflow:definition:read',
  'workflow:definition:create',
  'workflow:definition:update',
  'workflow:definition:publish',
]

describe('WorkflowEditorPage', () => {
  let backend: FakeBackendHandle
  let stored: ReturnType<typeof record>
  /** What the next write answers instead of succeeding, when set. */
  let refuse: FakeReply | null
  let router: Router
  let wrapper: VueWrapper | null

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    stored = record()
    refuse = null
    wrapper = null

    backend = installFakeBackend((request: RecordedRequest) => {
      const body = request.body as { definition?: JwssDefinition; name?: string }

      if (request.method !== 'get' && refuse) {
        const reply = refuse

        refuse = null

        return reply
      }

      if (request.method === 'post' && request.url === '/workflow/definitions') {
        stored = record({ name: body.name, definition: body.definition })

        return { status: 201, body: { success: true, data: stored } }
      }

      if (request.method === 'put') {
        // The server stores what it was sent; the screen must show that.
        stored = { ...stored, name: body.name ?? stored.name, definition: body.definition! }

        return { status: 200, body: { success: true, data: stored } }
      }

      if (request.url.endsWith('/publication')) {
        stored = { ...stored, status: 'ACTIVE' }

        return { status: 200, body: { success: true, data: stored } }
      }

      if (request.url.endsWith('/revisions')) {
        stored = record({ id: NEXT_ID, version: 2 })

        return { status: 201, body: { success: true, data: stored } }
      }

      return { status: 200, body: { success: true, data: stored } }
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

  afterEach(() => {
    wrapper?.unmount()
    backend.restore()
  })

  async function render(
    options: { permissions?: string[]; path?: string; attach?: boolean } = {},
  ): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(options.permissions ?? EVERY_PERMISSION) })

    await router.push(options.path ?? `/admin/workflows/${ID}`)
    await router.isReady()

    wrapper = mount(WorkflowEditorPage, {
      global: { plugins: [router] },
      attachTo: options.attach ? document.body : undefined,
    })

    await settle()

    return wrapper
  }

  async function settle(): Promise<void> {
    for (let round = 0; round < 6; round += 1) {
      await flushPromises()
    }
  }

  function writes(): RecordedRequest[] {
    return backend.requests.filter((request) => request.method !== 'get')
  }

  function lastWrite(): { name?: string; workflowKey?: string; definition: JwssDefinition } {
    return writes()[writes().length - 1].body as { definition: JwssDefinition }
  }

  function byLabel(page: VueWrapper, label: string) {
    return page.get(`[aria-label="${label}"]`)
  }

  async function save(page: VueWrapper): Promise<void> {
    await page.get('[data-testid="save-workflow"]').trigger('click')
    await settle()
  }

  describe('editing a draft (AC1, AC2)', () => {
    it('draws each state with the transitions out of it under it', async () => {
      const page = await render()

      expect(page.get('[data-testid="workflow-name"]').element).toHaveProperty(
        'value',
        'Purchase approval',
      )
      expect(page.findAll('[data-testid^="state-"][aria-label]')).toHaveLength(3)

      const first = page.get('[data-testid="state-0"]')

      expect(first.find('[data-testid="transition-0"]').exists()).toBe(true)
      expect(first.find('[data-testid="transition-1"]').exists()).toBe(true)
      // A final state has none, and is not offered one.
      expect(page.get('[data-testid="state-1"]').text()).toContain(
        'A final state has no transitions out of it.',
      )
      expect(page.find('[data-testid="add-transition-1"]').exists()).toBe(false)
    })

    it('writes JWSS the meta-schema accepts, and leaves what it does not edit as it was', async () => {
      const page = await render()

      await page.get('[data-testid="state-name-0"]').setValue('Line manager approval')
      await byLabel(page, 'Transition 2: action').setValue('RETURN')
      await save(page)

      const sent = lastWrite()

      expect(writes()[0].method).toBe('put')
      expect(writes()[0].url).toBe(`/workflow/definitions/${ID}`)
      expect(sent.name).toBe('Purchase approval')
      expect(jwssViolations(sent.definition)).toEqual([])

      const expected = jwss()

      expected.states[0].name = 'Line manager approval'
      expected.transitions[1].action = 'RETURN'
      // The shorthand, the escalation, the hook entry and the variable come
      // back exactly as they were loaded.
      expect(sent.definition).toEqual(expected)
    })

    it('writes what an added state and transition make, and the meta-schema accepts it', async () => {
      const page = await render()

      await page.get('[data-testid="add-state-wait"]').trigger('click')
      await page.get('[data-testid="add-transition-0"]').trigger('click')
      await byLabel(page, 'Transition 3: to').setValue('WAITING')
      await byLabel(page, 'Transition 3 allowed by: role code').setValue('APPROVER')
      await page.get('[data-testid="requires-comment-2"]').setValue(true)
      await save(page)

      const sent = lastWrite().definition

      expect(sent.states[sent.states.length - 1]).toEqual({
        code: 'WAITING',
        name: 'Returned',
        mapsToDocumentStatus: 'RETURNED',
      })
      expect(sent.transitions[sent.transitions.length - 1]).toEqual({
        from: 'MANAGER_APPROVAL',
        to: 'WAITING',
        action: 'APPROVE',
        allowedBy: { assigneeType: 'ROLE', roleCode: 'APPROVER' },
        requiresComment: true,
      })
      expect(jwssViolations(sent)).toEqual([])
    })

    it('renames a state and its transitions and the initial state follow', async () => {
      const page = await render()
      const code = page.get('[data-testid="state-code-0"]')

      await code.setValue('LINE_APPROVAL')
      await code.trigger('change')
      await save(page)

      const sent = lastWrite().definition

      expect(sent.initialState).toBe('LINE_APPROVAL')
      expect(sent.transitions.map((transition) => transition.from)).toEqual([
        'LINE_APPROVAL',
        'LINE_APPROVAL',
      ])
    })

    it('shows what the server stored after a save, and undoes and redoes an edit', async () => {
      const page = await render()
      const name = page.get('[data-testid="workflow-name"]')

      await name.setValue('Renamed')
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Save and publish')

      await page.get('[data-testid="undo"]').trigger('click')
      await settle()
      expect((name.element as HTMLInputElement).value).toBe('Purchase approval')
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')

      await page.get('[data-testid="redo"]').trigger('click')
      await settle()
      expect((name.element as HTMLInputElement).value).toBe('Renamed')

      await save(page)

      expect(page.get('[data-testid="notice"]').text()).toContain('what the server stored')
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()
    })

    it('offers a condition only the variables the engine builds, and the declared ones', async () => {
      const page = await render()
      const variable = byLabel(page, 'Transition 1 condition, operand 1: variable')
      const offered = variable.findAll('option').map((option) => option.attributes('value'))

      expect(offered).toEqual([
        'document.status',
        'document.documentTypeId',
        'document.documentNumber',
        'actor.userId',
        'variables.threshold',
      ])
    })
  })

  describe('the server’s verdict, at what it names (AC3)', () => {
    it('places each detail at the state or transition field it addresses', async () => {
      const page = await render()

      refuse = validationReply(
        ['definition.states.0.task.assignment.roleCode', '`GONE` is not a live role'],
        ['definition.transitions.0.to', '`NOWHERE` is not a declared state'],
        ['name', 'name is required'],
        ['definition.initialState', '`X` is not a declared state'],
      )
      await save(page)

      expect(
        page
          .get('[data-testid="state-0"]')
          .get('[aria-label$="task assigned to: role code"]')
          .attributes('aria-invalid'),
      ).toBe('true')
      expect(page.get('[data-testid="state-0"]').text()).toContain('`GONE` is not a live role')
      expect(page.get('[data-testid="transition-0"]').text()).toContain(
        '`NOWHERE` is not a declared state',
      )
      expect(page.get('[data-testid="workflow-name-error"]').text()).toBe('name is required')
      expect(page.get('[data-testid="initial-state-error"]').text()).toContain('`X`')
      // Every detail had a place, so nothing is listed besides.
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('puts S6’s list-wide refusal on the state it names, and S9’s on the list', async () => {
      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.states',
            rule: 'S6',
            code: 'UNREACHABLE_STATE',
            message: '`REJECTED` cannot be reached from the initial state',
          },
          {
            path: 'definition.states',
            rule: 'S9',
            code: 'NO_TERMINAL_STATUS',
            message: 'no state maps to COMPLETED or CANCELLED',
          },
        ]),
      }
      await save(page)

      expect(page.get('[data-testid="state-2-messages"]').text()).toContain(
        '`REJECTED` cannot be reached',
      )
      expect(page.find('[data-testid="state-0-messages"]').exists()).toBe(false)
      expect(page.get('[data-testid="states-error"]').text()).toContain(
        'no state maps to COMPLETED',
      )
    })

    it('lists what no input shows, and only that', async () => {
      const page = await render()

      refuse = validationReply(
        ['definition.variables.0.source', 'uses an operator the registry does not approve'],
        ['definition.transitions.1.guards.0.handler', 'no handler of that name'],
      )
      await save(page)

      const unplaced = page.get('[data-testid="unplaced-errors"]')

      expect(unplaced.text()).toContain(
        'definition.variables.0.source: uses an operator the registry does not approve',
      )
      // A detail under a drawn transition is that transition's, even for a
      // field it has no input for.
      expect(unplaced.text()).not.toContain('no handler')
      expect(page.get('[data-testid="transition-1-messages"]').text()).toContain('no handler')
    })

    it('clears a field’s message when it is edited, and every message when a row moves', async () => {
      const page = await render()

      refuse = validationReply(
        ['definition.transitions.0.to', 'not declared'],
        ['definition.states.1.name', 'too long'],
      )
      await save(page)

      await byLabel(page, 'Transition 1: to').setValue('REJECTED')
      expect(page.get('[data-testid="transition-0"]').text()).not.toContain('not declared')
      expect(page.get('[data-testid="state-1"]').text()).toContain('too long')

      await byLabel(page, 'Move State COMPLETED down').trigger('click')
      expect(page.text()).not.toContain('too long')
    })

    it('shows a refusal that is not a validation on the form, verbatim', async () => {
      const page = await render()

      refuse = { status: 409, body: errorBody('CONFLICT', 'revision 1 is not a draft') }
      await save(page)

      expect(page.get('[data-testid="form-error"]').text()).toBe('revision 1 is not a draft')
    })
  })

  describe('requiresComment, per transition (AC4)', () => {
    it('is authored per edge, on an AUTO edge too, and S12’s refusal lands on its checkbox', async () => {
      const page = await render()

      await byLabel(page, 'Transition 2: action').setValue('AUTO')

      const box = page.get('[data-testid="requires-comment-1"]')

      expect((box.element as HTMLInputElement).checked).toBe(true)
      expect(box.attributes('disabled')).toBeUndefined()

      refuse = validationReply(
        ['definition.transitions.1.requiresComment', 'false was expected'],
        ['definition.transitions.1', '{"required":["allowedBy"]} is not allowed'],
      )
      await save(page)

      expect(page.get('[data-testid="requires-comment-error-1"]').text()).toBe('false was expected')
      expect(page.get('[data-testid="requires-comment-1"]').attributes('aria-invalid')).toBe('true')
      expect(page.get('[data-testid="transition-1-messages"]').text()).toContain('allowedBy')
      expect(page.find('[data-testid="requires-comment-error-0"]').exists()).toBe(false)
    })

    it('writes the flag on the edge it was set on, and leaves it out when cleared', async () => {
      const page = await render()

      await page.get('[data-testid="requires-comment-0"]').setValue(true)
      await page.get('[data-testid="requires-comment-1"]').setValue(false)
      await save(page)

      const [approve, reject] = lastWrite().definition.transitions

      expect(approve.requiresComment).toBe(true)
      expect('requiresComment' in reject).toBe(false)
    })
  })

  describe('a published revision (AC5)', () => {
    it('opens read-only, says why, and offers a new revision', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render()

      expect(page.get('[data-testid="published-notice"]').text()).toContain(
        'published and cannot be edited',
      )
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(false)
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
      expect(page.get('[data-testid="workflow-name"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="requires-comment-0"]').attributes('disabled')).toBeDefined()
      expect(
        byLabel(page, 'Transition 1 condition, operand 1: variable').attributes('disabled'),
      ).toBeDefined()

      await page.get('[data-testid="new-revision"]').trigger('click')
      await settle()

      expect(writes()[writes().length - 1].url).toBe(`/workflow/definitions/${ID}/revisions`)
      expect(router.currentRoute.value.params.id).toBe(NEXT_ID)
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(page.get('[data-testid="workflow-name"]').attributes('disabled')).toBeUndefined()
    })

    it('says a deprecated revision is not bindable, and offers no deprecate yet', async () => {
      stored = record({ status: 'DEPRECATED' })

      const page = await render()

      expect(page.get('[data-testid="published-notice"]').text()).toContain('deprecated')
      // #573's route is not merged; there is no button for it rather than a dead one.
      expect(page.text()).not.toMatch(/Deprecate\b/)
    })

    it('offers no new revision to a caller who cannot create one', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: ['workflow:definition:read'] })

      expect(page.find('[data-testid="new-revision"]').exists()).toBe(false)
    })
  })

  describe('publishing', () => {
    it('saves unsaved edits first, so what is published is what is shown', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        `put /workflow/definitions/${ID}`,
        `post /workflow/definitions/${ID}/publication`,
      ])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="published-notice"]').exists()).toBe(true)
    })

    it('does not publish when the save before it is refused', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('')
      refuse = validationReply(['name', 'name is required'])
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(writes()).toHaveLength(1)
      expect(page.get('[data-testid="workflow-name-error"]').text()).toBe('name is required')
    })

    it('shows a role publish finds dead at the role field it names (D-111)', async () => {
      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.transitions.1.allowedBy',
            rule: 'liveRole',
            code: 'ROLE_NOT_LIVE',
            message: '`APPROVER` is not a live role in this tenant',
          },
        ]),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="transition-1"]').text()).toContain('is not a live role')
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
    })
  })

  describe('a new workflow', () => {
    it('is created from the starter, and opens as the revision it became', async () => {
      const page = await render({ path: '/admin/workflows/new' })

      expect(backend.requests).toHaveLength(0)
      expect(page.text()).toContain('New workflow')

      await page.get('[data-testid="workflow-key"]').setValue('purchase_approval')
      await page.get('[data-testid="workflow-name"]').setValue('Purchase approval')
      await byLabel(page, 'State PENDING_APPROVAL task assigned to: role code').setValue('APPROVER')
      await byLabel(page, 'Transition 1 allowed by: role code').setValue('APPROVER')
      await byLabel(page, 'Transition 2 allowed by: role code').setValue('APPROVER')
      await save(page)

      const sent = lastWrite()

      expect(writes()[0].url).toBe('/workflow/definitions')
      expect(sent.workflowKey).toBe('purchase_approval')
      expect(sent.definition.workflowKey).toBe('purchase_approval')
      expect(jwssViolations(sent.definition)).toEqual([])
      expect(router.currentRoute.value.name).toBe('admin-workflow-editor')
      expect(router.currentRoute.value.params.id).toBe(ID)
      // The key is fixed once saved.
      expect(page.get('[data-testid="workflow-key"]').attributes('disabled')).toBeDefined()
      // And the page did not read back over what it was just given.
      expect(backend.requests.filter((request) => request.method === 'get')).toHaveLength(0)
    })
  })

  describe('permissions', () => {
    it('lets a reader read a draft and change nothing', async () => {
      const page = await render({ permissions: ['workflow:definition:read'] })

      expect(page.find('[data-testid="read-only-notice"]').exists()).toBe(true)
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(false)
      expect(page.get('[data-testid="state-name-0"]').attributes('disabled')).toBeDefined()
    })

    it('offers publish only to a publisher', async () => {
      const page = await render({
        permissions: ['workflow:definition:read', 'workflow:definition:update'],
      })

      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(true)
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
    })
  })

  describe('the logic builder through this editor’s v-model (inherited from #695)', () => {
    /** Types into the focused field a character at a time, letting the page re-render each. */
    async function typeSlowly(text: string): Promise<HTMLInputElement> {
      const field = document.activeElement as HTMLInputElement

      for (const character of text) {
        field.value += character
        field.dispatchEvent(new Event('input', { bubbles: true }))
        await settle()

        // Focus stays on the very element typed into: nothing remounted it.
        expect(document.activeElement).toBe(field)
      }

      return field
    }

    it('keeps focus and unfilled operands while the host stores each keystroke', async () => {
      const page = await render({ attach: true })

      // An unfilled operand in one stored condition: its second operand made a
      // comparison with nothing in it yet, so nothing is emitted for it and
      // the draft still holds the condition as loaded.
      await byLabel(page, 'Transition 2 condition, operand 2: kind').setValue('comparison')
      expect(page.get('[data-testid="transition-1-detail"]').text()).toContain(
        'Choose what this operand is.',
      )
      expect(page.get('[data-testid="unfilled-expressions"]').text()).toContain('1 expression')

      // Typing into another, complete condition: each keystroke is emitted,
      // stored through the draft and its history, and echoed back.
      const text = byLabel(page, 'Transition 1 condition, operand 2: text')
      ;(text.element as HTMLInputElement).focus()
      const field = await typeSlowly('_X1')

      expect(field.value).toBe('PENDING_APPROVAL_X1')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeUndefined()

      // An edit elsewhere in the definition re-renders the editor around both.
      await page.get('[data-testid="state-name-0"]').setValue('Line manager approval')
      field.focus()
      await typeSlowly('Y')

      // The unfilled operands survived all of it.
      expect(page.get('[data-testid="transition-1-detail"]').text()).toContain(
        'Choose what this operand is.',
      )

      await save(page)

      const [approve, reject] = lastWrite().definition.transitions

      expect(approve.condition).toEqual({
        '==': [{ var: 'document.status' }, 'PENDING_APPROVAL_X1Y'],
      })
      // Nothing was emitted for the unfinished one, so it is saved as it was loaded.
      expect(reject.condition).toEqual({ '!=': [{ var: 'document.status' }, 'DRAFT'] })
    })
  })
})
