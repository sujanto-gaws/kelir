<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { toApiError } from '@/api/client'
import { createFormRevision, deleteForm, listForms } from '@/api/rad'
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
import type { FormSummary } from '@/types/rad'

import ConfirmDialog from './ConfirmDialog.vue'
import FormCreateDialog from './FormCreateDialog.vue'

/**
 * Form definition administration (FR-RAD-004; #373).
 *
 * **`rad_forms` has had a full write API since Sprint 7 and no screen ever
 * called it.** Sprint 14 built the document type and menu builders (**D-70**)
 * and that decision's own record names what it left out — *a form builder,
 * FR-RAD-004's other half, and the reason the e2e flow still seeds its form
 * over the API*. This screen and {@link FormBuilderPage} are that half.
 *
 * Gated the way the other six admin screens are: `rad:form:read` opens it, and
 * each action is offered only to a caller holding its own permission.
 */
const auth = useAuthStore()
const router = useRouter()

const canCreate = computed(() => auth.can('rad:form:create'))
const canUpdate = computed(() => auth.can('rad:form:update'))
const canDelete = computed(() => auth.can('rad:form:delete'))

const forms = usePaginatedList<FormSummary>(listForms)

const isCreateOpen = ref(false)

const confirming = ref<FormSummary | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

const revisionError = ref('')
const revisingId = ref('')

function open(row: FormSummary): void {
  void router.push({ name: 'admin-form-builder', params: { id: row.id } })
}

async function afterCreate(form: { id: string }): Promise<void> {
  isCreateOpen.value = false
  // Straight into the editor: a form created from a dialog has one field and a
  // title, and the next thing anybody wants is the thing they came to build.
  await router.push({ name: 'admin-form-builder', params: { id: form.id } })
}

/**
 * Opens the next revision of a published form.
 *
 * **This is the only way forward from `PUBLISHED`** ([ADR-0027]): a document
 * pins the revision it was filled against, so editing a published one would
 * change what an already-submitted document claims to have been. The row offers
 * *New revision* rather than *Edit*, and the builder says why.
 */
async function revise(row: FormSummary): Promise<void> {
  revisionError.value = ''
  revisingId.value = row.id

  try {
    const next = await createFormRevision(row.id)

    await router.push({ name: 'admin-form-builder', params: { id: next.id } })
  } catch (failure) {
    revisionError.value = toApiError(failure).message
  } finally {
    revisingId.value = ''
  }
}

function confirmDelete(row: FormSummary): void {
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
    await deleteForm(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await forms.refresh()
  } catch (failure) {
    // The backend's own words. A form a document type binds refuses deletion,
    // and *which* type is the thing the person needs.
    deleteError.value = toApiError(failure).message
  } finally {
    isDeleting.value = false
  }
}

function statusVariant(status: FormSummary['status']): 'default' | 'secondary' {
  return status === 'PUBLISHED' ? 'default' : 'secondary'
}

onMounted(() => {
  void forms.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Forms</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          What a document asks for. A published revision is fixed for every document that pins it.
        </p>
      </div>

      <Button v-if="canCreate" data-testid="new-form" @click="isCreateOpen = true">New form</Button>
    </div>

    <Alert v-if="forms.error.value" variant="destructive" data-testid="forms-error">
      {{ forms.error.value }}
    </Alert>

    <Alert v-if="revisionError" variant="destructive" data-testid="revision-error">
      {{ revisionError }}
    </Alert>

    <Table data-testid="forms">
      <TableHeader>
        <TableRow>
          <TableHead>Key</TableHead>
          <TableHead>Title</TableHead>
          <TableHead>Revision</TableHead>
          <TableHead>JFSS</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>

      <TableBody>
        <TableRow
          v-for="row in forms.items.value"
          :key="row.id"
          :data-testid="`form-${row.formKey}`"
        >
          <TableCell class="font-medium">{{ row.formKey }}</TableCell>
          <TableCell>{{ row.title }}</TableCell>
          <TableCell>{{ row.revision }}</TableCell>
          <TableCell class="text-sm text-muted-foreground">{{ row.jfssVersion }}</TableCell>
          <TableCell>
            <Badge :variant="statusVariant(row.status)">{{ row.status }}</Badge>
          </TableCell>
          <TableCell class="space-x-2 text-right">
            <!-- Open, not Edit: a published revision opens read-only, and
                 calling the button Edit would promise something the screen
                 then refuses. -->
            <Button
              size="sm"
              variant="secondary"
              :data-testid="`open-${row.formKey}`"
              @click="open(row)"
            >
              Open
            </Button>
            <Button
              v-if="canUpdate && row.status === 'PUBLISHED'"
              size="sm"
              variant="secondary"
              :disabled="revisingId === row.id"
              :data-testid="`revise-${row.formKey}`"
              @click="revise(row)"
            >
              New revision
            </Button>
            <Button
              v-if="canDelete"
              size="sm"
              variant="destructive"
              :data-testid="`delete-${row.formKey}`"
              @click="confirmDelete(row)"
            >
              Delete
            </Button>
          </TableCell>
        </TableRow>

        <TableRow v-if="!forms.isLoading.value && forms.items.value.length === 0">
          <TableCell colspan="6" class="text-center text-sm text-muted-foreground">
            No forms yet.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <div v-if="forms.totalPages.value > 1" class="flex items-center justify-between gap-3">
      <p class="text-sm text-muted-foreground">
        Page {{ forms.page.value }} of {{ forms.totalPages.value }}
      </p>
      <div class="space-x-2">
        <Button
          variant="secondary"
          :disabled="!forms.hasPrevious.value"
          data-testid="previous-page"
          @click="forms.goToPage(forms.page.value - 1)"
        >
          Previous
        </Button>
        <Button
          variant="secondary"
          :disabled="!forms.hasNext.value"
          data-testid="next-page"
          @click="forms.goToPage(forms.page.value + 1)"
        >
          Next
        </Button>
      </div>
    </div>

    <FormCreateDialog :open="isCreateOpen" @close="isCreateOpen = false" @created="afterCreate" />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Delete form"
      :description="`${confirming?.formKey ?? 'This form'} will be retired. Documents already raised against it keep the revision they pinned.`"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="remove()"
    />
  </section>
</template>
