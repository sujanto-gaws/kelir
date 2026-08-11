import { createPinia, setActivePinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createRouter, createWebHistory } from 'vue-router'

import App from './App.vue'

describe('App', () => {
  beforeEach(() => {
    setActivePinia(createPinia())

    // jsdom has no matchMedia; App reads it to pick the initial theme.
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockReturnValue({ matches: false, addEventListener: vi.fn() }),
    )
  })

  it('renders the routed view', async () => {
    const router = createRouter({
      history: createWebHistory(),
      routes: [{ path: '/', component: { template: '<main>kelir</main>' } }],
    })

    router.push('/')
    await router.isReady()

    const wrapper = mount(App, { global: { plugins: [router] } })

    expect(wrapper.html()).toContain('kelir')
  })
})
