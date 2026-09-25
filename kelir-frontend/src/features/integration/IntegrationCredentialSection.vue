<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { toApiError } from '@/api/client'
import { deleteIntegrationCredential, listIntegrationCredentials } from '@/api/integration'
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
import ConfirmDialog from '@/features/admin/ConfirmDialog.vue'
import { useAuthStore } from '@/stores/auth'
import { AUTH_TYPE_LABELS, type IntegrationCredential } from '@/types/integration'

import IntegrationCredentialFormDialog from './IntegrationCredentialFormDialog.vue'

/**
 * A system's credential references (FR-INT-001 AC-5, #520).
 *
 * **Under `integration:credential:*`, apart from the system's permissions**, as
 * the product owner answered: seeing a system does not mean seeing where its
 * secrets live. A caller without `:read` gets **no section at all** — not a
 * heading over an error — and is never sent to ask. A caller the client thought
 * could read but the server refused (a grant revoked since sign-in) gets the
 * same: the 403 means *not yours*, not *broken*.
 *
 * **The reference is rendered as it was stored.** No field carries a resolved
 * secret, and this table shows `secretReference` as it was entered. The API
 * checks only its shape, so a secret typed as a path segment would be shown
 * here too (#552); the text above tells the user what to enter instead.
 */
const props = defineProps<{ systemId: string }>()

const auth = useAuthStore()

const canRead = computed(() => auth.can('integration:credential:read'))
const canCreate = computed(() => auth.can('integration:credential:create'))
const canUpdate = computed(() => auth.can('integration:credential:update'))
const canDelete = computed(() => auth.can('integration:credential:delete'))

/** The server said no; the section withdraws rather than reporting a failure. */
const isRefused = ref(false)

const credentials = usePaginatedList<IntegrationCredential>(async (query) => {
  try {
    return await listIntegrationCredentials(props.systemId, query)
  } catch (failure) {
    if (toApiError(failure).isForbidden) {
      isRefused.value = true
    }

    throw failure
  }
})

const isVisible = computed(() => canRead.value && !isRefused.value)

const isFormOpen = ref(false)
const editing = ref<IntegrationCredential | null>(null)

const deleting = ref<IntegrationCredential | null>(null)
const isDeleteOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

function startAdding(): void {
  editing.value = null
  isFormOpen.value = true
}

function startEditing(credential: IntegrationCredential): void {
  editing.value = credential
  isFormOpen.value = true
}

function confirmDelete(credential: IntegrationCredential): void {
  deleting.value = credential
  deleteError.value = ''
  isDeleteOpen.value = true
}

async function remove(): Promise<void> {
  const target = deleting.value

  if (!target) {
    return
  }

  isDeleting.value = true
  deleteError.value = ''

  try {
    await deleteIntegrationCredential(props.systemId, target.id)
    isDeleteOpen.value = false
    deleting.value = null
    await credentials.refresh()
  } catch (failure) {
    deleteError.value = toApiError(failure).message
  } finally {
    isDeleting.value = false
  }
}

function validity(credential: IntegrationCredential): string {
  if (!credential.validFrom && !credential.validTo) {
    return 'Open-ended'
  }

  return `${credential.validFrom ?? '…'} to ${credential.validTo ?? '…'}`
}

onMounted(() => {
  // Not even asked for without the permission: the answer is known.
  if (canRead.value) {
    void credentials.load()
  }
})
</script>

<template>
  <section
    v-if="isVisible"
    class="space-y-4"
    data-testid="credentials-section"
    aria-labelledby="credentials-heading"
  >
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h3 id="credentials-heading" class="text-lg font-semibold">Credential references</h3>
        <p class="text-sm text-muted-foreground">
          Where this system's secrets are kept. Put each secret in the vault or the environment
          first, then enter where it is, such as <code>vault://…</code> or <code>env://NAME</code>,
          not the secret itself.
        </p>
      </div>
      <Button v-if="canCreate" data-testid="add-credential" @click="startAdding">
        Add credential reference
      </Button>
    </div>

    <Alert v-if="credentials.error.value" variant="destructive" data-testid="credentials-error">
      {{ credentials.error.value }}
    </Alert>

    <p v-if="credentials.isLoading.value" class="text-sm text-muted-foreground">
      Loading credential references…
    </p>

    <template v-else-if="!credentials.error.value">
      <p
        v-if="credentials.items.value.length === 0"
        class="text-sm text-muted-foreground"
        data-testid="credentials-empty"
      >
        No credential references yet.
      </p>

      <Table v-else data-testid="credentials-table">
        <TableHeader>
          <TableRow>
            <TableHead>Type</TableHead>
            <TableHead>Secret reference</TableHead>
            <TableHead>Validity</TableHead>
            <TableHead>Status</TableHead>
            <TableHead v-if="canUpdate || canDelete" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="credential in credentials.items.value"
            :key="credential.id"
            :data-testid="`credential-row-${credential.id}`"
          >
            <TableCell>{{ AUTH_TYPE_LABELS[credential.credentialType] }}</TableCell>
            <TableCell>
              <code class="text-xs" data-testid="credential-secret-reference-value">{{
                credential.secretReference
              }}</code>
            </TableCell>
            <TableCell class="text-sm">{{ validity(credential) }}</TableCell>
            <TableCell>
              <Badge :variant="credential.isActive ? 'default' : 'secondary'">
                {{ credential.isActive ? 'Active' : 'Inactive' }}
              </Badge>
            </TableCell>
            <TableCell v-if="canUpdate || canDelete" class="space-x-2 text-right">
              <Button
                v-if="canUpdate"
                size="sm"
                variant="secondary"
                :data-testid="`edit-credential-${credential.id}`"
                @click="startEditing(credential)"
              >
                Edit
              </Button>
              <Button
                v-if="canDelete"
                size="sm"
                variant="destructive"
                :data-testid="`delete-credential-${credential.id}`"
                @click="confirmDelete(credential)"
              >
                Delete
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="credentials.totalPages.value > 1" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ credentials.page.value }} of {{ credentials.totalPages.value }}
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!credentials.hasPrevious.value"
            @click="credentials.goToPage(credentials.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!credentials.hasNext.value"
            @click="credentials.goToPage(credentials.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <IntegrationCredentialFormDialog
      v-if="canCreate || canUpdate"
      v-model:open="isFormOpen"
      :system-id="systemId"
      :editing="editing"
      @saved="credentials.load()"
    />

    <ConfirmDialog
      v-model:open="isDeleteOpen"
      title="Delete credential reference"
      :description="`The reference ${deleting?.secretReference ?? ''} will be removed from this system. The secret it points at is not touched.`"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="remove()"
    />
  </section>
</template>
