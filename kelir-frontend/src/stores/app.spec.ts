import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useAppStore } from './app'

describe('useAppStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    document.documentElement.classList.remove('dark')
  })

  it('starts with the sidebar open', () => {
    expect(useAppStore().isSidebarOpen).toBe(true)
  })

  it('toggles the sidebar', () => {
    const store = useAppStore()

    store.toggleSidebar()
    expect(store.isSidebarOpen).toBe(false)

    store.toggleSidebar()
    expect(store.isSidebarOpen).toBe(true)
  })

  it('puts the dark class on the document element, not the app root', () => {
    // Tailwind's dark variant is defined against `.dark`; scoping it to the app
    // root would leave the page background light behind the app.
    const store = useAppStore()

    store.setTheme(true)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(store.isDark).toBe(true)

    store.setTheme(false)
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })
})
