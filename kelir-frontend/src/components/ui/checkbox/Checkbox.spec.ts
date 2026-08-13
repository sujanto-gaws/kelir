import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import Checkbox from './Checkbox.vue'

describe('Checkbox', () => {
  it('reports a tick back to the model', async () => {
    const wrapper = mount(Checkbox, { props: { modelValue: false } })

    await wrapper.get('input').setValue(true)

    expect(wrapper.emitted('update:modelValue')).toStrictEqual([[true]])
  })

  it('reflects the model into the checked state', async () => {
    const wrapper = mount(Checkbox, { props: { modelValue: false } })

    await wrapper.setProps({ modelValue: true })

    expect(wrapper.get('input').element.checked).toBe(true)
  })

  it('is disabled on request', () => {
    const wrapper = mount(Checkbox, { props: { modelValue: false, disabled: true } })

    expect(wrapper.get('input').element.disabled).toBe(true)
  })
})
