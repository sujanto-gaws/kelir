import { defineStore } from 'pinia'
import { ref } from 'vue'

/**
 * Shell state shared across pages: the navigation rail and the colour scheme.
 *
 * Page-local state stays in its component; only cross-page state belongs here
 * (coding standard §3.3). Actions are the only place this state is mutated.
 */
export const useAppStore = defineStore('app', () => {
  const isSidebarOpen = ref(true)
  const isDark = ref(false)

  function toggleSidebar(): void {
    isSidebarOpen.value = !isSidebarOpen.value
  }

  function setSidebar(open: boolean): void {
    isSidebarOpen.value = open
  }

  function toggleTheme(): void {
    setTheme(!isDark.value)
  }

  /**
   * The class lives on <html> because Tailwind's dark variant is defined
   * against `.dark` and must cover the whole document, not just the app root.
   */
  function setTheme(dark: boolean): void {
    isDark.value = dark
    document.documentElement.classList.toggle('dark', dark)
  }

  return { isSidebarOpen, isDark, toggleSidebar, setSidebar, toggleTheme, setTheme }
})
