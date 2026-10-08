<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'

import { listExternalSystems, listIntegrationLogs } from '@/api/integration'
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
  INTEGRATION_LOG_STATUS_LABELS,
  optionsOf,
  type IntegrationLogSummary,
} from '@/types/integration'

import IntegrationLogDetailDialog from './IntegrationLogDetailDialog.vue'
import {
  formatDuration,
  formatTimestamp,
  fromLocalInput,
  logStatusVariant,
  toLocalInput,
} from './integration-log'

/**
 * The integration log (FR-INT-006, #548; architectures/03 §3.2's
 * `IntegrationMonitoringModule`, of whose five pages this is the one built).
 *
 * **The URL is the state**, as on the system list: the system, status, date
 * range and page live in the query string and go on the wire as they are; the
 * server filters and orders newest first, and nothing is narrowed here. The
 * date inputs are in the browser's zone and the query holds the ISO instants
 * the API takes. `from` is inclusive and `to` exclusive, as the API has them,
 * so the second input is *Started before*.
 *
 * **The detail is `?log=<id>`**, a dialog over the list, so a row can be
 * linked to — the test call dialog does — and closing it keeps the filters.
 * `log` is the dialog's and never reaches the list request.
 *
 * **The route needs `integration:log:read`.** The system chooser and the link
 * to a system's page need `integration:external-system:read` as well, which a
 * log reader need not hold: without it the chooser is disabled and says why,
 * and a system is shown as text.
 */
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const canReadSystems = computed(() => auth.can('integration:external-system:read'))

const list = useQueryBackedList<IntegrationLogSummary>(listIntegrationLogs)

const statusOptions = optionsOf(INTEGRATION_LOG_STATUS_LABELS)

/** The query key the detail dialog owns. */
const DETAIL_KEY = 'log'

/** How many pages of systems the chooser reads before it stops. */
const MAX_SYSTEM_PAGES = 10

const systems = ref<{ value: string; label: string }[]>([])
const systemsError = ref('')

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

/** The list's own query: everything but the open row. */
const listQuery = computed<ListQuery>(() => {
  const rest: ListQuery = { ...currentQuery.value }
  delete rest[DETAIL_KEY]

  return rest
})

/**
 * The row the URL names, as written: the dialog opens for any value, and one
 * that is not a UUID shows as not found without a request being made.
 */
const openLogId = computed(() => currentQuery.value[DETAIL_KEY] ?? null)

const isDetailOpen = computed({
  get: () => openLogId.value !== null,
  set: (isOpen: boolean) => {
    if (!isOpen) {
      void router.push({ name: 'admin-integration-logs', query: listQuery.value })
    }
  },
})

/**
 * The chooser's options: every system the caller may read, and the one the
 * URL names even when it is not among them — a system the chooser cannot
 * list is still the filter the rows below were fetched with.
 */
const systemOptions = computed(() => {
  const selected = listQuery.value.externalSystemId

  if (!selected || systems.value.some((option) => option.value === selected)) {
    return systems.value
  }

  const fromRow = list.items.value.find((row) => row.externalSystemId === selected)
  const label = fromRow?.externalSystemCode ?? selected

  return [...systems.value, { value: selected, label }]
})

async function loadSystems(): Promise<void> {
  if (!canReadSystems.value) {
    return
  }

  try {
    const loaded: { value: string; label: string }[] = []

    for (let page = 1; page <= MAX_SYSTEM_PAGES; page += 1) {
      const result = await listExternalSystems({ page, pageSize: 100 })

      for (const system of result.items) {
        loaded.push({ value: system.id, label: `${system.systemCode} · ${system.systemName}` })
      }

      if (page * result.meta.pageSize >= result.meta.total || result.items.length === 0) {
        break
      }
    }

    systems.value = loaded
  } catch {
    systemsError.value = 'The systems could not be loaded, so the chooser is empty.'
  }
}

/** Any change but the page itself goes back to page 1, and closes the detail. */
function navigate(changes: ListQuery, resetPage = true): void {
  const next: ListQuery = { ...listQuery.value, ...changes }

  if (resetPage) {
    delete next.page
  }

  for (const [key, value] of Object.entries(next)) {
    if (value === '') {
      delete next[key]
    }
  }

  void router.push({ name: 'admin-integration-logs', query: next })
}

function setFilter(key: string, value: string): void {
  navigate({ [key]: value.trim() })
}

function setInstant(key: 'from' | 'to', value: string): void {
  navigate({ [key]: fromLocalInput(value) })
}

function clearFilters(): void {
  void router.push({ name: 'admin-integration-logs' })
}

const hasFilters = computed(() =>
  ['externalSystemId', 'status', 'from', 'to'].some((key) => Boolean(listQuery.value[key])),
)

/**
 * A range whose end is before its start. The server refuses it with a 422
 * whose message is the generic one, so the field says which bound is wrong;
 * the request still goes, and the server's refusal is what the list shows.
 */
const isRangeInverted = computed(() => {
  const { from, to } = listQuery.value

  return Boolean(from && to) && new Date(to).getTime() < new Date(from).getTime()
})

function goToPage(next: number): void {
  navigate({ page: String(Math.min(Math.max(1, next), list.totalPages.value)) }, false)
}

function openDetail(row: IntegrationLogSummary): void {
  void router.push({
    name: 'admin-integration-logs',
    query: { ...listQuery.value, [DETAIL_KEY]: row.id },
  })
}

/**
 * Re-read only when the list's own query changes: opening and closing a row
 * changes the URL too, and must not reload the rows under it.
 */
watch(
  () => JSON.stringify(listQuery.value),
  () => {
    void list.apply(listQuery.value)
  },
  { immediate: true },
)

onMounted(() => {
  void loadSystems()
})
</script>

<template>
  <section class="space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">Integration logs</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        Every call Kelir made to or received from an external system, newest first. When a call is
        logged, a secret the system echoes is masked only in the spellings Kelir lists, and any
        other spelling is stored as the system sent it.
      </p>
    </div>

    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <div class="space-y-2">
        <Label for="integration-logs-system">System</Label>
        <Select
          id="integration-logs-system"
          data-testid="integration-logs-system"
          :model-value="list.filters.value.externalSystemId ?? ''"
          :options="systemOptions"
          :disabled="!canReadSystems && !list.filters.value.externalSystemId"
          placeholder="Any"
          :described-by="
            !canReadSystems || systemsError ? 'integration-logs-system-hint' : undefined
          "
          @update:model-value="setFilter('externalSystemId', $event)"
        />
        <p
          v-if="!canReadSystems"
          id="integration-logs-system-hint"
          class="text-xs text-muted-foreground"
          data-testid="integration-logs-system-hint"
        >
          Choosing a system needs <code>integration:external-system:read</code>.
        </p>
        <p
          v-else-if="systemsError"
          id="integration-logs-system-hint"
          class="text-xs text-destructive"
          data-testid="integration-logs-system-hint"
        >
          {{ systemsError }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="integration-logs-status">Status</Label>
        <Select
          id="integration-logs-status"
          data-testid="integration-logs-status"
          :model-value="list.filters.value.status ?? ''"
          :options="statusOptions"
          placeholder="Any"
          @update:model-value="setFilter('status', $event)"
        />
      </div>

      <div class="space-y-2">
        <Label for="integration-logs-from">Started from</Label>
        <Input
          id="integration-logs-from"
          data-testid="integration-logs-from"
          type="datetime-local"
          :model-value="toLocalInput(list.filters.value.from)"
          @change="setInstant('from', ($event.target as HTMLInputElement).value)"
        />
      </div>

      <div class="space-y-2">
        <Label for="integration-logs-to">Started before</Label>
        <Input
          id="integration-logs-to"
          data-testid="integration-logs-to"
          type="datetime-local"
          :model-value="toLocalInput(list.filters.value.to)"
          :aria-invalid="isRangeInverted"
          :aria-describedby="isRangeInverted ? 'integration-logs-range-error' : undefined"
          @change="setInstant('to', ($event.target as HTMLInputElement).value)"
        />
        <p
          v-if="isRangeInverted"
          id="integration-logs-range-error"
          class="text-xs text-destructive"
          data-testid="integration-logs-range-error"
        >
          This is before <em>Started from</em>, so no call can match.
        </p>
      </div>
    </div>

    <div v-if="hasFilters">
      <Button
        variant="ghost"
        size="sm"
        data-testid="integration-logs-clear"
        @click="clearFilters()"
      >
        Clear filters
      </Button>
    </div>

    <Alert v-if="list.error.value" variant="destructive" data-testid="integration-logs-error">
      <p>{{ list.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="list.apply(listQuery)">
        Try again
      </Button>
    </Alert>

    <p v-if="list.isLoading.value" class="text-sm text-muted-foreground" role="status">
      Loading integration logs…
    </p>

    <template v-else-if="!list.error.value">
      <p
        v-if="list.isEmpty.value"
        class="text-sm text-muted-foreground"
        data-testid="integration-logs-empty"
      >
        {{ hasFilters ? 'No calls match these filters.' : 'No calls have been logged yet.' }}
      </p>

      <Table v-else data-testid="integration-logs-table">
        <TableHeader>
          <TableRow>
            <TableHead>Started</TableHead>
            <TableHead>System</TableHead>
            <TableHead>Request</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Duration</TableHead>
            <TableHead><span class="sr-only">Details</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="row in list.items.value"
            :key="row.id"
            :data-testid="`integration-log-row-${row.id}`"
          >
            <TableCell class="whitespace-nowrap">{{ formatTimestamp(row.startedAt) }}</TableCell>
            <TableCell>
              <template v-if="row.externalSystemId">
                <RouterLink
                  v-if="canReadSystems"
                  :to="{ name: 'admin-external-system', params: { id: row.externalSystemId } }"
                  class="font-medium text-primary underline-offset-4 hover:underline"
                  data-testid="integration-log-system-link"
                >
                  {{ row.externalSystemCode ?? row.externalSystemId }}
                </RouterLink>
                <span v-else class="font-medium">{{
                  row.externalSystemCode ?? row.externalSystemId
                }}</span>
                <p v-if="row.externalSystemName" class="text-xs text-muted-foreground">
                  {{ row.externalSystemName }}
                </p>
              </template>
              <template v-else>—</template>
            </TableCell>
            <TableCell class="max-w-md break-all">
              <code class="font-mono text-xs"
                >{{ row.method ?? '' }} {{ row.endpoint ?? '—' }}</code
              >
              <p
                v-if="row.errorMessage"
                class="mt-1 text-xs text-destructive"
                data-testid="integration-log-error-message"
              >
                {{ row.errorMessage }}
              </p>
            </TableCell>
            <TableCell class="whitespace-nowrap">
              <Badge :variant="logStatusVariant(row.status)" data-testid="integration-log-status">
                {{ INTEGRATION_LOG_STATUS_LABELS[row.status] }}
              </Badge>
              <span v-if="row.statusCode !== null" class="ml-2 text-xs text-muted-foreground">
                HTTP {{ row.statusCode }}
              </span>
            </TableCell>
            <TableCell class="whitespace-nowrap">
              {{ row.durationMs === null ? '—' : formatDuration(row.durationMs) }}
            </TableCell>
            <TableCell class="text-right">
              <Button
                variant="outline"
                size="sm"
                :data-testid="`integration-log-open-${row.id}`"
                @click="openDetail(row)"
              >
                Details
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="list.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ list.page.value }} of {{ list.totalPages.value }} · {{ list.total.value }} calls
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

    <IntegrationLogDetailDialog v-model:open="isDetailOpen" :log-id="openLogId" />
  </section>
</template>
