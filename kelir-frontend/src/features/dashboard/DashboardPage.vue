<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'

import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { getDashboardSummary } from '@/api/reporting'
import { ApiError } from '@/api/error'
import { useAuthStore } from '@/stores/auth'
import type { DashboardSummary } from '@/types/reporting'

/**
 * The dashboard (FR-RPT-001, #431).
 *
 * **This page called `/version` until Phase 8 opened**, and showed whoever
 * signed in the backend's version string. The version read is gone rather than
 * moved: it existed to prove the API client worked against a running backend,
 * and a page that fetches a real summary proves the same thing while being the
 * screen it is named after. `/version` is still served, and
 * `deployment.ts` is still where an operator reads it.
 *
 * **One request, and the widgets will not add a second.**
 * [ADR-0039](../../../../docs/architectures/adr/0039.%20A%20Dashboard%20Widget%20Is%20a%20Purpose-Built%20Endpoint.md)
 * (**D-78**) makes the dashboard one screen with one contract, so FR-RPT-002's
 * pending-task rows and FR-RPT-003's recent documents arrive as fields on
 * `DashboardSummary` and as cards in the grid below. This file is the shell
 * they land in.
 */
const auth = useAuthStore()

/**
 * **The summary is permissioned; this page is not.**
 *
 * `/` is the home route and `router/index.spec.ts` keeps it deliberately
 * exempt, because a signed-in person has to be able to land somewhere — a
 * dashboard that redirected to `/forbidden` would send every session without
 * the grant into the one page that cannot be its own home. So the *card*
 * degrades rather than the route refusing.
 *
 * This is cosmetic, as `can` itself says: the server re-checks and answers 403.
 * What it buys is an explanation instead of an error on a screen nobody chose
 * to open.
 */
const canReadSummary = computed(() => auth.can('reporting:dashboard:read'))

const summary = ref<DashboardSummary | null>(null)
const error = ref<ApiError | null>(null)
const isLoading = ref(false)

/** The cards, derived rather than written out, so the grid has one shape. */
const cards = computed(() => {
  const data = summary.value

  if (data === null) {
    return []
  }

  return [
    {
      key: 'tasks-waiting',
      label: 'Waiting for you',
      value: data.tasksWaiting,
      caption: data.tasksOverdue > 0 ? `${data.tasksOverdue} past its date` : 'Nothing late',
      // `tasksOverdue` is a subset of `tasksWaiting`, so it is a caption on this
      // card rather than a card of its own — two tiles would read as two
      // populations and invite somebody to add them.
      emphasis: data.tasksOverdue > 0,
      to: '/tasks',
      linkLabel: 'Open your inbox',
    },
    {
      key: 'draft-documents',
      label: 'Your drafts',
      value: data.draftDocuments,
      caption: 'Raised by you, not sent yet',
      emphasis: false,
      to: '/documents',
      linkLabel: 'Open documents',
    },
  ]
})

const isEmpty = computed(
  () =>
    summary.value !== null &&
    summary.value.tasksWaiting === 0 &&
    summary.value.draftDocuments === 0,
)

/**
 * The pending-task rows (FR-RPT-002, #432).
 *
 * **The server chose these and their order.** They are the top of the caller's
 * inbox — `createdAt DESC`, the order the inbox itself opens on — so a person
 * who reads the card and then opens their queue finds the same rows at the top
 * in the same sequence. Sorting them here would make the widget a second
 * opinion about which work matters most, taken by a card rather than by the
 * screen that owns the queue.
 */
const pendingTasks = computed(() => summary.value?.pendingTasks ?? [])

/**
 * How many are waiting but not shown.
 *
 * From `tasksWaiting` rather than from the array's length, because the array is
 * capped at five and the count is the whole queue. This is the number that
 * makes the card honest about what it is hiding.
 */
const notShown = computed(() =>
  Math.max((summary.value?.tasksWaiting ?? 0) - pendingTasks.value.length, 0),
)

/**
 * **Whether a row may be a link.**
 *
 * The task screen holds `workflow:task:read` and the dashboard does not — the
 * summary is served behind `reporting:dashboard:read` alone, because every row
 * on it is the caller's own waiting work (ADR-0039). So a viewer can legitimately
 * be told what is waiting for them and still not have the inbox surface, and a
 * link would take them to `/forbidden`.
 *
 * **This is a courtesy, not a control.** It hides a link that would not work; it
 * hides nothing the server sent, and it is not what protects anything. The rows
 * render either way.
 */
const canOpenTasks = computed(() => auth.can('workflow:task:read'))

/**
 * The deadline as a person reads it.
 *
 * Formatting only — **no comparison happens here.** Whether the date has passed
 * is `isOverdue`, which the server answered against the clock that stamped it.
 * A card subtracting `dueAt` from the browser's clock would be a second
 * opinion, and a task late on one machine and not on another is FR-TASK-007's
 * bug report nobody can reproduce.
 */
function dueLabel(dueAt: string): string {
  return new Date(dueAt).toLocaleString()
}

async function load(): Promise<void> {
  if (!canReadSummary.value) {
    return
  }

  isLoading.value = true
  error.value = null

  try {
    summary.value = await getDashboardSummary()
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught : null
    summary.value = null
  } finally {
    isLoading.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">Dashboard</h2>
      <p class="mt-1 text-sm text-muted-foreground">What is waiting for you.</p>
    </div>

    <p v-if="!canReadSummary" class="text-sm text-muted-foreground" data-testid="no-permission">
      Your account cannot see the dashboard summary. Ask an administrator for
      <code class="rounded bg-muted px-1 py-0.5 text-xs">reporting:dashboard:read</code>.
    </p>

    <p v-else-if="isLoading" class="text-sm text-muted-foreground" data-testid="loading">
      Loading your summary…
    </p>

    <Alert v-else-if="error" variant="destructive" data-testid="error">
      <p class="font-medium">Could not load your summary</p>
      <p class="mt-1">{{ error.message }}</p>
      <p class="mt-1 text-xs opacity-80">Code: {{ error.code }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="load()">Try again</Button>
    </Alert>

    <template v-else-if="summary">
      <p v-if="isEmpty" class="text-sm text-muted-foreground" data-testid="empty">
        Nothing is waiting for you, and you have no unsent drafts.
      </p>

      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3" data-testid="cards">
        <article
          v-for="card in cards"
          :key="card.key"
          class="rounded-lg border border-border bg-card p-4"
          :data-testid="card.key"
        >
          <p class="text-sm text-muted-foreground">{{ card.label }}</p>
          <p class="mt-1 text-3xl font-semibold tabular-nums">{{ card.value }}</p>
          <p
            class="mt-1 text-xs"
            :class="card.emphasis ? 'font-medium text-destructive' : 'text-muted-foreground'"
          >
            {{ card.caption }}
          </p>
          <RouterLink
            :to="card.to"
            class="mt-3 inline-block text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            {{ card.linkLabel }}
          </RouterLink>
        </article>
      </div>

      <!--
        The pending-task widget (FR-RPT-002, #432). It is a card, not a queue:
        the inbox is one click away and pages properly, and what this answers is
        *is there anything, and what is at the top of it*.
      -->
      <article
        class="rounded-lg border border-border bg-card p-4"
        data-testid="pending-tasks"
        aria-labelledby="pending-tasks-heading"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h3 id="pending-tasks-heading" class="text-sm font-medium">Your pending tasks</h3>
          <RouterLink
            v-if="canOpenTasks"
            to="/tasks"
            class="text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            Open your inbox
          </RouterLink>
        </div>

        <!--
          **The empty state is a sentence, not an absence** (#432 AC4). A card
          with nothing in it is indistinguishable from one that failed to load,
          and the reader has no way to tell which they are looking at.
        -->
        <p
          v-if="pendingTasks.length === 0"
          class="mt-3 text-sm text-muted-foreground"
          data-testid="pending-empty"
        >
          Nothing is waiting for your decision.
        </p>

        <ul v-else class="mt-3 divide-y divide-border">
          <li
            v-for="task in pendingTasks"
            :key="task.id"
            class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2 first:pt-0 last:pb-0"
            data-testid="pending-task"
          >
            <div class="min-w-0">
              <!--
                Linked only where the link works: the task route holds
                `workflow:task:read` and this page does not require it. The row
                is shown either way — what the viewer may see is the server's
                answer, and it already gave it.
              -->
              <RouterLink
                v-if="canOpenTasks"
                :to="`/tasks/${task.id}`"
                class="text-sm font-medium text-primary underline-offset-4 hover:underline"
              >
                {{ task.taskName }}
              </RouterLink>
              <span v-else class="text-sm font-medium">{{ task.taskName }}</span>

              <span class="block truncate text-xs text-muted-foreground">
                {{ task.documentNumber ?? task.documentRef }} · {{ task.documentTitle }}
              </span>

              <!--
                Whose approval it is, when the holder is standing in for
                somebody (#184). A widget that dropped this shows a delegate
                five tasks and no reason they are theirs.
              -->
              <span
                v-if="task.delegatedFromDisplayName"
                class="block text-xs text-muted-foreground"
                data-testid="pending-task-delegated"
              >
                On {{ task.delegatedFromDisplayName }}'s behalf
              </span>
            </div>

            <!--
              **`isOverdue` is read, never derived** (#185 AC4) — the same rule
              the inbox's own rows follow, and the reason this widget takes the
              server's row type rather than a shape of its own.
            -->
            <Badge v-if="task.isOverdue" variant="destructive" data-testid="pending-task-overdue">
              Overdue
            </Badge>
            <span v-else-if="task.dueAt" class="text-xs text-muted-foreground">
              Due {{ dueLabel(task.dueAt) }}
            </span>
          </li>
        </ul>

        <!--
          What the card is hiding, from the count rather than from the rows —
          `tasksWaiting` is the whole queue and this list is its first five.
        -->
        <p
          v-if="notShown > 0"
          class="mt-3 text-xs text-muted-foreground"
          data-testid="pending-more"
        >
          {{ notShown }} more waiting.
        </p>
      </article>
    </template>
  </section>
</template>
