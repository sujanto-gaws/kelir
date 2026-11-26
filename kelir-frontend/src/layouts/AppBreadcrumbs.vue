<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute, useRouter, type RouteMeta } from 'vue-router'
import { ChevronRight } from '@lucide/vue'

import { useAuthStore } from '@/stores/auth'
import { useBreadcrumbStore } from '@/stores/breadcrumb'

import { buildBreadcrumbs } from './breadcrumbs'

/**
 * The trail above every shell page but the dashboard (#968, NFR-USE-002).
 *
 * Built from the route table (`breadcrumbs.ts`); the record's name comes from
 * the page that loaded it (`useBreadcrumbName`). Labels are interpolated, so a
 * name a user typed is text, never markup.
 */
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const names = useBreadcrumbStore()

const metaByName = computed(() => {
  const map = new Map<string, RouteMeta>()
  for (const record of router.getRoutes()) {
    if (typeof record.name === 'string') {
      map.set(record.name, record.meta)
    }
  }
  return map
})

const crumbs = computed(() =>
  buildBreadcrumbs(
    route,
    (name) => metaByName.value.get(name),
    (permission) => auth.can(permission),
    names.published,
  ),
)
</script>

<template>
  <nav
    v-if="crumbs.length > 0"
    aria-label="Breadcrumb"
    class="mb-4 min-w-0"
    data-testid="breadcrumbs"
  >
    <ol class="flex min-w-0 flex-wrap items-center gap-x-1 gap-y-1 text-sm text-muted-foreground">
      <li
        v-for="(crumb, index) in crumbs"
        :key="`${index}-${crumb.label}`"
        class="flex min-w-0 max-w-full items-center gap-1"
      >
        <ChevronRight
          v-if="index > 0"
          class="size-3.5 shrink-0"
          aria-hidden="true"
          data-testid="breadcrumb-separator"
        />
        <RouterLink
          v-if="crumb.to && !crumb.current"
          :to="{ name: crumb.to }"
          class="truncate hover:text-foreground hover:underline"
          :title="crumb.label"
        >
          {{ crumb.label }}
        </RouterLink>
        <span
          v-else
          class="truncate"
          :class="crumb.current ? 'max-w-[32rem] font-medium text-foreground' : ''"
          :aria-current="crumb.current ? 'page' : undefined"
          :title="crumb.label"
        >
          {{ crumb.label }}
        </span>
      </li>
    </ol>
  </nav>
</template>
