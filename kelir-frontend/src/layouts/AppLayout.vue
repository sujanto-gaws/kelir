<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { FileText, Inbox, LayoutDashboard, Menu, Moon, Sun, Users } from 'lucide-vue-next'

import { Button } from '@/components/ui/button'
import { useAppStore } from '@/stores/app'

const appStore = useAppStore()
const route = useRoute()

/**
 * The navigation rail. Destinations beyond the dashboard are disabled until
 * their phase lands — showing them communicates the shape of the product, but
 * a link that 404s would not.
 */
const navigation = [
  { name: 'dashboard', label: 'Dashboard', icon: LayoutDashboard, enabled: true },
  { name: 'tasks', label: 'My Tasks', icon: Inbox, enabled: false },
  { name: 'documents', label: 'Documents', icon: FileText, enabled: false },
  { name: 'master-data', label: 'Master Data', icon: Users, enabled: false },
] as const

const currentTitle = computed(() =>
  typeof route.meta.title === 'string' ? route.meta.title : 'Kelir',
)
</script>

<template>
  <div class="flex min-h-screen bg-background text-foreground">
    <aside
      v-show="appStore.isSidebarOpen"
      class="w-60 shrink-0 border-r border-border bg-card"
      aria-label="Main navigation"
    >
      <div class="flex h-14 items-center border-b border-border px-4">
        <span class="text-lg font-semibold tracking-tight">Kelir</span>
      </div>

      <nav class="space-y-1 p-3">
        <template v-for="item in navigation" :key="item.name">
          <RouterLink
            v-if="item.enabled"
            :to="{ name: item.name }"
            class="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium hover:bg-secondary"
            active-class="bg-secondary"
          >
            <component :is="item.icon" class="size-4" aria-hidden="true" />
            {{ item.label }}
          </RouterLink>

          <span
            v-else
            class="flex cursor-not-allowed items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
            :title="`${item.label} arrives in a later phase`"
          >
            <component :is="item.icon" class="size-4" aria-hidden="true" />
            {{ item.label }}
          </span>
        </template>
      </nav>
    </aside>

    <div class="flex min-w-0 flex-1 flex-col">
      <header class="flex h-14 items-center gap-3 border-b border-border px-4">
        <Button
          variant="ghost"
          size="icon"
          aria-label="Toggle navigation"
          @click="appStore.toggleSidebar()"
        >
          <Menu class="size-4" aria-hidden="true" />
        </Button>

        <h1 class="text-sm font-medium">{{ currentTitle }}</h1>

        <Button
          variant="ghost"
          size="icon"
          class="ml-auto"
          :aria-label="appStore.isDark ? 'Switch to light theme' : 'Switch to dark theme'"
          @click="appStore.toggleTheme()"
        >
          <Sun v-if="appStore.isDark" class="size-4" aria-hidden="true" />
          <Moon v-else class="size-4" aria-hidden="true" />
        </Button>
      </header>

      <main class="flex-1 p-6">
        <RouterView />
      </main>
    </div>
  </div>
</template>
