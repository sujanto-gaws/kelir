<script setup lang="ts">
import { computed, useSlots, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { listOpenTasksOfRole } from '@/api/identity'
import { usePaginatedList } from '@/composables/usePaginatedList'
import type { OpenTaskNeedingRole, Role } from '@/types/identity'

/**
 * The open tasks a role is needed by (**D-89**, #532).
 *
 * Opened by `RoleListPage` in two places. On a `ROLE_HAS_OPEN_TASKS` refusal,
 * the refusal's own message says how many, and this lists which, from
 * `GET /identity/roles/{id}/open-tasks`. From a role's stranded-tasks notice
 * (#508), with no refusal: nobody holds the role, and this lists what waits on
 * it, so it is found without a delete attempt. Each row names the task, its
 * document, the state its workflow is in, who holds it, and why it needs the
 * role, which is what User Manual §11.2 promises.
 *
 * **Read when opened, not with the refusal.** The list is read outside the
 * delete's lock, so a task decided in between is simply not in it, and an
 * empty list means the delete can be tried again.
 *
 * Each row takes an action through the `task-actions` slot, which is where
 * `RoleListPage` puts the reassign (#512). The column is drawn only when the
 * slot is given. An action that changes a task calls `refresh`, exposed below,
 * so a task reassigned away from the role leaves the list.
 */
const props = defineProps<{
  role: Role | null
  /**
   * The server's refusal, verbatim: it carries the count. Absent or empty when
   * the list is opened from the stranded-tasks notice, where no delete was
   * tried.
   */
  refusal?: string
}>()

defineSlots<{
  'task-actions'?(props: { task: OpenTaskNeedingRole; role: Role }): unknown
}>()

const open = defineModel<boolean>('open', { default: false })

const slots = useSlots()
const hasActions = computed(() => slots['task-actions'] !== undefined)

const tasks = usePaginatedList<OpenTaskNeedingRole>((query) =>
  // `load` is only called while a role is set; see the watch below.
  listOpenTasksOfRole(props.role?.id ?? '', query),
)

const title = computed(() => `Open tasks need ${props.role?.name ?? 'this role'}`)

/** The holder by name, the id when the name did not join, or nobody. */
function holderOf(task: OpenTaskNeedingRole): string {
  if (task.assigneeUserId === null) {
    return 'Nobody has claimed it'
  }

  return task.assigneeDisplayName ?? task.assigneeUserId
}

watch(
  () => [open.value, props.role?.id] as const,
  async ([isOpen, roleId]) => {
    if (!isOpen || roleId === undefined) {
      return
    }

    // Each opening starts from the first page: a list left on page three by
    // the previous role would be a page this one may not have.
    tasks.page.value = 1
    await tasks.load()
  },
  { immediate: true },
)

defineExpose({
  /** Read the current page again, after a row's action changed a task. */
  refresh: () => tasks.refresh(),
})
</script>

<template>
  <Dialog v-model:open="open" :title="title" class="max-w-4xl">
    <Alert v-if="refusal" variant="destructive">{{ refusal }}</Alert>
    <p v-else class="text-sm text-muted-foreground" data-testid="role-open-tasks-stranded">
      Nobody holds this role any more, so nobody can decide these tasks. Reassign each one to a role
      or a user who can.
    </p>

    <p v-if="tasks.isLoading.value" class="mt-4 text-sm text-muted-foreground">
      Loading the open tasks…
    </p>

    <Alert v-else-if="tasks.error.value" variant="destructive" class="mt-4">
      <p>The open tasks could not be listed: {{ tasks.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="tasks.load()">Try again</Button>
    </Alert>

    <p
      v-else-if="tasks.items.value.length === 0"
      class="mt-4 text-sm text-muted-foreground"
      data-testid="role-open-tasks-empty"
    >
      <template v-if="refusal">
        No open task needs this role any more. They were decided or reassigned after the delete was
        refused, so it can be tried again.
      </template>
      <template v-else>No open task needs this role any more.</template>
    </p>

    <template v-else>
      <div class="mt-4">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Task</TableHead>
              <TableHead>Document</TableHead>
              <TableHead>Workflow state</TableHead>
              <TableHead>Held by</TableHead>
              <TableHead>Why it needs the role</TableHead>
              <TableHead v-if="hasActions" class="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>

          <TableBody>
            <TableRow v-for="task in tasks.items.value" :key="task.id" data-testid="role-open-task">
              <TableCell class="font-mono text-xs">{{ task.taskRef }}</TableCell>
              <TableCell>
                <span v-if="task.documentNumber" class="block font-mono text-xs">
                  {{ task.documentNumber }}
                </span>
                <span>{{ task.documentTitle ?? '—' }}</span>
              </TableCell>
              <TableCell class="font-mono text-xs">{{ task.currentState }}</TableCell>
              <TableCell
                :class="task.assigneeUserId === null ? 'text-muted-foreground' : undefined"
                data-testid="role-open-task-holder"
              >
                {{ holderOf(task) }}
              </TableCell>
              <TableCell class="text-muted-foreground">{{ task.why }}</TableCell>
              <TableCell v-if="hasActions && role" class="text-right">
                <slot name="task-actions" :task="task" :role="role" />
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>

      <div v-if="tasks.totalPages.value > 1" class="mt-4 flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ tasks.page.value }} of {{ tasks.totalPages.value }} · {{ tasks.total.value }} open
          tasks
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!tasks.hasPrevious.value"
            @click="tasks.goToPage(tasks.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!tasks.hasNext.value"
            @click="tasks.goToPage(tasks.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <template #footer>
      <Button variant="outline" @click="open = false">Close</Button>
    </template>
  </Dialog>
</template>
