<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { registerExternalSystem, updateExternalSystem } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { useFormErrors } from '@/composables/useFormErrors'
import {
  AUTH_TYPE_LABELS,
  EXTERNAL_SYSTEM_TYPE_LABELS,
  optionsOf,
  type AuthType,
  type ExternalSystem,
  type ExternalSystemType,
  type RetryPolicy,
  type UpdateExternalSystemRequest,
} from '@/types/integration'

import { blankToNull, numberOrUndefined, unplacedErrors } from './form-errors'

/**
 * Registering and editing an external system (FR-INT-001, #520).
 *
 * **One dialog for both**, because the fields are the same bar two: the code is
 * typed on register and read-only after (the backend refuses it on a `PUT`),
 * and the status is offered only on edit.
 *
 * **The status offers `ACTIVE` and `MAINTENANCE` and never `INACTIVE`.**
 * Turning a system off or on is the deactivate and activate verbs, under their
 * own permission; a `PUT` that moved into or out of `INACTIVE` is refused. An
 * inactive system's edit therefore sends no status at all.
 *
 * **The rules are the server's.** Code format, URL shape, ranges — each comes
 * back as a 422 detail on the field it names (`retryPolicy.maxRetries`
 * included), and a detail for a field this form has no input for is listed on
 * the form rather than lost.
 */
const props = defineProps<{ editing: ExternalSystem | null }>()
const emit = defineEmits<{ saved: [system: ExternalSystem] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.editing !== null)
const isInactive = computed(() => props.editing?.status === 'INACTIVE')

const systemCode = ref('')
const systemName = ref('')
const systemType = ref<ExternalSystemType | ''>('')
const baseUrl = ref('')
const authType = ref<AuthType | ''>('')
const timeoutSeconds = ref('')
const maxRetries = ref('')
const initialDelaySeconds = ref('')
const backoffMultiplier = ref('')
const deadLetterAfterAttempts = ref('')
const description = ref('')
const status = ref<'ACTIVE' | 'MAINTENANCE'>('ACTIVE')

const isSaving = ref(false)

/** A duplicate code is a 409 with no details; it belongs on the code input. */
const errors = useFormErrors(() =>
  isEditing.value ? [] : [{ match: /./, fields: ['systemCode'] }],
)

const PLACED = [
  'systemCode',
  'systemName',
  'systemType',
  'baseUrl',
  'authType',
  'timeoutSeconds',
  'retryPolicy.maxRetries',
  'retryPolicy.initialDelaySeconds',
  'retryPolicy.backoffMultiplier',
  'retryPolicy.deadLetterAfterAttempts',
  'description',
  'status',
] as const

const unplaced = computed(() => unplacedErrors(errors.fieldErrors.value, PLACED))

const typeOptions = optionsOf(EXTERNAL_SYSTEM_TYPE_LABELS)
const authOptions = optionsOf(AUTH_TYPE_LABELS)
const STATUS_OPTIONS = [
  { value: 'ACTIVE', label: 'Active' },
  { value: 'MAINTENANCE', label: 'Maintenance' },
]

function fieldError(path: string): string {
  return errors.fieldErrors.value[path] ?? ''
}

function asText(value: number | undefined): string {
  return value === undefined ? '' : String(value)
}

watch(
  open,
  (isOpen) => {
    if (!isOpen) {
      return
    }

    errors.reset()

    const system = props.editing

    systemCode.value = system?.systemCode ?? ''
    systemName.value = system?.systemName ?? ''
    systemType.value = system?.systemType ?? ''
    baseUrl.value = system?.baseUrl ?? ''
    authType.value = system?.authType ?? ''
    timeoutSeconds.value = system ? String(system.timeoutSeconds) : '30'
    maxRetries.value = asText(system?.retryPolicy.maxRetries)
    initialDelaySeconds.value = asText(system?.retryPolicy.initialDelaySeconds)
    backoffMultiplier.value = asText(system?.retryPolicy.backoffMultiplier)
    deadLetterAfterAttempts.value = asText(system?.retryPolicy.deadLetterAfterAttempts)
    description.value = system?.description ?? ''
    status.value = system?.status === 'MAINTENANCE' ? 'MAINTENANCE' : 'ACTIVE'
  },
  { immediate: true },
)

/**
 * The policy as entered. A blank key is left out, and on an edit that removes
 * it: the backend replaces the stored policy whole.
 */
function retryPolicy(): RetryPolicy {
  const policy: RetryPolicy = {}
  const entries: [keyof RetryPolicy, string | number][] = [
    ['maxRetries', maxRetries.value],
    ['initialDelaySeconds', initialDelaySeconds.value],
    ['backoffMultiplier', backoffMultiplier.value],
    ['deadLetterAfterAttempts', deadLetterAfterAttempts.value],
  ]

  for (const [key, raw] of entries) {
    const value = numberOrUndefined(raw)

    if (value !== undefined) {
      policy[key] = value
    }
  }

  return policy
}

async function save(): Promise<void> {
  isSaving.value = true
  errors.reset()

  try {
    const editing = props.editing
    let saved: ExternalSystem

    if (editing) {
      const request: UpdateExternalSystemRequest = {
        systemName: systemName.value.trim(),
        systemType: systemType.value === '' ? null : systemType.value,
        baseUrl: blankToNull(baseUrl.value),
        authType: authType.value === '' ? null : authType.value,
        timeoutSeconds: numberOrUndefined(timeoutSeconds.value),
        retryPolicy: retryPolicy(),
        description: blankToNull(description.value),
      }

      if (editing.status !== 'INACTIVE') {
        request.status = status.value
      }

      saved = await updateExternalSystem(editing.id, request)
    } else {
      saved = await registerExternalSystem({
        systemCode: systemCode.value.trim(),
        systemName: systemName.value.trim(),
        systemType: systemType.value === '' ? undefined : systemType.value,
        baseUrl: blankToNull(baseUrl.value) ?? undefined,
        authType: authType.value === '' ? undefined : authType.value,
        timeoutSeconds: numberOrUndefined(timeoutSeconds.value),
        retryPolicy: retryPolicy(),
        description: blankToNull(description.value) ?? undefined,
      })
    }

    open.value = false
    emit('saved', saved)
  } catch (failure) {
    errors.report(failure)
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <Dialog
    v-model:open="open"
    :title="isEditing ? 'Edit external system' : 'Register external system'"
    class="max-w-2xl"
  >
    <form
      class="grid gap-4 sm:grid-cols-2"
      data-testid="external-system-form"
      novalidate
      @submit.prevent="save"
    >
      <Alert
        v-if="errors.formError.value"
        variant="destructive"
        class="sm:col-span-2"
        data-testid="external-system-form-error"
      >
        {{ errors.formError.value }}
      </Alert>

      <Alert
        v-if="unplaced.length > 0"
        variant="destructive"
        class="sm:col-span-2"
        data-testid="external-system-unplaced-errors"
      >
        <ul class="list-disc pl-4">
          <li v-for="item in unplaced" :key="item.path">{{ item.path }}: {{ item.message }}</li>
        </ul>
      </Alert>

      <div class="space-y-2">
        <Label for="system-code">System code</Label>
        <Input
          id="system-code"
          v-model="systemCode"
          data-testid="system-code"
          placeholder="SAP_ERP"
          autocomplete="off"
          :disabled="isEditing"
          :invalid="!!fieldError('systemCode')"
          described-by="system-code-error"
        />
        <p v-if="isEditing" class="text-xs text-muted-foreground">
          A system code cannot change once it is registered.
        </p>
        <p
          v-if="fieldError('systemCode')"
          id="system-code-error"
          class="text-xs text-destructive"
          data-testid="systemCode-error"
        >
          {{ fieldError('systemCode') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="system-name">Name</Label>
        <Input
          id="system-name"
          v-model="systemName"
          data-testid="system-name"
          placeholder="Corporate ERP"
          :invalid="!!fieldError('systemName')"
          described-by="system-name-error"
        />
        <p
          v-if="fieldError('systemName')"
          id="system-name-error"
          class="text-xs text-destructive"
          data-testid="systemName-error"
        >
          {{ fieldError('systemName') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="system-type">Type</Label>
        <Select
          id="system-type"
          v-model="systemType"
          data-testid="system-type"
          placeholder="Not set"
          :options="typeOptions"
          :invalid="!!fieldError('systemType')"
        />
        <p
          v-if="fieldError('systemType')"
          class="text-xs text-destructive"
          data-testid="systemType-error"
        >
          {{ fieldError('systemType') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="system-auth-type">Authentication</Label>
        <Select
          id="system-auth-type"
          v-model="authType"
          data-testid="system-auth-type"
          placeholder="Not set"
          :options="authOptions"
          :invalid="!!fieldError('authType')"
        />
        <p
          v-if="fieldError('authType')"
          class="text-xs text-destructive"
          data-testid="authType-error"
        >
          {{ fieldError('authType') }}
        </p>
      </div>

      <div class="space-y-2 sm:col-span-2">
        <Label for="system-base-url">Base URL</Label>
        <Input
          id="system-base-url"
          v-model="baseUrl"
          type="url"
          data-testid="system-base-url"
          placeholder="https://erp.example.com/api"
          autocomplete="off"
          :invalid="!!fieldError('baseUrl')"
          described-by="system-base-url-hint system-base-url-error"
        />
        <!-- Said up front because the backend refuses it: a URL is not where a
             password goes, and `user:password@` in one would be a secret stored
             in plain sight. -->
        <p id="system-base-url-hint" class="text-xs text-muted-foreground">
          http or https. Never put a user name or password in the URL; they belong in a credential
          reference.
        </p>
        <p
          v-if="fieldError('baseUrl')"
          id="system-base-url-error"
          class="text-xs text-destructive"
          data-testid="baseUrl-error"
        >
          {{ fieldError('baseUrl') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="system-timeout">Timeout (seconds)</Label>
        <Input
          id="system-timeout"
          v-model="timeoutSeconds"
          type="number"
          data-testid="system-timeout"
          :invalid="!!fieldError('timeoutSeconds')"
        />
        <p
          v-if="fieldError('timeoutSeconds')"
          class="text-xs text-destructive"
          data-testid="timeoutSeconds-error"
        >
          {{ fieldError('timeoutSeconds') }}
        </p>
      </div>

      <div v-if="isEditing" class="space-y-2">
        <Label for="system-status">Status</Label>
        <Select
          v-if="!isInactive"
          id="system-status"
          v-model="status"
          data-testid="system-status"
          :options="STATUS_OPTIONS"
          :invalid="!!fieldError('status')"
        />
        <!-- Inactive is left through the Activate button, not this form. -->
        <p v-else class="text-sm text-muted-foreground" data-testid="system-status-inactive">
          Inactive. Use Activate on the system's page to turn it back on.
        </p>
        <p v-if="fieldError('status')" class="text-xs text-destructive" data-testid="status-error">
          {{ fieldError('status') }}
        </p>
      </div>

      <fieldset class="grid gap-4 rounded-md border border-border p-4 sm:col-span-2 sm:grid-cols-2">
        <legend class="px-1 text-sm font-medium">Retry policy</legend>
        <p class="text-xs text-muted-foreground sm:col-span-2">
          Leave a value blank to use the default.
        </p>

        <div class="space-y-2">
          <Label for="retry-max-retries">Max retries</Label>
          <Input
            id="retry-max-retries"
            v-model="maxRetries"
            type="number"
            data-testid="retry-max-retries"
            :invalid="!!fieldError('retryPolicy.maxRetries')"
          />
          <p
            v-if="fieldError('retryPolicy.maxRetries')"
            class="text-xs text-destructive"
            data-testid="retryPolicy.maxRetries-error"
          >
            {{ fieldError('retryPolicy.maxRetries') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="retry-initial-delay">Initial delay (seconds)</Label>
          <Input
            id="retry-initial-delay"
            v-model="initialDelaySeconds"
            type="number"
            data-testid="retry-initial-delay"
            :invalid="!!fieldError('retryPolicy.initialDelaySeconds')"
          />
          <p
            v-if="fieldError('retryPolicy.initialDelaySeconds')"
            class="text-xs text-destructive"
            data-testid="retryPolicy.initialDelaySeconds-error"
          >
            {{ fieldError('retryPolicy.initialDelaySeconds') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="retry-backoff">Backoff multiplier</Label>
          <Input
            id="retry-backoff"
            v-model="backoffMultiplier"
            type="number"
            data-testid="retry-backoff"
            :invalid="!!fieldError('retryPolicy.backoffMultiplier')"
          />
          <p
            v-if="fieldError('retryPolicy.backoffMultiplier')"
            class="text-xs text-destructive"
            data-testid="retryPolicy.backoffMultiplier-error"
          >
            {{ fieldError('retryPolicy.backoffMultiplier') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="retry-dead-letter">Dead-letter after attempts</Label>
          <Input
            id="retry-dead-letter"
            v-model="deadLetterAfterAttempts"
            type="number"
            data-testid="retry-dead-letter"
            :invalid="!!fieldError('retryPolicy.deadLetterAfterAttempts')"
          />
          <p
            v-if="fieldError('retryPolicy.deadLetterAfterAttempts')"
            class="text-xs text-destructive"
            data-testid="retryPolicy.deadLetterAfterAttempts-error"
          >
            {{ fieldError('retryPolicy.deadLetterAfterAttempts') }}
          </p>
        </div>
      </fieldset>

      <div class="space-y-2 sm:col-span-2">
        <Label for="system-description">Description</Label>
        <Textarea
          id="system-description"
          v-model="description"
          data-testid="system-description"
          :invalid="!!fieldError('description')"
        />
        <p
          v-if="fieldError('description')"
          class="text-xs text-destructive"
          data-testid="description-error"
        >
          {{ fieldError('description') }}
        </p>
      </div>

      <div class="flex justify-end gap-2 sm:col-span-2">
        <Button type="button" variant="outline" :disabled="isSaving" @click="open = false">
          Cancel
        </Button>
        <Button type="submit" :disabled="isSaving" data-testid="save-external-system">
          {{ isSaving ? 'Saving…' : isEditing ? 'Save' : 'Register' }}
        </Button>
      </div>
    </form>
  </Dialog>
</template>
