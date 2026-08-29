<script setup lang="ts">
import { ref, watch } from 'vue'

import { getDocumentWorkflow, listWorkflowHistory } from '@/api/tasks'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  INSTANCE_STATUS_LABELS,
  TASK_STATUS_LABELS,
  type DocumentWorkflow,
  type WorkflowHistoryEntry,
} from '@/types/workflow'

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
 * question anybody opens it with.
 *
 * # And the history, which is where a decision's reason becomes visible
 *
 * FR-WF-012's record — one entry per transition, oldest first (#181) — is read
 * here rather than left as an endpoint nothing calls. It carries the comment the
 * approver gave (FR-TASK-006, #182), and that is why it is on this screen at
 * all: **a reason recorded where the decision is not visible would not be
 * read**, which is #182 AC2 in one sentence.
 *
 * It is a **second request** rather than a field on the first, because the two
 * answer different questions and page differently — the instance and its tasks
 * are bounded by the process, and the history grows with every move a
 * long-running approval makes.
 *
 * [#187]: https://github.com/sujanto-gaws/kelir/issues/187
 */
const workflow = ref<DocumentWorkflow | null>(null)
const loading = ref(true)
/** True when there is simply no process, which is not an error. */
const none = ref(false)
const problem = ref('')

const history = ref<WorkflowHistoryEntry[]>([])
const historyTotal = ref(0)
const historyPage = ref(1)
const historyProblem = ref('')

/** One page at a time, because the endpoint pages and a long process fills it. */
const HISTORY_PAGE_SIZE = 20

/**
 * One page of the history, appended rather than replaced.
 *
 * Appended because the list is chronological and oldest first: a reader asking
 * "how did this get here" is following a sequence, and replacing the page would
 * take the beginning of the story away to show them the middle.
 *
 * **A failure here does not fail the panel.** The instance and its tasks have
 * already loaded and answer most of what somebody opened this for; a history
 * that could not be read is reported in its own place rather than replacing
 * everything above it with an error.
 */
async function loadHistory(id: string, page: number): Promise<void> {
  historyProblem.value = ''

  try {
    const result = await listWorkflowHistory(id, page, HISTORY_PAGE_SIZE)

    history.value = page === 1 ? result.items : [...history.value, ...result.items]
    historyTotal.value = result.meta.total
    historyPage.value = page
  } catch (error) {
    historyProblem.value =
      error instanceof ApiError ? error.message : 'The approval history could not be loaded.'
  }
}

function loadMoreHistory(): void {
  void loadHistory(props.documentId, historyPage.value + 1)
}

async function load(id: string): Promise<void> {
  loading.value = true
  none.value = false
  problem.value = ''
  workflow.value = null
  history.value = []
  historyTotal.value = 0
  historyPage.value = 1
  historyProblem.value = ''

  try {
    workflow.value = await getDocumentWorkflow(id)
    // Only when there *is* a process. A document nothing is deciding has no
    // history to read, and asking for one would answer 404 and print a failure
    // beside a panel that has just correctly said "no approval".
    await loadHistory(id, 1)
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
          These are the steps this approval has generated. How the document moved between them is
          below.
        </p>
      </div>

      <div class="space-y-2" data-testid="workflow-history">
        <h3 class="font-medium">How it got here</h3>

        <Alert v-if="historyProblem" variant="destructive" data-testid="workflow-history-problem">
          {{ historyProblem }}
        </Alert>

        <ol v-if="history.length" class="space-y-2">
          <li
            v-for="entry in history"
            :key="entry.id"
            class="rounded-md border border-border p-3 text-sm"
            :data-testid="`history-${entry.id}`"
          >
            <p class="font-medium">
              <template v-if="entry.fromState">{{ entry.fromState }} → </template>
              {{ entry.toState }}
              <span v-if="entry.action" class="text-muted-foreground"> · {{ entry.action }}</span>
            </p>
            <p class="text-muted-foreground">
              <!-- "The system" rather than a blank: an entry with no actor is
                   the engine having moved the process, and an empty space beside
                   a timestamp reads as missing data. -->
              {{ entry.actorUsername ?? 'The system' }}
              <!-- Both parties, where a delegation put the task in somebody
                   else's hands (#184 AC4). An account showing only the delegate
                   loses the accountability delegation exists to preserve: the
                   approval was the delegator's to give. -->
              <span v-if="entry.onBehalfOfUsername" data-testid="history-on-behalf-of">
                on {{ entry.onBehalfOfUsername }}’s behalf
              </span>
              · {{ entry.occurredAt }}
            </p>
            <!-- Why this branch (#186 AC5). "Why did this go to her and not
                 to him" is answered by which edges were considered and what
                 each one said — not by the expression, which is in the
                 definition and is noise on an approval screen. -->
            <ul
              v-if="entry.routing?.length"
              class="mt-2 space-y-1 text-xs text-muted-foreground"
              data-testid="history-routing"
            >
              <li v-for="step in entry.routing" :key="step.to">
                → {{ step.to }} ·
                <span :class="step.outcome ? 'font-medium text-foreground' : ''">
                  {{ step.outcome ? 'condition met' : 'condition not met' }}
                </span>
              </li>
            </ul>
            <!-- The reason, where the decision is. #182 AC2: a comment shown
                 anywhere else would be a comment nobody reads. -->
            <p v-if="entry.comment" class="mt-2 whitespace-pre-line" data-testid="history-comment">
              “{{ entry.comment }}”
            </p>
          </li>
        </ol>

        <p v-else-if="!historyProblem" class="text-sm text-muted-foreground">
          Nothing has been recorded for this approval yet.
        </p>

        <Button
          v-if="history.length < historyTotal"
          variant="outline"
          size="sm"
          data-testid="history-more"
          @click="loadMoreHistory"
        >
          Show later steps
        </Button>
      </div>
    </template>
  </div>
</template>
