import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import FormUnplacedErrors from './FormUnplacedErrors.vue'

const fieldErrors = {
  name: 'Name is too long',
  colour: 'Unknown field',
  'lines.2.sku': 'No such product',
}

describe('FormUnplacedErrors', () => {
  it('lists each detail the form has no place for, as path and message', () => {
    const wrapper = mount(FormUnplacedErrors, { props: { fieldErrors, placed: ['name'] } })

    expect(wrapper.findAll('li').map((item) => item.text())).toEqual([
      'colour: Unknown field',
      'lines.2.sku: No such product',
    ])
  })

  it('is announced, and carries the test id the form gives it', () => {
    // An alert, as the form's own message is: a refusal that is only drawn is
    // one a screen reader user never hears (NFR-USE-005).
    const wrapper = mount(FormUnplacedErrors, {
      props: { fieldErrors, placed: ['name'] },
      attrs: { 'data-testid': 'thing-unplaced-errors' },
    })

    expect(wrapper.find('[data-testid="thing-unplaced-errors"]').attributes('role')).toBe('alert')
  })

  it('draws nothing when every detail has a place, or there is none', async () => {
    const wrapper = mount(FormUnplacedErrors, {
      props: { fieldErrors, placed: ['name', 'colour', 'lines.2.sku'] },
    })

    expect(wrapper.find('[role="alert"]').exists()).toBe(false)

    await wrapper.setProps({ fieldErrors: {}, placed: [] })

    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('follows the form as a field is drawn or left out', async () => {
    const wrapper = mount(FormUnplacedErrors, {
      props: { fieldErrors, placed: ['name', 'colour', 'lines.2.sku'] },
    })

    await wrapper.setProps({ placed: ['name', 'colour'] })

    expect(wrapper.findAll('li').map((item) => item.text())).toEqual([
      'lines.2.sku: No such product',
    ])
  })
})
