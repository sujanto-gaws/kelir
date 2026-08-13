import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import Dialog from './Dialog.vue'

function renderOpen(props: Record<string, unknown> = {}) {
  return mount(Dialog, {
    props: { open: true, title: 'Reject document', ...props },
    slots: { default: 'Give a reason for the rejection.' },
  })
}

describe('Dialog', () => {
  it('renders nothing while closed', () => {
    const wrapper = mount(Dialog, { props: { open: false, title: 'Reject document' } })

    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
  })

  it('renders the title and the body once open', () => {
    const wrapper = renderOpen()

    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Reject document')
    expect(wrapper.text()).toContain('Give a reason for the rejection.')
  })

  it('closes on Escape', async () => {
    const wrapper = renderOpen()

    await wrapper.find('[role="dialog"]').trigger('keydown', { key: 'Escape' })

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
  })

  it('closes on an overlay click', async () => {
    const wrapper = renderOpen()

    // Not wrapper.trigger: with a v-if root, wrapper.element is the mount
    // container rather than the overlay, so the click would land nowhere.
    await wrapper.get('div.fixed').trigger('click')

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
  })

  it('stays open when the click lands inside the panel', async () => {
    const wrapper = renderOpen()

    await wrapper.find('[role="dialog"]').trigger('click')

    expect(wrapper.emitted('update:open')).toBeUndefined()
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true)
  })

  it('points its aria labels at elements that exist', () => {
    const wrapper = renderOpen({ description: 'This cannot be undone.' })
    const panel = wrapper.get('[role="dialog"]')

    const labelledBy = panel.attributes('aria-labelledby')
    const describedBy = panel.attributes('aria-describedby')

    expect(wrapper.get(`[id="${labelledBy}"]`).text()).toBe('Reject document')
    expect(wrapper.get(`[id="${describedBy}"]`).text()).toBe('This cannot be undone.')
  })

  it('omits aria-describedby when there is no description', () => {
    const wrapper = renderOpen()

    expect(wrapper.get('[role="dialog"]').attributes('aria-describedby')).toBeUndefined()
  })

  it('closes from the header close button', async () => {
    const wrapper = renderOpen()

    await wrapper.find('button[aria-label="Close"]').trigger('click')

    expect(wrapper.emitted('update:open')).toStrictEqual([[false]])
  })
})
