<script setup lang="ts">
import { computed, defineAsyncComponent, h, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'

import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { getDashboardSummary } from '@/api/reporting'
import { ApiError } from '@/api/error'
import { useAuthStore } from '@/stores/auth'
import { DOCUMENT_STATUS_LABELS, type DocumentStatus } from '@/types/document'
import type { DashboardSummary } from '@/types/reporting'

import { durationLabel } from './duration'

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
 * pending-task rows, FR-RPT-005's late tasks, FR-RPT-003's recent documents and
 * FR-RPT-006's approval time arrive as fields on `DashboardSummary` and as cards
 * in the grid below. This file is the shell they land in.
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

/**
 * The cards, derived rather than written out, so the grid has one shape.
 *
 * **Each tile's link follows the grant its screen holds** — `canOpen`, the same
 * courtesy every card further down extends. The tiles are served behind
 * `reporting:dashboard:read` alone, so a viewer can be told how much is waiting
 * and still not hold the inbox or the document list, and a link would take
 * them to `/forbidden`.
 *
 * **It hides a link that would not work; it hides nothing the server sent.**
 * The number and its caption render either way.
 */
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
      canOpen: canOpenTasks.value,
    },
    {
      key: 'draft-documents',
      label: 'Your drafts',
      value: data.draftDocuments,
      caption: 'Raised by you, not sent yet',
      emphasis: false,
      to: '/documents',
      linkLabel: 'Open documents',
      canOpen: canOpenDocuments.value,
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
 * The caller's late tasks (FR-RPT-005, #446).
 *
 * **The server chose these and their order** — most overdue first, `dueAt`
 * ascending. Sorting here would look harmless because the field is right there
 * on the row, and it is not: the order is the server's answer about which late
 * work is latest, and lateness itself is judged against the clock that stamped
 * the deadline rather than the browser's (FR-TASK-007).
 */
const overdueTasks = computed(() => summary.value?.overdueTasks ?? [])

/**
 * How many are late but not shown.
 *
 * From `tasksOverdue` rather than from the array's length, for the reason
 * `notShown` gives one card up: the array is capped at five and the count is
 * the whole late set, read in the same statement as the rows.
 */
const lateNotShown = computed(() =>
  Math.max((summary.value?.tasksOverdue ?? 0) - overdueTasks.value.length, 0),
)

/**
 * The documents the caller touched most recently (FR-RPT-003, #433).
 *
 * **The server chose these and their order** — most recent touch first, by the
 * `lastTouchedAt` each row carries. Sorting here on any other field would make
 * the card a second opinion about what *recent* means, and the one field that
 * invites it is `updatedAt`: it moves when **anybody** changes the document,
 * which is a different question from *what did I work on*.
 */
const recentDocuments = computed(() => summary.value?.recentDocuments ?? [])

/**
 * **Whether a row may be a link.**
 *
 * The same courtesy `canOpenTasks` is, one card over, and for the same reason:
 * the document route holds `document:read` and the summary is served behind
 * `reporting:dashboard:read` alone, because every row on it is the caller's own
 * work (ADR-0039). A person can legitimately be shown the documents they
 * themselves acted on and still not hold the document surface.
 *
 * **It hides a link that would not work; it hides nothing the server sent.**
 */
const canOpenDocuments = computed(() => auth.can('document:read'))

/**
 * What a status looks like at a glance — three groups, not ten colours.
 *
 * Copied in shape from `DocumentListPage`'s own helper rather than imported,
 * which is a deliberately small duplication: the alternative is exporting a
 * presentation detail from a feature module so a card can match it, and the
 * thing that would drift is a colour rather than a rule. **Nothing about which
 * rows exist is decided here.**
 */
function statusVariant(status: DocumentStatus): 'default' | 'secondary' | 'destructive' {
  if (status === 'REJECTED' || status === 'CANCELLED') {
    return 'destructive'
  }

  if (status === 'COMPLETED' || status === 'APPROVED') {
    return 'default'
  }

  return 'secondary'
}

/**
 * When the caller last touched it, as a person reads it.
 *
 * Formatting only. The value is the server's and is the one the list was
 * ordered by, so this function must not compute, compare or re-derive it — the
 * same rule `dueLabel` follows one card over.
 */
function touchedLabel(lastTouchedAt: string): string {
  return new Date(lastTouchedAt).toLocaleString()
}

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

/**
 * The caller's documents by status (FR-RPT-004, #447).
 *
 * **The server chose these and their order** — all ten statuses, zeros
 * included, in the lifecycle's order. Sorting by count would turn the card into
 * a ranking of statuses, and the order a reader can check it against is the
 * life of a document, which is the server's to state.
 *
 * **Documents the caller raised**, not every document they can see, so the card
 * says *your* documents. The drafts tile above is this list's `DRAFT` row by
 * construction on the server, and neither is derived from the other here.
 */
const documentsByStatus = computed(() => summary.value?.documentsByStatus ?? [])

/**
 * Whether the caller has raised nothing at all — ten zeros.
 *
 * A chart of ten empty bars looks like a chart that failed to draw, so this is
 * the condition under which the card says so in a sentence instead.
 */
const hasNoDocuments = computed(() => documentsByStatus.value.every((row) => row.count === 0))

/**
 * **The chart, fetched only when the card renders.**
 *
 * Unovis draws with d3, and this page is the home route, so a static import
 * here would put the library in front of every sign-in.
 * `defineAsyncComponent` is the edge that keeps it a chunk of its own, and
 * `scripts/check-bundle-split.mjs` fails the build when it is not one.
 *
 * **Neither fallback hides a number.** The counts are text beside the chart, so
 * a slow chunk shows a sentence in the chart's place, and a failed one — a
 * deploy that rotated the file away mid-session — says the chart could not be
 * drawn while the numbers stay where they are.
 */
const DocumentStatusChart = defineAsyncComponent({
  loader: () => import('./DocumentStatusChart.vue'),
  loadingComponent: () =>
    h(
      'p',
      { class: 'text-xs text-muted-foreground', 'data-testid': 'status-chart-loading' },
      'Drawing the chart…',
    ),
  errorComponent: () =>
    h(
      'p',
      { class: 'text-xs text-muted-foreground', 'data-testid': 'status-chart-error' },
      'The chart could not be drawn. The counts below are the same numbers.',
    ),
})

/**
 * An approval time on the card, or a dash where the server sent none.
 *
 * The card shows the times only when `documents` is above zero, and the server
 * sends `null` only when it is zero — so the dash is for a response that
 * disagreed with itself, where printing *under a minute* for `null` would be a
 * confident answer to a question nobody answered.
 */
function timeLabel(seconds: number | null): string {
  return seconds === null ? '—' : durationLabel(seconds)
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
      <p class="mt-1 text-sm text-muted-foreground">
        What is waiting for you, what is late, what you touched last, where your documents stand,
        and how long they take to be decided.
      </p>
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
          <!-- Linked only where the link works: `canOpen` is the grant the screen holds. -->
          <RouterLink
            v-if="card.canOpen"
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

      <!--
        The late-task widget (FR-RPT-005, #446). The waiting card already says
        *how many* are late; this card says *which*, so the caption and these
        rows are one answer read in one statement.
      -->
      <article
        class="rounded-lg border border-border bg-card p-4"
        data-testid="overdue-tasks"
        aria-labelledby="overdue-tasks-heading"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h3 id="overdue-tasks-heading" class="text-sm font-medium">What is late</h3>
          <RouterLink
            v-if="canOpenTasks"
            to="/tasks"
            class="text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            Open your inbox
          </RouterLink>
        </div>

        <!--
          **The empty state is a sentence, not an absence** — the same rule the
          pending-task card follows. Here it is also good news, and a blank card
          would withhold it.
        -->
        <p
          v-if="overdueTasks.length === 0"
          class="mt-3 text-sm text-muted-foreground"
          data-testid="overdue-empty"
        >
          Nothing waiting for you is past its date.
        </p>

        <!--
          In the order the server sent, most overdue first. **Nothing here
          sorts, and nothing here decides a row is late** — every row in this
          list is late because the server put it here.
        -->
        <ul v-else class="mt-3 divide-y divide-border">
          <li
            v-for="task in overdueTasks"
            :key="task.id"
            class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2 first:pt-0 last:pb-0"
            data-testid="overdue-task"
          >
            <div class="min-w-0">
              <!-- Linked only where the link works, as the pending-task rows are. -->
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
                Whose approval it is (#184). A late task that is somebody else's
                work stood in for is the one a delegate most needs explained.
              -->
              <span
                v-if="task.delegatedFromDisplayName"
                class="block text-xs text-muted-foreground"
                data-testid="overdue-task-delegated"
              >
                On {{ task.delegatedFromDisplayName }}'s behalf
              </span>
            </div>

            <!--
              The deadline, formatted and never compared: `dueLabel` is
              formatting only, and the row is late on the server's word.
            -->
            <span
              v-if="task.dueAt"
              class="text-xs font-medium text-destructive"
              data-testid="overdue-task-due"
            >
              Was due {{ dueLabel(task.dueAt) }}
            </span>
          </li>
        </ul>

        <!--
          What the card is hiding, from the count rather than from the rows —
          `tasksOverdue` is the whole late set and this list is its first five.
        -->
        <p
          v-if="lateNotShown > 0"
          class="mt-3 text-xs text-muted-foreground"
          data-testid="overdue-more"
        >
          {{ lateNotShown }} more late.
        </p>
      </article>

      <!--
        The recent-documents widget (FR-RPT-003, #433). A card, not a list: the
        document list is one click away and pages and searches properly, and
        what this answers is *what was I last working on*.
      -->
      <article
        class="rounded-lg border border-border bg-card p-4"
        data-testid="recent-documents"
        aria-labelledby="recent-documents-heading"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h3 id="recent-documents-heading" class="text-sm font-medium">What you touched last</h3>
          <RouterLink
            v-if="canOpenDocuments"
            to="/documents"
            class="text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            Open documents
          </RouterLink>
        </div>

        <!--
          **The empty state is a sentence, not an absence** (#433 AC5) — the same
          rule the pending-task card follows. A card with nothing in it is
          indistinguishable from one that failed to load, and the reader has no
          way to tell which they are looking at.
        -->
        <p
          v-if="recentDocuments.length === 0"
          class="mt-3 text-sm text-muted-foreground"
          data-testid="recent-empty"
        >
          You have not worked on any documents yet.
        </p>

        <ul v-else class="mt-3 divide-y divide-border">
          <li
            v-for="document in recentDocuments"
            :key="document.id"
            class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1 py-2 first:pt-0 last:pb-0"
            data-testid="recent-document"
          >
            <div class="min-w-0">
              <!--
                Linked only where the link works: the document route holds
                `document:read` and this page does not require it. The row is
                shown either way — what the viewer may see is the server's
                answer, and it already gave it.
              -->
              <RouterLink
                v-if="canOpenDocuments"
                :to="`/documents/${document.id}`"
                class="text-sm font-medium text-primary underline-offset-4 hover:underline"
              >
                {{ document.title }}
              </RouterLink>
              <span v-else class="text-sm font-medium">{{ document.title }}</span>

              <span class="block truncate text-xs text-muted-foreground">
                {{ document.documentNumber ?? document.documentRef }} ·
                {{ document.documentTypeCode }}
              </span>

              <!--
                **The date is read, never derived** — `lastTouchedAt` is the
                value the server ordered by, so the sequence on screen and the
                dates beside it are one answer. `updatedAt` is the field that
                would look interchangeable and is not: it moves when anybody
                changes the document.
              -->
              <span class="block text-xs text-muted-foreground" data-testid="recent-touched">
                {{ touchedLabel(document.lastTouchedAt) }}
              </span>
            </div>

            <Badge :variant="statusVariant(document.status)" data-testid="recent-status">
              {{ document.status }}
            </Badge>
          </li>
        </ul>
      </article>

      <!--
        The status widget (FR-RPT-004, #447). It answers *where do the documents
        I raised stand*; the document list is one click away for acting on any
        of them.
      -->
      <article
        class="rounded-lg border border-border bg-card p-4"
        data-testid="documents-by-status"
        aria-labelledby="documents-by-status-heading"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h3 id="documents-by-status-heading" class="text-sm font-medium">
            Your documents by status
          </h3>
          <!-- Linked only where the link works, as the recent-document rows are. -->
          <RouterLink
            v-if="canOpenDocuments"
            to="/documents"
            class="text-sm font-medium text-primary underline-offset-4 hover:underline"
          >
            Open documents
          </RouterLink>
        </div>

        <!--
          **The empty state is a sentence, not an absence** — and here not ten
          empty bars either, which read as a chart that failed to draw.
        -->
        <p
          v-if="hasNoDocuments"
          class="mt-3 text-sm text-muted-foreground"
          data-testid="status-empty"
        >
          You have not raised any documents yet.
        </p>

        <template v-else>
          <!--
            Held at the chart's height while its chunk arrives, so the counts
            below do not jump when it does.
          -->
          <div class="mt-3 min-h-[280px]">
            <DocumentStatusChart :counts="documentsByStatus" />
          </div>

          <!--
            **The numbers, as text.** The chart above is a picture of these and
            is hidden from assistive technology; this list is what a screen
            reader reads and what holds if the chart never loads. In the order
            the server sent — **nothing here sorts.**
          -->
          <ul
            class="mt-3 grid grid-cols-2 gap-x-6 gap-y-1 sm:grid-cols-5"
            aria-labelledby="documents-by-status-heading"
            data-testid="status-counts"
          >
            <li
              v-for="row in documentsByStatus"
              :key="row.status"
              class="flex items-baseline justify-between gap-2 text-xs"
              data-testid="status-count"
              :data-status="row.status"
            >
              <span class="text-muted-foreground" data-testid="status-label">
                {{ DOCUMENT_STATUS_LABELS[row.status] }}
              </span>
              <span class="font-medium tabular-nums" data-testid="status-value">
                {{ row.count }}
              </span>
            </li>
          </ul>
        </template>
      </article>

      <!--
        The approval time card (FR-RPT-006, #461; D-83). **Three numbers and no
        chart**: a median, a slowest and a count are read, not compared along an
        axis, so nothing here loads Unovis (ADR-0040 §6).

        **Nothing here measures.** Each document's time is the server's, from
        its first submission to the decision, and the window is the server's
        `windowDays` rather than a number this page holds.
      -->
      <article
        class="rounded-lg border border-border bg-card p-4"
        data-testid="approval-time"
        aria-labelledby="approval-time-heading"
      >
        <h3 id="approval-time-heading" class="text-sm font-medium">How long approval takes</h3>
        <p class="mt-1 text-xs text-muted-foreground" data-testid="approval-time-scope">
          Documents you raised, decided in the last {{ summary.approvalTime.windowDays }} days —
          from the first time each was sent to its approval or rejection, rounds sent back included.
        </p>

        <!--
          **The empty state is a sentence, not an absence** — and not three
          dashes either, which read as a card that failed to load.
        -->
        <p
          v-if="summary.approvalTime.documents === 0"
          class="mt-3 text-sm text-muted-foreground"
          data-testid="approval-time-empty"
        >
          None of your documents has been decided in the last
          {{ summary.approvalTime.windowDays }} days.
        </p>

        <dl v-else class="mt-3 grid grid-cols-3 gap-4">
          <div data-testid="approval-time-median">
            <dt class="text-xs text-muted-foreground">Median</dt>
            <dd class="mt-1 text-xl font-semibold tabular-nums">
              {{ timeLabel(summary.approvalTime.medianSeconds) }}
            </dd>
          </div>
          <div data-testid="approval-time-slowest">
            <dt class="text-xs text-muted-foreground">Slowest</dt>
            <dd class="mt-1 text-xl font-semibold tabular-nums">
              {{ timeLabel(summary.approvalTime.slowestSeconds) }}
            </dd>
          </div>
          <div data-testid="approval-time-documents">
            <dt class="text-xs text-muted-foreground">Decided</dt>
            <dd class="mt-1 text-xl font-semibold tabular-nums">
              {{ summary.approvalTime.documents }}
            </dd>
          </div>
        </dl>
      </article>
    </template>
  </section>
</template>
