<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

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
  PARTY_STATUS_LABELS,
  PARTY_TYPE_LABELS,
  ROLE_STATUS_LABELS,
  type MasterDataRow,
  type PartyStatus,
  type PartyType,
} from '@/types/master-data'
import { MASTER_DATA_VIEWS, viewByKey, type MasterDataView } from './views'

/**
 * Master data, found (FR-MDM-008; issue #101).
 *
 * **One component over four endpoints.** The backend shaped the role-view row
 * so that a client rendering `/suppliers`, `/customers` and `/employees` needs
 * one component and not three, and `/parties` differs only by having no role
 * number and no filters. Four screens differing in a column would be four
 * screens to keep in step.
 *
 * **The URL is the state.** Page, search and filters are read from the query
 * string and written back to it, so a filtered list can be linked to and
 * survives a reload (#101 AC3). Nothing shadows them: a control writes the URL
 * and the watcher below loads, which is the only path that loads.
 *
 * **A caller who may not read roles is not shown a broken screen** (#101 AC5).
 * The role views need `master-data:party-role:read` as well as
 * `master-data:party:read`, so a caller holding only the first sees the Parties
 * tab and no others — the permitted subset, rather than three tabs that answer
 * 403.
 */
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

/** The views this caller may actually open. */
const availableViews = computed(() =>
  MASTER_DATA_VIEWS.filter((candidate) =>
    candidate.permissions.every((permission) => auth.can(permission)),
  ),
)

const view = computed<MasterDataView>(() => {
  const requested = typeof route.params.view === 'string' ? viewByKey(route.params.view) : undefined

  // A view the caller cannot open falls back to one they can, rather than
  // rendering a table that will only ever answer 403. The guard has already
  // decided they may be on this page at all.
  if (requested && availableViews.value.includes(requested)) {
    return requested
  }

  return availableViews.value[0] ?? MASTER_DATA_VIEWS[0]
})

const list = useQueryBackedList<MasterDataRow>((query) => view.value.fetch(query))

const statusOptions = [
  { value: 'PARTY_ENABLED', label: PARTY_STATUS_LABELS.PARTY_ENABLED },
  { value: 'PARTY_DISABLED', label: PARTY_STATUS_LABELS.PARTY_DISABLED },
]
const partyTypeOptions = [
  { value: 'PERSON', label: PARTY_TYPE_LABELS.PERSON },
  { value: 'PARTY_GROUP', label: PARTY_TYPE_LABELS.PARTY_GROUP },
]
const roleStatusOptions = [
  { value: 'ACTIVE', label: ROLE_STATUS_LABELS.ACTIVE },
  { value: 'INACTIVE', label: ROLE_STATUS_LABELS.INACTIVE },
]

/** The query string as flat strings, which is what the composable applies. */
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

/**
 * Writes a change to the URL, and lets the watcher load.
 *
 * Any change other than the page itself sends the caller back to page 1:
 * staying on page 7 of a list that has just been narrowed to two rows would
 * show an empty table and a pager that disagrees with it.
 */
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

  void router.push({ name: 'master-data', params: { view: view.value.key }, query: next })
}

function openView(next: MasterDataView): void {
  // The filters do not travel between views: `/parties` accepts none of them,
  // and carrying a `roleStatusId` onto it would put a value in the URL that
  // nothing reads.
  void router.push({ name: 'master-data', params: { view: next.key }, query: {} })
}

function setFilter(key: string, value: string): void {
  navigate({ [key]: value })
}

function goToPage(next: number): void {
  const target = Math.min(Math.max(1, next), list.totalPages.value)

  navigate({ page: String(target) }, false)
}

function statusLabel(value: string | undefined): string {
  return value ? (PARTY_STATUS_LABELS[value as PartyStatus] ?? value) : ''
}

function typeLabel(value: string | undefined): string {
  return value ? (PARTY_TYPE_LABELS[value as PartyType] ?? value) : ''
}

// One load path: the URL changed, so re-apply it. Mounting counts, which is
// what `immediate` is for — a deep-linked filtered page must load its filters
// rather than the whole population and then narrow.
watch(
  () => [view.value.key, route.query] as const,
  () => {
    void list.apply(currentQuery.value)
  },
  { immediate: true, deep: true },
)
</script>

<template>
  <section class="space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">{{ view.title }}</h2>
      <p class="mt-1 text-sm text-muted-foreground">{{ view.description }}</p>
    </div>

    <nav class="flex flex-wrap gap-2" aria-label="Master data views">
      <Button
        v-for="candidate in availableViews"
        :key="candidate.key"
        :variant="candidate.key === view.key ? 'default' : 'outline'"
        size="sm"
        :aria-current="candidate.key === view.key ? 'page' : undefined"
        @click="openView(candidate)"
      >
        {{ candidate.title }}
      </Button>
    </nav>

    <div v-if="view.filterable" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <div class="space-y-2">
        <Label :for="`${view.key}-search`">Search</Label>
        <Input
          :id="`${view.key}-search`"
          :model-value="list.filters.value.search ?? ''"
          placeholder="Code, name or number"
          @change="setFilter('search', ($event.target as HTMLInputElement).value)"
        />
      </div>

      <div class="space-y-2">
        <Label :for="`${view.key}-status`">Party status</Label>
        <Select
          :id="`${view.key}-status`"
          :model-value="list.filters.value.statusId ?? ''"
          :options="statusOptions"
          placeholder="Any"
          @update:model-value="setFilter('statusId', $event)"
        />
      </div>

      <div class="space-y-2">
        <Label :for="`${view.key}-type`">Party type</Label>
        <Select
          :id="`${view.key}-type`"
          :model-value="list.filters.value.partyTypeId ?? ''"
          :options="partyTypeOptions"
          placeholder="Any"
          @update:model-value="setFilter('partyTypeId', $event)"
        />
      </div>

      <div class="space-y-2">
        <Label :for="`${view.key}-role-status`">Role status</Label>
        <Select
          :id="`${view.key}-role-status`"
          :model-value="list.filters.value.roleStatusId ?? ''"
          :options="roleStatusOptions"
          placeholder="Any"
          @update:model-value="setFilter('roleStatusId', $event)"
        />
      </div>
    </div>

    <Alert v-if="list.error.value" variant="destructive">
      <p>{{ list.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="list.apply(currentQuery)">
        Try again
      </Button>
    </Alert>

    <p v-if="list.isLoading.value" class="text-sm text-muted-foreground">
      Loading {{ view.title.toLowerCase() }}…
    </p>

    <template v-else-if="!list.error.value">
      <p v-if="list.isEmpty.value" class="text-sm text-muted-foreground">
        Nothing matches this view.
      </p>

      <Table v-else>
        <TableHeader>
          <TableRow>
            <TableHead>Code</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Type</TableHead>
            <TableHead>Status</TableHead>
            <TableHead v-if="view.numberLabel">{{ view.numberLabel }}</TableHead>
            <TableHead v-if="view.numberLabel">Role status</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow v-for="row in list.items.value" :key="row.id">
            <TableCell class="font-medium">{{ row.partyId }}</TableCell>
            <TableCell>{{ row.name }}</TableCell>
            <TableCell>{{ typeLabel(row.partyTypeId) }}</TableCell>
            <TableCell>
              <Badge :variant="row.statusId === 'PARTY_ENABLED' ? 'default' : 'secondary'">
                {{ statusLabel(row.statusId) }}
              </Badge>
            </TableCell>
            <TableCell v-if="view.numberLabel">
              <!-- A party may hold the role without a profile, which is legal;
                   an empty cell says so without pretending there is a number. -->
              <span v-if="row.roleNumber">{{ row.roleNumber }}</span>
              <span v-else class="text-muted-foreground">—</span>
            </TableCell>
            <TableCell v-if="view.numberLabel">
              <Badge :variant="row.roleStatusId === 'ACTIVE' ? 'default' : 'secondary'">
                {{ row.roleStatusId ? ROLE_STATUS_LABELS[row.roleStatusId] : '' }}
              </Badge>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="list.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ list.page.value }} of {{ list.totalPages.value }} · {{ list.total.value }}
          {{ view.title.toLowerCase() }}
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!list.hasPrevious.value"
            @click="goToPage(list.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!list.hasNext.value"
            @click="goToPage(list.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>
  </section>
</template>
