<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { listDocuments } from '@/api/documents'
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
  DOCUMENT_PRIORITY_LABELS,
  DOCUMENT_STATUS_LABELS,
  type DocumentStatus,
  type DocumentSummary,
} from '@/types/document'

/**
 * Documents, found (FR-DOC-013, FR-SRH-001; #171).
 *
 * **The URL is the state**, exactly as the master-data list has it (#101 AC3):
 * page, search and filters are read from the query string and written back to
 * it, so a filtered list can be linked to and survives a reload. Nothing
 * shadows them — a control writes the URL and the watcher loads, which is the
 * only path that loads.
 *
 * **Nothing is narrowed here.** Every parameter goes on the wire. On this
 * surface that is more than a performance rule: the list's visibility rule is
 * enforced in the backend's query, and a client-side filter would be a second
 * rule over the same rows — and the one in the query would stop being the
 * answer.
 *
 * **FR-SRH-001 is this screen.** The SRS's own note says the search area
 * surfaces the same capability, so there is no second search page to keep in
 * step with this one.
 */
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()

const list = useQueryBackedList<DocumentSummary>(listDocuments)

/**
 * The statuses a person may filter for.
 *
 * All ten, including the four nothing can currently reach. A filter is a
 * question about what is stored rather than a claim about what the product
 * does: a deployment migrating documents in from another system can have rows
 * in any of them, and a list that could not ask about those rows would hide
 * them.
 */
const statusOptions = (Object.keys(DOCUMENT_STATUS_LABELS) as DocumentStatus[]).map((status) => ({
  value: status,
  label: DOCUMENT_STATUS_LABELS[status],
}))

const priorityOptions = Object.entries(DOCUMENT_PRIORITY_LABELS).map(([value, label]) => ({
  value,
  label,
}))

/** Whether this caller may start a document at all. */
const canCreate = computed(() => auth.can('document:create'))

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

  void router.push({ name: 'documents', query: next })
}

function setFilter(key: string, value: string): void {
  navigate({ [key]: value })
}

function goToPage(next: number): void {
  navigate({ page: String(Math.min(Math.max(1, next), list.totalPages.value)) }, false)
}

function openDocument(id: string): void {
  void router.push({ name: 'document', params: { id } })
}

/**
 * What a status looks like when a person glances at a table.
 *
 * Three groups rather than ten colours: the thing still in play, the thing that
 * ended well, and the thing that ended badly. Ten distinguishable colours in one
 * column is a legend nobody reads.
 */
function statusVariant(status: DocumentStatus): 'default' | 'secondary' | 'destructive' {
  if (status === 'REJECTED' || status === 'CANCELLED') {
    return 'destructive'
  }

  if (status === 'COMPLETED' || status === 'APPROVED') {
    return 'default'
  }

  return 'secondary'
}

// One load path: the URL changed, so re-apply it. Mounting counts, which is
// what `immediate` is for — a deep-linked filtered page must load its filters
// rather than the whole population and then narrow.
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
        <h2 class="text-xl font-semibold tracking-tight">Documents</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          Everything raised in this tenant, newest first.
        </p>
      </div>

      <!-- Offered only to a caller who may use it. A button that always answers
           403 is worse than no button: it says the product is broken rather
           than that this person may not do it. -->
      <Button
        v-if="canCreate"
        data-testid="new-document"
        @click="router.push({ name: 'new-document' })"
      >
        New document
      </Button>
    </div>

    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <div class="space-y-2">
        <Label for="documents-search">Search</Label>
        <Input
          id="documents-search"
          data-testid="documents-search"
          :model-value="list.filters.value.search ?? ''"
          placeholder="Number, reference or title"
          @change="setFilter('search', ($event.target as HTMLInputElement).value)"
        />
      </div>

      <div class="space-y-2">
        <Label for="documents-status">Status</Label>
        <Select
          id="documents-status"
          data-testid="documents-status"
          :model-value="list.filters.value.status ?? ''"
          :options="statusOptions"
          placeholder="Any"
          @update:model-value="setFilter('status', $event)"
        />
      </div>

      <div class="space-y-2">
        <Label for="documents-priority">Priority</Label>
        <Select
          id="documents-priority"
          :model-value="list.filters.value.priority ?? ''"
          :options="priorityOptions"
          placeholder="Any"
          @update:model-value="setFilter('priority', $event)"
        />
      </div>
    </div>

    <Alert v-if="list.error.value" variant="destructive" data-testid="documents-error">
      <p>{{ list.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="list.apply(currentQuery)">
        Try again
      </Button>
    </Alert>

    <p v-if="list.isLoading.value" class="text-sm text-muted-foreground">Loading documents…</p>

    <template v-else-if="!list.error.value">
      <p
        v-if="list.isEmpty.value"
        class="text-sm text-muted-foreground"
        data-testid="documents-empty"
      >
        Nothing matches this view.
      </p>

      <Table v-else data-testid="documents-table">
        <TableHeader>
          <TableRow>
            <TableHead>Number</TableHead>
            <TableHead>Title</TableHead>
            <TableHead>Type</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Priority</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow
            v-for="row in list.items.value"
            :key="row.id"
            class="cursor-pointer"
            :data-testid="`document-row-${row.documentRef}`"
            @click="openDocument(row.id)"
          >
            <TableCell class="font-medium">
              <!-- A draft has no number and will not have one until it is
                   submitted. Showing its reference rather than an empty cell is
                   what lets somebody say which draft they mean. -->
              <span v-if="row.documentNumber">{{ row.documentNumber }}</span>
              <span v-else class="text-muted-foreground">{{ row.documentRef }}</span>
            </TableCell>
            <TableCell>{{ row.title }}</TableCell>
            <TableCell>{{ row.documentTypeCode }}</TableCell>
            <TableCell>
              <Badge :variant="statusVariant(row.status)">
                {{ DOCUMENT_STATUS_LABELS[row.status] }}
              </Badge>
            </TableCell>
            <TableCell>{{ DOCUMENT_PRIORITY_LABELS[row.priority] }}</TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="list.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ list.page.value }} of {{ list.totalPages.value }} ·
          {{ list.total.value }} documents
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
