<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import {
  Bell,
  Building2,
  FileText,
  Inbox,
  LayoutDashboard,
  LogOut,
  Menu,
  Moon,
  ShieldCheck,
  Sun,
  UserRoundCheck,
  UserCog,
  Users,
} from 'lucide-vue-next'

import { Button } from '@/components/ui/button'
import { LOGIN_ROUTE_NAME } from '@/router/guards'
import { useAppStore } from '@/stores/app'
import { useAuthStore } from '@/stores/auth'
import { useNotificationStore } from '@/stores/notifications'

const appStore = useAppStore()
const notifications = useNotificationStore()
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

/**
 * The navigation rail. Destinations beyond the dashboard are disabled until
 * their phase lands — showing them communicates the shape of the product, but
 * a link that 404s would not.
 *
 * `permission` hides an entry the user could never open. Hiding is cosmetic:
 * the backend re-checks every request and is the only thing that decides.
 */
const navigation = [
  { name: 'dashboard', label: 'Dashboard', icon: LayoutDashboard, enabled: true },
  {
    // Enabled in Sprint 10 (#179). The permission was already right: the inbox
    // reads the workflow module's task rows, so it borrows that module's read
    // permission rather than inventing one of its own.
    name: 'tasks',
    label: 'My Tasks',
    icon: Inbox,
    enabled: true,
    permission: 'workflow:task:read',
  },
  {
    name: 'documents',
    label: 'Documents',
    icon: FileText,
    enabled: true,
    permission: 'document:read',
  },
  {
    // #251. Below the two work queues and above the reference data, which is
    // where it sits in a day: you look at what came to you, then at what is
    // waiting, then at the things you need to fill a form in.
    name: 'notifications',
    label: 'Notifications',
    icon: Bell,
    enabled: true,
    permission: 'notification:read',
  },
  {
    name: 'master-data',
    label: 'Master Data',
    icon: Users,
    enabled: true,
    permission: 'master-data:party:read',
  },
  {
    name: 'admin-users',
    label: 'Users',
    icon: UserCog,
    enabled: true,
    permission: 'identity:user:read',
  },
  {
    name: 'admin-roles',
    label: 'Roles',
    icon: ShieldCheck,
    enabled: true,
    permission: 'identity:role:read',
  },
  {
    name: 'admin-delegations',
    label: 'Delegations',
    icon: UserRoundCheck,
    enabled: true,
    permission: 'identity:delegation:read',
  },
  {
    name: 'admin-tenants',
    label: 'Tenants',
    icon: Building2,
    enabled: true,
    permission: 'organization:tenant:read',
  },
] as const

const visibleNavigation = computed(() =>
  navigation.filter((item) => !('permission' in item) || auth.can(item.permission)),
)

const currentTitle = computed(() =>
  typeof route.meta.title === 'string' ? route.meta.title : 'Kelir',
)

/**
 * The badge, re-read whenever the person moves.
 *
 * **Not a timer.** Every navigation is a moment they are already looking at the
 * shell, and it costs one small request at a point where one was being made
 * anyway. A notification that arrives while a screen sits open is seen on the
 * next move — a stated limit, and the thing that closes it is a delivery
 * channel (#257) rather than a shorter interval.
 */
watch(
  () => route.fullPath,
  () => void notifications.refresh(),
  { immediate: true },
)

const isSigningOut = ref(false)

async function signOut(): Promise<void> {
  isSigningOut.value = true

  try {
    await auth.signOut()
    await router.replace({ name: LOGIN_ROUTE_NAME })
  } finally {
    isSigningOut.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen bg-background text-foreground">
    <!-- The label belongs on the <nav>, not on this <aside>. It was on the
         aside, which made the only landmark called "Main navigation" a
         complementary one and left the navigation landmark unnamed — found by
         the browser harness (#153) the first time anything asked the page for
         a navigation by that name. -->
    <aside v-show="appStore.isSidebarOpen" class="w-60 shrink-0 border-r border-border bg-card">
      <div class="flex h-14 items-center border-b border-border px-4">
        <span class="text-lg font-semibold tracking-tight">Kelir</span>
      </div>

      <nav class="space-y-1 p-3" aria-label="Main navigation">
        <template v-for="item in visibleNavigation" :key="item.name">
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

        <span v-if="auth.displayName" class="ml-auto text-sm text-muted-foreground">
          {{ auth.displayName }}
        </span>
        <span v-else class="ml-auto" />

        <!-- **The badge, and the only place a person learns something arrived
             without having gone looking.** The count refreshes on navigation
             rather than on a timer: a poll is a request per user per interval
             for a number that is usually unchanged, and real-time delivery is a
             channel (#257) rather than a shorter interval. -->
        <RouterLink
          v-if="auth.can('notification:read')"
          :to="{ name: 'notifications' }"
          class="relative"
          data-testid="notifications-bell"
        >
          <Button
            variant="ghost"
            size="icon"
            :aria-label="
              notifications.unread > 0
                ? `Notifications, ${notifications.unread} unread`
                : 'Notifications'
            "
          >
            <Bell class="size-4" aria-hidden="true" />
          </Button>
          <span
            v-if="notifications.unread > 0"
            class="absolute -right-1 -top-1 min-w-4 rounded-full bg-primary px-1 text-center text-[10px] leading-4 text-primary-foreground"
            aria-hidden="true"
            data-testid="notifications-badge"
          >
            {{ notifications.unread > 99 ? '99+' : notifications.unread }}
          </span>
        </RouterLink>

        <Button
          variant="ghost"
          size="icon"
          :aria-label="appStore.isDark ? 'Switch to light theme' : 'Switch to dark theme'"
          @click="appStore.toggleTheme()"
        >
          <Sun v-if="appStore.isDark" class="size-4" aria-hidden="true" />
          <Moon v-else class="size-4" aria-hidden="true" />
        </Button>

        <Button
          variant="ghost"
          size="icon"
          aria-label="Sign out"
          :loading="isSigningOut"
          @click="signOut()"
        >
          <LogOut class="size-4" aria-hidden="true" />
        </Button>
      </header>

      <main class="flex-1 p-6">
        <RouterView />
      </main>
    </div>
  </div>
</template>
