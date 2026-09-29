<script setup lang="ts">
import { computed } from 'vue'
import { TriangleAlert } from 'lucide-vue-next'

import { Button } from '@/components/ui/button'
import type { Role } from '@/types/identity'

/**
 * A role whose last holder has left while open tasks still need it (#508,
 * D-91 (2)).
 *
 * Removing the last grant, a grant's `valid_to` passing, or deactivating the
 * last holder is not refused, so the role stays live and its tasks stay open
 * with nobody to decide them. This says so on the role's row of the Roles page,
 * which is where an administrator finds it without first trying a delete.
 *
 * # Drawn from the server's counts, and only when it gave them
 *
 * `liveHolders` and `openTasks` are on a role only for a caller holding
 * `workflow:task:reassign`, and omitted for anyone else. A missing count is
 * *not told*, never 0, so without both nothing is drawn. `RoleListPage` checks
 * the permission as well, the way it gates every other control.
 *
 * # The list needs a second permission
 *
 * The tasks are listed by `GET /identity/roles/{id}/open-tasks`, which requires
 * `identity:role:delete`. With it, the notice is a button that opens that list
 * (`open`), where each task is reassigned. Without it the notice is plain text
 * that says why there is no list, rather than a link that would answer 403.
 */
const props = defineProps<{
  role: Role
  /** Whether the caller may read the role's open tasks (`identity:role:delete`). */
  canList: boolean
}>()

const emit = defineEmits<{ open: [role: Role] }>()

/** The tasks stranded on the role, or `null` when the role is not stranded. */
const stranded = computed<number | null>(() => {
  const { liveHolders, openTasks } = props.role

  if (typeof liveHolders !== 'number' || typeof openTasks !== 'number') {
    return null
  }

  return liveHolders === 0 && openTasks > 0 ? openTasks : null
})

const label = computed(() => {
  const count = stranded.value ?? 0

  return `${count} open ${count === 1 ? 'task' : 'tasks'}, 0 active holders`
})
</script>

<template>
  <div
    v-if="stranded !== null"
    class="mt-1 flex flex-wrap items-center gap-x-2 text-xs"
    data-testid="role-stranded-tasks"
  >
    <TriangleAlert class="size-3.5 shrink-0 text-destructive" aria-hidden="true" />

    <Button
      v-if="canList"
      variant="link"
      size="sm"
      class="h-auto px-0 text-xs text-destructive"
      :title="`List the open tasks that need ${role.name}, to reassign them`"
      data-testid="role-stranded-tasks-open"
      @click="emit('open', role)"
    >
      {{ label }}
    </Button>

    <template v-else>
      <span class="font-medium text-destructive">{{ label }}</span>
      <span class="text-muted-foreground" data-testid="role-stranded-tasks-unlisted">
        Listing them needs permission to delete roles.
      </span>
    </template>
  </div>
</template>
