<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { toApiError } from '@/api/client'
import {
  createWorkflowRevision,
  deprecateWorkflowDefinition,
  listWorkflowDefinitions,
} from '@/api/workflow'
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

import { useWorkflowDeprecation } from './useWorkflowDeprecation'
import WorkflowDeprecateDialog from './WorkflowDeprecateDialog.vue'

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
 * *Deprecate* joined the rows on 2026-10-10 (#713, plan 19 row 6b), with the
 * editor's warning naming the document types still bound to the revision.
 */
const auth = useAuthStore()
const router = useRouter()

const canCreate = computed(() => auth.can('workflow:definition:create'))
const canDeprecate = computed(() => auth.can('workflow:definition:deprecate'))

const workflows = usePaginatedList<WorkflowDefinitionSummary>(listWorkflowDefinitions)

const revisionError = ref('')
const revisingId = ref('')

const deprecation = useWorkflowDeprecation()
const deprecationError = ref('')
const deprecationNotice = ref('')
const deprecatingId = ref('')

/** Whether a row's actions wait: its new revision or its deprecation is in flight. */
function busy(row: WorkflowDefinitionSummary): boolean {
  return revisingId.value === row.id || deprecatingId.value === row.id
}

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

/**
 * Deprecates a row's revision once its warning is confirmed (#713, **D-101** B).
 * Offered on an `ACTIVE` row only, to a holder of `workflow:definition:deprecate`
 * (**D-108**).
 *
 * **The list is read again after it succeeds, and after a 409**, which the
 * route answers only for a revision that is no longer `ACTIVE`, so the row
 * shows what it now is. A 403 or a 404 reads nothing again.
 */
async function deprecate(): Promise<void> {
  const target = deprecation.confirm()

  if (!target) {
    return
  }

  deprecationError.value = ''
  deprecationNotice.value = ''
  deprecatingId.value = target.id

  try {
    await deprecateWorkflowDefinition(target.id)
    deprecationNotice.value = `Revision ${target.version} of ${target.workflowKey} is deprecated.`
    await workflows.load()
  } catch (failure) {
    const error = toApiError(failure)

    deprecationError.value = error.message

    if (error.status === 409) {
      await workflows.load()
    }
  } finally {
    deprecatingId.value = ''
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

    <Alert v-if="deprecationError" variant="destructive" data-testid="deprecation-error">
      {{ deprecationError }}
    </Alert>

    <Alert v-if="deprecationNotice" data-testid="deprecation-notice">
      {{ deprecationNotice }}
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
              :disabled="busy(row)"
              :data-testid="`revise-${row.workflowKey}-${row.version}`"
              @click="revise(row)"
            >
              New revision
            </Button>
            <!-- Deprecate (#713, D-101 B, D-108): an ACTIVE row only. -->
            <Button
              v-if="canDeprecate && row.status === 'ACTIVE'"
              size="sm"
              variant="destructive"
              :disabled="busy(row)"
              :data-testid="`deprecate-${row.workflowKey}-${row.version}`"
              @click="deprecation.ask(row)"
            >
              Deprecate
            </Button>
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

    <WorkflowDeprecateDialog :deprecation="deprecation" @confirm="deprecate" />
  </section>
</template>
