<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'

import { Alert } from '@/components/ui/alert'
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
    </template>
  </section>
</template>
