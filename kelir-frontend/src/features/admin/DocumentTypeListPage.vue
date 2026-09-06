<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { deleteDocumentType, getDocumentType, listDocumentTypes } from '@/api/document-types'
import { toApiError } from '@/api/client'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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
import type { DocumentType, DocumentTypeSummary } from '@/types/document-type'

import ConfirmDialog from './ConfirmDialog.vue'
import DocumentTypeFormDialog from './DocumentTypeFormDialog.vue'
import NumberingRuleDialog from './NumberingRuleDialog.vue'

/**
 * Document type administration (FR-RAD-008, FR-DTYPE-001..004; #341).
 *
 * **This screen is the discharge of a debt.** **D-64** accepted SRS §9
 * criterion 4 — *administrators can configure document types* — on the stated
 * basis that this builder was scheduled, and `v0.6.0`'s release notes say so in
 * text that has shipped. Until it existed, `api/document-types.ts` exported one
 * function: a read that filled a picker.
 *
 * It sits beside users, roles, delegations and tenants and is gated the way
 * they are: the route needs `document-type:read`, and each action is offered
 * only to a caller who holds its own permission — a button that always answers
 * 403 says the product is broken rather than that this person may not do it.
 */
const auth = useAuthStore()

const canCreate = computed(() => auth.can('document-type:create'))
const canUpdate = computed(() => auth.can('document-type:update'))
const canDelete = computed(() => auth.can('document-type:delete'))

const types = usePaginatedList<DocumentTypeSummary>(listDocumentTypes)

const isFormOpen = ref(false)
/**
 * The type being edited, loaded in full.
 *
 * **The list returns a summary and the dialog needs the whole type** — the
 * bindings and the workflows are not on the row, which is why the list endpoint
 * does not carry them. Opening the dialog therefore fetches; a dialog populated
 * from the summary would silently drop every binding it could not see.
 */
const editing = ref<DocumentType | null>(null)
const editingError = ref('')

const numbering = ref<DocumentTypeSummary | null>(null)
const isNumberingOpen = ref(false)

const confirming = ref<DocumentTypeSummary | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

function startCreating(): void {
  editing.value = null
  editingError.value = ''
  isFormOpen.value = true
}

async function startEditing(row: DocumentTypeSummary): Promise<void> {
  editingError.value = ''

  try {
    editing.value = await getDocumentType(row.id)
    isFormOpen.value = true
  } catch (failure) {
    editingError.value = toApiError(failure).message
  }
}

function startNumbering(row: DocumentTypeSummary): void {
  numbering.value = row
  isNumberingOpen.value = true
}

function closeForm(): void {
  isFormOpen.value = false
  editing.value = null
}

async function afterSave(): Promise<void> {
  closeForm()
  isNumberingOpen.value = false
  await types.refresh()
}

function confirmDelete(row: DocumentTypeSummary): void {
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
    await deleteDocumentType(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await types.refresh()
  } catch (failure) {
    // **Shown rather than swallowed**, and the backend's own words: a type with
    // documents refuses deletion, and the reason is the thing the person
    // needs — not "could not delete".
    deleteError.value = toApiError(failure).message
  } finally {
    isDeleting.value = false
  }
}

function statusVariant(status: DocumentTypeSummary['status']): 'default' | 'secondary' {
  return status === 'ACTIVE' ? 'default' : 'secondary'
}

onMounted(() => {
  void types.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Document types</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          What a document is, the form it renders and the workflow it routes through.
        </p>
      </div>

      <Button v-if="canCreate" data-testid="new-document-type" @click="startCreating">
        New document type
      </Button>
    </div>

    <Alert v-if="types.error.value" variant="destructive" data-testid="types-error">
      {{ types.error.value }}
    </Alert>

    <Alert v-if="editingError" variant="destructive" data-testid="editing-error">
      {{ editingError }}
    </Alert>

    <Table data-testid="document-types">
      <TableHeader>
        <TableRow>
          <TableHead>Code</TableHead>
          <TableHead>Name</TableHead>
          <TableHead>Category</TableHead>
          <TableHead>Form</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>

      <TableBody>
        <TableRow
          v-for="row in types.items.value"
          :key="row.id"
          :data-testid="`type-${row.typeCode}`"
        >
          <TableCell class="font-medium">{{ row.typeCode }}</TableCell>
          <TableCell>{{ row.name }}</TableCell>
          <TableCell>{{ row.category ?? '—' }}</TableCell>
          <TableCell>
            <!-- A type with no form is legal — a type is configured before its
                 form exists as often as after — and saying so is more useful
                 than a blank cell. -->
            <span v-if="row.formId" class="text-sm">Bound</span>
            <span v-else class="text-sm text-muted-foreground">No form</span>
          </TableCell>
          <TableCell>
            <Badge :variant="statusVariant(row.status)">{{ row.status }}</Badge>
          </TableCell>
          <TableCell class="space-x-2 text-right">
            <Button
              v-if="canUpdate"
              size="sm"
              variant="secondary"
              :data-testid="`edit-${row.typeCode}`"
              @click="startEditing(row)"
            >
              Edit
            </Button>
            <Button
              v-if="canUpdate"
              size="sm"
              variant="secondary"
              :data-testid="`numbering-${row.typeCode}`"
              @click="startNumbering(row)"
            >
              Numbering
            </Button>
            <Button
              v-if="canDelete"
              size="sm"
              variant="destructive"
              :data-testid="`delete-${row.typeCode}`"
              @click="confirmDelete(row)"
            >
              Delete
            </Button>
          </TableCell>
        </TableRow>

        <TableRow v-if="!types.isLoading.value && types.items.value.length === 0">
          <TableCell colspan="6" class="text-center text-sm text-muted-foreground">
            No document types yet.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <div v-if="types.totalPages.value > 1" class="flex items-center justify-between gap-3">
      <p class="text-sm text-muted-foreground">
        Page {{ types.page.value }} of {{ types.totalPages.value }}
      </p>
      <div class="space-x-2">
        <Button
          variant="secondary"
          :disabled="!types.hasPrevious.value"
          data-testid="previous-page"
          @click="types.goToPage(types.page.value - 1)"
        >
          Previous
        </Button>
        <Button
          variant="secondary"
          :disabled="!types.hasNext.value"
          data-testid="next-page"
          @click="types.goToPage(types.page.value + 1)"
        >
          Next
        </Button>
      </div>
    </div>

    <DocumentTypeFormDialog
      :open="isFormOpen"
      :editing="editing"
      @close="closeForm"
      @saved="afterSave"
    />

    <NumberingRuleDialog
      :open="isNumberingOpen"
      :document-type="numbering"
      @close="isNumberingOpen = false"
      @saved="afterSave"
    />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Delete document type"
      :description="`${confirming?.typeCode ?? 'This type'} will be removed. Documents already raised from it are kept.`"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="remove()"
    />
  </section>
</template>
