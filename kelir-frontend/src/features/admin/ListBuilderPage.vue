<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'

import { toApiError } from '@/api/client'
import { getList, getRenderableList, updateList } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { useAuthStore } from '@/stores/auth'
import type { ValidationDetail } from '@/types/api'
import type { ListColumn, ListDefinition, ListFilter, ListFilterType } from '@/types/rad'

/**
 * The list builder (FR-RAD-003, FR-RAD-004; #374).
 *
 * **A list definition is refused at render, not at save, and that is the
 * opposite of a form** ([SDD](../../../../docs/design/01.%20System%20Design%20Document.md)
 * §8.2.4). The storage API stores whatever it is given, because `rad_lists` is
 * generic and refusing `form_data.amount` there would put the document module's
 * vocabulary inside a table written for lists over anything. §8.2.4 states the
 * cost — *a broken list definition is stored happily and fails when somebody
 * opens it* — and names what makes it acceptable: **the builder calls the same
 * function to show an author the problem**.
 *
 * This screen is that sentence, and the mechanism is worth stating because it
 * is not the obvious one:
 *
 * - **The verdict is the server's, fetched over `GET /rad/lists/by-key/{key}`.**
 *   `renderable_list` runs `render::plan` *before* it checks that a document
 *   type binds the list, so an unbound list still gets a real answer about its
 *   columns, filters and sort. That ordering is what makes a validator out of
 *   an endpoint written to draw a screen.
 * - **It arrives after the save, not before it.** There is no dry-run endpoint,
 *   and adding one is a backend change #374 excludes. So the screen saves, asks
 *   immediately, and shows the answer — which is before anybody *opens* the
 *   list, though not before it is stored.
 * - **A list nothing binds is reported as its own state**, not as a broken
 *   definition. It is the ordinary condition of a list that has just been
 *   authored, and calling it an error would teach authors to ignore the panel.
 */
const auth = useAuthStore()
const route = useRoute()

const canUpdate = computed(() => auth.can('rad:list:update'))

/**
 * The thirteen fields a document row offers, and the nine of them a query can
 * order by (`render::SummaryField`, `DocumentSortKey`).
 *
 * **This is a chooser, not a validator**, and the same trade the form builder's
 * type list makes: the server resolves every key and refuses what it cannot
 * draw, so a key past this list is caught there. What the list buys is that a
 * typo is impossible in the common case, and the alternative — a free-text box
 * over a closed vocabulary — makes every author discover the vocabulary by
 * being refused. The cost is stated: a field added to `SummaryField` is a
 * second place to change.
 */
const SUMMARY_FIELDS = [
  'documentRef',
  'documentNumber',
  'documentTypeCode',
  'title',
  'status',
  'priority',
  'submittedAt',
  'createdAt',
  'updatedAt',
  'id',
  'documentTypeId',
  'entityType',
  'entityId',
] as const

/** The nine of those an `ORDER BY` has an arm for. A `form_data.` path never is. */
const SORTABLE = new Set([
  'documentRef',
  'documentNumber',
  'documentTypeCode',
  'title',
  'status',
  'priority',
  'submittedAt',
  'createdAt',
  'updatedAt',
])

/**
 * The refusal that means *nothing names this list yet* rather than *this
 * definition is wrong* (`document::service::list::LIST_NOT_BOUND`).
 */
const LIST_NOT_BOUND = 'LIST_NOT_BOUND'

/** What a document list can be filtered by (`render::FilterParameter`). */
const FILTER_PARAMETERS = ['search', 'status', 'priority', 'entityType', 'entityId'] as const

const FILTER_TYPES: ListFilterType[] = [
  'TEXT',
  'ENUM',
  'LOOKUP',
  'DATE_RANGE',
  'NUMBER_RANGE',
  'BOOLEAN',
]

function choices(values: readonly string[]): { value: string; label: string }[] {
  return values.map((value) => ({ value, label: value }))
}

const COLUMN_CHOICES = [
  ...choices(SUMMARY_FIELDS),
  { value: 'form_data.', label: 'form_data.… (a JFSS key)' },
]
const FILTER_CHOICES = choices(FILTER_PARAMETERS)
const FILTER_TYPE_CHOICES = choices(FILTER_TYPES)
const STATUS_CHOICES = choices(['ACTIVE', 'DEPRECATED'])
const DIRECTIONS = choices(['asc', 'desc'])

const definition = ref<ListDefinition | null>(null)
const title = ref('')
const status = ref('ACTIVE')
const pageSize = ref('20')
const columns = ref<ListColumn[]>([])
const filters = ref<ListFilter[]>([])
const sortKey = ref('')
const sortDir = ref('asc')

const isLoading = ref(true)
const isSaving = ref(false)
const loadError = ref('')
const saveError = ref('')
const details = ref<ValidationDetail[]>([])

/** What the server said the last time it was asked to draw this list. */
const renderVerdict = ref<'unknown' | 'drawable' | 'unbound' | 'broken'>('unknown')
const renderMessage = ref('')
const renderDetails = ref<ValidationDetail[]>([])

/** The sort choosers offer only columns this definition marks sortable. */
const sortableColumns = computed(() =>
  columns.value
    .filter((column) => column.isSortable && SORTABLE.has(column.columnKey.trim()))
    .map((column) => ({ value: column.columnKey, label: column.columnKey })),
)

function detailFor(path: string): string | undefined {
  return [...details.value, ...renderDetails.value].find((detail) => detail.path === path)?.message
}

/** `default_sort_json` is `[{"key": …, "dir": …}]` (§5.6). */
function readSort(value: unknown): { key: string; dir: string } {
  const first = Array.isArray(value) ? (value[0] as Record<string, unknown> | undefined) : undefined

  return {
    key: typeof first?.key === 'string' ? first.key : '',
    dir: first?.dir === 'desc' ? 'desc' : 'asc',
  }
}

async function load(): Promise<void> {
  isLoading.value = true
  loadError.value = ''

  try {
    const loaded = await getList(String(route.params.id))

    definition.value = loaded
    title.value = loaded.title
    status.value = loaded.status
    pageSize.value = String(loaded.pageSize)
    columns.value = loaded.columns.map((column) => ({ ...column }))
    filters.value = loaded.filters.map((filter) => ({ ...filter }))

    const sort = readSort(loaded.defaultSort)

    sortKey.value = sort.key
    sortDir.value = sort.dir

    await askWhetherItDraws(loaded.listKey)
  } catch (failure) {
    loadError.value = toApiError(failure).message
  } finally {
    isLoading.value = false
  }
}

/**
 * Asks the renderer's own endpoint what it makes of this definition.
 *
 * **Three answers, and only one of them is a defect.** A plan failure is the
 * author's problem and names the key; *nothing binds this list* is the ordinary
 * state of a list just authored; and a definition that is not `ACTIVE` cannot
 * be asked at all, which the panel says rather than reporting as broken.
 */
async function askWhetherItDraws(listKey: string): Promise<void> {
  renderDetails.value = []
  renderMessage.value = ''

  if (status.value !== 'ACTIVE') {
    renderVerdict.value = 'unknown'
    renderMessage.value =
      'Only an ACTIVE list can be drawn, so nothing was asked. Set it ACTIVE to check it.'

    return
  }

  try {
    await getRenderableList(listKey)
    renderVerdict.value = 'drawable'
  } catch (failure) {
    const failed = toApiError(failure)

    // **Told apart by the code, not by whether details are present.** Both
    // refusals carry them: `require_bound` answers `AppError::validation` with
    // a `LIST_NOT_BOUND` detail on `listId`, exactly as a plan failure answers
    // with `COLUMN_NOT_RENDERABLE` on a column. Branching on *has details* put
    // every freshly authored list in the broken panel, which is what the
    // browser flow caught and the unit spec did not — its fake returned a
    // detail-less 404, which is a shape the server never sends.
    const unbound = failed.details.some((detail) => detail.code === LIST_NOT_BOUND)

    renderVerdict.value = unbound ? 'unbound' : 'broken'
    renderDetails.value = unbound ? [] : failed.details
    renderMessage.value = failed.message
  }
}

function addColumn(): void {
  columns.value = [...columns.value, { columnKey: 'title', label: 'Title', isSortable: true }]
}

function removeColumn(at: number): void {
  columns.value = columns.value.filter((_, index) => index !== at)
}

function moveColumn(at: number, direction: -1 | 1): void {
  const target = at + direction

  if (target < 0 || target >= columns.value.length) {
    return
  }

  const next = [...columns.value]
  const [moved] = next.splice(at, 1)

  next.splice(target, 0, moved)
  columns.value = next
}

function patchColumn(at: number, changes: Partial<ListColumn>): void {
  columns.value = columns.value.map((column, index) =>
    index === at ? { ...column, ...changes } : column,
  )
}

function addFilter(): void {
  filters.value = [
    ...filters.value,
    { filterKey: 'search', label: 'Search', filterType: 'TEXT', isDefault: false },
  ]
}

function removeFilter(at: number): void {
  filters.value = filters.value.filter((_, index) => index !== at)
}

function patchFilter(at: number, changes: Partial<ListFilter>): void {
  filters.value = filters.value.map((filter, index) =>
    index === at ? { ...filter, ...changes } : filter,
  )
}

async function save(): Promise<void> {
  const current = definition.value

  if (!current) {
    return
  }

  isSaving.value = true
  saveError.value = ''
  details.value = []

  try {
    const saved = await updateList(current.id, {
      title: title.value.trim(),
      status: status.value as ListDefinition['status'],
      pageSize: Number(pageSize.value) || 20,
      // **Both collections, every time.** A collection that is sent replaces
      // the stored set wholesale, so sending one would delete the other.
      columns: columns.value,
      filters: filters.value,
      defaultSort: sortKey.value ? [{ key: sortKey.value, dir: sortDir.value }] : null,
    })

    definition.value = saved
    columns.value = saved.columns.map((column) => ({ ...column }))
    filters.value = saved.filters.map((filter) => ({ ...filter }))

    // **Immediately, and this is the point of the screen.** The save succeeded
    // whatever the definition says; whether anybody can open it is the next
    // question, and an author who has to guess is the state §8.2.4 accepted on
    // the condition that this asks.
    await askWhetherItDraws(saved.listKey)
  } catch (failure) {
    const failed = toApiError(failure)

    saveError.value = failed.message
    details.value = failed.details
  } finally {
    isSaving.value = false
  }
}

onMounted(() => {
  void load()
})
</script>

<template>
  <section class="space-y-6">
    <Alert v-if="loadError" variant="destructive" data-testid="load-error">{{ loadError }}</Alert>

    <p v-if="isLoading" class="text-sm text-muted-foreground">Loading…</p>

    <template v-if="definition">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 class="text-xl font-semibold tracking-tight">{{ definition.listKey }}</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Rows are the documents of every type that names this list.
          </p>
        </div>

        <div class="flex items-center gap-2">
          <Badge :variant="definition.status === 'ACTIVE' ? 'default' : 'secondary'">
            {{ definition.status }}
          </Badge>
          <Button v-if="canUpdate" :disabled="isSaving" data-testid="save-list" @click="save">
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>
        </div>
      </div>

      <Alert v-if="saveError" variant="destructive" data-testid="save-error">{{ saveError }}</Alert>

      <!-- **The panel §8.2.4 asked for.** Not an empty table and not silence:
           the server's own answer about whether this definition can be drawn,
           with the key it could not resolve. -->
      <Alert v-if="renderVerdict === 'broken'" variant="destructive" data-testid="render-broken">
        <p class="font-medium">This list is stored and cannot be drawn.</p>
        <ul class="mt-2 space-y-1">
          <li v-for="detail in renderDetails" :key="detail.path" class="text-sm">
            <span class="font-mono text-xs">{{ detail.path }}</span> — {{ detail.message }}
          </li>
        </ul>
      </Alert>

      <Alert v-else-if="renderVerdict === 'unbound'" data-testid="render-unbound">
        This definition is drawable, and no document type names it yet — so opening it would show
        nothing. Bind it on a document type to give it rows. {{ renderMessage }}
      </Alert>

      <Alert v-else-if="renderVerdict === 'drawable'" data-testid="render-drawable">
        The server can draw this list.
      </Alert>

      <Alert v-else-if="renderMessage" data-testid="render-unknown">{{ renderMessage }}</Alert>

      <div class="grid gap-4 sm:grid-cols-3">
        <div class="space-y-2">
          <Label for="list-title">Title</Label>
          <Input id="list-title" v-model="title" :disabled="!canUpdate" data-testid="list-title" />
        </div>

        <div class="space-y-2">
          <Label for="list-status">Status</Label>
          <Select
            id="list-status"
            v-model="status"
            :options="STATUS_CHOICES"
            :disabled="!canUpdate"
            data-testid="list-status"
          />
        </div>

        <div class="space-y-2">
          <Label for="list-page-size">Page size</Label>
          <Input
            id="list-page-size"
            v-model="pageSize"
            :disabled="!canUpdate"
            data-testid="list-page-size"
          />
          <!-- Said here because an author would otherwise expect a request to
               be able to widen it; a client that could would be overruling the
               configuration it had just read. -->
          <p class="text-xs text-muted-foreground">
            The definition decides this. A request asking for a bigger page is refused by name.
          </p>
        </div>
      </div>

      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-semibold">Columns</h3>
          <Button
            size="sm"
            variant="secondary"
            :disabled="!canUpdate"
            data-testid="add-column"
            @click="addColumn"
          >
            Add column
          </Button>
        </div>

        <div
          v-for="(column, at) in columns"
          :key="at"
          class="grid gap-2 rounded border p-3 sm:grid-cols-4"
          :data-testid="`column-${at}`"
        >
          <Select
            :model-value="column.columnKey"
            :options="COLUMN_CHOICES"
            :disabled="!canUpdate"
            :data-testid="`column-key-${at}`"
            @update:model-value="patchColumn(at, { columnKey: String($event) })"
          />
          <Input
            :model-value="column.label"
            :disabled="!canUpdate"
            :data-testid="`column-label-${at}`"
            @update:model-value="patchColumn(at, { label: String($event) })"
          />

          <div class="flex items-center gap-2">
            <input
              :id="`sortable-${at}`"
              type="checkbox"
              class="h-4 w-4"
              :checked="column.isSortable"
              :disabled="!canUpdate || !SORTABLE.has(column.columnKey.trim())"
              :data-testid="`column-sortable-${at}`"
              @change="patchColumn(at, { isSortable: ($event.target as HTMLInputElement).checked })"
            />
            <Label :for="`sortable-${at}`">Sortable</Label>
          </div>

          <div class="space-x-1 text-right">
            <Button
              size="sm"
              variant="secondary"
              :disabled="!canUpdate || at === 0"
              :data-testid="`column-up-${at}`"
              @click="moveColumn(at, -1)"
            >
              Up
            </Button>
            <Button
              size="sm"
              variant="destructive"
              :disabled="!canUpdate"
              :data-testid="`column-remove-${at}`"
              @click="removeColumn(at)"
            >
              Remove
            </Button>
          </div>

          <!-- A `form_data.` column needs the JFSS key typing, and can never be
               sorted: ordering by a JSONB path needs an index nothing creates. -->
          <div v-if="column.columnKey.startsWith('form_data.')" class="sm:col-span-4">
            <Input
              :model-value="column.columnKey"
              :disabled="!canUpdate"
              :data-testid="`column-path-${at}`"
              placeholder="form_data.amount"
              @update:model-value="patchColumn(at, { columnKey: String($event) })"
            />
            <p class="mt-1 text-xs text-muted-foreground">
              The JFSS data key the value is stored under. A payload column can never be sorted.
            </p>
          </div>

          <p
            v-if="detailFor(`columns.${at}.columnKey`)"
            class="text-xs text-destructive sm:col-span-4"
          >
            {{ detailFor(`columns.${at}.columnKey`) }}
          </p>
        </div>

        <p v-if="columns.length === 0" class="text-sm text-destructive" data-testid="no-columns">
          A list with no columns renders as an empty table, which reads as <em>no documents</em>.
          The server refuses to draw it.
        </p>
      </div>

      <div class="space-y-3">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-semibold">Filters</h3>
          <Button
            size="sm"
            variant="secondary"
            :disabled="!canUpdate"
            data-testid="add-filter"
            @click="addFilter"
          >
            Add filter
          </Button>
        </div>

        <div
          v-for="(filter, at) in filters"
          :key="at"
          class="grid gap-2 rounded border p-3 sm:grid-cols-4"
          :data-testid="`filter-${at}`"
        >
          <Select
            :model-value="filter.filterKey"
            :options="FILTER_CHOICES"
            :disabled="!canUpdate"
            :data-testid="`filter-key-${at}`"
            @update:model-value="patchFilter(at, { filterKey: String($event) })"
          />
          <Input
            :model-value="filter.label"
            :disabled="!canUpdate"
            :data-testid="`filter-label-${at}`"
            @update:model-value="patchFilter(at, { label: String($event) })"
          />
          <Select
            :model-value="filter.filterType"
            :options="FILTER_TYPE_CHOICES"
            :disabled="!canUpdate"
            :data-testid="`filter-type-${at}`"
            @update:model-value="patchFilter(at, { filterType: $event as ListFilterType })"
          />
          <div class="text-right">
            <Button
              size="sm"
              variant="destructive"
              :disabled="!canUpdate"
              :data-testid="`filter-remove-${at}`"
              @click="removeFilter(at)"
            >
              Remove
            </Button>
          </div>

          <p
            v-if="detailFor(`filters.${at}.filterKey`)"
            class="text-xs text-destructive sm:col-span-4"
          >
            {{ detailFor(`filters.${at}.filterKey`) }}
          </p>
        </div>
      </div>

      <div class="grid gap-4 sm:grid-cols-3">
        <div class="space-y-2">
          <Label for="sort-key">Opens sorted by</Label>
          <Select
            id="sort-key"
            v-model="sortKey"
            :options="[{ value: '', label: 'Newest first (the default)' }, ...sortableColumns]"
            :disabled="!canUpdate"
            data-testid="sort-key"
          />
          <!-- Only columns this definition marks sortable, because that is what
               `plan_sort` resolves against. -->
          <p class="text-xs text-muted-foreground">One of this list's own sortable columns.</p>
          <p v-if="detailFor('defaultSort')" class="text-xs text-destructive">
            {{ detailFor('defaultSort') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="sort-dir">Direction</Label>
          <Select
            id="sort-dir"
            v-model="sortDir"
            :options="DIRECTIONS"
            :disabled="!canUpdate || sortKey === ''"
            data-testid="sort-dir"
          />
        </div>
      </div>
    </template>
  </section>
</template>
