<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { toApiError } from '@/api/client'
import { createWorkflowRevision, listWorkflowDefinitions } from '@/api/workflow'
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
import type { WorkflowDefinitionSummary } from '@/types/workflow'

/**
 * Workflow definitions (FR-WF-018; #426 AC1, AC5).
 *
 * **`workflow_definitions` has had a full write API since Sprint 10, and this
 * is the first screen that calls it.** Every revision is a row: a workflow is
 * `(workflowKey, version)`, and a document type binds one revision, so the list
 * shows each rather than folding them under their key.
 *
 * Gated as the other admin screens are: `workflow:definition:read` opens it,
 * and each action is offered only to a caller holding its own permission.
 */
const auth = useAuthStore()
const router = useRouter()

const canCreate = computed(() => auth.can('workflow:definition:create'))

const workflows = usePaginatedList<WorkflowDefinitionSummary>(listWorkflowDefinitions)

const revisionError = ref('')
const revisingId = ref('')

function open(row: WorkflowDefinitionSummary): void {
  void router.push({ name: 'admin-workflow-editor', params: { id: row.id } })
}

/**
 * Opens the next revision of a published one: **the only way forward from
 * `ACTIVE`** (AC5). A running approval executes the revision it started
 * against, so the row offers *New revision* rather than *Edit*.
 */
async function revise(row: WorkflowDefinitionSummary): Promise<void> {
  revisionError.value = ''
  revisingId.value = row.id

  try {
    const next = await createWorkflowRevision(row.id)

    await router.push({ name: 'admin-workflow-editor', params: { id: next.id } })
  } catch (failure) {
    revisionError.value = toApiError(failure).message
  } finally {
    revisingId.value = ''
  }
}

function statusVariant(
  status: WorkflowDefinitionSummary['status'],
): 'default' | 'secondary' | 'outline' {
  if (status === 'ACTIVE') {
    return 'default'
  }

  return status === 'DRAFT' ? 'secondary' : 'outline'
}

onMounted(() => {
  void workflows.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Workflows</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          How a document is approved. A published revision is fixed for every approval that runs on
          it.
        </p>
      </div>

      <Button
        v-if="canCreate"
        data-testid="new-workflow"
        @click="router.push({ name: 'admin-workflow-new' })"
      >
        New workflow
      </Button>
    </div>

    <Alert v-if="workflows.error.value" variant="destructive" data-testid="workflows-error">
      {{ workflows.error.value }}
    </Alert>

    <Alert v-if="revisionError" variant="destructive" data-testid="revision-error">
      {{ revisionError }}
    </Alert>

    <p v-if="workflows.isLoading.value" class="text-sm text-muted-foreground">Loading…</p>

    <Table data-testid="workflows">
      <TableHeader>
        <TableRow>
          <TableHead>Key</TableHead>
          <TableHead>Name</TableHead>
          <TableHead>Revision</TableHead>
          <TableHead>JWSS</TableHead>
          <TableHead>Status</TableHead>
          <TableHead class="text-right">Actions</TableHead>
        </TableRow>
      </TableHeader>

      <TableBody>
        <TableRow
          v-for="row in workflows.items.value"
          :key="row.id"
          :data-testid="`workflow-${row.workflowKey}-${row.version}`"
        >
          <TableCell class="font-mono font-medium">{{ row.workflowKey }}</TableCell>
          <TableCell>{{ row.name }}</TableCell>
          <TableCell>{{ row.version }}</TableCell>
          <TableCell class="text-sm text-muted-foreground">{{ row.jwssVersion }}</TableCell>
          <TableCell>
            <Badge :variant="statusVariant(row.status)">{{ row.status }}</Badge>
          </TableCell>
          <TableCell class="space-x-2 text-right">
            <!-- Open, not Edit: a published revision opens read-only. -->
            <Button
              size="sm"
              variant="secondary"
              :data-testid="`open-${row.workflowKey}-${row.version}`"
              @click="open(row)"
            >
              Open
            </Button>
            <Button
              v-if="canCreate && row.status !== 'DRAFT'"
              size="sm"
              variant="secondary"
              :disabled="revisingId === row.id"
              :data-testid="`revise-${row.workflowKey}-${row.version}`"
              @click="revise(row)"
            >
              New revision
            </Button>
            <!-- Deprecate (#573, D-101 B) goes here: on an ACTIVE row, for a
                 holder of workflow:definition:deprecate (D-108), then a
                 refresh. Plan 19 merges it after #573's route, so until then
                 there is no button rather than one that cannot work. -->
          </TableCell>
        </TableRow>

        <TableRow v-if="!workflows.isLoading.value && workflows.items.value.length === 0">
          <TableCell colspan="6" class="text-center text-sm text-muted-foreground">
            No workflows yet. A document type with none bound is moved by hand.
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <div v-if="workflows.totalPages.value > 1" class="flex items-center justify-between gap-3">
      <p class="text-sm text-muted-foreground">
        Page {{ workflows.page.value }} of {{ workflows.totalPages.value }}
      </p>
      <div class="space-x-2">
        <Button
          variant="secondary"
          :disabled="!workflows.hasPrevious.value"
          data-testid="previous-page"
          @click="workflows.goToPage(workflows.page.value - 1)"
        >
          Previous
        </Button>
        <Button
          variant="secondary"
          :disabled="!workflows.hasNext.value"
          data-testid="next-page"
          @click="workflows.goToPage(workflows.page.value + 1)"
        >
          Next
        </Button>
      </div>
    </div>
  </section>
</template>
