<script setup lang="ts">
import { ref, watch } from 'vue'

import { getDocumentWorkflow } from '@/api/tasks'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { INSTANCE_STATUS_LABELS, TASK_STATUS_LABELS, type DocumentWorkflow } from '@/types/workflow'

const props = defineProps<{ documentId: string }>()

/**
 * The Workflow tab of the document workspace (#178, and the tab #172 left
 * saying "Phase 5 fills this").
 *
 * **This is where the seam becomes visible to a person.** The document's own
 * status is a projection of the process's state, and this panel is where both
 * are shown side by side — the workflow's name for the step, and the tasks the
 * process has generated, decided and outstanding.
 *
 * **"No approval" is a state, not a failure.** A document of a type that binds
 * no workflow has no process, which is a valid configuration ([#187] AC4): the
 * API answers 404 and this panel says so in words rather than showing an error.
 * A failure that is not that — a 403 on `workflow:instance:read` — is shown as
 * one, because a caller who cannot read the process should be told, not left
 * looking at a screen that says nothing is happening.
 *
 * **The decided tasks are the point of the list.** A panel showing only what is
 * outstanding cannot answer "who approved this and when", which is the first
 * question anybody opens it with. The document's fuller history — FR-WF-012 — is
 * a later release's, and this says as much rather than implying it is complete.
 *
 * [#187]: https://github.com/sujanto-gaws/kelir/issues/187
 */
const workflow = ref<DocumentWorkflow | null>(null)
const loading = ref(true)
/** True when there is simply no process, which is not an error. */
const none = ref(false)
const problem = ref('')

async function load(id: string): Promise<void> {
  loading.value = true
  none.value = false
  problem.value = ''
  workflow.value = null

  try {
    workflow.value = await getDocumentWorkflow(id)
  } catch (error) {
    // A 404 here means nothing is deciding this document. Anything else is a
    // real refusal and is shown as one, in the backend's own words — repeating
    // it in ours risks contradicting it (coding standard §3.3).
    if (error instanceof ApiError && error.status === 404) {
      none.value = true
    } else if (error instanceof ApiError) {
      problem.value = error.message
    } else {
      problem.value = 'The approval could not be loaded.'
    }
  } finally {
    loading.value = false
  }
}

watch(() => props.documentId, load, { immediate: true })
</script>

<template>
  <div class="space-y-4" data-testid="workflow-tab">
    <p v-if="loading" class="text-sm text-muted-foreground">Loading the approval…</p>

    <p v-else-if="none" class="text-sm text-muted-foreground" data-testid="workflow-none">
      No approval is running for this document. Its type starts one when it is configured to.
    </p>

    <Alert v-else-if="problem" variant="destructive" data-testid="workflow-problem">
      {{ problem }}
    </Alert>

    <template v-else-if="workflow">
      <div class="rounded-md border border-border p-4" data-testid="workflow-instance">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <p class="font-medium">{{ workflow.instance.workflowName }}</p>
            <p class="text-sm text-muted-foreground">
              {{ workflow.instance.instanceRef }} · revision
              {{ workflow.instance.definitionVersion }}
            </p>
          </div>
          <div class="flex items-center gap-2">
            <Badge data-testid="workflow-state">{{ workflow.instance.currentStateName }}</Badge>
            <Badge variant="outline">
              {{ INSTANCE_STATUS_LABELS[workflow.instance.status] }}
            </Badge>
          </div>
        </div>

        <dl
          v-if="workflow.instance.variables.length"
          class="mt-4 grid gap-2 text-sm sm:grid-cols-2"
          data-testid="workflow-variables"
        >
          <div v-for="variable in workflow.instance.variables" :key="variable.key">
            <dt class="text-muted-foreground">{{ variable.key }}</dt>
            <dd class="font-medium">{{ variable.value }}</dd>
          </div>
        </dl>
      </div>

      <div class="space-y-2" data-testid="workflow-tasks">
        <h3 class="font-medium">Steps</h3>

        <ol class="space-y-2">
          <li
            v-for="task in workflow.tasks"
            :key="task.id"
            class="rounded-md border border-border p-3 text-sm"
            :data-testid="`workflow-task-${task.taskRef}`"
          >
            <p class="font-medium">{{ task.taskName }}</p>
            <p class="text-muted-foreground">
              {{ TASK_STATUS_LABELS[task.status] }}
              <template v-if="task.action"> · {{ task.action }}</template>
              <template v-if="task.completedAt"> · {{ task.completedAt }}</template>
            </p>
          </li>
        </ol>

        <p class="text-sm text-muted-foreground" data-testid="workflow-history-note">
          These are the steps this approval has generated. A fuller account of how the document
          reached its current state arrives in a later release.
        </p>
      </div>
    </template>
  </div>
</template>
