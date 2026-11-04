import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import FormCreateDialog from './FormCreateDialog.vue'
import { setNumberingRule } from '@/api/document-types'
import { ApiError } from '@/api/error'
import { createForm, createList, createMenu, updateList, updateMenu } from '@/api/rad'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'

/**
 * Creating a form definition: where a refusal is shown (#641).
 *
 * The dialog places `formKey` and `title` under their inputs and lists every
 * other path above the form (coding standard §3.4), so a refused create always
 * says why. The details are the backend's own (`rad::domain::form`'s
 * `require_key` and `require_title`, `jfss::shape_errors` for the seeded
 * definition, `extract::detail` for an unknown field), not invented shapes.
 */

type Detail = [path: string, rule: string, code: string, message: string]

/** A 422 `VALIDATION_ERROR` as `AppError::validation` answers it. */
function refusal(...details: Detail[]): FakeReply {
  return {
    status: 422,
    body: errorBody(
      'VALIDATION_ERROR',
      'Validation failed',
      details.map(([path, rule, code, message]) => ({ path, rule, code, message })),
    ),
  }
}

const KEY_TOO_LONG: Detail = [
  'formKey',
  'maxLength',
  'TOO_LONG',
  'formKey must be at most 64 characters',
]
const TITLE_REQUIRED: Detail = ['title', 'required', 'REQUIRED', 'title is required']
const DEFINITION_INVALID: Detail = [
  'definition.components.0.key',
  'jfss',
  'INVALID_DEFINITION',
  '"field 1" does not match "^[a-z][a-z0-9_]*$"',
]
const UNKNOWN_FIELD: Detail = [
  'unknownField',
  'unknownField',
  'UNKNOWN_FIELD',
  'Unknown field `unknownField`. Check the spelling and casing — the API is camelCase.',
]

const created = {
  id: '0199a1a0-0000-7000-8000-0000000000f1',
  formKey: 'purchase_requisition',
  title: 'Purchase requisition',
  revision: 1,
  jfssVersion: '2.0.1',
  status: 'DRAFT',
  entityId: null,
  definition: {},
  createdAt: '2026-11-04T00:00:00Z',
  updatedAt: '2026-11-04T00:00:00Z',
}

describe('FormCreateDialog: where a refusal is shown (#641)', () => {
  let backend: FakeBackendHandle
  let reply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    reply = { status: 201, body: itemBody(created) }
    backend = installFakeBackend(() => reply)
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  async function create(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('[data-testid="form-key"]').setValue('purchase_requisition')
    await wrapper.get('[data-testid="form-title"]').setValue('Purchase requisition')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
  }

  function open(): VueWrapper {
    return mount(FormCreateDialog, { props: { open: true } })
  }

  function list(wrapper: VueWrapper) {
    return wrapper.find('[data-testid="form-create-unplaced-errors"]')
  }

  function listed(wrapper: VueWrapper): string[] {
    return list(wrapper)
      .findAll('li')
      .map((item) => item.text())
  }

  /** The text of the block an input sits in: its label, hint and message. */
  function blockOf(wrapper: VueWrapper, testId: string): string {
    return wrapper.get(`[data-testid="${testId}"]`).element.parentElement?.textContent ?? ''
  }

  /** How many times `message` is on screen anywhere in the dialog. */
  function shown(wrapper: VueWrapper, message: string): number {
    return wrapper.text().split(message).length - 1
  }

  it('lists a detail at a path it shows no message for, and does not create', async () => {
    reply = refusal(UNKNOWN_FIELD)
    const wrapper = open()

    await create(wrapper)

    expect(list(wrapper).attributes('role')).toBe('alert')
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    expect(wrapper.emitted('created')).toBeUndefined()
  })

  it('lists a refusal of the seeded definition, which has no input here', async () => {
    reply = refusal(DEFINITION_INVALID)
    const wrapper = open()

    await create(wrapper)

    expect(listed(wrapper)).toEqual([`definition.components.0.key: ${DEFINITION_INVALID[3]}`])
  })

  it.each([
    ['key', 'form-key', KEY_TOO_LONG],
    ['title', 'form-title', TITLE_REQUIRED],
  ] as const)('places a refusal of the %s under its input', async (_, testId, detail) => {
    reply = refusal(detail)
    const wrapper = open()

    await create(wrapper)

    expect(blockOf(wrapper, testId)).toContain(detail[3])
    expect(shown(wrapper, detail[3])).toBe(1)
    expect(list(wrapper).exists()).toBe(false)
  })

  it('shows a placed and an unplaced detail once each, each in its own place', async () => {
    reply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = open()

    await create(wrapper)

    expect(blockOf(wrapper, 'form-title')).toContain(TITLE_REQUIRED[3])
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(1)
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    expect(list(wrapper).text()).not.toContain(TITLE_REQUIRED[3])
  })

  // AC8: the envelope's own message is not shown beside the details it carries.
  it('does not also say "Validation failed" when the details say why', async () => {
    reply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = open()

    await create(wrapper)

    expect(wrapper.find('[data-testid="form-create-error"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('Validation failed')
  })

  it.each([
    ['a 422 naming no field', refusal(), 'Validation failed'],
    [
      'a 403',
      {
        status: 403,
        body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
      },
      'You do not have permission to perform this action',
    ],
    [
      'a 409 for a key that is taken',
      {
        status: 409,
        body: errorBody(
          'CONFLICT',
          'form `purchase_requisition` already exists; publish the current revision and create the next one rather than creating the key again',
        ),
      },
      'form `purchase_requisition` already exists; publish the current revision and create the next one rather than creating the key again',
    ],
    [
      'a network failure',
      { status: 0, networkError: true },
      'Could not reach the server. Check your connection and try again.',
    ],
  ] as [string, FakeReply, string][])(
    'shows %s as one message above the form, and no list',
    async (_, failure, message) => {
      reply = failure
      const wrapper = open()

      await create(wrapper)

      expect(wrapper.get('[data-testid="form-create-error"]').text()).toBe(message)
      expect(shown(wrapper, message)).toBe(1)
      expect(list(wrapper).exists()).toBe(false)
      expect(wrapper.emitted('created')).toBeUndefined()
    },
  )

  it('opens again without the last refusal, placed or listed', async () => {
    reply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = open()

    await create(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(0)
  })

  it('opens again without the last message above the form', async () => {
    reply = { status: 403, body: errorBody('FORBIDDEN', 'Not yours to create') }
    const wrapper = open()

    await create(wrapper)
    expect(wrapper.find('[data-testid="form-create-error"]').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(wrapper.find('[data-testid="form-create-error"]').exists()).toBe(false)
  })

  it('clears the last refusal when Create is pressed again', async () => {
    reply = refusal(TITLE_REQUIRED, UNKNOWN_FIELD)
    const wrapper = open()

    await create(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    reply = { status: 201, body: itemBody(created) }
    await create(wrapper)

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, TITLE_REQUIRED[3])).toBe(0)
    expect(wrapper.emitted('created')).toHaveLength(1)
  })

  it('sends the same request as before', async () => {
    const wrapper = open()

    await create(wrapper)

    const post = backend.requests.find((request) => request.method === 'post')
    const body = post?.body as { formKey: string; title: string; definition: { formId: string } }

    expect(post?.url).toBe('/rad/forms')
    expect(body.formKey).toBe('purchase_requisition')
    expect(body.title).toBe('Purchase requisition')
    expect(body.definition.formId).toBe('purchase_requisition')
  })

  // test-engineer, #641 AC11: the whole body, from padded input, so a lost
  // trim or a changed seed is seen. Green before row 22's change as after it.
  it('sends the whole request as before, the key and title trimmed', async () => {
    const wrapper = open()

    await wrapper.get('[data-testid="form-key"]').setValue('  purchase_requisition  ')
    await wrapper.get('[data-testid="form-title"]').setValue('  Purchase requisition  ')
    await wrapper.get('form').trigger('submit')
    await flushPromises()

    const post = backend.requests.find((request) => request.method === 'post')

    expect(post?.body).toEqual({
      formKey: 'purchase_requisition',
      title: 'Purchase requisition',
      definition: {
        formId: 'purchase_requisition',
        version: '2.0.1',
        title: 'Purchase requisition',
        components: [
          {
            id: 'field_1',
            role: 'data',
            type: 'textfield',
            key: 'field_1',
            label: 'Field 1',
            validation: { type: 'string' },
          },
        ],
      },
    })
  })
})

/**
 * #641 AC3. The five forms hand `report` `toApiError(failure)`. That is a
 * guard, not a conversion: every call they make already rejects with an
 * `ApiError` (`postItem` and `putItem` throw `toApiError(error)`), so a 403 or
 * a network failure reads the same with it as without it, and the same as
 * before. This pins the premise; a call that stopped rejecting with an
 * `ApiError` would show *Something went wrong* without the guard.
 */
describe('the calls the five forms make (#641 AC3)', () => {
  let backend: FakeBackendHandle

  afterEach(() => backend.restore())

  const calls: [string, () => Promise<unknown>][] = [
    [
      'createForm',
      () =>
        createForm({
          formKey: 'k',
          title: 't',
          definition: { formId: 'k', version: '2.0.1', components: [] },
        }),
    ],
    ['createMenu', () => createMenu({ menuKey: 'k', label: 'l' })],
    ['updateMenu', () => updateMenu('id', { label: 'l' })],
    [
      'setNumberingRule',
      () => setNumberingRule('id', { ruleTemplate: 't', sequenceScope: 'YEAR' }),
    ],
    ['createList', () => createList({ listKey: 'k', title: 't', columns: [], filters: [] })],
    ['updateList', () => updateList('id', { title: 't' })],
  ]

  it.each(calls)('%s rejects with an ApiError when nothing reaches the server', async (_, call) => {
    backend = installFakeBackend(() => ({ status: 0, networkError: true }))

    await expect(call()).rejects.toBeInstanceOf(ApiError)
  })

  it.each(calls)('%s rejects with an ApiError on a 403', async (_, call) => {
    backend = installFakeBackend(() => ({
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    }))

    await expect(call()).rejects.toBeInstanceOf(ApiError)
  })
})
