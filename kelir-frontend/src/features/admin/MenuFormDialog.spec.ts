import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import MenuFormDialog from './MenuFormDialog.vue'
import { registerSessionBridge } from '@/api/session'
import {
  errorBody,
  installFakeBackend,
  itemBody,
  type FakeBackendHandle,
  type FakeReply,
} from '@/lib/testing/fake-backend'
import type { MenuEntry } from '@/types/rad'

/**
 * A navigation entry's dialog: where a refusal is shown (#641, #672).
 *
 * `menuKey`, `label`, `routePath` and `parentMenuId` were placed before; #672
 * adds `icon` and `requiredPermission`, both bounded by the backend, each under
 * its own input with `aria-invalid` and `aria-describedby`. Any other path is
 * listed above the form. The details are copied from `rad::domain::menu`
 * (`bounded`, `check_route`), `rad::service::menu` (`duplicate_key`,
 * `refuse_a_cycle`) and `extract::detail`.
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

const KEY_TAKEN: Detail = [
  'menuKey',
  'unique',
  'DUPLICATE',
  'a menu entry called `purchasing` already exists in this tenant',
]
const LABEL_REQUIRED: Detail = ['label', 'required', 'REQUIRED', 'label is required']
const ROUTE_NOT_RELATIVE: Detail = [
  'routePath',
  'format',
  'ROUTE_NOT_RELATIVE',
  'a menu entry links somewhere in this application, so its route starts with a single `/` — an absolute URL is not something the navigation can open',
]
const PARENT_CYCLE: Detail = [
  'parentMenuId',
  'cycle',
  'MENU_WOULD_CYCLE',
  'that entry is already below this one, so nesting this one under it would make a loop the navigation could not draw',
]
const ICON_TOO_LONG: Detail = [
  'icon',
  'maxLength',
  'TOO_LONG',
  'icon must be at most 64 characters',
]
const PERMISSION_TOO_LONG: Detail = [
  'requiredPermission',
  'maxLength',
  'TOO_LONG',
  'requiredPermission must be at most 64 characters',
]
const UNKNOWN_FIELD: Detail = [
  'unknownField',
  'unknownField',
  'UNKNOWN_FIELD',
  'Unknown field `unknownField`. Check the spelling and casing — the API is camelCase.',
]

const purchasing: MenuEntry = {
  id: '0199a1a0-0000-7000-8000-0000000000m1',
  menuKey: 'purchasing',
  label: 'Purchasing',
  icon: 'shopping-cart',
  parentMenuId: null,
  routePath: null,
  requiredPermission: null,
  source: 'CONFIG',
  sortOrder: 0,
  isEnabled: true,
  createdAt: '2026-11-04T00:00:00Z',
  updatedAt: '2026-11-04T00:00:00Z',
}

const requisitions: MenuEntry = {
  ...purchasing,
  id: '0199a1a0-0000-7000-8000-0000000000m2',
  menuKey: 'requisitions',
  label: 'Requisitions',
  parentMenuId: purchasing.id,
  routePath: '/lists/purchase_requisitions',
  requiredPermission: 'document:read',
}

/** The two inputs #672 gave a slot, by test id, and the path each shows. */
const NEW_SLOTS = [
  ['icon', 'menu-icon', ICON_TOO_LONG],
  ['required permission', 'menu-permission', PERMISSION_TOO_LONG],
] as const

/** Every text input in the dialog, for checking that only one is marked. */
const INPUTS = [
  'menu-key',
  'menu-label',
  'menu-route',
  'menu-order',
  'menu-icon',
  'menu-permission',
]

describe('MenuFormDialog: where a refusal is shown (#641, #672)', () => {
  let backend: FakeBackendHandle
  let reply: FakeReply

  beforeEach(() => {
    setActivePinia(createPinia())
    window.localStorage.clear()

    reply = { status: 200, body: itemBody(purchasing) }
    backend = installFakeBackend(() => reply)
  })

  afterEach(() => {
    backend.restore()
    registerSessionBridge(null)
  })

  function open(editing: MenuEntry | null = null): VueWrapper {
    return mount(MenuFormDialog, {
      props: { open: true, editing, entries: [purchasing, requisitions] },
    })
  }

  async function save(wrapper: VueWrapper): Promise<void> {
    // Creating, the key and label start empty; editing, they hold the entry's.
    if ((wrapper.get('[data-testid="menu-key"]').element as HTMLInputElement).value === '') {
      await wrapper.get('[data-testid="menu-key"]').setValue('purchasing')
      await wrapper.get('[data-testid="menu-label"]').setValue('Purchasing')
    }
    await wrapper.get('form').trigger('submit')
    await flushPromises()
  }

  function list(wrapper: VueWrapper) {
    return wrapper.find('[data-testid="menu-unplaced-errors"]')
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

  /**
   * The message an input names through `aria-describedby`, drawn in the same
   * block as the input: empty when it names none.
   */
  function describedMessage(wrapper: VueWrapper, testId: string): string {
    const input = wrapper.get(`[data-testid="${testId}"]`)
    const id = input.attributes('aria-describedby')

    if (!id) {
      return ''
    }

    const message = wrapper.get(`[id="${id}"]`)

    expect(message.element.parentElement?.contains(input.element)).toBe(true)

    return message.text()
  }

  it('lists a detail at a path it shows no message for, and does not save', async () => {
    reply = refusal(UNKNOWN_FIELD)
    const wrapper = open()

    await save(wrapper)

    expect(list(wrapper).attributes('role')).toBe('alert')
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
    expect(wrapper.emitted('saved')).toBeUndefined()
  })

  it.each([
    ['key', 'menu-key', KEY_TAKEN],
    ['label', 'menu-label', LABEL_REQUIRED],
    ['route', 'menu-route', ROUTE_NOT_RELATIVE],
    ['parent', 'menu-parent', PARENT_CYCLE],
  ] as const)('still places a refusal of the %s under its input', async (_, testId, detail) => {
    reply = refusal(detail)
    const wrapper = open()

    await save(wrapper)

    expect(blockOf(wrapper, testId)).toContain(detail[3])
    expect(shown(wrapper, detail[3])).toBe(1)
    expect(list(wrapper).exists()).toBe(false)
  })

  describe.each([
    ['creating', null],
    ['editing', requisitions],
  ] as const)('when %s', (_, editing) => {
    it.each(NEW_SLOTS)(
      'places a refusal of the %s under its input, named by it',
      async (__, testId, detail) => {
        reply = refusal(detail)
        const wrapper = open(editing)

        await save(wrapper)

        const input = wrapper.get(`[data-testid="${testId}"]`)
        const message = wrapper.get(`[data-testid="${testId}-message"]`)

        expect(message.text()).toBe(detail[3])
        expect(input.attributes('aria-invalid')).toBe('true')
        expect(input.attributes('aria-describedby')).toBe(message.attributes('id'))
        expect(describedMessage(wrapper, testId)).toBe(detail[3])

        for (const other of INPUTS.filter((candidate) => candidate !== testId)) {
          expect(wrapper.get(`[data-testid="${other}"]`).attributes('aria-invalid')).toBe('false')
        }

        expect(shown(wrapper, detail[3])).toBe(1)
        expect(list(wrapper).exists()).toBe(false)
        expect(wrapper.find('[data-testid="menu-error"]').exists()).toBe(false)
        expect(wrapper.emitted('saved')).toBeUndefined()
      },
    )
  })

  it('places both new refusals at once, each under its own input', async () => {
    reply = refusal(ICON_TOO_LONG, PERMISSION_TOO_LONG)
    const wrapper = open(requisitions)

    await save(wrapper)

    expect(describedMessage(wrapper, 'menu-icon')).toBe(ICON_TOO_LONG[3])
    expect(describedMessage(wrapper, 'menu-permission')).toBe(PERMISSION_TOO_LONG[3])
  })

  it('names no message from an input with nothing to say', async () => {
    reply = refusal(UNKNOWN_FIELD)
    const wrapper = open()

    await save(wrapper)

    for (const [, testId] of NEW_SLOTS) {
      const input = wrapper.get(`[data-testid="${testId}"]`)

      expect(input.attributes('aria-invalid')).toBe('false')
      expect(input.attributes('aria-describedby')).toBeUndefined()
      expect(wrapper.find(`[data-testid="${testId}-message"]`).exists()).toBe(false)
    }
  })

  it('shows a placed and an unplaced detail once each, each in its own place', async () => {
    reply = refusal(ICON_TOO_LONG, UNKNOWN_FIELD)
    const wrapper = open()

    await save(wrapper)

    expect(describedMessage(wrapper, 'menu-icon')).toBe(ICON_TOO_LONG[3])
    expect(shown(wrapper, ICON_TOO_LONG[3])).toBe(1)
    expect(listed(wrapper)).toEqual([`unknownField: ${UNKNOWN_FIELD[3]}`])
  })

  it('does not also say "Validation failed" when the details say why', async () => {
    reply = refusal(LABEL_REQUIRED, UNKNOWN_FIELD)
    const wrapper = open()

    await save(wrapper)

    expect(wrapper.find('[data-testid="menu-error"]').exists()).toBe(false)
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
    // `rad::service::menu` answers a taken key with a 422 on `menuKey`, not a
    // 409; a 409 from elsewhere (a constraint the service did not foresee) has
    // no `ConflictRule` here and is the one message, as before.
    [
      'a 409',
      { status: 409, body: errorBody('CONFLICT', 'menu entry was changed by another request') },
      'menu entry was changed by another request',
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

      await save(wrapper)

      expect(wrapper.get('[data-testid="menu-error"]').text()).toBe(message)
      expect(shown(wrapper, message)).toBe(1)
      expect(list(wrapper).exists()).toBe(false)
      expect(wrapper.emitted('saved')).toBeUndefined()
    },
  )

  it('opens again without the last refusal, placed or listed', async () => {
    reply = refusal(PERMISSION_TOO_LONG, UNKNOWN_FIELD)
    const wrapper = open(requisitions)

    await save(wrapper)
    expect(list(wrapper).exists()).toBe(true)
    expect(describedMessage(wrapper, 'menu-permission')).toBe(PERMISSION_TOO_LONG[3])

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ editing: purchasing })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(list(wrapper).exists()).toBe(false)
    expect(describedMessage(wrapper, 'menu-permission')).toBe('')
    expect(wrapper.get('[data-testid="menu-permission"]').attributes('aria-invalid')).toBe('false')
  })

  it('opens again without the last message above the form', async () => {
    reply = { status: 403, body: errorBody('FORBIDDEN', 'Not yours to change') }
    const wrapper = open(requisitions)

    await save(wrapper)
    expect(wrapper.find('[data-testid="menu-error"]').exists()).toBe(true)

    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    await flushPromises()

    expect(wrapper.find('[data-testid="menu-error"]').exists()).toBe(false)
  })

  it('clears the last refusal when Save is pressed again', async () => {
    reply = refusal(ICON_TOO_LONG, UNKNOWN_FIELD)
    const wrapper = open(requisitions)

    await save(wrapper)
    expect(list(wrapper).exists()).toBe(true)

    reply = { status: 200, body: itemBody(requisitions) }
    await save(wrapper)

    expect(list(wrapper).exists()).toBe(false)
    expect(describedMessage(wrapper, 'menu-icon')).toBe('')
    expect(wrapper.emitted('saved')).toHaveLength(1)
  })

  it('sends the same request as before', async () => {
    const wrapper = open(requisitions)

    await save(wrapper)

    const put = backend.requests.find((request) => request.method === 'put')

    expect(put?.url).toBe(`/rad/menus/${requisitions.id}`)
    expect(put?.body).toEqual({
      label: 'Requisitions',
      icon: 'shopping-cart',
      parentMenuId: purchasing.id,
      routePath: '/lists/purchase_requisitions',
      requiredPermission: 'document:read',
      sortOrder: 0,
      isEnabled: true,
    })
  })

  // test-engineer, #641 AC11: the create body, which the case above (an
  // edit) does not reach. Green before row 22's change as after it.
  it('sends the same request as before when creating, the key trimmed', async () => {
    const wrapper = open()

    await wrapper.get('[data-testid="menu-key"]').setValue('  purchasing  ')
    await wrapper.get('[data-testid="menu-label"]').setValue('Purchasing')
    await save(wrapper)

    const post = backend.requests.find((request) => request.method === 'post')

    expect(post?.url).toBe('/rad/menus')
    expect(post?.body).toEqual({
      menuKey: 'purchasing',
      label: 'Purchasing',
      icon: null,
      parentMenuId: null,
      routePath: null,
      requiredPermission: null,
      sortOrder: 0,
      isEnabled: true,
    })
  })

  // test-engineer, #641 AC4 and AC5: `sortOrder` is an `i32` and the Order
  // input has no slot, so a number past it, refused by `extract::detail`, is
  // listed. A path added to PLACED without a slot would make it vanish.
  it('lists a refusal of the order, which has no slot', async () => {
    const ORDER_OUT_OF_RANGE: Detail = [
      'sortOrder',
      'type',
      'INVALID_TYPE',
      'Invalid value: integer `99999999999`, expected i32',
    ]
    reply = refusal(ORDER_OUT_OF_RANGE)
    const wrapper = open()

    await wrapper.get('[data-testid="menu-order"]').setValue('99999999999')
    await save(wrapper)

    const post = backend.requests.find((request) => request.method === 'post')

    expect((post?.body as { sortOrder: number }).sortOrder).toBe(99999999999)
    expect(listed(wrapper)).toEqual([`sortOrder: ${ORDER_OUT_OF_RANGE[3]}`])
    expect(shown(wrapper, ORDER_OUT_OF_RANGE[3])).toBe(1)
    expect(wrapper.find('[data-testid="menu-error"]').exists()).toBe(false)
    expect(wrapper.emitted('saved')).toBeUndefined()
  })
})
