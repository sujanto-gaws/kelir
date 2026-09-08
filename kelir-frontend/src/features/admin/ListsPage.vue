<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { toApiError } from '@/api/client'
import { createList, deleteList, listLists } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { usePaginatedList } from '@/composables/usePaginatedList'
import { useAuthStore } from '@/stores/auth'
import type { ValidationDetail } from '@/types/api'
import type { ListSummary } from '@/types/rad'

import ConfirmDialog from './ConfirmDialog.vue'

/**
 * List definition administration (FR-RAD-003, FR-RAD-004; #374).
 *
 * **The same gap as the form builder, one table over.** `rad_lists` got a
 * renderer in Sprint 14 (**D-68**) after sitting unread since Sprint 7, and
 * nothing in the product writes one: `rad.ts` exported `listLists` and
 * `getRenderableList`, both reads.
 *
 * The create dialog is inline rather than its own file — a list is created from
 * a key and a title and then edited, so the dialog has two fields and no
 * behaviour of its own. `FormCreateDialog` is separate because it seeds a JFSS
 * document, which is a decision worth its own place.
 */
const auth = useAuthStore()
const router = useRouter()

const canCreate = computed(() => auth.can('rad:list:create'))
const canDelete = computed(() => auth.can('rad:list:delete'))

const lists = usePaginatedList<ListSummary>(listLists)

const isCreateOpen = ref(false)
const listKey = ref('')
const title = ref('')
const isSaving = ref(false)
const createError = ref('')
const createDetails = ref<ValidationDetail[]>([])

const confirming = ref<ListSummary | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

function startCreating(): void {
  listKey.value = ''
  title.value = ''
  createError.value = ''
  createDetails.value = []
  isCreateOpen.value = true
}

function messageFor(path: string): string | undefined {
  return createDetails.value.find((detail) => detail.path === path)?.message
}

async function create(): Promise<void> {
  isSaving.value = true
  createError.value = ''
  createDetails.value = []

  try {
    const created = await createList({
      listKey: listKey.value.trim(),
      title: title.value.trim(),
      // **One column, because a list with none is refused at render.**
      // `render::plan` calls that `LIST_HAS_NO_COLUMNS` and says why: an empty
      // table reads as *no documents* to everybody who did not write the
      // definition. Creating a list that cannot be drawn would put an author
      // in that state before they had typed anything.
      columns: [{ columnKey: 'title', label: 'Title', isSortable: true }],
      filters: [],
    })

    isCreateOpen.value = false
    await router.push({ name: 'admin-list-builder', params: { id: created.id } })
  } catch (failure) {
    const failed = toApiError(failure)

    createError.value = failed.message
    createDetails.value = failed.details
  } finally {
    isSaving.value = false
  }
}

function open(row: ListSummary): void {
  void router.push({ name: 'admin-list-builder', params: { id: row.id } })
}

function confirmDelete(row: ListSummary): void {
  confirming.value = row
  deleteError.value = ''
  isConfirmOpen.value = true
}

async function remove(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isDeleting.value = true
  deleteError.value = ''

  try {
    await deleteList(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await lists.refresh()
  } catch (failure) {
    deleteError.value = toApiError(failure).message
  } finally {
    isDeleting.value = false
  }
}

onMounted(() => {
  void lists.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Lists</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          What a screen of documents shows: its columns, its filters and the order it opens in.
        </p>
      </div>

      <Button v-if="canCreate" data-testid="new-list" @click="startCreating">New list</Button>
    </div>

    <Alert v-if="lists.error.value" variant="destructive" data-testid="lists-error">
      {{ lists.error.value }}
    </Alert>

    <Table data-testid="lists">
      <TableHeader>
        <TableRow>
          <TableHead>Key</TableHead>
          <TableHead>Title</TableHead>
          <TableHead>Page size</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>

      <TableBody>
        <TableRow
          v-for="row in lists.items.value"
          :key="row.id"
          :data-testid="`list-${row.listKey}`"
        >
          <TableCell class="font-medium">{{ row.listKey }}</TableCell>
          <TableCell>{{ row.title }}</TableCell>
          <TableCell>{{ row.pageSize }}</TableCell>
          <TableCell>
            <Badge :variant="row.status === 'ACTIVE' ? 'default' : 'secondary'">
              {{ row.status }}
            </Badge>
          </TableCell>
          <TableCell class="space-x-2 text-right">
            <Button
              size="sm"
              variant="secondary"
              :data-testid="`open-${row.listKey}`"
              @click="open(row)"
            >
              Open
            </Button>
            <Button
              v-if="canDelete"
              size="sm"
              variant="destructive"
              :data-testid="`delete-${row.listKey}`"
              @click="confirmDelete(row)"
            >
              Delete
            </Button>
          </TableCell>
        </TableRow>

        <TableRow v-if="!lists.isLoading.value && lists.items.value.length === 0">
          <TableCell colspan="5" class="text-center text-sm text-muted-foreground">
            No lists yet.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <div
      v-if="isCreateOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      role="dialog"
      aria-modal="true"
      aria-label="New list"
      data-testid="list-create-dialog"
    >
      <div class="w-full max-w-lg rounded-lg bg-background p-6 shadow-lg">
        <h3 class="text-lg font-semibold">New list</h3>

        <Alert
          v-if="createError"
          variant="destructive"
          class="mt-4"
          data-testid="list-create-error"
        >
          {{ createError }}
        </Alert>

        <form class="mt-4 space-y-4" @submit.prevent="create">
          <div class="space-y-2">
            <Label for="list-key">Key</Label>
            <Input
              id="list-key"
              v-model="listKey"
              data-testid="list-key"
              placeholder="purchase_requisitions"
            />
            <p class="text-xs text-muted-foreground">
              What a menu route and a document type name this list by. It cannot change.
            </p>
            <p v-if="messageFor('listKey')" class="text-xs text-destructive">
              {{ messageFor('listKey') }}
            </p>
          </div>

          <div class="space-y-2">
            <Label for="list-title">Title</Label>
            <Input id="list-title" v-model="title" data-testid="list-title" />
            <p v-if="messageFor('title')" class="text-xs text-destructive">
              {{ messageFor('title') }}
            </p>
          </div>

          <div class="flex justify-end gap-2 pt-2">
            <Button type="button" variant="secondary" @click="isCreateOpen = false">Cancel</Button>
            <Button type="submit" :disabled="isSaving" data-testid="save-list">
              {{ isSaving ? 'Creating…' : 'Create' }}
            </Button>
          </div>
        </form>
      </div>
    </div>

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Delete list"
      :description="`${confirming?.listKey ?? 'This list'} will be removed. Document types that name it keep the reference.`"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="remove()"
    />
  </section>
</template>
