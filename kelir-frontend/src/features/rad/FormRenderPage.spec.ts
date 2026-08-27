import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import purchaseRequisition from './__fixtures__/purchase-requisition.json'
import FormRenderPage from './FormRenderPage.vue'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'

/**
 * Submitting a rendered form, and what the page does with the answer (#164).
 *
 * **Driven through the real API client** — `fake-backend.ts` replaces the axios
 * adapter and nothing else, so the envelope unwrapping, the `ApiError`
 * classification and the S10.3 `details` all run for real. A spec that stubbed
 * `submitForm` would be asserting that the page calls a function.
 *
 * **What is not here is whether the server's arithmetic is right.** That is
 * `kelir-backend/tests/rad_form_submissions.rs`, and the two sides are held to
 * one answer by `parity/forms.json`. What is here is the page's half of #164
 * AC5 and AC6: a refusal reaches the field it is about, and a value the server
 * stored differently is *shown* rather than swallowed.
 */

const blank = { template: '<div />' }

const FORM_ID = '0199a1a0-0000-7000-8000-000000000001'

/** The form the page fetches, as `GET /rad/forms/{id}` returns it. */
function form(): unknown {
  return {
    id: FORM_ID,
    formKey: 'purchase-requisition',
    title: 'Purchase requisition',
    revision: 1,
    jfssVersion: '2.0.1',
    status: 'PUBLISHED',
    entityId: null,
    definition: purchaseRequisition,
    publishedAt: '2026-08-27T00:00:00Z',
    publishedBy: null,
    createdAt: '2026-08-27T00:00:00Z',
    updatedAt: '2026-08-27T00:00:00Z',
  }
}

/** A stored submission, as `POST /rad/forms/{id}/submissions` returns it. */
function submission(payload: Record<string, unknown>): unknown {
  return {
    id: '0199a1a0-0000-7000-8000-0000000000ff',
    formId: FORM_ID,
    formRevision: 1,
    payload,
    submittedAt: '2026-08-27T09:00:00Z',
    submittedBy: null,
    createdAt: '2026-08-27T09:00:00Z',
    updatedAt: '2026-08-27T09:00:00Z',
  }
}

describe('FormRenderPage, submitting', () => {
  let backend: FakeBackendHandle
  let router: Router
  let onSubmit: (request: RecordedRequest) => FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    onSubmit = (request) => ({
      status: 201,
      body: itemBody(submission((request.body as { payload: Record<string, unknown> }).payload)),
    })

    backend = installFakeBackend((request) => {
      if (request.url.includes('/submissions')) {
        return onSubmit(request)
      }

      if (request.url.includes('/lookups/')) {
        return {
          status: 200,
          body: {
            success: true,
            data: [{ value: 'supplier-1', label: 'Acme Supplies', description: 'SUP-0001' }],
            meta: { page: 1, pageSize: 20, total: 1 },
          },
        }
      }

      return { status: 200, body: itemBody(form()) }
    })

    router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/forms/:id', name: 'form', component: blank }],
    })
  })

  afterEach(() => backend.restore())

  async function render(): Promise<VueWrapper> {
    await router.push(`/forms/${FORM_ID}`)
    await router.isReady()

    const wrapper = mount(FormRenderPage, { global: { plugins: [router] } })

    // The fetch, the engine's dynamic import, and the calculation passes its
    // arrival wakes.
    for (let round = 0; round < 6; round += 1) {
      await flushPromises()
    }

    return wrapper
  }

  /** Fills in the fixture far enough that its own rules let a submit through. */
  async function fillIn(wrapper: VueWrapper): Promise<void> {
    await wrapper.find('#jfss-title-field').setValue('Two standing desks')
    await wrapper.find('#jfss-supplier-field').setValue('supplier-1')
    // A native `<select>` carries strings and a JFSS option value is any JSON,
    // so `SelectField` addresses each option by its **index** — `'1'` is
    // "Normal". Setting `'NORMAL'` would match no option and leave the field
    // empty, which is a quiet way for a spec to assert that a form refuses.
    await wrapper.find('#jfss-priority-field').setValue('1')
    await wrapper.find('#jfss-row-0-line-description').setValue('Standing desk')
    await flushPromises()
  }

  async function press(wrapper: VueWrapper, label: string): Promise<void> {
    const button = wrapper.findAll('button').find((candidate) => candidate.text() === label)

    expect(button, `the form renders a "${label}" button`).toBeTruthy()

    await button?.trigger('click')
    await flushPromises()
  }

  it('posts every data key, hidden ones included (JFSS S10.1)', async () => {
    const wrapper = await render()

    await fillIn(wrapper)
    await press(wrapper, 'Submit request')

    const posted = backend.requests.find((request) => request.url.includes('/submissions'))

    expect(posted, 'a submission was posted').toBeTruthy()

    const payload = (posted?.body as { payload: Record<string, unknown> }).payload

    // `justification` is hidden — its `conditional` needs a budget above 1,000
    // and nothing typed one — and it is submitted anyway. S10.1.1 is why: a
    // conditional that depends on a hidden field would otherwise be decided
    // from different inputs on the two sides.
    expect(Object.keys(payload)).toContain('justification')
    expect(Object.keys(payload)).toContain('grand_total')
  })

  it('does not post at all while the definition’s own rules refuse', async () => {
    const wrapper = await render()

    // Nothing filled in: `title`, `supplier_id` and `priority` are all
    // `required` in the fixture.
    await press(wrapper, 'Submit request')

    expect(backend.countOf(`/rad/forms/${FORM_ID}/submissions`)).toBe(0)
    expect(wrapper.find('[data-testid="submit-success"]').exists()).toBe(false)
  })

  it('reports what was stored, and against which revision', async () => {
    const wrapper = await render()

    await fillIn(wrapper)
    await press(wrapper, 'Submit request')

    expect(wrapper.find('[data-testid="submit-success"]').text()).toContain('revision 1')
  })

  /**
   * **AC6 reaching the field it is about.** The refusal carries an S10.3 `path`,
   * and a path is not a `key` — `line_items.0.quantity` addresses a row.
   */
  it('places a server refusal against the row it names', async () => {
    onSubmit = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'line_items.0.quantity',
          rule: 'minimum',
          code: 'VALIDATION_FAILED',
          message: 'A line orders at least one.',
        },
      ]),
    })

    const wrapper = await render()

    await fillIn(wrapper)
    await press(wrapper, 'Submit request')

    expect(wrapper.find('[data-testid="submit-error"]').exists()).toBe(true)
    expect(wrapper.find('#jfss-row-0-line-quantity-error').text()).toBe(
      'A line orders at least one.',
    )
  })

  /**
   * A `server`-scoped rule has no client verdict to compete with — the
   * Validation Rule Registry §3.3 says the frontend shows the message only
   * after a failed submission — so the message must survive on a field the
   * client itself is happy with.
   */
  it('keeps a server message until the value it was about changes', async () => {
    onSubmit = () => ({
      status: 422,
      body: errorBody('VALIDATION_ERROR', 'Validation failed', [
        {
          path: 'cost_centre',
          rule: 'unique',
          code: 'VALIDATION_FAILED',
          message: 'That cost centre is already in use.',
        },
      ]),
    })

    const wrapper = await render()

    await fillIn(wrapper)
    await wrapper.find('#jfss-cost-centre-field').setValue('FN-0142')
    await press(wrapper, 'Submit request')

    expect(wrapper.find('#jfss-cost-centre-field-error').text()).toBe(
      'That cost centre is already in use.',
    )

    // Editing a *different* field leaves it standing: clearing everything on
    // any keystroke would erase a verdict only the server can reach.
    await wrapper.find('#jfss-notes-field').setValue('anything')
    await flushPromises()

    expect(wrapper.find('#jfss-cost-centre-field-error').exists()).toBe(true)

    // Correcting the field it was about clears it.
    await wrapper.find('#jfss-cost-centre-field').setValue('FN-0143')
    await flushPromises()

    expect(wrapper.find('#jfss-cost-centre-field-error').exists()).toBe(false)
  })

  /**
   * **AC5's other half.** Both sides run one engine compiled for two runtimes
   * and `parity/forms.json` holds them to the same answers, so this banner
   * should never appear — which is exactly why it is on the screen and not in a
   * log. *A form that changes your number without saying so is its own defect.*
   */
  it('says so when the server stored a different number from the one on screen', async () => {
    onSubmit = (request) => {
      const payload = (request.body as { payload: Record<string, unknown> }).payload

      return {
        status: 201,
        body: itemBody(submission({ ...payload, grand_total: 41 })),
      }
    }

    const wrapper = await render()

    await fillIn(wrapper)
    await wrapper.find('#jfss-row-0-line-quantity').setValue('2')
    await wrapper.find('#jfss-row-0-line-unit-price').setValue('10')
    await flushPromises()
    await press(wrapper, 'Submit request')

    const banner = wrapper.find('[data-testid="submit-corrections"]')

    expect(banner.exists()).toBe(true)
    expect(banner.text()).toContain('grand_total')
    expect(banner.text()).toContain('41')
  })

  /**
   * **A finding the browser harness made on 2026-08-27, kept where the unit
   * suite can see it.**
   *
   * `serde_json` serializes a map in key order and JavaScript serializes an
   * object in insertion order, so a datagrid row that came back completely
   * unchanged encoded differently on the two sides and every submission
   * reported every row as a value the server had altered. The banner that is
   * supposed to appear only when something is genuinely wrong was crying wolf
   * on every submit, and only a real backend produced the ordering that showed
   * it — every stub until then echoed the payload back as it arrived.
   */
  it('does not call a row corrected because the server spells its keys in order', async () => {
    onSubmit = (request) => {
      const payload = (request.body as { payload: Record<string, unknown> }).payload
      const rows = (payload.line_items as Record<string, unknown>[]) ?? []

      const sorted = rows.map((row) =>
        Object.fromEntries(Object.entries(row).sort(([left], [right]) => (left < right ? -1 : 1))),
      )

      return { status: 201, body: itemBody(submission({ ...payload, line_items: sorted })) }
    }

    const wrapper = await render()

    await fillIn(wrapper)
    await wrapper.find('#jfss-row-0-line-quantity').setValue('2')
    await wrapper.find('#jfss-row-0-line-unit-price').setValue('10')
    await flushPromises()
    await press(wrapper, 'Submit request')

    expect(wrapper.find('[data-testid="submit-success"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="submit-corrections"]').exists()).toBe(false)
  })

  it('shows nothing about corrections when the two agree', async () => {
    const wrapper = await render()

    await fillIn(wrapper)
    await press(wrapper, 'Submit request')

    expect(wrapper.find('[data-testid="submit-corrections"]').exists()).toBe(false)
  })

  /**
   * A refusal with nothing to place against a field — a 403, or a revision that
   * stopped being published between the render and the submit. Surfaced
   * verbatim, because a refused submit that looks like nothing happened is
   * worse than one that says why.
   */
  it('surfaces a refusal that names no field', async () => {
    onSubmit = () => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    })

    const wrapper = await render()

    await fillIn(wrapper)
    await press(wrapper, 'Submit request')

    expect(wrapper.find('[data-testid="submit-error"]').text()).toContain(
      'You do not have permission',
    )
    expect(wrapper.find('[data-testid="submit-success"]').exists()).toBe(false)
  })
})
