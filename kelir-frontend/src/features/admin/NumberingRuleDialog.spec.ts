import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import NumberingRuleDialog from './NumberingRuleDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
  type RecordedRequest,
} from '@/lib/testing/fake-backend'
import type { DocumentTypeSummary, NumberingRule } from '@/types/document-type'

/**
 * A document type's numbering rule: where a refusal is shown (#641).
 *
 * `ruleTemplate` and `sequencePadding` are placed under their inputs. The two
 * Selects offer a closed list, so a refusal of `sequenceScope` or `gapPolicy`
 * is of a stale value and is listed, as is `nextSequence`, which the dialog
 * does not send. The details are `document_type::numbering::validate_set`'s.
 */

type Detail = [path: string, rule: string, code: string, message: string]

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

const MISSING_SEQUENCE: Detail = [
  'ruleTemplate',
  'format',
  'MISSING_SEQUENCE',
  'ruleTemplate must contain {sequence}; without it every document of this type is numbered the same, and the second one is refused at submit having already done the work',
]
const PADDING_OUT_OF_RANGE: Detail = [
  'sequencePadding',
  'range',
  'OUT_OF_RANGE',
  'sequencePadding must be between 1 and 20',
]
const NEXT_NOT_SUPPORTED: Detail = [
  'nextSequence',
  'conflict',
  'NOT_SUPPORTED_FOR_SCOPE',
  'a DEPARTMENT_YEAR rule counts separately in each department and each starts at 1, so nextSequence cannot be set; it was accepted and ignored before, which is why it is refused now',
]
/** `extract::detail`'s answer to an enum variant that does not exist. */
const SCOPE_UNKNOWN: Detail = [
  'sequenceScope',
  'type',
  'INVALID_TYPE',
  'Unknown variant `QUARTER`, expected one of `GLOBAL`, `YEAR`, `MONTH`, `DEPARTMENT_YEAR`',
]
const POLICY_UNKNOWN: Detail = [
  'gapPolicy',
  'type',
  'INVALID_TYPE',
  'Unknown variant `SOMETIMES`, expected `GAPLESS` or `ALLOW_GAPS`',
]
const UNKNOWN_FIELD: Detail = [
  'unknownField',
  'unknownField',
  'UNKNOWN_FIELD',
  'Unknown field `unknownField`. Check the spelling and casing — the API is camelCase.',
]

const requisition: DocumentTypeSummary = {
  id: '0199a1a0-0000-7000-8000-0000000000d1',
  typeCode: 'PR',
  name: 'Purchase requisition',
  category: null,
  formId: null,
  status: 'ACTIVE',
  createdAt: '2026-11-04T00:00:00Z',
  updatedAt: '2026-11-04T00:00:00Z',
}

const order: DocumentTypeSummary = {
  ...requisition,
  id: '0199a1a0-0000-7000-8000-0000000000d2',
  typeCode: 'PO',
  name: 'Purchase order',
}

const rule: NumberingRule = {
  id: '0199a1a0-0000-7000-8000-0000000000n1',
  documentTypeId: requisition.id,
  ruleTemplate: 'PR-{year}-{sequence}',
  sequenceScope: 'YEAR',
  sequencePadding: 6,
  gapPolicy: 'GAPLESS',
  sequenceKey: '2026',
  nextSequence: 1,
  isActive: true,
}

describe('NumberingRuleDialog: where a refusal is shown (#641)', () => {
  let backend: FakeBackendHandle
  /** What `GET …/numbering-rule` answers. */
  let readReply: FakeReply
  /** What `PUT …/numbering-rule` answers. */
  let saveReply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    readReply = { status: 200, body: itemBody(rule) }
    saveReply = { status: 200, body: itemBody(rule) }
    backend = installFakeBackend((request: RecordedRequest) =>
      request.method === 'put' ? saveReply : readReply,
    )
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  async function open(documentType: DocumentTypeSummary = requisition): Promise<VueWrapper> {
    const wrapper = mount(NumberingRuleDialog, { props: { open: true, documentType } })

    await flushPromises()

    return wrapper
  }

  async function save(wrapper: VueWrapper): Promise<void> {
    await wrapper.get('form').trigger('submit')
    await flushPromises()
  }

  function list(wrapper: VueWrapper) {
    return wrapper.find('[data-testid="numbering-unplaced-errors"]')
  }

  function listed(wrapper: VueWrapper): string[] {
    return list(wrapper)
      .findAll('li')
      .map((item) => item.text())
  }

  function blockOf(wrapper: VueWrapper, testId: string): string {
    return wrapper.get(`[data-testid="${testId}"]`).element.parentElement?.textContent ?? ''
  }

  function shown(wrapper: VueWrapper, message: string): number {
    return wrapper.text().split(message).length - 1
  }

  it('lists a detail at a path it shows no message for, and does not save', async () => {
    saveReply = refusal(UNKNOWN_FIELD)
    const wrapper = await open()

    await save(wrapper)

    expect(list(wrapper).attributes('role')).toBe('alert')
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    expect(wrapper.emitted('saved')).toBeUndefined()
  })

  it.each([
    ['template', 'rule-template', MISSING_SEQUENCE],
    ['padding', 'rule-padding', PADDING_OUT_OF_RANGE],
  ] as const)('places a refusal of the %s under its input', async (_, testId, detail) => {
    saveReply = refusal(detail)
    const wrapper = await open()

    await save(wrapper)

    expect(blockOf(wrapper, testId)).toContain(detail[3])
    expect(shown(wrapper, detail[3])).toBe(1)
    expect(list(wrapper).exists()).toBe(false)
  })

  // The Selects offer only what the backend accepts, so a refusal of one is of
  // a value that went stale; it is listed rather than given a slot.
  it('lists a refusal of either chooser and of the next sequence', async () => {
    saveReply = refusal(SCOPE_UNKNOWN, POLICY_UNKNOWN, NEXT_NOT_SUPPORTED)
    const wrapper = await open()

    await save(wrapper)

    expect(listed(wrapper)).toEqual([
      `sequenceScope: ${SCOPE_UNKNOWN[3]}`,
      `gapPolicy: ${POLICY_UNKNOWN[3]}`,
      `nextSequence: ${NEXT_NOT_SUPPORTED[3]}`,
    ])
  })

  it('shows a placed and an unplaced detail once each, each in its own place', async () => {
    saveReply = refusal(MISSING_SEQUENCE, UNKNOWN_FIELD)
    const wrapper = await open()

    await save(wrapper)

    expect(blockOf(wrapper, 'rule-template')).toContain(MISSING_SEQUENCE[3])
    expect(shown(wrapper, MISSING_SEQUENCE[3])).toBe(1)
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
  })

  it('does not also say "Validation failed" when the details say why', async () => {
    saveReply = refusal(PADDING_OUT_OF_RANGE, UNKNOWN_FIELD)
    const wrapper = await open()

    await save(wrapper)

    expect(wrapper.find('[data-testid="numbering-error"]').exists()).toBe(false)
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
      'a 409',
      { status: 409, body: errorBody('CONFLICT', 'numbering rule was changed by another request') },
      'numbering rule was changed by another request',
    ],
    [
      'a network failure',
      { status: 0, networkError: true },
      'Could not reach the server. Check your connection and try again.',
    ],
  ] as [string, FakeReply, string][])(
    'shows %s as one message above the form, and no list',
    async (_, failure, message) => {
      saveReply = failure
      const wrapper = await open()

      await save(wrapper)

      expect(wrapper.get('[data-testid="numbering-error"]').text()).toBe(message)
      expect(shown(wrapper, message)).toBe(1)
      expect(list(wrapper).exists()).toBe(false)
      expect(wrapper.emitted('saved')).toBeUndefined()
    },
  )

  it('opens again without the last refusal, placed or listed', async () => {
    saveReply = refusal(MISSING_SEQUENCE, UNKNOWN_FIELD)
    const wrapper = await open()

    await save(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ documentType: order })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, MISSING_SEQUENCE[3])).toBe(0)
  })

  it('opens again without the last message above the form', async () => {
    saveReply = { status: 403, body: errorBody('FORBIDDEN', 'Not yours to number') }
    const wrapper = await open()

    await save(wrapper)
    expect(wrapper.find('[data-testid="numbering-error"]').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(wrapper.find('[data-testid="numbering-error"]').exists()).toBe(false)
  })

  it('clears the last refusal when Save is pressed again', async () => {
    saveReply = refusal(PADDING_OUT_OF_RANGE, UNKNOWN_FIELD)
    const wrapper = await open()

    await save(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    saveReply = { status: 200, body: itemBody(rule) }
    await save(wrapper)

    expect(list(wrapper).exists()).toBe(false)
    expect(shown(wrapper, PADDING_OUT_OF_RANGE[3])).toBe(0)
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })

  it('still shows a failed read other than a 404 above the form', async () => {
    readReply = {
      status: 403,
      body: errorBody('FORBIDDEN', 'You do not have permission to perform this action'),
    }
    const wrapper = await open()

    expect(wrapper.get('[data-testid="numbering-error"]').text()).toBe(
      'You do not have permission to perform this action',
    )
    expect(list(wrapper).exists()).toBe(false)
    expect(wrapper.find('[data-testid="numbering-is-new"]').exists()).toBe(false)
  })

  it('still reads a 404 as a type with no rule yet, not as a failure', async () => {
    readReply = { status: 404, body: errorBody('NOT_FOUND', 'Numbering rule not found') }
    const wrapper = await open()

    expect(wrapper.find('[data-testid="numbering-is-new"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="numbering-error"]').exists()).toBe(false)
    expect((wrapper.get('[data-testid="rule-template"]').element as HTMLInputElement).value).toBe(
      'PR-{year}-{sequence}',
    )
  })

  it('sends the same request as before', async () => {
    const wrapper = await open()

    await save(wrapper)

    const put = backend.requests.find((request) => request.method === 'put')

    expect(put?.url).toBe(`/document-types/${requisition.id}/numbering-rule`)
    expect(put?.body).toEqual({
      ruleTemplate: 'PR-{year}-{sequence}',
      sequenceScope: 'YEAR',
      sequencePadding: 6,
      gapPolicy: 'GAPLESS',
    })
  })
})
