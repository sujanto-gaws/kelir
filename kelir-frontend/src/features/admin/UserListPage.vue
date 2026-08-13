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
import { deactivateUser, listRoles, listUsers } from '@/api/identity'
import { toApiError } from '@/api/client'
import { usePaginatedList } from '@/composables/usePaginatedList'
import { useAuthStore } from '@/stores/auth'
import { USER_STATUS_LABELS, type Role, type User } from '@/types/identity'
import ConfirmDialog from './ConfirmDialog.vue'
import UserFormDialog from './UserFormDialog.vue'

/**
 * User administration (FR-IDM-001, FR-IDM-003).
 *
 * Every control is gated on the permission the backend requires for the call
 * behind it. That is presentation only — `identity/service.rs` re-checks each
 * one and is what actually decides — but a button that can only ever produce a
 * 403 is worse than no button.
 */
const auth = useAuthStore()

const canCreate = computed(() => auth.can('identity:user:create'))
const canUpdate = computed(() => auth.can('identity:user:update'))
const canDelete = computed(() => auth.can('identity:user:delete'))
/** The role catalogue is behind its own permission, not the user one. */
const canReadRoles = computed(() => auth.can('identity:role:read'))

const users = usePaginatedList<User>(listUsers)

const roles = ref<Role[]>([])
const rolesError = ref('')

const isFormOpen = ref(false)
const editing = ref<User | null>(null)

const confirming = ref<User | null>(null)
const isConfirmOpen = ref(false)
const isDeactivating = ref(false)
const deactivateError = ref('')

/**
 * The backend refuses to deactivate the caller's own account with a 400, so the
 * UI refuses first — for the same reason, and with the same outcome, but
 * without spending a request to say so.
 */
function isSelf(user: User): boolean {
  return auth.user?.id === user.id
}

function statusVariant(status: User['status']): 'default' | 'secondary' | 'destructive' {
  if (status === 'ACTIVE') {
    return 'default'
  }

  return status === 'LOCKED' ? 'destructive' : 'secondary'
}

async function loadRoles(): Promise<void> {
  if (!canReadRoles.value) {
    // Without `identity:role:read` the catalogue is unreadable, so the form
    // offers no role picker rather than an empty one that looks broken.
    return
  }

  rolesError.value = ''

  try {
    // One page at the backend's maximum: the role catalogue is small and a
    // paged picker inside a form would be a worse trade than a single call.
    roles.value = (await listRoles({ page: 1, pageSize: 100 })).items
  } catch (error) {
    rolesError.value = toApiError(error).message
  }
}

function openCreate(): void {
  editing.value = null
  isFormOpen.value = true
}

function openEdit(user: User): void {
  editing.value = user
  isFormOpen.value = true
}

function openDeactivate(user: User): void {
  confirming.value = user
  deactivateError.value = ''
  isConfirmOpen.value = true
}

async function onSaved(): Promise<void> {
  await users.refresh()
}

async function confirmDeactivate(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isDeactivating.value = true
  deactivateError.value = ''

  try {
    await deactivateUser(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await users.refresh()
  } catch (error) {
    // Surfaced in the dialog rather than swallowed — a refusal the user cannot
    // see is indistinguishable from a button that does nothing.
    deactivateError.value = toApiError(error).message
  } finally {
    isDeactivating.value = false
  }
}

onMounted(async () => {
  await Promise.all([users.load(), loadRoles()])
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Users</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          Accounts that can sign in, and the roles that decide what they may do.
        </p>
      </div>

      <Button v-if="canCreate" @click="openCreate()">New user</Button>
    </div>

    <Alert v-if="users.error.value" variant="destructive">
      <p>{{ users.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="users.load()">Try again</Button>
    </Alert>

    <Alert v-if="rolesError" variant="destructive">
      Roles could not be loaded, so the role picker is unavailable: {{ rolesError }}
    </Alert>

    <p v-if="users.isLoading.value" class="text-sm text-muted-foreground">Loading users…</p>

    <template v-else-if="!users.error.value">
      <p v-if="users.items.value.length === 0" class="text-sm text-muted-foreground">
        No users to show.
      </p>

      <Table v-else>
        <TableHeader>
          <TableRow>
            <TableHead>Username</TableHead>
            <TableHead>Display name</TableHead>
            <TableHead>Email</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Roles</TableHead>
            <TableHead v-if="canUpdate || canDelete" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow v-for="user in users.items.value" :key="user.id">
            <TableCell class="font-medium">{{ user.username }}</TableCell>
            <TableCell>{{ user.displayName }}</TableCell>
            <TableCell>{{ user.email }}</TableCell>
            <TableCell>
              <Badge :variant="statusVariant(user.status)">
                {{ USER_STATUS_LABELS[user.status] }}
              </Badge>
            </TableCell>
            <TableCell>
              <span v-if="user.roles.length === 0" class="text-muted-foreground">None</span>
              <span v-else class="flex flex-wrap gap-1">
                <Badge v-for="role in user.roles" :key="role.id" variant="outline">
                  {{ role.name }}
                </Badge>
              </span>
            </TableCell>
            <TableCell v-if="canUpdate || canDelete" class="text-right">
              <div class="flex justify-end gap-2">
                <Button v-if="canUpdate" variant="outline" size="sm" @click="openEdit(user)">
                  Edit
                </Button>
                <Button
                  v-if="canDelete"
                  variant="ghost"
                  size="sm"
                  :disabled="isSelf(user)"
                  :title="
                    isSelf(user) ? 'You cannot deactivate your own account' : 'Deactivate this user'
                  "
                  @click="openDeactivate(user)"
                >
                  Deactivate
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="users.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ users.page.value }} of {{ users.totalPages.value }} ·
          {{ users.total.value }} users
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!users.hasPrevious.value"
            @click="users.goToPage(users.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!users.hasNext.value"
            @click="users.goToPage(users.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <UserFormDialog v-model:open="isFormOpen" :user="editing" :roles="roles" @saved="onSaved()" />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Deactivate user"
      :description="`${confirming?.displayName ?? 'This user'} will no longer be able to sign in, and will be removed from this list.`"
      confirm-label="Deactivate"
      :error="deactivateError"
      :pending="isDeactivating"
      @confirm="confirmDeactivate()"
    />
  </section>
</template>
