<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

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
import type { Permission, Role } from '@/types/identity'
import ConfirmDialog from './ConfirmDialog.vue'
import RoleFormDialog from './RoleFormDialog.vue'

/**
 * Role administration (FR-IDM-002, FR-IDM-004, FR-IDM-005).
 */
const auth = useAuthStore()

const canCreate = computed(() => auth.can('identity:role:create'))
const canUpdate = computed(() => auth.can('identity:role:update'))
const canDelete = computed(() => auth.can('identity:role:delete'))

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
    // Includes the 409 for a system role, should one ever be reached — the
    // server's exact wording, not ours.
    deleteError.value = toApiError(error).message
  } finally {
    isDeleting.value = false
  }
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
            <TableCell>{{ role.name }}</TableCell>
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
  </section>
</template>
