import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import LoginPage from './LoginPage.vue'

describe('LoginPage', () => {
  it('reports both fields as required when submitted empty', async () => {
    const wrapper = mount(LoginPage)

    await wrapper.find('form').trigger('submit')

    expect(wrapper.text()).toContain('Enter your username or email address')
    expect(wrapper.text()).toContain('Enter your password')
  })

  it('marks invalid inputs for assistive technology', async () => {
    const wrapper = mount(LoginPage)

    await wrapper.find('form').trigger('submit')

    expect(wrapper.find('#username').attributes('aria-invalid')).toBe('true')
  })

  it('does not claim to sign in until phase 2 wires it', async () => {
    const wrapper = mount(LoginPage)

    await wrapper.find('#username').setValue('someone@example.com')
    await wrapper.find('#password').setValue('hunter2')
    await wrapper.find('form').trigger('submit')

    expect(wrapper.text()).toContain('Authentication is not wired yet')
  })

  it('uses a password input so the value is masked', () => {
    const wrapper = mount(LoginPage)

    expect(wrapper.find('#password').attributes('type')).toBe('password')
  })
})
