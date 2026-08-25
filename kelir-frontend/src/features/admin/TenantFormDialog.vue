<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { createTenant, updateTenant } from '@/api/organization'
import { useFormErrors, type ConflictRule } from '@/composables/useFormErrors'
import { TENANT_STATUS_LABELS, type Tenant, type TenantStatus } from '@/types/organization'

/**
 * Create and edit a tenant (FR-ORG-001).
 *
 * **Creating asks for an administrator and editing does not**, and that is the
 * shape of the endpoint rather than a UI choice. A tenant with no user is a row
 * nobody can sign in to, so the backend creates both in one transaction and
 * this form collects both. Once the tenant exists, its people are managed from
 * inside it — this dialog never edits them.
 */
const props = defineProps<{ tenant: Tenant | null }>()

const emit = defineEmits<{ saved: [tenant: Tenant] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.tenant !== null)

const tenantCode = ref('')
const name = ref('')
const status = ref<TenantStatus>('ACTIVE')

const adminUsername = ref('')
const adminEmail = ref('')
const adminDisplayName = ref('')
const adminPassword = ref('')

const isSaving = ref(false)
const submitted = ref(false)

const statusOptions = (Object.keys(TENANT_STATUS_LABELS) as TenantStatus[]).map((value) => ({
  value,
  label: TENANT_STATUS_LABELS[value],
}))

/**
 * Only creation can collide, and it can collide on three different things: the
 * tenant code, or the administrator's username or email — the last two through
 * the identity module's own conflict, whose message says "username or email"
 * without saying which.
 *
 * `tenantCode` is not part of the update request, so editing has nothing to
 * conflict on.
 */
const conflictRules = computed<ConflictRule[]>(() =>
  isEditing.value
    ? []
    : [
        { match: /tenant code is already in use/i, fields: ['tenantCode'] },
        {
          match: /username or email/i,
          fields: ['administrator.username', 'administrator.email'],
        },
      ],
)

const { fieldErrors, formError, report, reset, clearField } = useFormErrors(conflictRules)

/**
 * Mirrors the backend's own rules so a slip costs no round trip. The backend
 * re-checks every one of these and its answer wins — these exist to make the
 * form usable, not to be the validation.
 */
const localErrors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (name.value.trim() === '') {
    found.name = 'Enter a name'
  }

  if (isEditing.value) {
    return found
  }

  const code = tenantCode.value.trim().toUpperCase()

  if (code === '') {
    found.tenantCode = 'Enter a tenant code'
  } else if (!/^[A-Z0-9_-]{2,64}$/.test(code)) {
    // Letters, digits, dash and underscore only — this value is read out over
    // the phone and typed into a login form, so anything a person cannot see is
    // a support call rather than a feature.
    found.tenantCode = 'Use 2–64 letters, digits, dashes or underscores'
  }

  if (adminUsername.value.trim() === '') {
    found['administrator.username'] = 'Enter a username'
  }

  if (!adminEmail.value.includes('@')) {
    found['administrator.email'] = 'Enter a valid email address'
  }

  if (adminDisplayName.value.trim() === '') {
    found['administrator.displayName'] = 'Enter a display name'
  }

  if (adminPassword.value.length < 12) {
    found['administrator.password'] = 'Use at least 12 characters'
  }

  return found
})

// The server has seen the real values, so where both have an opinion its answer
// is the one worth showing.
const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...fieldErrors.value,
}))

function loadFromProps(): void {
  const source = props.tenant

  tenantCode.value = source?.tenantCode ?? ''
  name.value = source?.name ?? ''
  status.value = source?.status ?? 'ACTIVE'

  adminUsername.value = ''
  adminEmail.value = ''
  adminDisplayName.value = ''
  adminPassword.value = ''

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

async function submit(): Promise<void> {
  submitted.value = true
  reset()

  if (Object.keys(localErrors.value).length > 0) {
    return
  }

  isSaving.value = true

  try {
    const saved = props.tenant
      ? await updateTenant(props.tenant.id, {
          name: name.value.trim(),
          status: status.value,
        })
      : await createTenant({
          tenantCode: tenantCode.value.trim().toUpperCase(),
          name: name.value.trim(),
          administrator: {
            username: adminUsername.value.trim(),
            email: adminEmail.value.trim(),
            displayName: adminDisplayName.value.trim(),
            password: adminPassword.value,
          },
        })

    emit('saved', saved)
    open.value = false
  } catch (error) {
    // Includes the 403 a caller outside the administering tenant gets, and the
    // 400 refusing to suspend the tenant the request came from. Both land on
    // the form in the backend's own words rather than closing the dialog over
    // an unchanged tenant.
    report(error)
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <Dialog
    v-model:open="open"
    :title="isEditing ? 'Edit tenant' : 'New tenant'"
    :description="
      isEditing
        ? 'Rename the tenant, or take it offline. Suspending it ends its users’ sessions.'
        : 'A tenant is created together with the administrator who can sign in to it.'
    "
  >
    <form id="tenant-form" class="space-y-4" novalidate @submit.prevent="submit">
      <Alert v-if="formError" variant="destructive">{{ formError }}</Alert>

      <div class="space-y-2">
        <Label for="tenant-code">Tenant code</Label>
        <Input
          id="tenant-code"
          v-model="tenantCode"
          :disabled="isEditing || isSaving"
          :invalid="Boolean(errors.tenantCode)"
          described-by="tenant-code-error"
          placeholder="TNT-001"
          @update:model-value="clearField('tenantCode')"
        />
        <p v-if="isEditing" class="text-xs text-muted-foreground">
          Fixed once the tenant exists — its users sign in with it.
        </p>
        <p v-if="errors.tenantCode" id="tenant-code-error" class="text-sm text-destructive">
          {{ errors.tenantCode }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="tenant-name">Name</Label>
        <Input
          id="tenant-name"
          v-model="name"
          :disabled="isSaving"
          :invalid="Boolean(errors.name)"
          described-by="tenant-name-error"
          @update:model-value="clearField('name')"
        />
        <p v-if="errors.name" id="tenant-name-error" class="text-sm text-destructive">
          {{ errors.name }}
        </p>
      </div>

      <div v-if="isEditing" class="space-y-2">
        <Label for="tenant-status">Status</Label>
        <Select
          id="tenant-status"
          v-model="status"
          :options="statusOptions"
          :disabled="isSaving || tenant?.isDefault"
        />
        <p v-if="tenant?.isDefault" class="text-xs text-muted-foreground">
          This is the tenant you administer from, so it cannot be taken offline.
        </p>
        <p v-else-if="status !== 'ACTIVE'" class="text-xs text-muted-foreground">
          Its {{ tenant?.userCount }} user(s) will be signed out and cannot sign in again until it
          is active.
        </p>
      </div>

      <fieldset v-if="!isEditing" class="space-y-4">
        <legend class="text-sm font-medium leading-none">First administrator</legend>
        <p class="text-xs text-muted-foreground">
          They must change this password after signing in. Everything else in the tenant is set up
          by them, not here.
        </p>

        <div class="space-y-2">
          <Label for="tenant-admin-username">Username</Label>
          <Input
            id="tenant-admin-username"
            v-model="adminUsername"
            autocomplete="off"
            :disabled="isSaving"
            :invalid="Boolean(errors['administrator.username'])"
            described-by="tenant-admin-username-error"
            @update:model-value="clearField('administrator.username')"
          />
          <p
            v-if="errors['administrator.username']"
            id="tenant-admin-username-error"
            class="text-sm text-destructive"
          >
            {{ errors['administrator.username'] }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="tenant-admin-email">Email</Label>
          <Input
            id="tenant-admin-email"
            v-model="adminEmail"
            autocomplete="off"
            :disabled="isSaving"
            :invalid="Boolean(errors['administrator.email'])"
            described-by="tenant-admin-email-error"
            @update:model-value="clearField('administrator.email')"
          />
          <p
            v-if="errors['administrator.email']"
            id="tenant-admin-email-error"
            class="text-sm text-destructive"
          >
            {{ errors['administrator.email'] }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="tenant-admin-display-name">Display name</Label>
          <Input
            id="tenant-admin-display-name"
            v-model="adminDisplayName"
            :disabled="isSaving"
            :invalid="Boolean(errors['administrator.displayName'])"
            described-by="tenant-admin-display-name-error"
            @update:model-value="clearField('administrator.displayName')"
          />
          <p
            v-if="errors['administrator.displayName']"
            id="tenant-admin-display-name-error"
            class="text-sm text-destructive"
          >
            {{ errors['administrator.displayName'] }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="tenant-admin-password">Password</Label>
          <Input
            id="tenant-admin-password"
            v-model="adminPassword"
            type="password"
            autocomplete="new-password"
            :disabled="isSaving"
            :invalid="Boolean(errors['administrator.password'])"
            described-by="tenant-admin-password-error"
            @update:model-value="clearField('administrator.password')"
          />
          <p
            v-if="errors['administrator.password']"
            id="tenant-admin-password-error"
            class="text-sm text-destructive"
          >
            {{ errors['administrator.password'] }}
          </p>
        </div>
      </fieldset>
    </form>

    <template #footer>
      <Button variant="outline" :disabled="isSaving" @click="open = false">Cancel</Button>
      <Button type="submit" form="tenant-form" :loading="isSaving">
        {{ isEditing ? 'Save changes' : 'Create tenant' }}
      </Button>
    </template>
  </Dialog>
</template>
