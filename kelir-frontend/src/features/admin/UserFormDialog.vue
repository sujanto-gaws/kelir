<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { createUser, updateUser } from '@/api/identity'
import { useFormErrors, type ConflictRule } from '@/composables/useFormErrors'
import {
  USER_STATUS_LABELS,
  type Role,
  type UpdateUserRequest,
  type User,
  type UserStatus,
} from '@/types/identity'

/**
 * Create and edit a user (FR-IDM-001, FR-IDM-003).
 *
 * One dialog for both, because the fields are almost the same set and keeping
 * them together is what stops the two drifting apart. `user` decides the mode:
 * null creates, a record edits.
 *
 * Validation is the backend's. What is checked here is only what would
 * otherwise cost a round trip to learn (a blank required field); everything
 * that needs to know about other rows — a duplicate username, a duplicate
 * email — is left to the server and shown against the field it belongs to.
 */
const props = defineProps<{
  user: User | null
  /** The catalogue for the role multi-select, loaded once by the page. */
  roles: Role[]
}>()

const emit = defineEmits<{ saved: [user: User] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.user !== null)

const username = ref('')
const email = ref('')
const displayName = ref('')
const password = ref('')
const departmentId = ref('')
const status = ref<UserStatus>('ACTIVE')
const roleIds = ref<string[]>([])

const isSaving = ref(false)
const submitted = ref(false)

/**
 * The backend answers a duplicate with 409 and no `details`, and its message
 * names both candidates without saying which collided — so on create the error
 * goes against both inputs. On edit the username cannot be changed, so the
 * email is the only field that could have caused it.
 */
const conflictRules = computed<ConflictRule[]>(() => [
  {
    match: /already in use/i,
    fields: isEditing.value ? ['email'] : ['username', 'email'],
  },
])

const { fieldErrors, formError, report, reset, clearField } = useFormErrors(conflictRules)

const statusOptions = Object.entries(USER_STATUS_LABELS).map(([value, label]) => ({
  value,
  label,
}))

const localErrors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (!isEditing.value && username.value.trim() === '') {
    found.username = 'Enter a username'
  }

  if (email.value.trim() === '') {
    found.email = 'Enter an email address'
  }

  if (displayName.value.trim() === '') {
    found.displayName = 'Enter a display name'
  }

  // Mirrors MIN_PASSWORD_LENGTH in the backend; the server re-checks it.
  if (!isEditing.value && password.value.length < 12) {
    found.password = 'Password must be at least 12 characters'
  }

  return found
})

// The server has seen the actual values, so where both have an opinion its
// answer is the one worth showing.
const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...fieldErrors.value,
}))

function loadFromProps(): void {
  const source = props.user

  username.value = source?.username ?? ''
  email.value = source?.email ?? ''
  displayName.value = source?.displayName ?? ''
  departmentId.value = source?.departmentId ?? ''
  status.value = source?.status ?? 'ACTIVE'
  roleIds.value = source ? source.roles.map((role) => role.id) : []
  password.value = ''
  submitted.value = false
  reset()
}

// Re-seed whenever the dialog opens, so a cancelled edit does not leak into the
// next one. `immediate` covers being mounted already open, where there is no
// transition to react to and the fields would otherwise stay blank.
watch(
  open,
  (isOpen) => {
    if (isOpen) {
      loadFromProps()
    }
  },
  { immediate: true },
)

function toggleRole(id: string, selected: boolean): void {
  roleIds.value = selected
    ? [...new Set([...roleIds.value, id])]
    : roleIds.value.filter((candidate) => candidate !== id)
}

function buildUpdate(): UpdateUserRequest {
  const request: UpdateUserRequest = {
    email: email.value.trim(),
    displayName: displayName.value.trim(),
    status: status.value,
    // Always sent: the backend replaces the grants wholesale when the key is
    // present and leaves them untouched when it is absent, so omitting it would
    // make deselecting every role impossible.
    roleIds: roleIds.value,
  }

  const department = departmentId.value.trim()

  // The backend COALESCEs, so sending null cannot clear a department — only a
  // real value is worth sending.
  if (department !== '') {
    request.departmentId = department
  }

  return request
}

async function submit(): Promise<void> {
  submitted.value = true
  reset()

  if (Object.keys(localErrors.value).length > 0) {
    return
  }

  isSaving.value = true

  try {
    const department = departmentId.value.trim()

    const saved = props.user
      ? await updateUser(props.user.id, buildUpdate())
      : await createUser({
          username: username.value.trim(),
          email: email.value.trim(),
          displayName: displayName.value.trim(),
          password: password.value,
          departmentId: department === '' ? null : department,
          roleIds: roleIds.value,
        })

    emit('saved', saved)
    open.value = false
  } catch (error) {
    // Field-level where the backend named a field, on the form otherwise — a
    // 403 or a network failure still gets said out loud.
    report(error)
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <Dialog
    v-model:open="open"
    :title="isEditing ? 'Edit user' : 'New user'"
    :description="
      isEditing
        ? 'Change this account’s details and the roles it holds.'
        : 'Create an account and grant it roles.'
    "
  >
    <form id="user-form" class="space-y-4" novalidate @submit.prevent="submit">
      <Alert v-if="formError" variant="destructive">{{ formError }}</Alert>

      <div class="space-y-2">
        <Label for="user-username">Username</Label>
        <Input
          id="user-username"
          v-model="username"
          :disabled="isEditing || isSaving"
          :invalid="Boolean(errors.username)"
          described-by="user-username-error"
          @update:model-value="clearField('username')"
        />
        <p v-if="isEditing" class="text-xs text-muted-foreground">
          A username cannot be changed after the account is created.
        </p>
        <p v-if="errors.username" id="user-username-error" class="text-sm text-destructive">
          {{ errors.username }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="user-email">Email</Label>
        <Input
          id="user-email"
          v-model="email"
          type="email"
          :disabled="isSaving"
          :invalid="Boolean(errors.email)"
          described-by="user-email-error"
          @update:model-value="clearField('email')"
        />
        <p v-if="errors.email" id="user-email-error" class="text-sm text-destructive">
          {{ errors.email }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="user-display-name">Display name</Label>
        <Input
          id="user-display-name"
          v-model="displayName"
          :disabled="isSaving"
          :invalid="Boolean(errors.displayName)"
          described-by="user-display-name-error"
          @update:model-value="clearField('displayName')"
        />
        <p v-if="errors.displayName" id="user-display-name-error" class="text-sm text-destructive">
          {{ errors.displayName }}
        </p>
      </div>

      <div v-if="!isEditing" class="space-y-2">
        <Label for="user-password">Initial password</Label>
        <Input
          id="user-password"
          v-model="password"
          type="password"
          autocomplete="new-password"
          :disabled="isSaving"
          :invalid="Boolean(errors.password)"
          described-by="user-password-error"
          @update:model-value="clearField('password')"
        />
        <p v-if="errors.password" id="user-password-error" class="text-sm text-destructive">
          {{ errors.password }}
        </p>
      </div>

      <div v-if="isEditing" class="space-y-2">
        <Label for="user-status">Status</Label>
        <Select
          id="user-status"
          v-model="status"
          :options="statusOptions"
          :disabled="isSaving"
          described-by="user-status-hint"
        />
        <p id="user-status-hint" class="text-xs text-muted-foreground">
          Only an active account can sign in. Any other status ends the account’s sessions.
        </p>
      </div>

      <div class="space-y-2">
        <Label for="user-department">Department ID</Label>
        <Input
          id="user-department"
          v-model="departmentId"
          :disabled="isSaving"
          placeholder="Optional"
          described-by="user-department-hint"
        />
        <p id="user-department-hint" class="text-xs text-muted-foreground">
          A department UUID. There is no department directory yet, so this is entered by hand and
          cannot be cleared once set.
        </p>
      </div>

      <fieldset class="space-y-2">
        <legend class="text-sm font-medium leading-none">Roles</legend>

        <p v-if="roles.length === 0" class="text-sm text-muted-foreground">
          No roles are available to grant.
        </p>

        <div v-else class="max-h-40 space-y-2 overflow-y-auto rounded-md border border-border p-3">
          <div v-for="role in roles" :key="role.id" class="flex items-center gap-2">
            <Checkbox
              :id="`user-role-${role.id}`"
              :model-value="roleIds.includes(role.id)"
              :disabled="isSaving"
              @update:model-value="toggleRole(role.id, $event)"
            />
            <Label :for="`user-role-${role.id}`" class="font-normal">
              {{ role.name }}
              <span class="text-muted-foreground">({{ role.roleCode }})</span>
            </Label>
          </div>
        </div>
      </fieldset>
    </form>

    <template #footer>
      <Button variant="outline" :disabled="isSaving" @click="open = false">Cancel</Button>
      <Button type="submit" form="user-form" :loading="isSaving">
        {{ isEditing ? 'Save changes' : 'Create user' }}
      </Button>
    </template>
  </Dialog>
</template>
