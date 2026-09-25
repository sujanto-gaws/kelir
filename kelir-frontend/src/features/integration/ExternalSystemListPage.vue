<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'

import { listExternalSystems } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useQueryBackedList, type ListQuery } from '@/composables/useQueryBackedList'
import { useAuthStore } from '@/stores/auth'
import {
  AUTH_TYPE_LABELS,
  EXTERNAL_SYSTEM_STATUS_LABELS,
  EXTERNAL_SYSTEM_TYPE_LABELS,
  optionsOf,
  type ExternalSystem,
  type ExternalSystemStatus,
} from '@/types/integration'

import ExternalSystemFormDialog from './ExternalSystemFormDialog.vue'

/**
 * The external system registry (FR-INT-001, #520; architectures/03 §3.1).
 *
 * **The URL is the state**, as on the document list: page, search, status and
 * type live in the query string, go on the wire as they are, and nothing is
 * narrowed here. The route needs `integration:external-system:read`; the
 * register button is offered only with `:create`.
 *
 * **A registered system opens on its own page**, because that is where
 * everything after registering happens — its endpoints and credential
 * references are managed there.
 */
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const canCreate = computed(() => auth.can('integration:external-system:create'))

const list = useQueryBackedList<ExternalSystem>(listExternalSystems)

const statusOptions = optionsOf(EXTERNAL_SYSTEM_STATUS_LABELS)
const typeOptions = optionsOf(EXTERNAL_SYSTEM_TYPE_LABELS)

const isRegisterOpen = ref(false)

const currentQuery = computed<ListQuery>(() => {
  const query: ListQuery = {}

  for (const [key, value] of Object.entries(route.query)) {
    const first = Array.isArray(value) ? value[0] : value

    if (typeof first === 'string' && first !== '') {
      query[key] = first
    }
  }

  return query
})

/** Any change but the page itself goes back to page 1. */
function navigate(changes: ListQuery, resetPage = true): void {
  const next: ListQuery = { ...currentQuery.value, ...changes }

  if (resetPage) {
    delete next.page
  }

  for (const [key, value] of Object.entries(next)) {
    if (value === '') {
      delete next[key]
    }
  }

  void router.push({ name: 'admin-external-systems', query: next })
}

function setFilter(key: string, value: string): void {
  navigate({ [key]: value.trim() })
}

function goToPage(next: number): void {
  navigate({ page: String(Math.min(Math.max(1, next), list.totalPages.value)) }, false)
}

function statusVariant(status: ExternalSystemStatus): 'default' | 'secondary' | 'outline' {
  if (status === 'ACTIVE') {
    return 'default'
  }

  return status === 'MAINTENANCE' ? 'outline' : 'secondary'
}

function afterRegister(system: ExternalSystem): void {
  void router.push({ name: 'admin-external-system', params: { id: system.id } })
}

watch(
  () => route.query,
  () => {
    void list.apply(currentQuery.value)
  },
  { immediate: true, deep: true },
)
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">External systems</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          The systems Kelir integrates with, their endpoints and where their credentials are kept.
        </p>
      </div>

      <Button
        v-if="canCreate"
        data-testid="register-external-system"
        @click="isRegisterOpen = true"
      >
        Register system
      </Button>
    </div>

    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <div class="space-y-2">
        <Label for="external-systems-search">Search</Label>
        <Input
          id="external-systems-search"
          data-testid="external-systems-search"
          type="search"
          :model-value="list.filters.value.search ?? ''"
          placeholder="Code or name"
          @change="setFilter('search', ($event.target as HTMLInputElement).value)"
        />
      </div>

      <div class="space-y-2">
        <Label for="external-systems-status">Status</Label>
        <Select
          id="external-systems-status"
          data-testid="external-systems-status"
          :model-value="list.filters.value.status ?? ''"
          :options="statusOptions"
          placeholder="Any"
          @update:model-value="setFilter('status', $event)"
        />
      </div>

      <div class="space-y-2">
        <Label for="external-systems-type">Type</Label>
        <Select
          id="external-systems-type"
          data-testid="external-systems-type"
          :model-value="list.filters.value.systemType ?? ''"
          :options="typeOptions"
          placeholder="Any"
          @update:model-value="setFilter('systemType', $event)"
        />
      </div>
    </div>

    <Alert v-if="list.error.value" variant="destructive" data-testid="external-systems-error">
      <p>{{ list.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="list.apply(currentQuery)">
        Try again
      </Button>
    </Alert>

    <p v-if="list.isLoading.value" class="text-sm text-muted-foreground">
      Loading external systems…
    </p>

    <template v-else-if="!list.error.value">
      <p
        v-if="list.isEmpty.value"
        class="text-sm text-muted-foreground"
        data-testid="external-systems-empty"
      >
        No external systems match this view.
      </p>

      <Table v-else data-testid="external-systems-table">
        <TableHeader>
          <TableRow>
            <TableHead>Code</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Type</TableHead>
            <TableHead>Authentication</TableHead>
            <TableHead>Status</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="system in list.items.value"
            :key="system.id"
            :data-testid="`external-system-row-${system.systemCode}`"
          >
            <TableCell class="font-medium">
              <RouterLink
                :to="{ name: 'admin-external-system', params: { id: system.id } }"
                class="text-primary underline-offset-4 hover:underline"
              >
                {{ system.systemCode }}
              </RouterLink>
            </TableCell>
            <TableCell>{{ system.systemName }}</TableCell>
            <TableCell>
              {{ system.systemType ? EXTERNAL_SYSTEM_TYPE_LABELS[system.systemType] : '—' }}
            </TableCell>
            <TableCell>{{ system.authType ? AUTH_TYPE_LABELS[system.authType] : '—' }}</TableCell>
            <TableCell>
              <Badge :variant="statusVariant(system.status)">
                {{ EXTERNAL_SYSTEM_STATUS_LABELS[system.status] }}
              </Badge>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="list.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ list.page.value }} of {{ list.totalPages.value }} · {{ list.total.value }}
          systems
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!list.hasPrevious.value"
            data-testid="previous-page"
            @click="goToPage(list.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!list.hasNext.value"
            data-testid="next-page"
            @click="goToPage(list.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <ExternalSystemFormDialog
      v-if="canCreate"
      v-model:open="isRegisterOpen"
      :editing="null"
      @saved="afterRegister"
    />
  </section>
</template>
