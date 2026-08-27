import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import DocumentWorkspace from './DocumentWorkspace.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import { ALLOWED_TRANSITIONS, type DocumentStatus } from '@/types/document'

/**
 * The document workspace (#172).
 *
 * **Driven through the real API client** — `fake-backend.ts` replaces the axios
 * adapter and nothing else, so envelope unwrapping, `ApiError` classification
 * and the S10.3 `details` all run for real. A spec that stubbed `getDocument`
 * would be asserting that the workspace calls a function.
 *
 * What is **not** here is whether the backend numbers, validates or refuses
 * correctly: that is `kelir-backend/tests/documents*.rs`. What is here is the
 * workspace's half of AC1–AC4 — the right mode for the status, the refusal
 * reaching the screen, and a tab that says what will fill it.
 */

const blank = { template: '<div />' }

const DOCUMENT_ID = '0199a1a0-0000-7000-8000-0000000000d1'
const FORM_ID = '0199a1a0-0000-7000-8000-0000000000f1'

/** A definition small enough to assert over, carrying one required field. */
const DEFINITION = {
  formId: 'purchase-requisition',
  version: '2.0.1',
  title: 'Purchase requisition',
  components: [
    {
      id: 'subject-field',
      role: 'data',
      type: 'textfield',
      key: 'subject',
      label: 'Subject',
      validation: { type: 'string', required: true, maxLength: 200 },
    },
    {
      id: 'submit-button',
      role: 'action',
      type: 'button',
      label: 'Submit',
      action: 'submit',
    },
  ],
}

function document(overrides: Record<string, unknown> = {}): unknown {
  return {
    id: DOCUMENT_ID,
    documentRef: 'DOC-2026-000001',
    documentNumber: null,
    documentTypeId: '0199a1a0-0000-7000-8000-0000000000t1',
    formId: FORM_ID,
    title: 'Two standing desks',
    status: 'DRAFT',
    priority: 'NORMAL',
    formData: { subject: 'Two standing desks' },
    metadata: {},
    entityType: null,
    entityId: null,
    requestedForDepartmentId: null,
    requestedForFacilityId: null,
    requestedBy: null,
    submittedAt: null,
    createdBy: null,
    createdAt: '2026-08-27T00:00:00Z',
    updatedAt: '2026-08-27T00:00:00Z',
    ...overrides,
  }
}

function form(): unknown {
  return {
    id: FORM_ID,
    formKey: 'purchase-requisition',
    title: 'Purchase requisition',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'PUBLISHED',
    entityId: null,
    definition: DEFINITION,
    publishedAt: '2026-08-27T00:00:00Z',
    publishedBy: null,
    createdAt: '2026-08-27T00:00:00Z',
    updatedAt: '2026-08-27T00:00:00Z',
  }
}

describe('DocumentWorkspace', () => {
  let backend: FakeBackendHandle
  let router: Router
  let current: Record<string, unknown>
  let onSubmit: (request: RecordedRequest) => FakeReply
  let onUpdate: (request: RecordedRequest) => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    current = document() as Record<string, unknown>

    onSubmit = () => ({
      status: 200,
      body: itemBody({ ...current, status: 'SUBMITTED', documentNumber: 'PR-2026-000001' }),
    })
    onUpdate = () => ({ status: 200, body: itemBody(current) })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/submission')) {
        return onSubmit(request)
      }

      if (request.url.includes('/status-history')) {
        return {
          status: 200,
          body: itemBody([
            {
              previousStatus: null,
              status: 'DRAFT',
              changedBy: null,
              reason: null,
              changedAt: '2026-08-27T00:00:00Z',
            },
          ]),
        }
      }

      if (request.url.includes('/rad/forms/')) {
        return { status: 200, body: itemBody(form()) }
      }

      if (request.method === 'put') {
        return onUpdate(request)
      }

      return { status: 200, body: itemBody(current) }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/documents/:id', name: 'document', component: blank },
        { path: '/documents', name: 'documents', component: blank },
      ],
    })
  })

  afterEach(() => backend.restore())

  async function render(): Promise<VueWrapper> {
    await router.push(`/documents/${DOCUMENT_ID}`)
    await router.isReady()

    const wrapper = mount(DocumentWorkspace, { global: { plugins: [router] } })

    // The document fetch, the form fetch, the history fetch, the engine's
    // dynamic import, and the calculation pass its arrival wakes.
    for (let round = 0; round < 8; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  it('renders a draft in edit mode', async () => {
    // AC1. The `fieldset` is what closes every control at once, so its
    // `disabled` is the mode.
    const wrapper = await render()

    expect(wrapper.get('[data-testid="document-form"]').attributes('disabled')).toBeUndefined()
    expect(wrapper.find('[data-testid="save-draft"]').exists()).toBe(true)
  })

  it('renders a submitted document in read mode', async () => {
    current = document({ status: 'SUBMITTED', documentNumber: 'PR-2026-000001' }) as Record<
      string,
      unknown
    >

    const wrapper = await render()

    expect(wrapper.get('[data-testid="document-form"]').attributes('disabled')).toBeDefined()
    // And no Save, because there is nothing to save into.
    expect(wrapper.find('[data-testid="save-draft"]').exists()).toBe(false)
  })

  it('shows the status, the number and the reference without opening a tab', async () => {
    // AC2. The things a person opening a document is looking for.
    current = document({ status: 'SUBMITTED', documentNumber: 'PR-2026-000001' }) as Record<
      string,
      unknown
    >

    const wrapper = await render()

    expect(wrapper.get('[data-testid="document-status"]').text()).toBe('Submitted')
    expect(wrapper.get('[data-testid="document-number"]').text()).toBe('PR-2026-000001')
    expect(wrapper.get('[data-testid="document-ref"]').text()).toBe('DOC-2026-000001')
  })

  it('says a draft has no number yet rather than showing an empty cell', async () => {
    // An empty cell reads as a value that failed to load, which sends somebody
    // looking for a fault that is not there.
    const wrapper = await render()

    expect(wrapper.get('[data-testid="document-number"]').text()).toContain('submitted')
  })

  it('shows the refusal the API returned rather than a generic failure', async () => {
    // AC3, and the case that is easiest to get wrong: a 422 whose details name
    // `documentTypeId` has nothing on the form to attach to, so a workspace
    // that only placed details by `path` would show nothing at all.
    onSubmit = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'documentTypeId',
          rule: 'required',
          code: 'NO_NUMBERING_RULE',
          message:
            'this document type has no active numbering rule, so a number cannot be assigned',
        },
      ]),
    })

    const wrapper = await render()
    await press(wrapper, 'Submit')

    expect(wrapper.get('[data-testid="document-problem"]').text()).toContain('numbering rule')
  })

  it('names every tab a later phase will fill', async () => {
    // AC4: neither an empty tab nor a silent one.
    const wrapper = await render()

    for (const [tab, phase] of [
      ['workflow', 'Phase 5'],
      ['attachments', 'Phase 6'],
      ['comments', 'Phase 6'],
    ]) {
      await wrapper.get(`[data-testid="tab-${tab}"]`).trigger('click')

      // Scoped to the panel rather than to the first `empty-tab` on the page:
      // the tabs are `v-show`, so all three are in the DOM and an unscoped
      // lookup would assert about whichever came first — green over the wrong
      // panel, which is the shape a test is supposed to catch rather than have.
      const empty = wrapper.get(`[data-testid="panel-${tab}"] [data-testid="empty-tab"]`)
      expect(empty.text()).toContain(phase)
    }
  })

  it('offers only the transitions the backend will accept', async () => {
    // The client's copy of the legality table is advisory — the backend refuses
    // anything it gets wrong — but a button that always 422s is worse than none.
    current = document({ status: 'SUBMITTED', documentNumber: 'PR-2026-000001' }) as Record<
      string,
      unknown
    >

    const wrapper = await render()
    await wrapper.get('[data-testid="tab-history"]').trigger('click')

    for (const status of ALLOWED_TRANSITIONS.SUBMITTED) {
      expect(
        wrapper.find(`[data-testid="transition-${status}"]`).exists(),
        `${status} is offered from SUBMITTED`,
      ).toBe(true)
    }

    // And nothing outside it. COMPLETED from SUBMITTED is the case the backend
    // refuses by name.
    expect(wrapper.find('[data-testid="transition-COMPLETED"]').exists()).toBe(false)
  })

  it('offers no transition out of a terminal status', async () => {
    current = document({ status: 'COMPLETED', documentNumber: 'PR-2026-000001' }) as Record<
      string,
      unknown
    >

    const wrapper = await render()
    await wrapper.get('[data-testid="tab-history"]').trigger('click')

    const terminal: DocumentStatus[] = ['COMPLETED', 'CANCELLED', 'ARCHIVED', 'PENDING_APPROVAL']

    for (const status of terminal) {
      expect(ALLOWED_TRANSITIONS[status]).toEqual([])
    }

    expect(wrapper.findAll('[data-testid^="transition-"]')).toHaveLength(0)
  })

  it('saves the draft before submitting it', async () => {
    // The submit re-evaluates and numbers what is *stored*, so submitting
    // without saving would attach the number to the payload as it was before
    // the last keystroke.
    const wrapper = await render()
    await press(wrapper, 'Submit')

    const calls = backend.requests.map((request) => `${request.method} ${request.url}`)
    const save = calls.findIndex((call) => call === `put /documents/${DOCUMENT_ID}`)
    const submit = calls.findIndex((call) => call.endsWith('/submission'))

    expect(save, 'the draft is saved').toBeGreaterThanOrEqual(0)
    expect(submit, 'the draft is submitted').toBeGreaterThan(save)
  })
})

async function press(wrapper: VueWrapper, label: string): Promise<void> {
  const button = wrapper.findAll('button').find((candidate) => candidate.text() === label)

  expect(button, `the workspace renders a "${label}" button`).toBeTruthy()

  await button!.trigger('click')

  for (let round = 0; round < 4; round += 1) {
    await flushPromises()
  }
}
