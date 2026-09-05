<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { getRenderableList, listActions, listRenderedRows } from '@/api/rad'
import { toApiError } from '@/api/client'
import { Alert } from '@/components/ui/alert'
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
import { DOCUMENT_PRIORITY_LABELS, DOCUMENT_STATUS_LABELS } from '@/types/document'
import type { ListRow, RadAction, RenderableColumn, RenderableList } from '@/types/rad'

import { cellText, nextSort } from './cells'

/**
 * A configured list, rendered (FR-RAD-003, FR-RAD-010; #340).
 *
 * **The counterpart of the JFSS form renderer, and it works the same way: a
 * definition in, a screen out, no per-list code.** The storage API has existed
 * since Sprint 7 and nothing read it; this is the reader. Nothing on this page
 * names a column, a filter or a sort — every one of them comes from
 * `getRenderableList`, and a definition that declares different columns
 * produces a different table with no change here.
 *
 * **What this page deliberately does not decide:**
 *
 * - *Which columns exist, and whether one may be sorted on.* `domain/render.rs`
 *   resolved that against what the query can actually order by, so a
 *   `form_data.*` column arrives `sortable: false` however the definition
 *   marked it. Re-deriving it here would be a second copy of that rule.
 * - *Which actions to show.* The server drops any whose `required_permission`
 *   the caller lacks, so everything in `actions` is invocable. There is nothing
 *   to disable — a disabled button publishes the existence of an action the
 *   permission was set to hide.
 * - *Which rows match a filter.* Every parameter goes on the wire. Narrowing a
 *   fetched page here would make `meta.total` disagree with the rows under it,
 *   and on this surface the visibility rule lives in the backend's query.
 *
 * **A failure is named, never an empty table** (#340 AC4). A list that is a
 * draft, that no document type binds, or that declares a column nothing can
 * resolve renders its refusal where the table would have been — because a table
 * with no rows reads as *no documents*, which is #326's failure one panel over.
 *
 * **The URL is the state**, as `DocumentListPage` has it (#101 AC3): page, sort
 * and every filter are read from the query string and written back to it, so a
 * filtered list can be linked to and survives a reload.
 */
const route = useRoute()
const router = useRouter()

/** The definition, once it has loaded. */
const definition = ref<RenderableList | null>(null)
/** Why the definition could not be rendered, empty when it could. */
const refusal = ref('')
const isLoadingDefinition = ref(true)
const actions = ref<RadAction[]>([])

const listKey = computed(() => String(route.params.listKey ?? ''))

const rows = useQueryBackedList<ListRow>((query) =>
  listRenderedRows(definition.value?.id ?? '', query),
)

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
 * The sort in force: the URL's, or the definition's own where the URL is silent.
 *
 * The definition's default is *not* written into the URL on load. A URL that
 * carried it would pin the list to whatever the default was on the day somebody
 * copied the link, and the point of a default is that it moves when its author
 * moves it.
 */
const sort = computed(() => {
  const key = currentQuery.value.sort

  if (key) {
    return { key, descending: currentQuery.value.dir === 'desc' }
  }

  if (!definition.value) {
    return null
  }

  return {
    key: definition.value.defaultSortKey,
    descending: definition.value.defaultSortDescending,
  }
})

function isSortedBy(column: RenderableColumn): boolean {
  return sort.value?.key === column.key
}

/** The arrow a header shows, or nothing. */
function sortMarker(column: RenderableColumn): string {
  if (!isSortedBy(column)) {
    return ''
  }

  return sort.value?.descending ? '↓' : '↑'
}

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

  void router.push({ name: 'rendered-list', params: { listKey: listKey.value }, query: next })
}

/** Clicking a header cycles ascending, descending, then back to the default. */
function toggleSort(column: RenderableColumn): void {
  if (!column.sortable) {
    return
  }

  const next = nextSort(column, sort.value)

  navigate({ sort: next?.key ?? '', dir: next?.descending ? 'desc' : '' })
}

function setFilter(key: string, value: string): void {
  navigate({ [key]: value })
}

function goToPage(next: number): void {
  navigate({ page: String(Math.min(Math.max(1, next), rows.totalPages.value)) }, false)
}

/**
 * The options an `ENUM` filter offers.
 *
 * From the definition's own `options` where it declares any, and otherwise from
 * the vocabulary the parameter has — a `status` filter with no configured
 * options is far more likely to mean *all ten statuses* than *none*, and a
 * select with no options is a control nobody can use.
 */
function optionsFor(filter: { parameter: string; options: unknown }): {
  value: string
  label: string
}[] {
  if (Array.isArray(filter.options)) {
    return filter.options.map((option) =>
      typeof option === 'string'
        ? { value: option, label: option }
        : (option as { value: string; label: string }),
    )
  }

  if (filter.parameter === 'status') {
    return Object.entries(DOCUMENT_STATUS_LABELS).map(([value, label]) => ({ value, label }))
  }

  if (filter.parameter === 'priority') {
    return Object.entries(DOCUMENT_PRIORITY_LABELS).map(([value, label]) => ({ value, label }))
  }

  return []
}

/** A `NAVIGATE` action's route, from its own config. */
function openRow(row: ListRow): void {
  // Every rendered list is over documents, so a row opens the document it is.
  // A configured `NAVIGATE` action can send somebody elsewhere; this is what
  // clicking the row itself does, and it needs no configuration to work.
  void router.push({ name: 'document', params: { id: row.id } })
}

function runAction(action: RadAction, row: ListRow): void {
  const target = action.config?.route

  if (action.actionType === 'NAVIGATE' && typeof target === 'string') {
    void router.push(target.replace(':id', row.id))
    return
  }

  // Every other action type is configured and not yet executable here. Saying
  // so beats a button that silently does nothing — the action exists, and what
  // it does is #341's and the plugin surface's.
  refusal.value = `\`${action.actionKey}\` is a ${action.actionType} action, which this list cannot run yet.`
}

/** Loads the definition, then lets the row watcher run. */
async function loadDefinition(): Promise<void> {
  isLoadingDefinition.value = true
  refusal.value = ''
  definition.value = null

  try {
    definition.value = await getRenderableList(listKey.value)
  } catch (error) {
    // **Named, not empty.** The backend's refusal already says which column,
    // filter or sort is wrong, or that the list is a draft; this shows it
    // rather than rendering a table with no rows.
    const failure = toApiError(error)

    refusal.value = failure.details.length
      ? failure.details.map((detail) => `${detail.path}: ${detail.message}`).join('\n')
      : failure.message
  } finally {
    isLoadingDefinition.value = false
  }
}

async function loadActions(): Promise<void> {
  try {
    actions.value = (await listActions('LIST')).items
  } catch {
    // A list that renders without its buttons is still a list. Failing the
    // whole screen because the action catalogue was unreachable would make a
    // configuration surface a dependency of a reading one.
    actions.value = []
  }
}

watch(
  listKey,
  () => {
    void loadDefinition()
    void loadActions()
  },
  { immediate: true },
)

// One load path for the rows: the definition arrived or the URL changed, so
// re-apply it. A deep-linked filtered page loads its filters rather than the
// whole population and then narrows.
watch(
  [definition, () => route.query],
  () => {
    if (definition.value) {
      void rows.apply(currentQuery.value)
    }
  },
  { immediate: true, deep: true },
)
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight" data-testid="list-title">
          {{ definition?.title ?? listKey }}
        </h2>
        <p v-if="definition" class="mt-1 text-sm text-muted-foreground">
          {{ rows.total.value }} row{{ rows.total.value === 1 ? '' : 's' }}
        </p>
      </div>
    </div>

    <!-- The definition could not be rendered. This is where the table would
         have been, deliberately: a refusal beside an empty table would let
         somebody read the table. -->
    <Alert v-if="refusal" variant="destructive" data-testid="list-refusal">
      <p class="whitespace-pre-line">{{ refusal }}</p>
    </Alert>

    <p v-else-if="isLoadingDefinition" class="text-sm text-muted-foreground">Loading…</p>

    <template v-else-if="definition">
      <div v-if="definition.filters.length" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <div v-for="filter in definition.filters" :key="filter.key" class="space-y-2">
          <Label :for="`filter-${filter.key}`">{{ filter.label }}</Label>

          <Select
            v-if="filter.filterType === 'ENUM'"
            :id="`filter-${filter.key}`"
            :data-testid="`filter-${filter.key}`"
            :options="optionsFor(filter)"
            placeholder="Any"
            :model-value="currentQuery[filter.key] ?? ''"
            @update:model-value="setFilter(filter.key, String($event))"
          />

          <Input
            v-else
            :id="`filter-${filter.key}`"
            :data-testid="`filter-${filter.key}`"
            :model-value="currentQuery[filter.key] ?? ''"
            :placeholder="filter.label"
            @update:model-value="setFilter(filter.key, String($event))"
          />
        </div>
      </div>

      <Alert v-if="rows.error.value" variant="destructive" data-testid="rows-error">
        {{ rows.error.value }}
      </Alert>

      <Table data-testid="rendered-list">
        <TableHeader>
          <TableRow>
            <TableHead
              v-for="column in definition.columns"
              :key="column.key"
              :style="column.width ? { width: column.width } : undefined"
              :data-testid="`column-${column.key}`"
              :aria-sort="
                isSortedBy(column) ? (sort?.descending ? 'descending' : 'ascending') : undefined
              "
            >
              <button
                v-if="column.sortable"
                type="button"
                class="font-medium hover:underline"
                :data-testid="`sort-${column.key}`"
                @click="toggleSort(column)"
              >
                {{ column.label }} {{ sortMarker(column) }}
              </button>
              <span v-else>{{ column.label }}</span>
            </TableHead>
            <TableHead v-if="actions.length" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow
            v-for="row in rows.items.value"
            :key="row.id"
            class="cursor-pointer"
            :data-testid="`row-${row.id}`"
            @click="openRow(row)"
          >
            <TableCell
              v-for="column in definition.columns"
              :key="column.key"
              :data-testid="`cell-${column.key}`"
            >
              {{ cellText(column, row.cells[column.key]) }}
            </TableCell>

            <TableCell v-if="actions.length" class="space-x-2 text-right">
              <Button
                v-for="action in actions"
                :key="action.id"
                size="sm"
                variant="secondary"
                :data-testid="`action-${action.actionKey}`"
                @click.stop="runAction(action, row)"
              >
                {{ action.label }}
              </Button>
            </TableCell>
          </TableRow>

          <TableRow v-if="rows.isEmpty.value">
            <TableCell
              :colspan="definition.columns.length + (actions.length ? 1 : 0)"
              class="text-center text-sm text-muted-foreground"
              data-testid="no-rows"
            >
              No documents match this list yet.
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="rows.totalPages.value > 1" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ rows.page.value }} of {{ rows.totalPages.value }}
        </p>

        <div class="space-x-2">
          <Button
            variant="secondary"
            :disabled="!rows.hasPrevious.value"
            data-testid="previous-page"
            @click="goToPage(rows.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="secondary"
            :disabled="!rows.hasNext.value"
            data-testid="next-page"
            @click="goToPage(rows.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>
  </section>
</template>
