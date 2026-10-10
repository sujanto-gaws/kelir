import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import WorkflowEditorPage from './WorkflowEditorPage.vue'
import { apiClient } from '@/api/client'
import {
  errorBody,
  installFakeBackend,
  pageBody,
  validationReply,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { jwssViolations } from '@/lib/testing/jwss-meta-schema'
import { press, tabTo } from '@/lib/testing/keyboard'
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
 * **Nineteen distinct mutations** over row 6's code, run 2026-10-09, all
 * nineteen red (restated 2026-10-10: this paragraph said *the other six* and
 * the records below add up to more). Fourteen reddened a test in this file and
 * are listed in the table. The other five reddened only a unit spec and are
 * recorded there: same-path messages kept first-wins (`workflowVerdict.spec.ts`),
 * removing a state that keeps its outgoing transitions and dropping the
 * `isRestoring` guard (`useWorkflowDraft.spec.ts`), `retype` keeping every
 * field (`assignmentRule.spec.ts`), and *New revision* on a draft row
 * (`WorkflowListPage.spec.ts`). **Three of the fourteen are recorded twice**,
 * here and in the unit spec they also reddened — the S6/S7 placement, the
 * offered `document.amount`, and the rename — which is why the per-file
 * records total more than nineteen.
 *
 * **The test-engineer campaign's forty further mutations**, run 2026-10-10,
 * are in their own table below (*The test-engineer campaign*), M01–M40.
 *
 * Seen red, 2026-10-09, in this file:
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
    description: null as string | null,
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

/** Every permission above, the deprecation (**D-108**), and the read its warning makes (#713). */
const DEPRECATOR = [...EVERY_PERMISSION, 'workflow:definition:deprecate', 'document-type:read']

describe('WorkflowEditorPage', () => {
  let backend: FakeBackendHandle
  let stored: ReturnType<typeof record>
  /** What the next write answers instead of succeeding, when set. */
  let refuse: FakeReply | null
  /** The document types the list's `workflowDefinitionId` filter answers (#713). */
  let boundTypes: unknown[]
  /** How many there are in all, when more than the page holds. */
  let boundTotal: number | null
  /** What the bound-types read answers instead, when set. */
  let typesReply: FakeReply | null
  let router: Router
  let wrapper: VueWrapper | null

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    stored = record()
    refuse = null
    boundTypes = []
    boundTotal = null
    typesReply = null
    wrapper = null

    backend = installFakeBackend((request: RecordedRequest) => {
      const body = request.body as {
        definition?: JwssDefinition
        name?: string
        description?: string
      }

      if (request.url === '/document-types') {
        return (
          typesReply ?? {
            status: 200,
            body: pageBody(boundTypes, { pageSize: 100, total: boundTotal ?? boundTypes.length }),
          }
        )
      }

      if (request.method !== 'get' && refuse) {
        const reply = refuse

        refuse = null

        return reply
      }

      if (request.method === 'post' && request.url === '/workflow/definitions') {
        // The column is the trimmed text, or null when blank (`trimmed` in
        // `service/definition.rs`).
        stored = record({
          name: body.name,
          description: body.description?.trim() || null,
          definition: body.definition,
        })

        return { status: 201, body: { success: true, data: stored } }
      }

      if (request.method === 'put') {
        // The server stores what it was sent; the screen must show that. An
        // absent description leaves the column (`COALESCE($4, description)`),
        // and a sent one is stored trimmed, `''` included.
        stored = {
          ...stored,
          name: body.name ?? stored.name,
          description:
            body.description === undefined ? stored.description : body.description.trim(),
          definition: body.definition!,
        }

        return { status: 200, body: { success: true, data: stored } }
      }

      if (request.url.endsWith('/publication')) {
        stored = { ...stored, status: 'ACTIVE' }

        return { status: 200, body: { success: true, data: stored } }
      }

      if (request.url.endsWith('/deprecation')) {
        stored = { ...stored, status: 'DEPRECATED' }

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

  // --- Fixes after the campaign, 2026-10-10 ----------------------------------
  //
  // Seen red, 2026-10-10: eight mutations over the fixes, all red.
  //
  // | Mutation | Reddened |
  // |---|---|
  // | Not read-only while a save or publish is in flight | *takes no edit while a save is in flight…*, *…while a publish is in flight*, *draws a refusal on the row it named…* |
  // | A cleared description left out of the request | *clears the stored description when the author clears it* |
  // | The first declared code that appears wins, not the longest | *puts S6 on the state it names when that code holds a backtick* |
  // | Publish offered only to a caller who can edit | *offers publish to a caller who may publish and not update…* |
  // | `freePrefixes` ignored by the offered-path check | *counts a typed formData path as offered…*; `JsonLogicBuilder.spec.ts` *counts a path under a free prefix…* |
  // | A bare prefix counted as a path under it | `JsonLogicBuilder.spec.ts` *does not count the bare prefix…* |
  // | The task note never shown | *says beside the task checkbox…*, *says it for a due time or a priority alone too* |
  // | No re-read after `NOT_A_DRAFT` | *reads the revision again after NOT_A_DRAFT…* |
  describe('fixes after the campaign, 2026-10-10', () => {
    it('counts a typed formData path as offered, with no badge', async () => {
      const page = await render()

      await byLabel(page, 'Transition 1 condition, operand 1: variable path').setValue(
        'formData.amount',
      )

      expect(page.get('[data-testid="transition-0-detail"]').text()).not.toContain(
        'Not in the offered',
      )

      await byLabel(page, 'Transition 1 condition, operand 1: variable path').setValue('foo.bar')

      expect(page.get('[data-testid="transition-0-detail"]').text()).toContain('Not in the offered')

      await save(page)

      expect(lastWrite().definition.transitions[0].condition).toEqual({
        '==': [{ var: 'foo.bar' }, 'PENDING_APPROVAL'],
      })
    })

    it('says beside the task checkbox what unticking it removes, and undo restores it', async () => {
      const page = await render()

      expect(page.get('[data-testid="state-task-unshown-0"]').text()).toContain(
        'Unticking this also removes the stored escalation, due hours and priority.',
      )
      // A task with none of them has nothing unshown to lose.
      await page.get('[data-testid="add-state-task"]').trigger('click')
      expect(page.find('[data-testid="state-task-unshown-3"]').exists()).toBe(false)

      await page.get('[data-testid="state-has-task-0"]').setValue(false)
      expect(page.find('[data-testid="state-task-unshown-0"]').exists()).toBe(false)

      await page.get('[data-testid="undo"]').trigger('click')
      await settle()
      await save(page)

      expect(lastWrite().definition.states[0].task?.escalation).toEqual({
        afterHours: 72,
        assignment: { assigneeType: 'ROLE', roleCode: 'HEAD' },
      })
    })

    it('says it for a due time or a priority alone too', async () => {
      const loaded = jwss()

      delete loaded.states[0].task!.escalation
      loaded.states[0].task!.priority = 'HIGH'
      stored = record({ definition: loaded })

      const page = await render()

      expect(page.find('[data-testid="state-task-unshown-0"]').exists()).toBe(true)
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

    it('says a deprecated revision is not bindable, and offers Deprecate only on an active one', async () => {
      // Inverted 2026-10-10 (#713): this asserted no Deprecate anywhere until
      // #573's route merged. The route is #711's, and the action is here.
      stored = record({ status: 'DEPRECATED' })

      const deprecated = await render({ permissions: DEPRECATOR })

      expect(deprecated.get('[data-testid="published-notice"]').text()).toContain('deprecated')
      expect(deprecated.text()).not.toMatch(/Deprecate\b/)

      deprecated.unmount()
      stored = record({ status: 'ACTIVE' })

      const active = await render({ permissions: DEPRECATOR })

      expect(active.get('[data-testid="deprecate-workflow"]').text()).toMatch(/Deprecate\b/)
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

  // --- The test-engineer campaign, 2026-10-10 ----------------------------------
  //
  // What the builder's suite above did not reach, traced to #426's criteria.
  // Four defects were pinned with `it.fails` (five tests), each saying what it
  // would take to turn it into a plain `it`. All five are plain `it` since the
  // fixes of 2026-10-10; each says what changed.
  //
  // Seen to fail (coding standard §2.9): forty mutations the builder did not
  // list, run 2026-10-10 against this suite, the list and draft specs, the
  // route table and the layout. Thirty-nine went red; M35 survives as
  // equivalent. Eighteen survived the first run and were killed by tests
  // added here; they are marked *.
  //
  // | # | Mutation | Reddened |
  // |---|---|---|
  // | M01 | A detail past the rows on screen counted as placed | *lists a detail addressed past the rows…* |
  // | M02 | `definition.states` dropped from the root paths | *can empty a definition…* |
  // | M03 | Only `ACTIVE` counts as published | *leaves nothing but New revision operable on an DEPRECATED…* |
  // | M04 | Create alone may edit an existing draft | *lets a creator start a new workflow…* |
  // | M05* | Publish on a never-saved, untouched workflow skips the save | *tries to save an untouched new workflow…* |
  // | M06 | The list offers New revision on `ACTIVE` only | `WorkflowListPage.spec.ts` *offers a new revision of a deprecated…* |
  // | M07 | The nav entry gated on update, not read | `AppLayout.spec.ts` *links to the workflows only…* |
  // | M08 | The `new` route gated on read, not create | `router/index.spec.ts` *has the workflow editor behind…* |
  // | M09 | Save not disabled while saving | *saves once for a double click* |
  // | M10 | A state code renamed per keystroke, not on commit | *takes a rename typed through another state’s code…* |
  // | M11 | Non-numeric due hours sent as `NaN` | *writes the due hours as a number…* |
  // | M12 | An undeclared target not offered as itself | *keeps the edges into a removed state…* |
  // | M13 | Transitions out of an undeclared state not drawn | *draws a detail on a transition out of an undeclared state…* |
  // | M14 | One message at the request and document path shown twice | *shows a message the request and the document both carry once…* |
  // | M15* | Transition move buttons always enabled | *offers a move only where there is a neighbour…* |
  // | M16* | The last state’s Move down enabled | *offers a move only where there is a neighbour…* |
  // | M17* | A stored unresolved assignee type not shown | *shows a stored assignee type Kelir does not resolve…* |
  // | M18* | A rename follows edges when another state shares the old code | `useWorkflowDraft.spec.ts` *renames only the state when another state already has its old code* |
  // | M19* | Undo leaves the coalescing key set | `useWorkflowDraft.spec.ts` *records an edit to the field an undo just restored…* |
  // | M20 | Load leaves the coalescing key set | *undoes a save’s edits no further than…* |
  // | M21* | A root edit leaves the document-path message | *clears a message at the field it names…* |
  // | M22* | A state field edit leaves its message | *clears a message at the field it names…* |
  // | M23* | An allowed-by edit leaves its message | *clears a message at the field it names…* |
  // | M24* | The unfilled count keeps removed rows | *stops counting an unfilled operand…* |
  // | M25* | The editor’s New revision failure swallowed | *says why a new revision could not be opened…* |
  // | M26* | Loading a revision keeps the last one’s messages | *drops one revision’s messages when another is opened…* |
  // | M27* | An unticked Final written as `isFinal: false` | *writes an unticked Final and a cleared department as absent…* |
  // | M28* | A blank department scope written as `''` | *writes an unticked Final and a cleared department as absent…* |
  // | M29 | S6/S7 never placed on state 0 | `workflowVerdict.spec.ts` *moves an S7 detail…* |
  // | M30 | A save’s refusal not placed | *puts S6’s list-wide refusal…*; *puts S6 on a state whose code is not ASCII* |
  // | M31* | A publish’s refusal not placed | *places a publish’s list-wide refusal…* |
  // | M32 | A reader may edit a draft | *lets a reader read a draft…* and two more |
  // | M33 | New revision offered without create | *offers no new revision to a caller who cannot create one* |
  // | M34* | Removing a transition drops the last row key, not its own | *keeps a later transition’s unfilled operand…* |
  // | M35 | Publish keeps the local draft, not what was published | **Survives, equivalent**: publish stores no change to `definition_json`, and the draft it keeps is the one just saved or loaded |
  // | M36 | The AC2 helper accepts everything | `jwssRegistry.spec.ts` *…S12 on an AUTO edge* |
  // | M37 | The key stays editable after the first save | *is created from the starter…* |
  // | M38 | A cleared description sent as `''` | the pinned *clears the stored description…* passes: a candidate fix |
  // | M39* | A state card lists drawn fields again at its top | *draws a message at the input it names and not again…* |
  // | M40* | A transition row lists drawn fields again | *draws a message at the input it names and not again…* |

  /**
   * Every control a keyboard or pointer could operate that is not disabled,
   * less the logic builder's *Collapse* and *Expand*: they change the view and
   * not the definition, so they stay operable on a read-only revision.
   */
  function enabledControls(page: VueWrapper): string[] {
    return page
      .findAll('input, select, textarea, button')
      .filter((control) => !(control.element as HTMLInputElement).disabled)
      .filter((control) => !/^(Collapse|Expand) /.test(control.attributes('aria-label') ?? ''))
      .map(
        (control) =>
          control.attributes('data-testid') ??
          control.attributes('aria-label') ??
          control.attributes('id') ??
          control.text(),
      )
  }

  /**
   * Holds the next request until the returned function is called, as a slow
   * network does: the fake backend answers synchronously otherwise, so there
   * is never a moment between a click and its reply for a user to act in.
   */
  function holdNextRequest(): () => Promise<void> {
    type Adapter = (config: InternalAxiosRequestConfig) => Promise<AxiosResponse>

    const fake = apiClient.defaults.adapter as Adapter
    let release: () => void = () => undefined
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    apiClient.defaults.adapter = (async (config: InternalAxiosRequestConfig) => {
      apiClient.defaults.adapter = fake
      await gate

      return fake(config)
    }) as Adapter

    return async () => {
      release()
      await settle()
    }
  }

  describe('the campaign: what is written (AC1, AC2)', () => {
    it('writes back every field it has no input for, unknown future keys included, in their order', async () => {
      const loaded = jwss() as JwssDefinition & Record<string, unknown>

      loaded.description = 'Purchases over the threshold'
      loaded.settings = { allowCancel: true, auditLevel: 'FULL' }
      loaded.variables = [
        { key: 'threshold', dataType: 'NUMBER', source: { var: 'formData.limit' } },
      ]
      loaded.transitions[1].guards = [
        { hook: 'before_workflow_transition', handler: 'core:continue_always', priority: 100 },
      ]
      // A key a later JWSS minor might add: the meta-schema refuses it today, and
      // that refusal is the server's to give, not the editor's to pre-empt by
      // dropping it.
      loaded.futureRootKey = { kept: true }
      ;(loaded.states[0] as unknown as Record<string, unknown>).futureStateKey = 1
      ;(loaded.states[0].task as unknown as Record<string, unknown>).futureTaskKey = 'x'
      ;(loaded.transitions[0] as unknown as Record<string, unknown>).futureEdgeKey = [1]
      stored = record({ definition: structuredClone(loaded) })

      const page = await render()

      await page.get('[data-testid="state-name-0"]').setValue('Line manager approval')
      await byLabel(page, 'Transition 1: action').setValue('RETURN')
      await save(page)

      const expected = structuredClone(loaded)

      expected.states[0].name = 'Line manager approval'
      expected.transitions[0].action = 'RETURN'

      // Serialised, so the order of every key is asserted as well as its value.
      expect(JSON.stringify(lastWrite().definition)).toBe(JSON.stringify(expected))
    })

    it('writes a definition carrying every optional JWSS member the meta-schema accepts', async () => {
      const loaded = jwss()

      loaded.description = 'Purchases over the threshold'
      loaded.settings = { allowCancel: true }
      loaded.transitions[1].guards = [
        { hook: 'before_workflow_transition', handler: 'core:continue_always', priority: 100 },
      ]
      stored = record({ definition: loaded })

      const page = await render()

      await page.get('[data-testid="requires-comment-0"]').setValue(true)
      await save(page)

      expect(jwssViolations(lastWrite().definition)).toEqual([])
      expect(lastWrite().definition.settings).toEqual({ allowCancel: true })
      expect(lastWrite().definition.transitions[1].guards).toHaveLength(1)
    })

    it('keeps the edges into a removed state, so S2 has something to name at their target', async () => {
      const page = await render()

      await page.get('[data-testid="remove-state-1"]').trigger('click')

      const target = byLabel(page, 'Transition 1: to')

      expect((target.element as HTMLSelectElement).value).toBe('COMPLETED')
      expect(target.text()).toContain('COMPLETED (not declared)')

      refuse = validationReply([
        'definition.transitions.0.to',
        '`COMPLETED` is not a declared state',
      ])
      await save(page)

      expect(lastWrite().definition.transitions.map((transition) => transition.to)).toEqual([
        'COMPLETED',
        'REJECTED',
      ])
      expect(byLabel(page, 'Transition 1: to').attributes('aria-invalid')).toBe('true')
      expect(page.get('[data-testid="transition-0"]').text()).toContain('is not a declared state')
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('keeps the initial state when its state is removed, and shows S1 at the chooser', async () => {
      const page = await render()

      await page.get('[data-testid="remove-state-0"]').trigger('click')

      refuse = validationReply([
        'definition.initialState',
        '`MANAGER_APPROVAL` is not a declared state',
      ])
      await save(page)

      const sent = lastWrite().definition

      expect(sent.initialState).toBe('MANAGER_APPROVAL')
      // The edges out of it went with it; nothing else was dropped.
      expect(sent.transitions).toEqual([])
      expect(sent.states.map((state) => state.code)).toEqual(['COMPLETED', 'REJECTED'])
      expect(page.get('[data-testid="initial-state-error"]').text()).toContain('MANAGER_APPROVAL')
    })

    it('renames a state to a code another holds without moving edges, and S3 lands on its code', async () => {
      const page = await render()
      const code = page.get('[data-testid="state-code-2"]')

      await code.setValue('COMPLETED')
      await code.trigger('change')

      refuse = validationReply([
        'definition.states.2.code',
        '`COMPLETED` is declared more than once; state codes are unique',
      ])
      await save(page)

      const sent = lastWrite().definition

      expect(sent.states.map((state) => state.code)).toEqual([
        'MANAGER_APPROVAL',
        'COMPLETED',
        'COMPLETED',
      ])
      // Which COMPLETED the REJECT edge meant is the author's call.
      expect(sent.transitions[1].to).toBe('REJECTED')
      expect(page.get('[data-testid="state-code-2"]').attributes('aria-invalid')).toBe('true')
      expect(page.get('[data-testid="state-2"]').text()).toContain('declared more than once')
      expect(page.get('[data-testid="state-code-1"]').attributes('aria-invalid')).not.toBe('true')
    })

    it('takes a rename typed through another state’s code only once it is committed', async () => {
      const page = await render({ attach: true })
      const code = page.get('[data-testid="state-code-0"]').element as HTMLInputElement

      code.focus()
      code.value = ''
      code.dispatchEvent(new Event('input', { bubbles: true }))

      // On its way to COMPLETED_X the text spells COMPLETED, another state's code.
      for (const character of 'COMPLETED_X') {
        code.value += character
        code.dispatchEvent(new Event('input', { bubbles: true }))
        await settle()
      }

      code.dispatchEvent(new Event('change', { bubbles: true }))
      await save(page)

      const sent = lastWrite().definition

      expect(sent.initialState).toBe('COMPLETED_X')
      expect(sent.transitions.map((transition) => transition.from)).toEqual([
        'COMPLETED_X',
        'COMPLETED_X',
      ])
      expect(sent.states.map((state) => state.code)).toEqual([
        'COMPLETED_X',
        'COMPLETED',
        'REJECTED',
      ])
    })

    it('can empty a definition, and shows the server’s refusal of it at the state list', async () => {
      const page = await render()

      for (let round = 0; round < 3; round += 1) {
        await page.get('[data-testid="remove-state-0"]').trigger('click')
      }

      expect(page.text()).toContain('No states.')

      refuse = validationReply(['definition.states', '[] has less than 2 items'])
      await save(page)

      expect(lastWrite().definition.states).toEqual([])
      expect(lastWrite().definition.transitions).toEqual([])
      expect(page.get('[data-testid="states-error"]').text()).toContain('less than 2 items')
      // Drawn once, at the list, and not listed a second time as unplaced.
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('writes the due hours as a number, leaves them out when blank, and sends other text as typed', async () => {
      const page = await render()
      // Re-read after each save: a save reloads the draft, and its rows remount.
      const due = () => page.get('[data-testid="state-0"]').get('input[inputmode="decimal"]')

      await due().setValue('12')
      await save(page)
      expect(lastWrite().definition.states[0].task?.dueInHours).toBe(12)

      await due().setValue('soon')
      await save(page)
      // Sent as typed, so the meta-schema refuses it at this field rather than
      // the editor dropping it.
      expect(lastWrite().definition.states[0].task?.dueInHours).toBe('soon')

      await due().setValue('')
      await save(page)
      expect(lastWrite().definition.states[0].task).not.toHaveProperty('dueInHours')
    })

    it('offers a move only where there is a neighbour to swap with', async () => {
      const page = await render()
      const disabled = (label: string) => byLabel(page, label).attributes('disabled') !== undefined

      expect(disabled('Move State MANAGER_APPROVAL up')).toBe(true)
      expect(disabled('Move State MANAGER_APPROVAL down')).toBe(false)
      expect(disabled('Move State REJECTED down')).toBe(true)
      // The two edges out of MANAGER_APPROVAL swap with each other and nothing else.
      expect(disabled('Move Transition 1 up')).toBe(true)
      expect(disabled('Move Transition 1 down')).toBe(false)
      expect(disabled('Move Transition 2 up')).toBe(false)
      expect(disabled('Move Transition 2 down')).toBe(true)
    })

    it('shows a stored assignee type Kelir does not resolve as itself, and offers it to no other rule', async () => {
      const loaded = jwss()

      loaded.states[0].task!.assignment = { assigneeType: 'MANAGER_OF_OWNER' }
      stored = record({ definition: loaded })

      const page = await render()
      const stored0 = byLabel(page, 'State MANAGER_APPROVAL task assigned to: who')

      expect((stored0.element as HTMLSelectElement).value).toBe('MANAGER_OF_OWNER')
      expect(stored0.text()).toContain('not resolved by Kelir')

      const other = byLabel(page, 'Transition 1 allowed by: who')
        .findAll('option')
        .map((option) => option.attributes('value'))

      expect(other).not.toContain('MANAGER_OF_OWNER')
      expect(other).not.toContain('EXPRESSION')

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      await save(page)

      expect(lastWrite().definition.states[0].task?.assignment).toEqual({
        assigneeType: 'MANAGER_OF_OWNER',
      })
    })

    it('writes an unticked Final and a cleared department as absent, as JWSS defaults them', async () => {
      const page = await render()

      await page.get('[data-testid="state-1"]').get('input[type="checkbox"]').setValue(false)
      await byLabel(page, 'State MANAGER_APPROVAL task assigned to: who').setValue(
        'DEPARTMENT_ROLE',
      )

      const scope = byLabel(page, 'State MANAGER_APPROVAL task assigned to: department')

      await scope.setValue('OWNER_DEPARTMENT')
      await scope.setValue('')
      await save(page)

      const sent = lastWrite().definition

      expect(sent.states[1]).toEqual({
        code: 'COMPLETED',
        name: 'Completed',
        mapsToDocumentStatus: 'COMPLETED',
      })
      expect(sent.states[0].task?.assignment).toEqual({
        assigneeType: 'DEPARTMENT_ROLE',
        roleCode: 'APPROVER',
      })
    })

    it('saves once for a double click', async () => {
      const page = await render()
      const button = page.get('[data-testid="save-workflow"]')

      await button.trigger('click')
      await button.trigger('click')
      await settle()

      expect(writes()).toHaveLength(1)
    })

    it('creates, then publishes, a new workflow from one click', async () => {
      const page = await render({ path: '/admin/workflows/new' })

      await page.get('[data-testid="workflow-key"]').setValue('purchase_approval')
      await page.get('[data-testid="workflow-name"]').setValue('Purchase approval')
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Save and publish')

      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        'post /workflow/definitions',
        `post /workflow/definitions/${ID}/publication`,
      ])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })

    it('tries to save an untouched new workflow before publishing, and says why it cannot', async () => {
      const page = await render({ path: '/admin/workflows/new' })

      // Nothing is edited, so the draft is clean; it has still never been saved.
      refuse = validationReply(['workflowKey', 'workflowKey is required'])
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        'post /workflow/definitions',
      ])
      expect(page.get('[data-testid="workflow-key-error"]').text()).toBe('workflowKey is required')
    })

    it('clears the stored description when the author clears it', async () => {
      // `PUT` treats an absent `description` as *leave it* (`UpdateWorkflowRequest`,
      // and `COALESCE($4, description)` in `repository/definition.rs`), so a
      // cleared description is sent as `''`, and the JWSS carries none.
      // Pinned `it.fails` by the campaign until fixed 2026-10-10.
      const loaded = jwss()

      loaded.description = 'Old description'
      stored = record({ description: 'Old description', definition: loaded })

      const page = await render()

      await page.get('#workflow-description').setValue('')
      await save(page)

      const body = writes()[0].body as Record<string, unknown>

      expect(body).toHaveProperty('description')
      expect(body.description).toBe('')
      expect('description' in (body.definition as object)).toBe(false)
      expect(jwssViolations(body.definition)).toEqual([])
    })

    it('sends no description for a workflow that never had one', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      await save(page)

      expect('description' in (writes()[0].body as object)).toBe(false)
    })
  })

  describe('the campaign: undo and redo', () => {
    it('keeps the history through a refusal, and an undo clears the refusal’s messages', async () => {
      const page = await render()
      const name = page.get('[data-testid="workflow-name"]')

      await name.setValue('Renamed')
      refuse = validationReply(['name', 'name is taken'])
      await save(page)

      expect(page.get('[data-testid="workflow-name-error"]').text()).toBe('name is taken')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeUndefined()

      await page.get('[data-testid="undo"]').trigger('click')
      await settle()

      expect((name.element as HTMLInputElement).value).toBe('Purchase approval')
      expect(page.find('[data-testid="workflow-name-error"]').exists()).toBe(false)

      await page.get('[data-testid="redo"]').trigger('click')
      await settle()
      await save(page)

      expect(lastWrite().name).toBe('Renamed')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()
    })

    it('undoes a save’s edits no further than what the server stored', async () => {
      const page = await render()
      const name = page.get('[data-testid="workflow-name"]')

      await name.setValue('Saved name')
      await save(page)
      await name.setValue('After the save')
      await page.get('[data-testid="undo"]').trigger('click')
      await settle()

      expect((name.element as HTMLInputElement).value).toBe('Saved name')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')
    })

    it('undoes and redoes a condition the logic builder is showing', async () => {
      const page = await render()
      const operand = () => byLabel(page, 'Transition 1 condition, operand 2: text')

      await operand().setValue('PENDING_APPROVAL_X')
      await operand().setValue('PENDING_APPROVAL_XY')

      await page.get('[data-testid="undo"]').trigger('click')
      await settle()

      // Both keystrokes were one step, and the builder re-read the restored value.
      expect((operand().element as HTMLInputElement).value).toBe('PENDING_APPROVAL')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()

      await page.get('[data-testid="redo"]').trigger('click')
      await settle()

      expect((operand().element as HTMLInputElement).value).toBe('PENDING_APPROVAL_XY')

      await save(page)

      expect(lastWrite().definition.transitions[0].condition).toEqual({
        '==': [{ var: 'document.status' }, 'PENDING_APPROVAL_XY'],
      })
    })

    it('keeps a condition’s unfilled operand through the undo of an edit elsewhere', async () => {
      const page = await render()

      await byLabel(page, 'Transition 2 condition, operand 2: kind').setValue('comparison')
      await page.get('[data-testid="state-name-0"]').setValue('Line manager approval')
      await page.get('[data-testid="undo"]').trigger('click')
      await settle()

      expect((page.get('[data-testid="state-name-0"]').element as HTMLInputElement).value).toBe(
        'Manager approval',
      )
      expect(page.get('[data-testid="transition-1-detail"]').text()).toContain(
        'Choose what this operand is.',
      )
      expect(page.get('[data-testid="unfilled-expressions"]').text()).toContain('1 expression')
    })
  })

  describe('the campaign: the server’s verdict (AC3)', () => {
    it('clears a message at the field it names when that field is edited, and only that one', async () => {
      const page = await render()

      refuse = validationReply(
        ['definition.name', 'name is too long'],
        ['definition.states.0.name', 'the state name is blank'],
        ['definition.transitions.0.allowedBy.roleCode', '`APPROVER` is not a live role'],
        ['definition.states.1.name', 'untouched'],
      )
      await save(page)

      await page.get('[data-testid="workflow-name"]').setValue('Shorter')
      expect(page.find('[data-testid="workflow-name-error"]').exists()).toBe(false)

      await page.get('[data-testid="state-name-0"]').setValue('Manager approval, again')
      expect(page.get('[data-testid="state-0"]').text()).not.toContain('the state name is blank')

      await byLabel(page, 'Transition 1 allowed by: role code').setValue('MANAGER')
      expect(page.get('[data-testid="transition-0"]').text()).not.toContain('is not a live role')
      expect(
        byLabel(page, 'Transition 1 allowed by: role code').attributes('aria-invalid'),
      ).not.toBe('true')

      expect(page.get('[data-testid="state-1"]').text()).toContain('untouched')
    })

    it('keeps a later transition’s unfilled operand when an earlier one is removed', async () => {
      const page = await render()

      await byLabel(page, 'Transition 2 condition, operand 2: kind').setValue('comparison')
      await byLabel(page, 'Remove Transition 1').trigger('click')
      await settle()

      // The REJECT edge is transition 1 now, and its builder is the one that held
      // the unfilled operand, not a neighbour's reused under it.
      expect(byLabel(page, 'Transition 1: action').element).toHaveProperty('value', 'REJECT')
      expect(page.get('[data-testid="transition-0-detail"]').text()).toContain(
        'Choose what this operand is.',
      )
    })

    it('stops counting an unfilled operand once its transition is removed', async () => {
      const page = await render()

      await byLabel(page, 'Transition 2 condition, operand 2: kind').setValue('comparison')
      expect(page.find('[data-testid="unfilled-expressions"]').exists()).toBe(true)

      await byLabel(page, 'Remove Transition 2').trigger('click')
      await settle()

      expect(page.find('[data-testid="unfilled-expressions"]').exists()).toBe(false)
    })

    it('says why a new revision could not be opened, and stays on the published one', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render()

      refuse = { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:create') }
      await page.get('[data-testid="new-revision"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="form-error"]').text()).toBe(
        'Missing workflow:definition:create',
      )
      expect(router.currentRoute.value.params.id).toBe(ID)
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })

    it('drops one revision’s messages when another is opened in the same screen', async () => {
      const page = await render()

      refuse = validationReply(['definition.states.1.name', 'about revision 1'])
      await save(page)
      expect(page.text()).toContain('about revision 1')

      stored = record({ id: NEXT_ID, version: 2 })
      await router.push(`/admin/workflows/${NEXT_ID}`)
      await settle()

      expect(page.text()).toContain('Revision 2')
      expect(page.text()).not.toContain('about revision 1')
    })

    it('places a publish’s list-wide refusal on the state it names, as a save’s is', async () => {
      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.states',
            rule: 'S6',
            code: 'DEAD_END_STATE',
            message: 'no final state is reachable from `MANAGER_APPROVAL`, so a document…',
          },
        ]),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="state-0-messages"]').text()).toContain(
        'no final state is reachable',
      )
      expect(page.find('[data-testid="states-error"]').exists()).toBe(false)
    })

    it('draws a message at the input it names and not again in its row’s list', async () => {
      const page = await render()

      refuse = validationReply(
        ['definition.states.0.name', 'the state name is blank'],
        ['definition.states.0.task.assignment.roleCode', 'the role is blank'],
        ['definition.transitions.0.to', 'not declared'],
        ['definition.transitions.0.allowedBy.roleCode', 'the role is dead'],
      )
      await save(page)

      expect(page.find('[data-testid="state-0-messages"]').exists()).toBe(false)
      expect(page.find('[data-testid="transition-0-messages"]').exists()).toBe(false)
      expect(
        page.get('[data-testid="state-0"]').text().split('the state name is blank'),
      ).toHaveLength(2)
      expect(page.get('[data-testid="state-0"]').text().split('the role is dead')).toHaveLength(2)
    })

    it('lists a detail addressed past the rows on screen, at the root, or with no path', async () => {
      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          { path: 'definition.transitions.7.to', rule: 'S2', code: 'X', message: 'row seven' },
          { path: 'definition', rule: 'shape', code: 'X', message: 'an unknown member' },
          { path: 'definition.version', rule: 'shape', code: 'X', message: 'not 1.0.0' },
          {
            path: undefined as unknown as string,
            rule: 'x',
            code: 'X',
            message: 'a detail with no path',
          },
        ]),
      }
      await save(page)

      const unplaced = page.get('[data-testid="unplaced-errors"]').text()

      expect(unplaced).toContain('definition.transitions.7.to: row seven')
      expect(unplaced).toContain('definition: an unknown member')
      expect(unplaced).toContain('definition.version: not 1.0.0')
      expect(unplaced).toContain('a detail with no path')
    })

    it('draws a detail on a transition out of an undeclared state where that transition is drawn', async () => {
      const loaded = jwss()

      loaded.transitions.push({
        from: 'GHOST',
        to: 'COMPLETED',
        action: 'APPROVE',
        allowedBy: 'OWNER',
      })
      stored = record({ definition: loaded })

      const page = await render()

      refuse = validationReply(['definition.transitions.2.from', '`GHOST` is not a declared state'])
      await save(page)

      expect(page.get('[data-testid="unattached-transitions"]').text()).toContain(
        '`GHOST` is not a declared state',
      )
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('shows a message the request and the document both carry once, at the field', async () => {
      const page = await render({ path: '/admin/workflows/new' })

      refuse = validationReply(
        ['workflowKey', 'must match ^[a-z][a-z0-9_]*$'],
        ['definition.workflowKey', 'must match ^[a-z][a-z0-9_]*$'],
      )
      await save(page)

      expect(page.get('[data-testid="workflow-key-error"]').text()).toBe(
        'must match ^[a-z][a-z0-9_]*$',
      )
      expect(page.get('[data-testid="workflow-key"]').attributes('aria-invalid')).toBe('true')
    })

    it('puts S6 on a state whose code is not ASCII', async () => {
      const loaded = jwss()

      loaded.states.push({
        code: 'ÜBERPRÜFUNG',
        name: 'Prüfung',
        mapsToDocumentStatus: 'IN_REVIEW',
      })
      stored = record({ definition: loaded })

      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.states.3.code',
            rule: 'shape',
            code: 'PATTERN',
            message: '"ÜBERPRÜFUNG" does not match "^[A-Z][A-Z0-9_]*$"',
          },
          {
            path: 'definition.states',
            rule: 'S6',
            code: 'UNREACHABLE_STATE',
            message: '`ÜBERPRÜFUNG` cannot be reached from the initial state',
          },
        ]),
      }
      await save(page)

      expect(page.get('[data-testid="state-3-messages"]').text()).toContain('cannot be reached')
      expect(page.get('[data-testid="state-3"]').text()).toContain('does not match')
      expect(page.find('[data-testid="states-error"]').exists()).toBe(false)
    })

    it('leaves S7 at the list when the state it names is not declared', async () => {
      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.transitions',
            rule: 'S7',
            code: 'AMBIGUOUS_FALLBACK',
            message:
              '2 transitions leave `GHOST` on APPROVE with no condition; at most one may be the fallback',
          },
        ]),
      }
      await save(page)

      expect(page.get('[data-testid="states-error"]').text()).toContain('leave `GHOST`')
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('puts S6 on the state it names when that code holds a backtick', async () => {
      // A code holding a backtick (refused by the meta-schema at its own
      // field, while S6 still runs and names it) used to be cut short at the
      // first backtick pair, onto another state's code. The message is now
      // matched against the declared codes, the longest first. Pinned
      // `it.fails` by the campaign until fixed 2026-10-10.
      const loaded = jwss()

      loaded.states.push(
        { code: 'A', name: 'A', mapsToDocumentStatus: 'IN_REVIEW' },
        { code: 'A`B', name: 'A with a backtick', mapsToDocumentStatus: 'IN_REVIEW' },
      )
      stored = record({ definition: loaded })

      const page = await render()

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.states',
            rule: 'S6',
            code: 'UNREACHABLE_STATE',
            message: '`A`B` cannot be reached from the initial state',
          },
        ]),
      }
      await save(page)

      expect(page.find('[data-testid="state-3-messages"]').exists()).toBe(false)
      expect(page.get('[data-testid="state-4-messages"]').text()).toContain('cannot be reached')
    })

    it('says why when the revision was published under the author (NOT_A_DRAFT is a 422 at status)', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'status',
            rule: 'immutable',
            code: 'NOT_A_DRAFT',
            message: 'revision 1 of `purchase_approval` is published and cannot be edited',
          },
        ]),
      }
      await save(page)

      expect(page.get('[data-testid="unplaced-errors"]').text()).toContain(
        'is published and cannot be edited',
      )
      // The edit is not lost: it can be carried to a new revision by hand.
      expect((page.get('[data-testid="workflow-name"]').element as HTMLInputElement).value).toBe(
        'Renamed',
      )
    })

    it('reads the revision again after NOT_A_DRAFT, and offers a new revision with the edit kept', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      // Somebody else published it between the load and this save.
      stored = record({ status: 'ACTIVE' })
      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'status',
            rule: 'immutable',
            code: 'NOT_A_DRAFT',
            message: 'revision 1 of `purchase_approval` is published and cannot be edited',
          },
        ]),
      }
      await save(page)

      expect(backend.requests.filter((request) => request.method === 'get')).toHaveLength(2)
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="published-notice"]').exists()).toBe(true)
      expect(page.find('[data-testid="new-revision"]').exists()).toBe(true)
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(false)
      // The refusal stays said, and the refused edit stays on screen to carry over.
      expect(page.get('[data-testid="unplaced-errors"]').text()).toContain('is published')
      expect((page.get('[data-testid="workflow-name"]').element as HTMLInputElement).value).toBe(
        'Renamed',
      )
    })

    it('reads nothing again after a refusal that is not NOT_A_DRAFT', async () => {
      const page = await render()

      refuse = validationReply(['name', 'name is required'])
      await save(page)

      expect(backend.requests.filter((request) => request.method === 'get')).toHaveLength(1)
    })

    it('shows a 409 on publish verbatim, and leaves the draft a draft that can still be edited', async () => {
      const page = await render()

      refuse = {
        status: 409,
        body: errorBody('CONFLICT', 'revision 1 of `purchase_approval` is ACTIVE, not DRAFT'),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="form-error"]').text()).toBe(
        'revision 1 of `purchase_approval` is ACTIVE, not DRAFT',
      )
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(true)
      expect(page.find('[data-testid="unplaced-errors"]').exists()).toBe(false)
    })

    it('shows a 403 verbatim on the form and keeps the edit', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      refuse = { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:update') }
      await save(page)

      expect(page.get('[data-testid="form-error"]').text()).toBe(
        'Missing workflow:definition:update',
      )
      expect((page.get('[data-testid="workflow-name"]').element as HTMLInputElement).value).toBe(
        'Renamed',
      )
      expect(page.find('[data-testid="notice"]').exists()).toBe(false)
    })

    it('says a network failure is one, keeps the edits, and a retry sends them', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      refuse = { status: 0, networkError: true }
      await save(page)

      expect(page.get('[data-testid="form-error"]').text()).toContain('Could not reach the server')
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')

      await save(page)

      expect(lastWrite().name).toBe('Renamed')
      expect(page.find('[data-testid="form-error"]').exists()).toBe(false)
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
    })

    it('takes no edit while a save is in flight, so nothing typed is lost under its reply', async () => {
      // The campaign pinned this `it.fails` as *keeps what is typed while a save
      // is in flight*: the reply reloads the draft, and whatever was typed in
      // the gap was gone with nothing said. Fixed 2026-10-10 by the second of
      // its two candidate fixes, the one the coordinator chose: the editor is
      // read-only while it waits, so there is nothing to lose. The assertion is
      // that, rather than the typed text surviving.
      const page = await render()
      const release = holdNextRequest()

      await page.get('[data-testid="save-workflow"]').trigger('click')

      expect(page.get('[data-testid="state-name-0"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="workflow-name"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="remove-state-0"]').attributes('disabled')).toBeDefined()
      expect(page.get('[data-testid="requires-comment-0"]').attributes('disabled')).toBeDefined()
      expect(
        byLabel(page, 'Transition 1 condition, operand 2: text').attributes('disabled'),
      ).toBeDefined()
      expect(page.get('[data-testid="publish-workflow"]').attributes('disabled')).toBeDefined()

      await page.get('[data-testid="state-name-0"]').setValue('Typed during the save')
      await release()

      // The input event of a disabled field reached nothing: the draft is what
      // was sent and stored, and the screen takes edits again.
      expect((page.get('[data-testid="state-name-0"]').element as HTMLInputElement).value).toBe(
        'Manager approval',
      )
      expect(page.get('[data-testid="state-name-0"]').attributes('disabled')).toBeUndefined()
      expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')
    })

    it('takes no edit while a publish is in flight', async () => {
      const page = await render()
      const release = holdNextRequest()

      await page.get('[data-testid="publish-workflow"]').trigger('click')

      expect(page.get('[data-testid="state-name-0"]').attributes('disabled')).toBeDefined()

      await release()

      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })

    it('draws a refusal on the row it named, since no row can be removed while it is in flight', async () => {
      // The same window: the verdict addresses rows by their position in what
      // was sent, so a removal in the gap drew a detail about the sent state 1
      // (COMPLETED) on whatever was state 1 then (REJECTED). The editor is
      // read-only while it waits, so the removal does not happen. Pinned
      // `it.fails` by the campaign until fixed 2026-10-10.
      const page = await render()
      const release = holdNextRequest()

      refuse = validationReply(['definition.states.1.name', 'about COMPLETED'])
      await page.get('[data-testid="save-workflow"]').trigger('click')
      await page.get('[data-testid="remove-state-0"]').trigger('click')
      await release()

      const shownOn = page
        .findAll('section[aria-label]')
        .filter((card) => card.text().includes('about COMPLETED'))
        .map((card) => card.attributes('aria-label'))

      expect(shownOn).not.toContain('State REJECTED')
      expect(shownOn).toEqual(['State COMPLETED'])
    })
  })

  describe('the campaign: a published revision is read-only (AC5)', () => {
    it.each(['ACTIVE', 'DEPRECATED'])(
      'leaves nothing but New revision operable on an %s revision, keyboard included',
      async (status) => {
        stored = record({ status })

        const page = await render({ attach: true })

        expect(enabledControls(page)).toEqual(['new-revision'])

        // Nothing disabled takes focus, so Tab cannot reach it.
        for (const control of page.findAll('input, select, textarea')) {
          ;(control.element as HTMLElement).focus()
          expect(document.activeElement).not.toBe(control.element)
        }

        // No shortcut edits, saves or undoes behind the disabled controls.
        for (const key of ['z', 'y', 's', 'Enter', 'Delete']) {
          for (const target of [document, page.get('[data-testid="state-0"]').element]) {
            target.dispatchEvent(
              new KeyboardEvent('keydown', { key, ctrlKey: key.length === 1, bubbles: true }),
            )
          }
        }
        // Collapsing a condition changes the view, and writes nothing.
        for (const toggle of page.findAll('button[aria-label^="Collapse "]')) {
          await toggle.trigger('click')
        }
        await settle()

        expect(writes()).toEqual([])
        expect((page.get('[data-testid="workflow-name"]').element as HTMLInputElement).value).toBe(
          'Purchase approval',
        )
      },
    )

    it('becomes read-only the moment it is published from this screen', async () => {
      const page = await render()

      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(enabledControls(page)).toEqual(['new-revision'])
    })

    it('offers Deprecate on an ACTIVE revision, and calls the route only once it is confirmed', async () => {
      // Inverted 2026-10-10 (#713): this asserted no deprecate control and no
      // deprecation route on an ACTIVE revision, until #573's route merged.
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR })

      const controls = page
        .findAll('button, a, [role="button"]')
        .filter((control) => /deprecat|retire/i.test(control.text()))

      expect(controls.map((control) => control.attributes('data-testid'))).toEqual([
        'deprecate-workflow',
      ])

      await page.get('[data-testid="deprecate-workflow"]').trigger('click')
      await settle()

      expect(backend.requests.some((request) => /deprecat/i.test(request.url))).toBe(false)

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        `post /workflow/definitions/${ID}/deprecation`,
      ])
    })
  })

  describe('the campaign: permissions', () => {
    it('lets a creator start a new workflow, and not edit an existing draft', async () => {
      const creator = ['workflow:definition:read', 'workflow:definition:create']
      const fresh = await render({ path: '/admin/workflows/new', permissions: creator })

      expect(fresh.find('[data-testid="save-workflow"]').exists()).toBe(true)
      expect(fresh.get('[data-testid="workflow-key"]').attributes('disabled')).toBeUndefined()
      fresh.unmount()
      wrapper = null

      const existing = await render({ permissions: creator })

      expect(existing.find('[data-testid="read-only-notice"]').exists()).toBe(true)
      expect(existing.find('[data-testid="save-workflow"]').exists()).toBe(false)
      expect(enabledControls(existing)).toEqual([])
    })

    it('offers publish to a caller who may publish and not update, and publishes the stored draft', async () => {
      // `workflow:definition:publish` is its own grant, and `publish_definition`
      // checks only it. The product owner decided on 2026-10-10 that it alone
      // may publish: a reviewer who signs a workflow off without authoring it.
      // Pinned `it.fails` by the campaign until then.
      const page = await render({
        permissions: ['workflow:definition:read', 'workflow:definition:publish'],
      })

      expect(page.get('[data-testid="read-only-notice"]').text()).toContain(
        'You can publish it as it is stored.',
      )
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(false)
      expect(page.get('[data-testid="state-name-0"]').attributes('disabled')).toBeDefined()

      const publish = page.get('[data-testid="publish-workflow"]')

      expect(publish.text()).toBe('Publish')

      await publish.trigger('click')
      await settle()

      // No save first: there is nothing unsaved, and no grant to save with.
      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        `post /workflow/definitions/${ID}/publication`,
      ])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
    })

    it('offers no publish on a published revision, whatever the caller holds', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render()

      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
    })
  })

  // --- The test-engineer's verification of #709, 2026-10-10 -------------------
  //
  // The fixes' claims, probed past the tests that came with them: every
  // control while a request is in flight (not a sample of five), a publisher
  // without update on every status, the description column across saves, the
  // `formData.` prefix at its edges, and the NOT_A_DRAFT re-read when it fails.
  // One defect was pinned `it.fails`: a publish's 409 did not re-read. Fixed
  // 2026-10-10, and the test is a plain `it`. Seen red, 2026-10-10, three
  // mutations over the fix: no re-read in publish's catch (*reads the revision
  // again when a publish is refused…*, *offers an editor New revision after a
  // publish refused…*); a re-read after any publish refusal (*reads nothing
  // again after a 422…*, *…after a 403…*); publish keyed on NOT_A_DRAFT as a
  // save is (the first two again).
  //
  // Seen to fail (coding standard §2.9): twenty-eight mutations over #709's
  // fixes, run 2026-10-10. Twenty-five red, three survive as equivalent. Rows
  // marked * survived the builder's tests and were killed only by tests here.
  //
  // | # | Mutation | Reddened |
  // |---|---|---|
  // | V01 | `readOnly` ignores `isSaving` | *takes no edit while a save is in flight…* and five more |
  // | V02* | Undo not disabled while saving | *leaves no control operable while a save is in flight…* |
  // | V03* | The expression assignment's builder never disabled | *leaves no control operable while a save is in flight…* |
  // | V04 | Add transition never disabled | the read-only tests, and *…while a save is in flight…* |
  // | V05 | Publish offered on any status | *opens read-only, says why…* and more |
  // | V06* | Publish offered with no stored draft | *offers a caller with publish alone nothing on a new workflow…* |
  // | V07 | `publish()` saves first whatever `editable` is | **Survives, equivalent**: a caller who cannot edit has no edit to make dirty |
  // | V08 | `''` sent whenever a record exists | *sends no description for a workflow that never had one* |
  // | V09 | `''` sent when the column already holds `''` | **Survives, equivalent**: `''` over `''` stores `''` |
  // | V10 | A cleared description left out (the pre-fix) | *clears the stored description…*, *…cleared a second time* |
  // | V11 | A bare prefix counts as under it | *counts formData. as offered…: false* |
  // | V12* | The prefix matched case-insensitively | *counts FormData.amount…*, *counts formdata.amount…* |
  // | V13* | The prefix without its dot | *counts formDataX as offered…: false* and three more |
  // | V14* | No prefixes on the expression assignment | *counts a formData path as offered in an expression assignment…* |
  // | V15* | A failed re-read not caught | the run fails on an unhandled rejection during *…when the re-read fails* |
  // | V16* | The re-read not awaited, so it runs after `isSaving` clears | *…while NOT_A_DRAFT’s re-read is in flight* |
  // | V17 | The re-read replaces the record whole | **Survives, equivalent**: `record.definition` is read only after a publish replaces it |
  // | V18 | The re-read reloads the draft | *reads the revision again after NOT_A_DRAFT…* and two more |
  // | V19 | The re-read after any 422 | *reads nothing again after a refusal that is not NOT_A_DRAFT* |
  // | V20 | No re-read | *reads the revision again after NOT_A_DRAFT…* and three more |
  // | V21 | A publish's refusal not placed | *places a refusal of that publish where it names…* and two more |
  // | V22* | The task note shown on a read-only card | *offers a caller with publish alone no publish… on an ACTIVE revision* |
  // | V23 | Publish not disabled while saving | *takes no edit while a save is in flight…* and three more |
  // | V24 | Publish offered only to an editor (the pre-fix) | *offers publish to a caller who may publish and not update…* and three more |
  // | V25 | S6 takes the first declared code, not the longest | *puts S6 on the state it names when that code holds a backtick* |
  // | V26 | `publish()` does not set `isSaving` | *takes no edit while a publish is in flight*, *…through a save and the publish after it* |
  // | V27* | The read-only notice offers publish with no stored draft | *offers a caller with publish alone nothing on a new workflow…* |
  // | V28 | Undo and redo shown to a caller who cannot edit | *lets a reader read a draft…* and more |

  type Adapter = (config: InternalAxiosRequestConfig) => Promise<AxiosResponse>

  /**
   * Holds the first request `matches` accepts until the returned function is
   * called; every other request is answered at once. {@link holdNextRequest}
   * holds only the next one, which cannot reach the publish after a save.
   */
  function holdRequest(matches: (config: InternalAxiosRequestConfig) => boolean) {
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
      await settle()
    }
  }

  /** Fails the next read the way an unreachable server does; writes go through. */
  function failNextRead(): void {
    const fake = apiClient.defaults.adapter as Adapter

    apiClient.defaults.adapter = ((config: InternalAxiosRequestConfig) => {
      if ((config.method ?? 'get').toLowerCase() !== 'get') {
        return fake(config)
      }

      apiClient.defaults.adapter = fake

      return Promise.reject(new AxiosError('Network Error', AxiosError.ERR_NETWORK, config))
    }) as Adapter
  }

  const PUBLISHER = ['workflow:definition:read', 'workflow:definition:publish']

  function notADraft(): FakeReply {
    return {
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'status',
          rule: 'immutable',
          code: 'NOT_A_DRAFT',
          message: 'revision 1 of `purchase_approval` is published and cannot be edited',
        },
      ]),
    }
  }

  /**
   * Tries every way a keyboard or pointer could edit: each shortcut an editor
   * might bind, on the page and on a state card, a press of every button but
   * the view-only *Collapse* and *Expand*, and focus on every field.
   */
  function tryEverything(page: VueWrapper): void {
    for (const key of ['z', 'y', 's', 'Enter', 'Delete', 'Backspace']) {
      for (const target of [document, page.get('[data-testid="state-0"]').element]) {
        target.dispatchEvent(
          new KeyboardEvent('keydown', { key, ctrlKey: key.length === 1, bubbles: true }),
        )
      }
    }

    for (const button of page.findAll('button')) {
      if (!/^(Collapse|Expand) /.test(button.attributes('aria-label') ?? '')) {
        ;(button.element as HTMLButtonElement).click()
      }
    }

    for (const field of page.findAll('input, select, textarea')) {
      ;(field.element as HTMLElement).focus()
      expect(document.activeElement).not.toBe(field.element)
    }
  }

  function expressionAssigned(): JwssDefinition {
    const loaded = jwss()

    loaded.states[0].task!.assignment = {
      assigneeType: 'EXPRESSION',
      expression: { '==': [{ var: 'formData.amount' }, 1000] },
    }

    return loaded
  }

  describe('verification of #709: nothing edits while a request is in flight', () => {
    it('leaves no control operable while a save is in flight, and the draft is what was sent', async () => {
      // An expression assignment puts a second logic builder on the screen,
      // and the edit before the save makes Undo live.
      stored = record({ definition: expressionAssigned() })

      const page = await render({ attach: true })

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeUndefined()

      const release = holdNextRequest()

      await page.get('[data-testid="save-workflow"]').trigger('click')

      expect(enabledControls(page)).toEqual([])

      tryEverything(page)
      await settle()

      expect(page.findAll('section[aria-label^="State "]')).toHaveLength(3)

      await release()

      expect(writes()).toHaveLength(1)
      expect(lastWrite().name).toBe('Renamed')
      expect(page.findAll('section[aria-label^="State "]')).toHaveLength(3)
      // What the reply loaded is what was sent: saving again sends it unchanged,
      // and the history starts at what was stored.
      expect(page.get('[data-testid="undo"]').attributes('disabled')).toBeDefined()

      await save(page)

      expect(writes()[1].body).toEqual(writes()[0].body)
    })

    it('leaves no control operable through a save and the publish after it', async () => {
      const page = await render({ attach: true })

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')

      const release = holdRequest((config) => (config.url ?? '').endsWith('/publication'))

      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      // The save has been answered and the publish, held, is not yet recorded.
      expect(writes().map((request) => request.method)).toEqual(['put'])
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(enabledControls(page)).toEqual([])

      tryEverything(page)
      await release()

      expect(writes()).toHaveLength(2)
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })

    it('leaves no control operable while NOT_A_DRAFT’s re-read is in flight', async () => {
      const page = await render({ attach: true })

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      stored = record({ status: 'ACTIVE' })
      refuse = notADraft()

      const release = holdRequest((config) => (config.method ?? 'get').toLowerCase() === 'get')

      await page.get('[data-testid="save-workflow"]').trigger('click')
      await settle()

      expect(writes()).toHaveLength(1)
      expect(enabledControls(page)).toEqual([])

      await release()

      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })
  })

  describe('verification of #709: publishing without update', () => {
    it.each([
      ['publish alone', PUBLISHER, []],
      ['create and publish', [...PUBLISHER, 'workflow:definition:create'], ['new-revision']],
    ])(
      'lets a caller with %s operate Publish and nothing else, and publishes the stored draft',
      async (_, permissions, afterwards) => {
        const page = await render({ permissions, attach: true })

        expect(enabledControls(page)).toEqual(['publish-workflow'])
        expect(page.find('[data-testid="undo"]').exists()).toBe(false)
        expect(page.find('[data-testid="redo"]').exists()).toBe(false)

        for (const key of ['z', 'y', 's', 'Enter', 'Delete']) {
          document.dispatchEvent(
            new KeyboardEvent('keydown', { key, ctrlKey: key.length === 1, bubbles: true }),
          )
        }
        for (const field of page.findAll('input, select, textarea')) {
          ;(field.element as HTMLElement).focus()
          expect(document.activeElement).not.toBe(field.element)
        }
        await settle()

        expect(writes()).toEqual([])
        expect(page.get('[data-testid="publish-workflow"]').text()).toBe('Publish')

        await page.get('[data-testid="publish-workflow"]').trigger('click')
        await settle()

        expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
          `post /workflow/definitions/${ID}/publication`,
        ])
        expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
        expect(enabledControls(page)).toEqual(afterwards)
      },
    )

    it.each(['ACTIVE', 'DEPRECATED'])(
      'offers a caller with publish alone no publish, and nothing else, on an %s revision',
      async (status) => {
        stored = record({ status })

        const page = await render({ permissions: PUBLISHER })

        expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
        expect(page.find('[data-testid="published-notice"]').exists()).toBe(true)
        expect(enabledControls(page)).toEqual([])
        // The note about unticking the task is for someone who can untick it.
        expect(page.find('[data-testid="state-task-unshown-0"]').exists()).toBe(false)
      },
    )

    it('offers a caller with publish alone nothing on a new workflow, which has no stored draft', async () => {
      const page = await render({ permissions: PUBLISHER, path: '/admin/workflows/new' })

      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
      expect(page.get('[data-testid="read-only-notice"]').text()).not.toContain('publish')
      expect(enabledControls(page)).toEqual([])
    })

    it('places a refusal of that publish where it names, on the read-only screen', async () => {
      const page = await render({ permissions: PUBLISHER })

      refuse = {
        status: 422,
        body: errorBody('VALIDATION_ERROR', 'Validation failed', [
          {
            path: 'definition.transitions.1.allowedBy',
            rule: 'liveRole',
            code: 'ROLE_NOT_LIVE',
            message: '`APPROVER` is not a live role in this tenant',
          },
          {
            path: 'definition.states',
            rule: 'S6',
            code: 'UNREACHABLE_STATE',
            message: '`REJECTED` cannot be reached from the initial state',
          },
          {
            path: 'definition.variables.0.key',
            rule: 'server',
            code: 'INVALID',
            message: 'threshold is not allowed here',
          },
        ]),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="transition-1"]').text()).toContain('is not a live role')
      expect(page.get('[data-testid="state-2-messages"]').text()).toContain(
        '`REJECTED` cannot be reached',
      )
      expect(page.get('[data-testid="unplaced-errors"]').text()).toContain(
        'threshold is not allowed here',
      )
      expect(page.get('[data-testid="unplaced-errors"]').text()).not.toContain('live role')
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(enabledControls(page)).toEqual(['publish-workflow'])
    })

    // Defect, low: a 409 on publish (somebody published first) is shown, and
    // the screen still says DRAFT and still offers Publish. A save's
    // NOT_A_DRAFT re-reads the revision (#709 item 8); a publish's CONFLICT did
    // not, and a caller without update has no save to reach that re-read
    // through, so every press refused again. Pinned `it.fails` by #709's
    // verification; a plain `it` since publish re-reads on its 409 (2026-10-10).
    it('reads the revision again when a publish is refused because it was published first', async () => {
      const page = await render({ permissions: PUBLISHER })

      stored = record({ status: 'ACTIVE' })
      refuse = {
        status: 409,
        body: errorBody('CONFLICT', 'revision 1 of `purchase_approval` is ACTIVE, not DRAFT'),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="form-error"]').text()).toContain('is ACTIVE, not DRAFT')
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
      expect(page.find('[data-testid="published-notice"]').exists()).toBe(true)
    })

    it('offers an editor New revision after a publish refused because it was published first', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      // The save goes through; the publish after it finds the revision taken.
      await save(page)

      stored = { ...stored, status: 'ACTIVE' }
      refuse = {
        status: 409,
        body: errorBody(
          'CONFLICT',
          'revision 1 of `purchase_approval` was published by another request',
        ),
      }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="save-workflow"]').exists()).toBe(false)
      expect(page.find('[data-testid="new-revision"]').exists()).toBe(true)
      expect(page.get('[data-testid="form-error"]').text()).toContain(
        'published by another request',
      )
    })

    it.each([
      ['a 422 about the draft', 422],
      ['a 403', 403],
    ])('reads nothing again after %s on publish', async (_reason, status) => {
      const page = await render({ permissions: PUBLISHER })

      refuse =
        status === 422
          ? validationReply([
              'definition.states.0.task.assignment.roleCode',
              '`APPROVER` is not a live role',
            ])
          : { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:publish') }
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(backend.requests.filter((request) => request.method === 'get')).toHaveLength(1)
      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(true)
    })
  })

  describe('verification of #709: clearing the description', () => {
    function description(page: VueWrapper) {
      return page.get('#workflow-description')
    }

    it('clears a description that was set and saved in this screen', async () => {
      const page = await render()

      await description(page).setValue('Routine purchases')
      await save(page)

      expect((writes()[0].body as { description?: string }).description).toBe('Routine purchases')
      expect(stored.description).toBe('Routine purchases')

      await description(page).setValue('')
      await save(page)

      const body = writes()[1].body as { description?: string; definition: object }

      expect(body.description).toBe('')
      expect('description' in body.definition).toBe(false)
      expect(stored.description).toBe('')
      expect((description(page).element as HTMLTextAreaElement).value).toBe('')
    })

    it('leaves the column clear when it is cleared a second time', async () => {
      const loaded = jwss()

      loaded.description = 'Old description'
      stored = record({ description: 'Old description', definition: loaded })

      const page = await render()

      await description(page).setValue('')
      await save(page)
      expect(stored.description).toBe('')

      await description(page).setValue('Typed again')
      await description(page).setValue('')
      await save(page)

      expect(stored.description).toBe('')
      expect('description' in stored.definition).toBe(false)

      // And a save that touches something else leaves it clear too.
      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      await save(page)

      expect(stored.description).toBe('')
    })

    it('sends none when a description typed and cleared was never saved', async () => {
      const page = await render()

      await description(page).setValue('Not kept')
      await description(page).setValue('')
      await save(page)

      expect('description' in (writes()[0].body as object)).toBe(false)
      expect(stored.description).toBeNull()
    })

    it('creates a new workflow with no description when one was typed and cleared', async () => {
      const page = await render({ path: '/admin/workflows/new' })

      await page.get('[data-testid="workflow-key"]').setValue('purchase_approval')
      await page.get('[data-testid="workflow-name"]').setValue('Purchase approval')
      await description(page).setValue('Not kept')
      await description(page).setValue('')
      await save(page)

      const body = writes()[0].body as { description?: string; definition: object }

      expect(body.description).toBeUndefined()
      expect('description' in body.definition).toBe(false)
      expect(stored.description).toBeNull()
    })
  })

  describe('verification of #709: the formData. prefix', () => {
    it.each([
      ['formData.amount', true],
      ['formData.a.b', true],
      ['formData.', false],
      ['formData', false],
      ['formDataX', false],
      ['formDataX.amount', false],
      ['FormData.amount', false],
      ['formdata.amount', false],
      ['document.formData.amount', false],
    ])('counts %s as offered in a condition: %s', async (path, offered) => {
      const page = await render()

      await byLabel(page, 'Transition 1 condition, operand 1: variable path').setValue(path)

      expect(
        page.get('[data-testid="transition-0-detail"]').text().includes('Not in the offered'),
      ).toBe(!offered)
    })

    it('counts a formData path as offered in an expression assignment, and flags one outside it', async () => {
      stored = record({ definition: expressionAssigned() })

      const page = await render()
      const card = page.get('[data-testid="state-0"]')

      expect(card.text()).not.toContain('Not in the offered')

      await byLabel(
        page,
        'State MANAGER_APPROVAL task assigned to: expression, operand 1: variable path',
      ).setValue('formDataX.amount')

      expect(card.text()).toContain('Not in the offered')
    })
  })

  describe('verification of #709: the NOT_A_DRAFT re-read', () => {
    it('leaves the screen a draft, with the refusal and the edit, when the re-read fails', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      stored = record({ status: 'ACTIVE' })
      refuse = notADraft()
      failNextRead()
      await save(page)

      expect(page.get('[data-testid="status"]').text()).toBe('DRAFT')
      expect(page.get('[data-testid="unplaced-errors"]').text()).toContain('is published')
      expect((page.get('[data-testid="workflow-name"]').element as HTMLInputElement).value).toBe(
        'Renamed',
      )
      expect(page.get('[data-testid="workflow-name"]').attributes('disabled')).toBeUndefined()
      expect(page.find('[data-testid="load-error"]').exists()).toBe(false)
    })

    it('reads again on the next refusal when the author keeps editing after a failed re-read', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      stored = record({ status: 'ACTIVE' })
      refuse = notADraft()
      failNextRead()
      await save(page)

      await page.get('[data-testid="state-name-0"]').setValue('Edited after the refusal')
      refuse = notADraft()
      await save(page)

      expect(lastWrite().name).toBe('Renamed')
      expect(lastWrite().definition.states[0].name).toBe('Edited after the refusal')
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect((page.get('[data-testid="state-name-0"]').element as HTMLInputElement).value).toBe(
        'Edited after the refusal',
      )
      expect(enabledControls(page)).toEqual(['new-revision'])
    })

    it('takes no edit after a re-read that found it published, keyboard included', async () => {
      const page = await render({ attach: true })

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      stored = record({ status: 'ACTIVE' })
      refuse = notADraft()
      await save(page)

      expect(enabledControls(page)).toEqual(['new-revision'])

      for (const key of ['z', 'y', 's', 'Enter', 'Delete']) {
        document.dispatchEvent(
          new KeyboardEvent('keydown', { key, ctrlKey: key.length === 1, bubbles: true }),
        )
      }
      await settle()

      expect(writes()).toHaveLength(1)
      expect(page.find('[data-testid="publish-workflow"]').exists()).toBe(false)
    })

    it('publishes nothing when Save and publish meets NOT_A_DRAFT, and turns read-only', async () => {
      const page = await render()

      await page.get('[data-testid="workflow-name"]').setValue('Renamed')
      stored = record({ status: 'ACTIVE' })
      refuse = notADraft()
      await page.get('[data-testid="publish-workflow"]').trigger('click')
      await settle()

      expect(writes().map((request) => request.method)).toEqual(['put'])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="new-revision"]').exists()).toBe(true)
    })
  })

  // --- Deprecating a revision (#713, plan 19 row 6b; D-101 B, D-108) ---------
  //
  // Seen to fail (coding standard §2.9): the twenty-six mutations run over this
  // row on 2026-10-10, and the tests each reddened here, are tabled in
  // `useWorkflowDeprecation.spec.ts`'s header.

  describe('deprecating a revision (#713)', () => {
    const TYPES = [
      {
        id: 'dt-1',
        typeCode: 'PURCHASE_REQUEST',
        name: 'Purchase request',
        category: null,
        formId: null,
        status: 'ACTIVE',
        createdAt: '2026-10-09T00:00:00Z',
        updatedAt: '2026-10-09T00:00:00Z',
      },
      {
        id: 'dt-2',
        typeCode: 'CAPEX_REQUEST',
        name: 'Capital expense',
        category: null,
        formId: null,
        status: 'DRAFT',
        createdAt: '2026-10-09T00:00:00Z',
        updatedAt: '2026-10-09T00:00:00Z',
      },
    ]

    async function ask(page: VueWrapper): Promise<void> {
      await page.get('[data-testid="deprecate-workflow"]').trigger('click')
      await settle()
    }

    function typeReads(): RecordedRequest[] {
      return backend.requests.filter((request) => request.url === '/document-types')
    }

    function definitionReads(): RecordedRequest[] {
      return backend.requests.filter(
        (request) => request.method === 'get' && request.url === `/workflow/definitions/${ID}`,
      )
    }

    function cancelButton(page: VueWrapper) {
      return page.findAll('button').find((button) => button.text() === 'Cancel')!
    }

    it.each([
      ['on an ACTIVE revision to a holder of deprecate', 'ACTIVE', DEPRECATOR, true],
      [
        'not on an ACTIVE revision to a caller without deprecate',
        'ACTIVE',
        EVERY_PERMISSION,
        false,
      ],
      ['not on a DRAFT revision', 'DRAFT', DEPRECATOR, false],
      ['not on a DEPRECATED revision', 'DEPRECATED', DEPRECATOR, false],
    ])('offers Deprecate %s', async (_case, status, permissions, offered) => {
      stored = record({ status })

      const page = await render({ permissions })

      expect(page.find('[data-testid="deprecate-workflow"]').exists()).toBe(offered)
    })

    it('warns before it deprecates, naming the document types still bound, read in one call', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)

      expect(typeReads()).toHaveLength(1)
      expect(typeReads()[0].params).toEqual({ workflowDefinitionId: ID, pageSize: 100 })
      expect(page.get('[data-testid="bound-types"]').text()).toContain(
        'blocks their submissions until each is bound to a published revision',
      )
      expect(page.findAll('[data-testid="bound-type"]').map((item) => item.text())).toEqual([
        'Purchase request (PURCHASE_REQUEST)',
        'Capital expense (CAPEX_REQUEST)',
      ])
      expect(page.find('[data-testid="bound-types-more"]').exists()).toBe(false)
      // Nothing is deprecated by the warning itself.
      expect(writes()).toEqual([])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
    })

    it('names the first hundred and says how many more there are', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES
      boundTotal = 130

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)

      expect(page.findAll('[data-testid="bound-type"]')).toHaveLength(2)
      expect(page.get('[data-testid="bound-types-more"]').text()).toBe('and 128 more, 130 in all.')
    })

    it('says no document type is bound when none is, and deprecates once confirmed', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)

      expect(page.get('[data-testid="bound-types-none"]').text()).toBe(
        'No document type is bound to this revision.',
      )
      expect(page.find('[data-testid="bound-types"]').exists()).toBe(false)

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(writes().map((request) => request.url)).toEqual([
        `/workflow/definitions/${ID}/deprecation`,
      ])
    })

    it('says it could not check without document-type:read, reads nothing, and still deprecates', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({
        permissions: DEPRECATOR.filter((permission) => permission !== 'document-type:read'),
      })

      await ask(page)

      expect(typeReads()).toEqual([])
      expect(page.get('[data-testid="bound-types-unchecked"]').text()).toContain(
        'You cannot read document types, so whether any is still bound to this revision could not be checked.',
      )
      expect(page.get('[data-testid="bound-types-unchecked"]').text()).toContain(
        'Deprecating blocks submissions for any document type still bound to it',
      )
      expect(page.get('[data-testid="confirm-action"]').attributes('disabled')).toBeUndefined()

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
    })

    it.each([
      [
        'refused',
        { status: 403, body: errorBody('FORBIDDEN', 'Missing document-type:read') },
        'You cannot read document types',
      ],
      [
        'failing',
        { status: 500, body: errorBody('INTERNAL_ERROR', 'The database is unreachable') },
        'could not be checked: The database is unreachable',
      ],
    ])(
      'says it could not check when the read is %s, and leaves cancel and confirm to the caller',
      async (_case, reply, says) => {
        stored = record({ status: 'ACTIVE' })
        typesReply = reply as FakeReply

        const page = await render({ permissions: DEPRECATOR })

        await ask(page)

        expect(page.get('[data-testid="bound-types-unchecked"]').text()).toContain(says)
        expect(page.find('[data-testid="bound-type"]').exists()).toBe(false)
        expect(page.get('[data-testid="confirm-action"]').attributes('disabled')).toBeUndefined()
        expect(cancelButton(page).attributes('disabled')).toBeUndefined()
      },
    )

    it('holds the confirm button, not Cancel, while the bound types are being read', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES

      const page = await render({ permissions: DEPRECATOR })
      const release = holdRequest((config) => config.url === '/document-types')

      await ask(page)

      expect(page.find('[data-testid="bound-types-checking"]').exists()).toBe(true)
      expect(page.get('[data-testid="confirm-action"]').attributes('disabled')).toBeDefined()
      expect(cancelButton(page).attributes('disabled')).toBeUndefined()

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await release()

      expect(writes()).toEqual([])
      expect(page.findAll('[data-testid="bound-type"]')).toHaveLength(2)
      expect(page.get('[data-testid="confirm-action"]').attributes('disabled')).toBeUndefined()
    })

    it('does nothing when cancelled', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)
      await cancelButton(page).trigger('click')
      await settle()

      expect(page.find('[data-testid="confirm-action"]').exists()).toBe(false)
      expect(writes()).toEqual([])
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.get('[data-testid="published-notice"]').text()).toContain('is published')
      expect(page.find('[data-testid="deprecate-workflow"]').exists()).toBe(true)
    })

    it('turns the screen read-only and says it is deprecated once it succeeds', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)
      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(page.find('[data-testid="confirm-action"]').exists()).toBe(false)
      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
      expect(page.get('[data-testid="published-notice"]').text()).toContain(
        'This revision is deprecated and cannot be edited',
      )
      expect(page.get('[data-testid="notice"]').text()).toContain(
        'New documents no longer route to revision 1',
      )
      expect(page.find('[data-testid="deprecate-workflow"]').exists()).toBe(false)
      expect(enabledControls(page)).toEqual(['new-revision'])
    })

    it.each([
      [
        'already deprecated',
        'DEPRECATED',
        'revision 1 of `purchase_approval` is already deprecated',
      ],
      [
        'a draft',
        'DRAFT',
        'revision 1 of `purchase_approval` is a draft, and only a published revision can be deprecated; nothing routes to a draft, so delete it instead',
      ],
    ])('reads the revision again when the route answers 409, %s', async (_case, now, message) => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)
      stored = record({ status: now })
      refuse = { status: 409, body: errorBody('CONFLICT', message) }
      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(definitionReads()).toHaveLength(2)
      expect(page.get('[data-testid="form-error"]').text()).toBe(message)
      expect(page.get('[data-testid="status"]').text()).toBe(now)
      expect(page.find('[data-testid="deprecate-workflow"]').exists()).toBe(false)
    })

    it.each([
      [
        '403',
        { status: 403, body: errorBody('FORBIDDEN', 'Missing workflow:definition:deprecate') },
      ],
      ['404', { status: 404, body: errorBody('NOT_FOUND', 'Workflow definition not found') }],
    ])('reads nothing again after a %s, and says why', async (_case, reply) => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR })

      await ask(page)
      // Were it read again, the screen would say DEPRECATED.
      stored = record({ status: 'DEPRECATED' })
      refuse = reply as FakeReply
      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(definitionReads()).toHaveLength(1)
      expect(page.get('[data-testid="form-error"]').text()).toContain(
        (reply.body as { error: { message: string } }).error.message,
      )
      expect(page.get('[data-testid="status"]').text()).toBe('ACTIVE')
      expect(page.find('[data-testid="deprecate-workflow"]').exists()).toBe(true)
    })

    it('leaves no control operable while the deprecation is in flight', async () => {
      stored = record({ status: 'ACTIVE' })
      boundTypes = TYPES

      const page = await render({ permissions: DEPRECATOR, attach: true })

      await ask(page)

      const release = holdRequest((config) => (config.url ?? '').endsWith('/deprecation'))

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(enabledControls(page)).toEqual([])

      tryEverything(page)
      await settle()

      expect(page.find('[data-testid="confirm-action"]').exists()).toBe(false)

      await release()

      expect(writes()).toHaveLength(1)
      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
      expect(enabledControls(page)).toEqual(['new-revision'])
    })

    it('leaves no control operable while a 409’s re-read is in flight', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR, attach: true })

      await ask(page)
      stored = record({ status: 'DEPRECATED' })
      refuse = {
        status: 409,
        body: errorBody('CONFLICT', 'revision 1 of `purchase_approval` is already deprecated'),
      }

      const release = holdRequest(
        (config) =>
          (config.method ?? 'get').toLowerCase() === 'get' &&
          config.url === `/workflow/definitions/${ID}`,
      )

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(writes()).toHaveLength(1)
      expect(enabledControls(page)).toEqual([])

      await release()

      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
    })
  })

  // --- The test-engineer campaign on #713, 2026-10-10 --------------------------
  //
  // Past the builder's tests: what the success path shows, the hundred-type
  // boundary, and the warning by keyboard. Seen to fail: the screen keeping
  // its own definition after a success (*shows the revision the route
  // answers…*), and `unnamed >= 0` (*names exactly a hundred…*). Focus is not
  // handed back when the warning closes: pinned with `it.fails` below.

  describe('deprecating a revision, adversarially (#713)', () => {
    function hundred(): unknown[] {
      return Array.from({ length: 100 }, (_, index) => ({
        id: `dt-${index}`,
        typeCode: `TYPE_${String(index).padStart(3, '0')}`,
        name: `Type ${index}`,
        category: null,
        formId: null,
        status: index % 2 === 0 ? 'ACTIVE' : 'DEPRECATED',
        createdAt: '2026-10-09T00:00:00Z',
        updatedAt: '2026-10-09T00:00:00Z',
      }))
    }

    function opener(page: VueWrapper): HTMLButtonElement {
      return page.get('[data-testid="deprecate-workflow"]').element as HTMLButtonElement
    }

    it('shows the revision the route answers, not the one it had on screen', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR })

      await page.get('[data-testid="deprecate-workflow"]').trigger('click')
      await settle()

      // The route reads the revision back after the change; here its name as
      // stored differs from what this screen loaded, so only taking the answer
      // shows it.
      const answered = jwss()

      answered.name = 'Purchase approval, as stored'
      stored = record({ status: 'ACTIVE', definition: answered, updatedAt: '2026-10-10T09:00:00Z' })

      await page.get('[data-testid="confirm-action"]').trigger('click')
      await settle()

      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
      expect(page.get('[data-testid="workflow-name"]').element).toHaveProperty(
        'value',
        'Purchase approval, as stored',
      )
      // Taken from the answer, not read again.
      expect(
        backend.requests.filter(
          (request) => request.method === 'get' && request.url === `/workflow/definitions/${ID}`,
        ),
      ).toHaveLength(1)
    })

    it.each([
      [100, null],
      [101, 'and 1 more, 101 in all.'],
    ])(
      'names exactly a hundred of %i bound types, and says what is past them',
      async (total, more) => {
        stored = record({ status: 'ACTIVE' })
        boundTypes = hundred()
        boundTotal = total

        const page = await render({ permissions: DEPRECATOR })

        await page.get('[data-testid="deprecate-workflow"]').trigger('click')
        await settle()

        expect(page.findAll('[data-testid="bound-type"]')).toHaveLength(100)
        expect(page.get('[data-testid="bound-types"]').text()).toContain('These document types are')

        if (more === null) {
          expect(page.find('[data-testid="bound-types-more"]').exists()).toBe(false)
        } else {
          expect(page.get('[data-testid="bound-types-more"]').text()).toBe(more)
        }

        // A bound type is named, not linked: deprecated or not, the warning
        // offers nothing to open, so nothing the caller cannot open is offered.
        expect(page.get('[data-testid="bound-types"]').findAll('a, button')).toHaveLength(0)
      },
    )

    it('is operated by keyboard alone: Escape cancels, Enter on Deprecate confirms', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR, attach: true })

      opener(page).focus()
      press('Enter')
      await settle()

      const panel = page.get('[role="dialog"]').element

      expect(panel.contains(document.activeElement)).toBe(true)

      document.activeElement!.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
      )
      await settle()

      expect(page.find('[role="dialog"]').exists()).toBe(false)
      expect(writes()).toEqual([])

      opener(page).focus()
      press('Enter')
      await settle()

      tabTo((element) => element.getAttribute('data-testid') === 'confirm-action')
      press('Enter')
      await settle()

      expect(writes().map((request) => `${request.method} ${request.url}`)).toEqual([
        `post /workflow/definitions/${ID}/deprecation`,
      ])
      expect(page.get('[data-testid="status"]').text()).toBe('DEPRECATED')
    })

    // DEFECT (test-engineer, 2026-10-10, PR #715): closing the warning leaves
    // focus on `<body>`, not on the *Deprecate* button that opened it. The
    // shared `Dialog` moves focus in on open and never hands it back (WAI-ARIA
    // dialog pattern), so a keyboard user restarts from the top of the page.
    // Not this row's code; it is every `ConfirmDialog`'s. Flip to `it` when
    // `Dialog` restores focus.
    it.fails('hands focus back to Deprecate when the warning is cancelled', async () => {
      stored = record({ status: 'ACTIVE' })

      const page = await render({ permissions: DEPRECATOR, attach: true })

      opener(page).focus()
      press('Enter')
      await settle()

      await page
        .findAll('button')
        .find((button) => button.text() === 'Cancel')!
        .trigger('click')
      await settle()

      expect(page.find('[role="dialog"]').exists()).toBe(false)
      expect(document.activeElement).toBe(opener(page))
    })
  })
})
