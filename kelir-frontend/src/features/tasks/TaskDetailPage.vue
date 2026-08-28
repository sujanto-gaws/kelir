<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { claimTask, decideTask, getTask } from '@/api/tasks'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { DOCUMENT_STATUS_LABELS } from '@/types/document'
import {
  TASK_STATUS_LABELS,
  type AvailableDecision,
  type DecisionAction,
  type TaskDetail,
} from '@/types/workflow'

/**
 * One task, and what it is asking (FR-TASK-003, 004, 005, 006; #179 AC4, #182).
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
 * # The comment is part of the decision, not a step after it
 *
 * FR-TASK-006. One box, filled before the button is pressed, sent in the same
 * request — because a decision and the reason for it are entered together, and a
 * screen that recorded the decision and then asked for a reason would have
 * already committed the half nobody can take back.
 *
 * **Whether a reason is required is the definition's to say** (JWSS §4.1), and
 * this screen reads `requiresComment` rather than deciding that rejections need
 * one. Deciding it here would be a second rule: the first workflow that marked
 * an `APPROVE` would have this screen sending a request the server refuses, from
 * a button the product itself drew. #182 AC4 is that both ends agree, and they
 * agree by there being one rule.
 *
 * **The client-side refusal is a courtesy and the server's is the control.**
 * Nothing here is trusted — `engine::fire` checks again, against the edge
 * `condition` actually selects, which this screen cannot know in advance.
 */
const route = useRoute()
const router = useRouter()

const task = ref<TaskDetail | null>(null)
const loading = ref(true)
const failed = ref(false)
const busy = ref(false)
const problem = ref('')
const notice = ref('')
const comment = ref('')
/** Set when the person pressed a decision that needs a reason and gave none. */
const missingComment = ref(false)

/**
 * Only what this release can perform, one entry per action.
 *
 * **Collapsed by action**, because a state may declare two transitions for one
 * verb with disjoint conditions (JWSS §4, S7) and the engine picks between them
 * when the decision arrives. Two buttons both saying "Approve" would ask the
 * person to choose something they cannot see, and the request carries the verb
 * rather than the edge.
 *
 * A collapsed entry **requires a comment if any of its edges does**. The screen
 * cannot know which edge will fire, and of the two ways to be wrong, asking for
 * a reason that turns out to be optional costs a sentence — while not asking
 * produces a refusal from a button the product drew.
 */
const offered = computed<AvailableDecision[]>(() => {
  const byAction = new Map<string, AvailableDecision>()

  for (const decision of task.value?.decisions ?? []) {
    if (!decision.supported) {
      continue
    }

    const seen = byAction.get(decision.action)

    if (seen) {
      seen.requiresComment = seen.requiresComment || decision.requiresComment
    } else {
      byAction.set(decision.action, { ...decision })
    }
  }

  return [...byAction.values()]
})

/** Declared by the definition and not performable yet — shown, not offered. */
const deferred = computed(() =>
  (task.value?.decisions ?? []).filter((decision) => !decision.supported),
)

/** True when any decision on offer would refuse an empty box. */
const commentEverRequired = computed(() =>
  offered.value.some((decision) => decision.requiresComment),
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
  missingComment.value = false

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

// **The comment is cleared here and not in `load`**, because `load` also runs
// after a claim and after a decision — and a person who typed a reason, then
// claimed the task so nobody else took it, should not find the box empty.
// A different task is the one thing that makes a half-written reason wrong.
watch(
  () => route.params.id,
  (id) => {
    comment.value = ''
    void load(String(id))
  },
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

async function decide(decision: AvailableDecision): Promise<void> {
  if (!task.value || busy.value) {
    return
  }

  // The client half of #182 AC4. Refused here so the person is told beside the
  // box rather than by a round trip — and refused again by the server, which is
  // the half that counts.
  if (decision.requiresComment && !comment.value.trim()) {
    missingComment.value = true
    problem.value = ''
    notice.value = ''
    return
  }

  busy.value = true
  problem.value = ''
  notice.value = ''
  missingComment.value = false

  try {
    const result = await decideTask(task.value.id, decision.action as DecisionAction, comment.value)
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
          <!-- The reason, entered with the decision rather than after it. The
               box is above the buttons because that is the order the two are
               done in, and a field below the control that sends it reads as an
               afterthought. -->
          <div v-if="offered.length" class="mt-3 space-y-2" data-testid="task-comment">
            <Label for="decision-comment">
              Reason
              <span v-if="commentEverRequired" class="text-destructive">*</span>
              <span v-else class="font-normal text-muted-foreground"> (optional)</span>
            </Label>

            <Textarea
              id="decision-comment"
              v-model="comment"
              :rows="3"
              :disabled="busy"
              :invalid="missingComment"
              described-by="decision-comment-help"
              placeholder="Why you are deciding this way"
            />

            <p
              v-if="missingComment"
              id="decision-comment-help"
              class="text-sm text-destructive"
              data-testid="comment-required"
            >
              This decision needs a reason. It is recorded with the decision and shown to whoever
              raised the document.
            </p>
            <p v-else id="decision-comment-help" class="text-sm text-muted-foreground">
              Recorded with the decision and shown in the document's approval history.
            </p>
          </div>

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
              @click="decide(decision)"
            >
              {{ decision.action === 'APPROVE' ? 'Approve' : 'Reject' }} →
              {{ decision.toStateName }}
              <span v-if="decision.requiresComment" aria-hidden="true"> *</span>
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
