<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { claimTask, decideTask, getTask } from '@/api/tasks'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DOCUMENT_STATUS_LABELS } from '@/types/document'
import { TASK_STATUS_LABELS, type DecisionAction, type TaskDetail } from '@/types/workflow'

/**
 * One task, and what it is asking (FR-TASK-003; #179 AC4).
 *
 * *"A task that says only 'approve?' is a task its holder cannot responsibly
 * action."* So this screen names the document, the process, the step, and every
 * transition the definition offers from where the process is — with the
 * definition's own words for each, because `MANAGER_APPROVAL` is a code and
 * "Manager approval" is what somebody was told to expect.
 *
 * **The decisions come from the backend, not from a list here.** A workflow's
 * transitions are its own, so a screen that enumerated them would be a screen
 * that only works for the workflows somebody thought of. It also carries
 * `supported`, which is how a transition this release cannot perform — `RETURN`,
 * FR-WF-008, Sprint 11's #183 — is **shown without being offered**: drawing a
 * button that produces a 422 would be the product refusing a control it drew.
 *
 * **There is no comment box.** FR-TASK-006 is Sprint 11's #182 and the API takes
 * no comment, so a rejection recorded here has no reason on it. That is a real
 * cost and it is said on the screen rather than left for somebody to discover
 * after refusing a colleague's requisition.
 */
const route = useRoute()
const router = useRouter()

const task = ref<TaskDetail | null>(null)
const loading = ref(true)
const failed = ref(false)
const busy = ref(false)
const problem = ref('')
const notice = ref('')

/** Only what this release can perform is offered as a button. */
const offered = computed(() =>
  (task.value?.decisions ?? []).filter((decision) => decision.supported),
)

/** Declared by the definition and not performable yet — shown, not offered. */
const deferred = computed(() =>
  (task.value?.decisions ?? []).filter((decision) => !decision.supported),
)

/** A task that has been decided is read, not acted on. */
const open = computed(
  () =>
    task.value !== null &&
    (task.value.status === 'CREATED' ||
      task.value.status === 'ASSIGNED' ||
      task.value.status === 'IN_PROGRESS'),
)

/** Claiming is offered only for work that is going spare. */
const claimable = computed(() => open.value && task.value?.assignment === 'ROLE')

async function load(id: string): Promise<void> {
  loading.value = true
  failed.value = false
  problem.value = ''
  notice.value = ''

  try {
    task.value = await getTask(id)
  } catch {
    // Which failure it was is deliberately not distinguished: a task the caller
    // may not see answers 404, and saying more would confirm it exists.
    failed.value = true
    task.value = null
  } finally {
    loading.value = false
  }
}

watch(
  () => route.params.id,
  (id) => load(String(id)),
  { immediate: true },
)

function report(error: unknown): void {
  problem.value = error instanceof ApiError ? error.message : 'Something went wrong. Try again.'
}

async function claim(): Promise<void> {
  if (!task.value || busy.value) {
    return
  }

  busy.value = true
  problem.value = ''
  notice.value = ''

  try {
    await claimTask(task.value.id)
    await load(task.value.id)
    notice.value = 'This task is yours.'
  } catch (error) {
    // A 409 here is somebody else getting there first, and the backend's own
    // words say which — taken, or already finished. Repeating it in ours would
    // risk contradicting it.
    report(error)
  } finally {
    busy.value = false
  }
}

async function decide(action: DecisionAction): Promise<void> {
  if (!task.value || busy.value) {
    return
  }

  busy.value = true
  problem.value = ''
  notice.value = ''

  try {
    const result = await decideTask(task.value.id, action)
    await load(task.value.id)
    notice.value = `Recorded. The document is now ${DOCUMENT_STATUS_LABELS[result.documentStatus]}.`
  } catch (error) {
    report(error)
  } finally {
    busy.value = false
  }
}

function openDocument(): void {
  if (task.value) {
    void router.push({ name: 'document', params: { id: task.value.documentId } })
  }
}
</script>

<template>
  <section class="space-y-6">
    <p v-if="loading" data-testid="task-loading" class="text-sm text-muted-foreground">Loading…</p>

    <Alert v-else-if="failed" variant="destructive" data-testid="task-error">
      This task could not be opened.
    </Alert>

    <template v-else-if="task">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 class="text-xl font-semibold tracking-tight" data-testid="task-name">
            {{ task.taskName }}
          </h2>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ task.taskRef }} · {{ task.workflowName }} · {{ task.currentStateName }}
          </p>
        </div>

        <div class="flex items-center gap-2">
          <Badge v-if="task.assignment === 'MINE'" data-testid="task-assignment">Mine</Badge>
          <Badge v-else variant="secondary" data-testid="task-assignment">
            Unclaimed{{ task.candidateRoleCode ? ` · ${task.candidateRoleCode}` : '' }}
          </Badge>
          <Badge variant="outline">{{ TASK_STATUS_LABELS[task.status] }}</Badge>
        </div>
      </div>

      <Alert v-if="problem" variant="destructive" data-testid="task-problem">{{ problem }}</Alert>
      <Alert v-if="notice" data-testid="task-notice">{{ notice }}</Alert>

      <div class="rounded-md border border-border p-4" data-testid="task-document">
        <h3 class="font-medium">What this is about</h3>
        <dl class="mt-3 grid gap-2 text-sm sm:grid-cols-2">
          <div>
            <dt class="text-muted-foreground">Document</dt>
            <dd class="font-medium">{{ task.documentNumber ?? task.documentRef }}</dd>
          </div>
          <div>
            <dt class="text-muted-foreground">Title</dt>
            <dd class="font-medium">{{ task.documentTitle }}</dd>
          </div>
        </dl>
        <Button
          variant="outline"
          size="sm"
          class="mt-3"
          data-testid="open-document"
          @click="openDocument"
        >
          Open the document
        </Button>
      </div>

      <div class="rounded-md border border-border p-4" data-testid="task-decisions">
        <h3 class="font-medium">What you are being asked</h3>

        <p v-if="!open" class="mt-2 text-sm text-muted-foreground" data-testid="task-decided">
          This task has been decided. It is here so you can see what happened.
        </p>

        <template v-else>
          <div class="mt-3 flex flex-wrap gap-2">
            <Button
              v-if="claimable"
              variant="outline"
              :disabled="busy"
              data-testid="claim-task"
              @click="claim"
            >
              Claim it
            </Button>

            <Button
              v-for="decision in offered"
              :key="decision.action"
              :variant="decision.action === 'APPROVE' ? 'default' : 'destructive'"
              :disabled="busy"
              :data-testid="`decide-${decision.action}`"
              @click="decide(decision.action as DecisionAction)"
            >
              {{ decision.action === 'APPROVE' ? 'Approve' : 'Reject' }} →
              {{ decision.toStateName }}
            </Button>
          </div>

          <!-- Shown and not offered. A definition may declare a transition this
               release cannot perform; drawing a button for it would produce a
               refusal from a control the product itself put there. -->
          <p
            v-if="deferred.length"
            class="mt-3 text-sm text-muted-foreground"
            data-testid="task-deferred"
          >
            This workflow also allows
            <template v-for="(decision, index) in deferred" :key="decision.action">
              <span class="font-medium">{{ decision.action }}</span>
              <span> → {{ decision.toStateName }}</span>
              <span v-if="index < deferred.length - 1">, </span>
            </template>
            . Those arrive in a later release.
          </p>

          <p class="mt-3 text-sm text-muted-foreground" data-testid="task-no-comment">
            A decision recorded here carries no comment yet — that arrives with the approval screen
            in a later release. Say anything that needs saying on the document itself.
          </p>
        </template>
      </div>

      <div>
        <Button variant="outline" size="sm" @click="router.push({ name: 'tasks' })">
          Back to my tasks
        </Button>
      </div>
    </template>
  </section>
</template>
