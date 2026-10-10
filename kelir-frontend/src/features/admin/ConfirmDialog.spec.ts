import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import ConfirmDialog from './ConfirmDialog.vue'

/**
 * `ConfirmDialog` gained a default slot and `confirmDisabled` for the workflow
 * editor's *Deprecate* warning (#713, plan 19 row 6b). **Eleven other screens
 * use it**, every one self-closing and passing neither (test-engineer campaign,
 * 2026-10-10: the delegation, document-type, form, list, menu, role, tenant and
 * user lists; the external system page, and its credential and endpoint
 * sections). What they rely on is held here, so that a change made for the
 * warning cannot quietly change them.
 *
 * Seen to fail, 2026-10-10: `confirmDisabled` defaulting to `true` (*leaves
 * confirm and Cancel operable…*); the slot's wrapper rendered without a slot
 * (*renders what it rendered before…*); `confirmDisabled` also holding Cancel
 * (*holds confirm and not Cancel…*).
 */

function render(props: Record<string, unknown> = {}, slots: Record<string, string> = {}) {
  return mount(ConfirmDialog, {
    props: {
      open: true,
      title: 'Delete document type',
      description: 'PURCHASE_REQUEST will be removed.',
      confirmLabel: 'Delete',
      ...props,
    },
    slots,
  })
}

function cancelButton(wrapper: ReturnType<typeof render>) {
  return wrapper.findAll('button').find((button) => button.text() === 'Cancel')!
}

describe('ConfirmDialog', () => {
  it('renders what it rendered before for a caller passing no slot', () => {
    const wrapper = render({ error: 'Type has documents' })
    const panel = wrapper.get('[role="dialog"]')

    expect(panel.find('[data-testid="confirm-action"]').text()).toBe('Delete')
    expect(panel.text()).toContain('PURCHASE_REQUEST will be removed.')
    expect(panel.text()).toContain('Type has documents')

    const body = panel.get('.mt-4.text-sm')

    // The description, then the refusal: no empty box between them.
    expect(Array.from(body.element.children).map((child) => child.tagName)).toEqual(['P', 'DIV'])
    expect(body.element.children[1].getAttribute('role')).toBe('alert')
  })

  it('leaves confirm and Cancel operable when given neither the slot nor the prop', async () => {
    const wrapper = render()

    expect(wrapper.get('[data-testid="confirm-action"]').attributes('disabled')).toBeUndefined()
    expect(cancelButton(wrapper).attributes('disabled')).toBeUndefined()

    await wrapper.get('[data-testid="confirm-action"]').trigger('click')

    expect(wrapper.emitted('confirm')).toHaveLength(1)
    // Confirming does not close it: the caller closes it once the call is done.
    expect(wrapper.emitted('update:open')).toBeUndefined()
  })

  it('holds both buttons while pending, as every caller’s request relies on', async () => {
    const wrapper = render({ pending: true })

    expect(wrapper.get('[data-testid="confirm-action"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="confirm-action"]').attributes('aria-busy')).toBe('true')
    expect(cancelButton(wrapper).attributes('disabled')).toBeDefined()

    await wrapper.get('[data-testid="confirm-action"]').trigger('click')

    expect(wrapper.emitted('confirm')).toBeUndefined()
  })

  it('holds confirm and not Cancel under confirmDisabled, and Cancel still closes it', async () => {
    const wrapper = render({ confirmDisabled: true })

    expect(wrapper.get('[data-testid="confirm-action"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[data-testid="confirm-action"]').attributes('aria-busy')).toBe('false')
    expect(cancelButton(wrapper).attributes('disabled')).toBeUndefined()

    await wrapper.get('[data-testid="confirm-action"]').trigger('click')

    expect(wrapper.emitted('confirm')).toBeUndefined()

    await cancelButton(wrapper).trigger('click')

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
  })

  it('puts the slot between the description and the refusal', () => {
    const wrapper = render(
      { error: 'Refused' },
      { default: '<ul data-testid="slotted"><li>Purchase request</li></ul>' },
    )
    const body = wrapper.get('[role="dialog"] .mt-4.text-sm')
    const order = Array.from(
      body.element.querySelectorAll('p, [data-testid="slotted"], [role="alert"]'),
    ).map(
      (element) =>
        element.getAttribute('data-testid') ?? element.getAttribute('role') ?? element.tagName,
    )

    expect(order).toEqual(['P', 'slotted', 'alert'])
  })
})
