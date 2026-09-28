<script setup lang="ts">
import { computed, onMounted, ref, useTemplateRef } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { deleteRole, listPermissions, listRoles } from '@/api/identity'
import { toApiError } from '@/api/client'
import { usePaginatedList } from '@/composables/usePaginatedList'
import { useAuthStore } from '@/stores/auth'
import {
  ROLE_HAS_OPEN_TASKS,
  type OpenTaskNeedingRole,
  type Permission,
  type Role,
} from '@/types/identity'
import ConfirmDialog from './ConfirmDialog.vue'
import ReassignTaskDialog from './ReassignTaskDialog.vue'
import RoleFormDialog from './RoleFormDialog.vue'
import RoleOpenTasksDialog from './RoleOpenTasksDialog.vue'
import RoleStrandedTasksNotice from './RoleStrandedTasksNotice.vue'

/**
 * Role administration (FR-IDM-002, FR-IDM-004, FR-IDM-005).
 */
const auth = useAuthStore()

const canCreate = computed(() => auth.can('identity:role:create'))
const canUpdate = computed(() => auth.can('identity:role:update'))
const canDelete = computed(() => auth.can('identity:role:delete'))
/**
 * Reassigning is its own permission (#512, ADR-0042): the open tasks are listed
 * to whoever may delete the role, and the action on each only to whoever may
 * also reassign. The server checks it again.
 */
const canReassign = computed(() => auth.can('workflow:task:reassign'))

/**
 * The backend's refusal, mirrored from
 * `kelir-backend/src/modules/identity/service.rs` (the 409 raised when
 * `is_system` is true), so the UI can refuse before spending a request and say
 * the same thing the server would.
 *
 * Mirrored strings drift. If the backend's wording changes, this is the line to
 * change with it — and `RoleListPage.spec.ts` asserts the UI shows exactly this
 * text, so the copy cannot quietly diverge from what is asserted.
 */
const SYSTEM_ROLE_REFUSAL =
  'System roles cannot be deleted; a tenant would be left unable to grant permissions'

const roles = usePaginatedList<Role>(listRoles)

const permissions = ref<Permission[]>([])
const permissionsError = ref('')

const isFormOpen = ref(false)
const editing = ref<Role | null>(null)

const confirming = ref<Role | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

/**
 * The role whose open tasks are listed, and the refusal's words: a delete the
 * open tasks refused (#532), or no words when the list was opened from the
 * role's stranded-tasks notice (#508).
 */
const blocked = ref<Role | null>(null)
const blockedRefusal = ref('')
const isOpenTasksOpen = ref(false)
const openTasks = useTemplateRef<InstanceType<typeof RoleOpenTasksDialog>>('openTasks')

/** The listed task being reassigned (#512). */
const reassigning = ref<OpenTaskNeedingRole | null>(null)
const isReassignOpen = ref(false)

async function loadPermissions(): Promise<void> {
  permissionsError.value = ''

  try {
    // An item envelope wrapping an array, with no pagination — see
    // `api/identity.ts`.
    permissions.value = await listPermissions()
  } catch (error) {
    permissionsError.value = toApiError(error).message
  }
}

function openCreate(): void {
  editing.value = null
  isFormOpen.value = true
}

function openEdit(role: Role): void {
  editing.value = role
  isFormOpen.value = true
}

function openDelete(role: Role): void {
  confirming.value = role
  deleteError.value = ''
  isConfirmOpen.value = true
}

async function onSaved(): Promise<void> {
  await roles.refresh()
}

async function confirmDelete(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isDeleting.value = true
  deleteError.value = ''

  try {
    await deleteRole(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await roles.refresh()
  } catch (error) {
    const refusal = toApiError(error)

    // A delete answers 409 for three reasons, one at a time and told apart by
    // the code, none of which the list can predict but the first:
    // - `CONFLICT`: a system role, should one ever be reached past the
    //   disabled button.
    // - `ROLE_HAS_OPEN_TASKS`: open tasks still need the role (D-89). This one
    //   opens the list of those tasks (#532) in place of the confirmation.
    // - `ROLE_NAMED_BY_PUBLISHED_DEFINITION`: no open task needs it, but a
    //   published workflow definition names it (D-91 (3), #510). Its message
    //   names each definition, so it is shown as it is.
    // Everything but the second stays in the confirmation. The server's exact
    // wording every time, not ours.
    if (refusal.code === ROLE_HAS_OPEN_TASKS) {
      blocked.value = target
      blockedRefusal.value = refusal.message
      isConfirmOpen.value = false
      isOpenTasksOpen.value = true
      return
    }

    deleteError.value = refusal.message
  } finally {
    isDeleting.value = false
  }
}

/**
 * A role nobody holds while open tasks need it (#508) opens the same list a
 * refused delete does, where the same Reassign sits: one list, one reassign.
 */
function openStranded(role: Role): void {
  blocked.value = role
  blockedRefusal.value = ''
  isOpenTasksOpen.value = true
}

function openReassign(task: OpenTaskNeedingRole): void {
  reassigning.value = task
  isReassignOpen.value = true
}

/**
 * The list is read again rather than the row dropped: a task reassigned to a
 * user may still need the role, when a decision on it is reserved to it, and
 * only the server says which. The roles are read again too, so a role's
 * stranded-tasks notice (#508) shows the count the reassign left, or goes.
 */
async function onReassigned(): Promise<void> {
  reassigning.value = null
  await Promise.all([openTasks.value?.refresh(), roles.refresh()])
}

onMounted(async () => {
  await Promise.all([roles.load(), loadPermissions()])
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Roles</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          Named sets of permissions. Granting a role to a user grants everything in it.
        </p>
      </div>

      <Button v-if="canCreate" @click="openCreate()">New role</Button>
    </div>

    <Alert v-if="roles.error.value" variant="destructive">
      <p>{{ roles.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="roles.load()">Try again</Button>
    </Alert>

    <Alert v-if="permissionsError" variant="destructive">
      The permission catalogue could not be loaded, so permissions cannot be edited:
      {{ permissionsError }}
    </Alert>

    <p v-if="roles.isLoading.value" class="text-sm text-muted-foreground">Loading roles…</p>

    <template v-else-if="!roles.error.value">
      <p v-if="roles.items.value.length === 0" class="text-sm text-muted-foreground">
        No roles to show.
      </p>

      <Table v-else>
        <TableHeader>
          <TableRow>
            <TableHead>Code</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Description</TableHead>
            <TableHead>Permissions</TableHead>
            <TableHead v-if="canUpdate || canDelete" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow v-for="role in roles.items.value" :key="role.id">
            <TableCell class="font-medium">
              <span class="font-mono text-xs">{{ role.roleCode }}</span>
              <Badge v-if="role.isSystem" variant="secondary" class="ml-2">System</Badge>
            </TableCell>
            <TableCell>
              {{ role.name }}
              <!-- The server sends the counts only to a reassigner; checked here
                   as well, like every control on this page. -->
              <RoleStrandedTasksNotice
                v-if="canReassign"
                :role="role"
                :can-list="canDelete"
                @open="openStranded"
              />
            </TableCell>
            <TableCell class="text-muted-foreground">{{ role.description ?? '—' }}</TableCell>
            <TableCell>{{ role.permissions.length }}</TableCell>
            <TableCell v-if="canUpdate || canDelete" class="text-right">
              <div class="flex justify-end gap-2">
                <Button v-if="canUpdate" variant="outline" size="sm" @click="openEdit(role)">
                  Edit
                </Button>
                <Button
                  v-if="canDelete"
                  variant="ghost"
                  size="sm"
                  :disabled="role.isSystem"
                  :title="role.isSystem ? SYSTEM_ROLE_REFUSAL : 'Delete this role'"
                  @click="openDelete(role)"
                >
                  Delete
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <p
        v-if="roles.items.value.some((role) => role.isSystem)"
        class="text-xs text-muted-foreground"
      >
        {{ SYSTEM_ROLE_REFUSAL }}.
      </p>

      <div v-if="roles.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ roles.page.value }} of {{ roles.totalPages.value }} ·
          {{ roles.total.value }} roles
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!roles.hasPrevious.value"
            @click="roles.goToPage(roles.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!roles.hasNext.value"
            @click="roles.goToPage(roles.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <RoleFormDialog
      v-model:open="isFormOpen"
      :role="editing"
      :permissions="permissions"
      @saved="onSaved()"
    />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Delete role"
      :description="`${confirming?.name ?? 'This role'} will be removed. Users who hold it lose the permissions it grants.`"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="confirmDelete()"
    />

    <RoleOpenTasksDialog
      v-if="canDelete"
      ref="openTasks"
      v-model:open="isOpenTasksOpen"
      :role="blocked"
      :refusal="blockedRefusal"
    >
      <template v-if="canReassign" #task-actions="{ task }">
        <Button variant="outline" size="sm" @click="openReassign(task)">Reassign</Button>
      </template>
    </RoleOpenTasksDialog>

    <!-- Beside the list rather than inside its slot: Escape in a dialog nested
         in another's markup would close both. -->
    <ReassignTaskDialog
      v-if="canDelete && canReassign"
      v-model:open="isReassignOpen"
      :task="reassigning"
      :from-role="blocked"
      @reassigned="onReassigned()"
    />
  </section>
</template>
