import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import Select from './Select.vue'

const options = [
  { value: 'draft', label: 'Draft' },
  { value: 'posted', label: 'Posted' },
]

describe('Select', () => {
  it('renders one option per entry', () => {
    const wrapper = mount(Select, { props: { modelValue: '', options } })

    expect(wrapper.findAll('option').map((option) => option.text())).toStrictEqual([
      'Draft',
      'Posted',
    ])
  })

  it('adds a blank leading option only when a placeholder is given', () => {
    const wrapper = mount(Select, {
      props: { modelValue: '', options, placeholder: 'Any status' },
    })
    const first = wrapper.findAll('option')[0]

    expect(first.text()).toBe('Any status')
    expect(first.attributes('value')).toBe('')
  })

  it('reports the chosen value back to the model', async () => {
    const wrapper = mount(Select, { props: { modelValue: '', options } })

    await wrapper.get('select').setValue('posted')

    expect(wrapper.emitted('update:modelValue')).toStrictEqual([['posted']])
  })

  it('shows the model value as the current selection', () => {
    const wrapper = mount(Select, { props: { modelValue: 'posted', options } })

    expect(wrapper.get('select').element.value).toBe('posted')
  })
})
