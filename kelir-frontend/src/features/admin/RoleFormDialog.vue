<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { createRole, updateRole } from '@/api/identity'
import { useFormErrors, type ConflictRule } from '@/composables/useFormErrors'
import type { Permission, Role } from '@/types/identity'

/**
 * Create and edit a role, including the permissions it grants
 * (FR-IDM-002, FR-IDM-004, FR-IDM-005).
 *
 * The permission catalogue is grouped by module because it is a flat list of
 * `module:resource:action` codes on the wire and a hundred ungrouped checkboxes
 * is not a thing anyone can read.
 */
const props = defineProps<{
  role: Role | null
  /** The full catalogue from `GET /identity/permissions`. */
  permissions: Permission[]
}>()

const emit = defineEmits<{ saved: [role: Role] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.role !== null)

const roleCode = ref('')
const name = ref('')
const description = ref('')
const permissionIds = ref<string[]>([])

const isSaving = ref(false)
const submitted = ref(false)

// Only creation can collide: `roleCode` is not part of the update request, so
// the backend has nothing to conflict on when editing.
const conflictRules = computed<ConflictRule[]>(() =>
  isEditing.value ? [] : [{ match: /role code is already in use/i, fields: ['roleCode'] }],
)

const { fieldErrors, formError, report, reset, clearField } = useFormErrors(conflictRules)

/** Grouped by module, and each group sorted, so the list is stable between loads. */
const groupedPermissions = computed(() => {
  const groups = new Map<string, Permission[]>()

  for (const permission of props.permissions) {
    const group = groups.get(permission.module) ?? []
    group.push(permission)
    groups.set(permission.module, group)
  }

  return [...groups.entries()]
    .map(([module, items]) => ({
      module,
      items: [...items].sort((a, b) => a.permissionCode.localeCompare(b.permissionCode)),
    }))
    .sort((a, b) => a.module.localeCompare(b.module))
})

/**
 * The backend does not validate role fields at all — an empty code reaches the
 * database — so these two checks are the only thing standing between a slip and
 * an unusable row.
 */
const localErrors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (!isEditing.value && roleCode.value.trim() === '') {
    found.roleCode = 'Enter a role code'
  }

  if (name.value.trim() === '') {
    found.name = 'Enter a name'
  }

  return found
})

const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...fieldErrors.value,
}))

function loadFromProps(): void {
  const source = props.role

  roleCode.value = source?.roleCode ?? ''
  name.value = source?.name ?? ''
  description.value = source?.description ?? ''
  permissionIds.value = source ? source.permissions.map((permission) => permission.id) : []
  submitted.value = false
  reset()
}

// `immediate` covers being mounted already open, where there is no transition
// to react to and the fields would otherwise stay blank.
watch(
  open,
  (isOpen) => {
    if (isOpen) {
      loadFromProps()
    }
  },
  { immediate: true },
)

function togglePermission(id: string, selected: boolean): void {
  permissionIds.value = selected
    ? [...new Set([...permissionIds.value, id])]
    : permissionIds.value.filter((candidate) => candidate !== id)
}

async function submit(): Promise<void> {
  submitted.value = true
  reset()

  if (Object.keys(localErrors.value).length > 0) {
    return
  }

  isSaving.value = true

  const trimmedDescription = description.value.trim()

  try {
    const saved = props.role
      ? await updateRole(props.role.id, {
          name: name.value.trim(),
          // The backend COALESCEs, so an emptied description cannot be cleared;
          // sending it only when it has content keeps that honest.
          ...(trimmedDescription === '' ? {} : { description: trimmedDescription }),
          permissionIds: permissionIds.value,
        })
      : await createRole({
          roleCode: roleCode.value.trim(),
          name: name.value.trim(),
          description: trimmedDescription === '' ? null : trimmedDescription,
          permissionIds: permissionIds.value,
        })

    emit('saved', saved)
    open.value = false
  } catch (error) {
    // A 403 from `identity:role:update` lands on the form verbatim. The caller
    // is told the change did not happen instead of watching the dialog close
    // over an unchanged role.
    report(error)
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <Dialog
    v-model:open="open"
    :title="isEditing ? 'Edit role' : 'New role'"
    description="A role is a named set of permissions. Granting it to a user grants all of them."
  >
    <form id="role-form" class="space-y-4" novalidate @submit.prevent="submit">
      <Alert v-if="formError" variant="destructive">{{ formError }}</Alert>

      <div class="space-y-2">
        <Label for="role-code">Role code</Label>
        <Input
          id="role-code"
          v-model="roleCode"
          :disabled="isEditing || isSaving"
          :invalid="Boolean(errors.roleCode)"
          described-by="role-code-error"
          placeholder="ROLE-CLERK"
          @update:model-value="clearField('roleCode')"
        />
        <p v-if="isEditing" class="text-xs text-muted-foreground">
          A role code is fixed once the role exists.
        </p>
        <p v-if="errors.roleCode" id="role-code-error" class="text-sm text-destructive">
          {{ errors.roleCode }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="role-name">Name</Label>
        <Input
          id="role-name"
          v-model="name"
          :disabled="isSaving"
          :invalid="Boolean(errors.name)"
          described-by="role-name-error"
          @update:model-value="clearField('name')"
        />
        <p v-if="errors.name" id="role-name-error" class="text-sm text-destructive">
          {{ errors.name }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="role-description">Description</Label>
        <Textarea id="role-description" v-model="description" :disabled="isSaving" :rows="2" />
      </div>

      <fieldset class="space-y-3">
        <legend class="text-sm font-medium leading-none">Permissions</legend>

        <p v-if="permissions.length === 0" class="text-sm text-muted-foreground">
          The permission catalogue could not be loaded, so permissions cannot be changed here.
        </p>

        <div v-else class="max-h-64 space-y-4 overflow-y-auto rounded-md border border-border p-3">
          <div v-for="group in groupedPermissions" :key="group.module" class="space-y-2">
            <p class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {{ group.module }}
            </p>

            <div v-for="permission in group.items" :key="permission.id" class="flex gap-2">
              <Checkbox
                :id="`role-permission-${permission.id}`"
                class="mt-0.5"
                :model-value="permissionIds.includes(permission.id)"
                :disabled="isSaving"
                @update:model-value="togglePermission(permission.id, $event)"
              />
              <Label :for="`role-permission-${permission.id}`" class="font-normal leading-snug">
                <span class="font-mono text-xs">{{ permission.permissionCode }}</span>
                <span v-if="permission.description" class="block text-muted-foreground">
                  {{ permission.description }}
                </span>
              </Label>
            </div>
          </div>
        </div>
      </fieldset>
    </form>

    <template #footer>
      <Button variant="outline" :disabled="isSaving" @click="open = false">Cancel</Button>
      <Button type="submit" form="role-form" :loading="isSaving">
        {{ isEditing ? 'Save changes' : 'Create role' }}
      </Button>
    </template>
  </Dialog>
</template>
