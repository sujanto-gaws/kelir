import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import FormBuilderPage from './FormBuilderPage.vue'
import {
  errorBody,
  installFakeBackend,
  type FakeBackendHandle,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { useAuthStore } from '@/stores/auth'
import type { CurrentUser } from '@/types/auth'

/**
 * The form builder (#373).
 *
 * **The three properties this screen exists to hold**, each asserted here
 * because none of them is visible to the backend:
 *
 * 1. A published revision opens read-only, and *says so* rather than greying
 *    out forty fields with no explanation ([ADR-0027]).
 * 2. What the server refuses lands on the component it named, by the S10.3
 *    `path`, rather than in a banner above the whole definition.
 * 3. What is on screen after a save is what the server stored — read back, not
 *    the local copy that was sent.
 *
 * Whether a definition is valid JFSS is the backend's question and
 * `kelir-backend/tests/rad_forms.rs` holds it.
 *
 * # Seen to fail (coding standard §2.9)
 *
 * Four mutations, run 2026-09-08, all four red — each reddening exactly one
 * test, which is the property worth having: an assertion that goes red for
 * two unrelated reasons is an assertion nobody can act on.
 *
 * | Mutation | Reddened |
 * |---|---|
 * | `isReadOnly` drops `isPublished` and checks only the permission | *opens a published revision read-only and says why* |
 * | `detailsFor` returns every detail rather than the ones addressing its index | *shows a refusal against the component the server named* |
 * | `save` keeps the local definition instead of the one read back | *shows the definition the server stored, not the one it sent* |
 * | `removeComponent` leaves the lookup binding behind | *drops a lookup binding when its component is removed* |
 */

const FORM_ID = '0199a1a0-0000-7000-8000-0000000000f1'
const blank = { template: '<div />' }

function definition(overrides: Record<string, unknown> = {}) {
  return {
    formId: 'purchase_requisition',
    version: '2.0.1',
    title: 'Purchase requisition',
    components: [
      {
        id: 'field_1',
        role: 'data',
        type: 'textfield',
        key: 'field_1',
        label: 'Reference',
        validation: { type: 'string' },
      },
      {
        id: 'field_2',
        role: 'data',
        type: 'number',
        key: 'amount',
        label: 'Amount',
        validation: { type: 'number' },
      },
    ],
    ...overrides,
  }
}

function form(overrides: Record<string, unknown> = {}) {
  return {
    id: FORM_ID,
    formKey: 'purchase_requisition',
    title: 'Purchase requisition',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'DRAFT',
    entityId: null,
    definition: definition(),
    publishedAt: null,
    publishedBy: null,
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

const EVERY_PERMISSION = ['rad:form:read', 'rad:form:update', 'rad:form:publish']

describe('FormBuilderPage', () => {
  let backend: FakeBackendHandle
  let current: Record<string, unknown>
  let saveResponse: { status: number; body: unknown } | null
  let router: Router

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    current = form()
    saveResponse = null

    backend = installFakeBackend((request: RecordedRequest) => {
      if (request.method === 'put' && saveResponse) {
        return saveResponse
      }

      if (request.method === 'put') {
        return { status: 200, body: { success: true, data: current } }
      }

      // The lookup chooser and anything else the preview reaches for.
      if (request.url.includes('/lookups/')) {
        return {
          status: 200,
          body: { success: true, data: [], meta: { page: 1, pageSize: 20, total: 0 } },
        }
      }

      return { status: 200, body: { success: true, data: current } }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/admin/forms/:id', name: 'admin-form-builder', component: blank }],
    })
  })

  afterEach(() => backend.restore())

  async function render(permissions: string[] = EVERY_PERMISSION): Promise<VueWrapper> {
    useAuthStore().$patch({ user: principal(permissions) })

    await router.push(`/admin/forms/${FORM_ID}`)
    await router.isReady()

    const wrapper = mount(FormBuilderPage, { global: { plugins: [router] } })

    // The form fetch, the evaluator's dynamic import, and the pass its arrival
    // wakes in the preview.
    for (let round = 0; round < 8; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('edits a draft, component by component', async () => {
    const wrapper = await render()

    expect(wrapper.find('[data-testid="component-field_1"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="component-field_2"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="save-definition"]').exists()).toBe(true)
  })

  /**
   * **ADR-0027 on screen.** A published revision is what documents pinned, so
   * editing it would change what they were filled against. The screen refuses
   * the edit and explains — a disabled field with no reason reads as a bug.
   */
  it('opens a published revision read-only and says why', async () => {
    current = form({ status: 'PUBLISHED', publishedAt: '2026-09-08T01:00:00Z' })

    const wrapper = await render()

    expect(wrapper.find('[data-testid="published-notice"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="save-definition"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="new-revision"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="definition-title"]').attributes('disabled')).toBeDefined()
  })

  it('offers no edit to a caller who may only read', async () => {
    const wrapper = await render(['rad:form:read'])

    expect(wrapper.find('[data-testid="save-definition"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="add-field"]').attributes('disabled')).toBeDefined()
  })

  /**
   * **The whole point of the S10.3 `path`.** A refusal about the second
   * component belongs on the second component; forty fields with one banner
   * above them is the same information and none of the help.
   */
  it('shows a refusal against the component the server named', async () => {
    saveResponse = {
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'The definition is not valid', [
        {
          path: 'components.1.rules.0',
          rule: 'rule',
          code: 'RULE_NOT_REGISTERED',
          message: '`totallyMadeUp` is not a rule in the JFSS Validation Rule Registry',
        },
      ]),
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await flushPromises()

    const second = wrapper.find('[data-testid="component-errors-field_2"]')

    expect(second.exists()).toBe(true)
    expect(second.text()).toContain('totallyMadeUp')
    // And not on the first, which the server said nothing about.
    expect(wrapper.find('[data-testid="component-errors-field_1"]').exists()).toBe(false)
  })

  /**
   * A detail addressed to nothing on screen is still shown. The alternative is
   * a save that fails with a message nobody ever sees.
   */
  it('shows a refusal that names no component rather than dropping it', async () => {
    saveResponse = {
      status: 422,
      body: errorBody('VALIDATION_FAILED', 'The definition is not valid', [
        {
          path: 'settings.lookups.field_9',
          rule: 'lookup',
          code: 'LOOKUP_BINDING_UNKNOWN_COMPONENT',
          message: 'settings.lookups names no component `field_9`',
        },
      ]),
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="form-errors"]').text()).toContain('names no component')
  })

  /**
   * **What is on screen after a save is what was stored.** The server may
   * normalise, and a screen that kept its own copy would be showing a
   * definition no document will ever be filled against.
   */
  it('shows the definition the server stored, not the one it sent', async () => {
    saveResponse = {
      status: 200,
      body: {
        success: true,
        data: form({
          definition: definition({
            components: [
              {
                id: 'field_1',
                role: 'data',
                type: 'textfield',
                key: 'field_1',
                label: 'Reference number',
                validation: { type: 'string' },
              },
            ],
          }),
        }),
      },
    }

    const wrapper = await render()

    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="save-confirmed"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="component-field_2"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('Reference number')
  })

  /**
   * **A binding naming no component is refused at save** (`domain/jfss.rs`), so
   * a removal that left the entry behind would make the next save fail with a
   * message about a field no longer on screen.
   */
  it('drops a lookup binding when its component is removed', async () => {
    current = form({
      definition: definition({
        settings: { lookups: { field_2: 'supplier' } },
        components: [
          {
            id: 'field_1',
            role: 'data',
            type: 'textfield',
            key: 'field_1',
            label: 'Reference',
            validation: { type: 'string' },
          },
          {
            id: 'field_2',
            role: 'data',
            type: 'lookup',
            key: 'supplier_id',
            label: 'Supplier',
            validation: { type: 'string' },
          },
        ],
      }),
    })

    const wrapper = await render()

    await wrapper.find('[data-testid="remove-field_2"]').trigger('click')
    await wrapper.find('[data-testid="save-definition"]').trigger('click')
    await flushPromises()

    const put = backend.requests.find((request) => request.method === 'put')
    const sent = put?.body as { definition: { settings?: { lookups?: Record<string, string> } } }

    expect(sent.definition.settings?.lookups).toEqual({})
  })
})
