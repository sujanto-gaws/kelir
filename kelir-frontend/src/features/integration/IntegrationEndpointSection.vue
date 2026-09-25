<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { toApiError } from '@/api/client'
import { listIntegrationEndpoints, updateIntegrationEndpoint } from '@/api/integration'
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
import { ENDPOINT_STATUS_LABELS, type IntegrationEndpoint } from '@/types/integration'

import IntegrationEndpointFormDialog from './IntegrationEndpointFormDialog.vue'

/**
 * A system's endpoints, on its detail page (FR-INT-001, #520).
 *
 * **Governed by the system's permissions**, as the product owner answered:
 * whoever may read the system reads its endpoints, and whoever may update it
 * adds, edits, retires and reinstates them. There is no delete — a retired
 * endpoint stays on the list, marked, because an integration log may name it.
 */
const props = defineProps<{ systemId: string; canUpdate: boolean }>()

const endpoints = usePaginatedList<IntegrationEndpoint>((query) =>
  listIntegrationEndpoints(props.systemId, query),
)

const isFormOpen = ref(false)
const editing = ref<IntegrationEndpoint | null>(null)

const retiring = ref<IntegrationEndpoint | null>(null)
const isRetireOpen = ref(false)
const isChanging = ref(false)
const retireError = ref('')
/** A reinstate that failed, shown above the table. */
const actionError = ref('')

function startAdding(): void {
  editing.value = null
  isFormOpen.value = true
}

function startEditing(endpoint: IntegrationEndpoint): void {
  editing.value = endpoint
  isFormOpen.value = true
}

function confirmRetire(endpoint: IntegrationEndpoint): void {
  retiring.value = endpoint
  retireError.value = ''
  isRetireOpen.value = true
}

async function retire(): Promise<void> {
  const target = retiring.value

  if (!target) {
    return
  }

  isChanging.value = true
  retireError.value = ''

  try {
    await updateIntegrationEndpoint(props.systemId, target.id, { status: 'INACTIVE' })
    isRetireOpen.value = false
    retiring.value = null
    await endpoints.load()
  } catch (failure) {
    retireError.value = toApiError(failure).message
  } finally {
    isChanging.value = false
  }
}

/** Bringing one back is not destructive, so it asks nothing first. */
async function reinstate(endpoint: IntegrationEndpoint): Promise<void> {
  actionError.value = ''

  try {
    await updateIntegrationEndpoint(props.systemId, endpoint.id, { status: 'ACTIVE' })
    await endpoints.load()
  } catch (failure) {
    actionError.value = toApiError(failure).message
  }
}

onMounted(() => {
  void endpoints.load()
})
</script>

<template>
  <section class="space-y-4" data-testid="endpoints-section" aria-labelledby="endpoints-heading">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h3 id="endpoints-heading" class="text-lg font-semibold">Endpoints</h3>
        <p class="text-sm text-muted-foreground">The operations Kelir may call on this system.</p>
      </div>
      <Button v-if="canUpdate" data-testid="add-endpoint" @click="startAdding">
        Add endpoint
      </Button>
    </div>

    <Alert v-if="endpoints.error.value" variant="destructive" data-testid="endpoints-error">
      {{ endpoints.error.value }}
    </Alert>

    <Alert v-if="actionError" variant="destructive" data-testid="endpoint-action-error">
      {{ actionError }}
    </Alert>

    <p v-if="endpoints.isLoading.value" class="text-sm text-muted-foreground">Loading endpoints…</p>

    <template v-else-if="!endpoints.error.value">
      <p
        v-if="endpoints.items.value.length === 0"
        class="text-sm text-muted-foreground"
        data-testid="endpoints-empty"
      >
        No endpoints yet.
      </p>

      <Table v-else data-testid="endpoints-table">
        <TableHeader>
          <TableRow>
            <TableHead>Code</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Method</TableHead>
            <TableHead>Path</TableHead>
            <TableHead>Status</TableHead>
            <TableHead v-if="canUpdate" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="endpoint in endpoints.items.value"
            :key="endpoint.id"
            :data-testid="`endpoint-row-${endpoint.endpointCode}`"
          >
            <TableCell class="font-medium">{{ endpoint.endpointCode }}</TableCell>
            <TableCell>{{ endpoint.name }}</TableCell>
            <TableCell
              ><code class="text-xs">{{ endpoint.method }}</code></TableCell
            >
            <TableCell
              ><code class="text-xs">{{ endpoint.path }}</code></TableCell
            >
            <TableCell>
              <Badge :variant="endpoint.status === 'ACTIVE' ? 'default' : 'secondary'">
                {{ ENDPOINT_STATUS_LABELS[endpoint.status] }}
              </Badge>
            </TableCell>
            <TableCell v-if="canUpdate" class="space-x-2 text-right">
              <Button
                size="sm"
                variant="secondary"
                :data-testid="`edit-endpoint-${endpoint.endpointCode}`"
                @click="startEditing(endpoint)"
              >
                Edit
              </Button>
              <Button
                v-if="endpoint.status === 'ACTIVE'"
                size="sm"
                variant="destructive"
                :data-testid="`retire-endpoint-${endpoint.endpointCode}`"
                @click="confirmRetire(endpoint)"
              >
                Retire
              </Button>
              <Button
                v-else
                size="sm"
                variant="outline"
                :data-testid="`reinstate-endpoint-${endpoint.endpointCode}`"
                @click="reinstate(endpoint)"
              >
                Reinstate
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="endpoints.totalPages.value > 1" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ endpoints.page.value }} of {{ endpoints.totalPages.value }}
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!endpoints.hasPrevious.value"
            @click="endpoints.goToPage(endpoints.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!endpoints.hasNext.value"
            @click="endpoints.goToPage(endpoints.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <IntegrationEndpointFormDialog
      v-if="canUpdate"
      v-model:open="isFormOpen"
      :system-id="systemId"
      :editing="editing"
      @saved="endpoints.load()"
    />

    <ConfirmDialog
      v-model:open="isRetireOpen"
      title="Retire endpoint"
      :description="`${retiring?.endpointCode ?? 'This endpoint'} will no longer be called. It stays listed and can be reinstated.`"
      confirm-label="Retire"
      :error="retireError"
      :pending="isChanging"
      @confirm="retire()"
    />
  </section>
</template>
