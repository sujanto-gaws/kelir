<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { listTasks } from '@/api/tasks'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useQueryBackedList, type ListQuery } from '@/composables/useQueryBackedList'
import { TASK_STATUS_LABELS, type InboxTask } from '@/types/workflow'

/**
 * The task inbox — what is waiting for the person looking at it (FR-TASK-001,
 * 002; #179).
 *
 * **The URL is the state**, as it is on every list in this product (#101 AC3):
 * the page and the scope are read from the query string and written back to it,
 * so a view can be linked to and survives a reload.
 *
 * **Nothing is narrowed here.** The visibility rule — *mine, or offered to a
 * role I hold* — is a predicate in the backend's own query, and a client-side
 * filter would be a second rule over the same rows. On this surface that is
 * sharper than a performance point: it would mean showing somebody a task the
 * API will then refuse them, which reads as a broken product while being a leak.
 *
 * **Mine and unclaimed are shown apart, because they are different situations.**
 * A task already assigned to me is work I have; an unclaimed one is work going
 * spare that somebody else may take while I am reading. The backend answers
 * which is which (`assignment`), so this screen does not derive it — two places
 * deriving it would derive it differently.
 *
 * **Acting on a task is not here.** Opening one is; the buttons are FR-TASK-004
 * and 005 in Sprint 11 (#182), and the detail screen says so where a person
 * would go looking for them.
 */
const route = useRoute()
const router = useRouter()

const list = useQueryBackedList<InboxTask>(listTasks)

/**
 * What the inbox may be narrowed to.
 *
 * Two values, not a status filter. FR-TASK-009 (completed tasks) is unscheduled,
 * so an inbox that offered `CANCELLED` would be asking a question nobody has
 * specified — and the backend refuses it, which would be a control the screen
 * drew and the API rejected.
 */
const scopeOptions = [
  { value: 'open', label: 'Waiting for me' },
  { value: 'all', label: 'Everything I have held' },
]

const currentQuery = computed<ListQuery>(() => {
  const query: ListQuery = {}

  for (const [key, value] of Object.entries(route.query)) {
    const first = Array.isArray(value) ? value[0] : value

    if (typeof first === 'string' && first !== '') {
      query[key] = first
    }
  }

  return query
})

function navigate(changes: ListQuery, resetPage = true): void {
  const next: ListQuery = { ...currentQuery.value, ...changes }

  if (resetPage) {
    delete next.page
  }

  for (const [key, value] of Object.entries(next)) {
    if (value === '') {
      delete next[key]
    }
  }

  void router.push({ name: 'tasks', query: next })
}

function goToPage(next: number): void {
  navigate({ page: String(Math.min(Math.max(1, next), list.totalPages.value)) }, false)
}

function openTask(id: string): void {
  void router.push({ name: 'task', params: { id } })
}

// One load path: the URL changed, so re-apply it. `immediate` is what makes a
// deep-linked view load its own scope rather than the default one.
watch(
  () => route.query,
  () => {
    void list.apply(currentQuery.value)
  },
  { immediate: true, deep: true },
)
</script>

<template>
  <section class="space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">My tasks</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        Approvals assigned to you, and approvals offered to roles you hold.
      </p>
    </div>

    <div class="grid gap-3 sm:max-w-xs">
      <div class="space-y-2">
        <Label for="tasks-scope">Show</Label>
        <Select
          id="tasks-scope"
          data-testid="tasks-scope"
          :model-value="list.filters.value.scope ?? 'open'"
          :options="scopeOptions"
          @update:model-value="navigate({ scope: $event })"
        />
      </div>
    </div>

    <Alert v-if="list.error.value" variant="destructive" data-testid="tasks-error">
      <p>{{ list.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="list.apply(currentQuery)">
        Try again
      </Button>
    </Alert>

    <p v-if="list.isLoading.value" class="text-sm text-muted-foreground">Loading tasks…</p>

    <template v-else-if="!list.error.value">
      <p v-if="list.isEmpty.value" class="text-sm text-muted-foreground" data-testid="tasks-empty">
        Nothing is waiting for you.
      </p>

      <Table v-else data-testid="tasks-table">
        <TableHeader>
          <TableRow>
            <TableHead>Task</TableHead>
            <TableHead>Document</TableHead>
            <TableHead>Workflow</TableHead>
            <TableHead>Whose</TableHead>
            <TableHead>Status</TableHead>
            <TableHead class="sr-only">Open</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="task in list.items.value"
            :key="task.id"
            :data-testid="`task-row-${task.taskRef}`"
          >
            <TableCell class="font-medium">{{ task.taskName }}</TableCell>
            <TableCell>
              <span class="font-medium">{{ task.documentNumber ?? task.documentRef }}</span>
              <span class="block text-sm text-muted-foreground">{{ task.documentTitle }}</span>
            </TableCell>
            <TableCell>
              {{ task.workflowName }}
              <span class="block text-sm text-muted-foreground">{{ task.currentState }}</span>
            </TableCell>
            <TableCell>
              <!-- The distinction #179 AC1 exists for. "Unclaimed" carries the
                   role it is offered to, because a person holding three roles
                   needs to know which queue this came out of. -->
              <Badge v-if="task.assignment === 'MINE'" data-testid="assignment-mine">Mine</Badge>
              <Badge v-else variant="secondary" data-testid="assignment-role">
                Unclaimed{{ task.candidateRoleCode ? ` · ${task.candidateRoleCode}` : '' }}
              </Badge>
            </TableCell>
            <TableCell>{{ TASK_STATUS_LABELS[task.status] }}</TableCell>
            <TableCell class="text-right">
              <Button
                variant="outline"
                size="sm"
                :data-testid="`open-task-${task.taskRef}`"
                @click="openTask(task.id)"
              >
                Open
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="list.totalPages.value > 1" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ list.page.value }} of {{ list.totalPages.value }} · {{ list.total.value }} tasks
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="list.page.value <= 1"
            @click="goToPage(list.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="list.page.value >= list.totalPages.value"
            @click="goToPage(list.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>
  </section>
</template>
