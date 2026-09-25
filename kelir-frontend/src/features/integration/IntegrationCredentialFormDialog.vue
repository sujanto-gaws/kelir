<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { createIntegrationCredential, updateIntegrationCredential } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { useFormErrors } from '@/composables/useFormErrors'
import {
  AUTH_TYPE_LABELS,
  optionsOf,
  type AuthType,
  type IntegrationCredential,
} from '@/types/integration'

import { blankToNull, unplacedErrors } from './form-errors'

/**
 * Adding and editing a credential *reference* (FR-INT-001 AC-5, #520).
 *
 * **This form asks for where a secret lives**, a vault path or an environment
 * variable's name, not the value. So the input
 * is a plain text box, not a password field: a masked input says *type your
 * secret here*, and the one thing this form must never invite is someone
 * pasting an API key into it. The label, placeholder and hint all say
 * *reference*, and the backend refuses anything without the shape of
 * `vault://…` or `env://NAME` with `NOT_A_SECRET_REFERENCE`. It checks shape
 * only: a secret typed as a path segment has the right shape (#552).
 */
const props = defineProps<{ systemId: string; editing: IntegrationCredential | null }>()
const emit = defineEmits<{ saved: [credential: IntegrationCredential] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.editing !== null)

const credentialType = ref<AuthType | ''>('')
const secretReference = ref('')
const validFrom = ref('')
const validTo = ref('')
const isActive = ref(true)
const isSaving = ref(false)

const errors = useFormErrors()

const PLACED = ['credentialType', 'secretReference', 'validFrom', 'validTo', 'isActive'] as const
const unplaced = computed(() => unplacedErrors(errors.fieldErrors.value, PLACED))

const typeOptions = optionsOf(AUTH_TYPE_LABELS)

function fieldError(field: string): string {
  return errors.fieldErrors.value[field] ?? ''
}

watch(
  open,
  (isOpen) => {
    if (!isOpen) {
      return
    }

    errors.reset()
    credentialType.value = props.editing?.credentialType ?? ''
    secretReference.value = props.editing?.secretReference ?? ''
    validFrom.value = props.editing?.validFrom ?? ''
    validTo.value = props.editing?.validTo ?? ''
    isActive.value = props.editing?.isActive ?? true
  },
  { immediate: true },
)

async function save(): Promise<void> {
  isSaving.value = true
  errors.reset()

  // A blank type is sent as absent on create, where the backend names it as
  // required against the field, rather than refused here in other words.
  const type = credentialType.value === '' ? undefined : credentialType.value

  try {
    const saved = props.editing
      ? await updateIntegrationCredential(props.systemId, props.editing.id, {
          credentialType: type,
          secretReference: secretReference.value.trim(),
          validFrom: blankToNull(validFrom.value),
          validTo: blankToNull(validTo.value),
          isActive: isActive.value,
        })
      : await createIntegrationCredential(props.systemId, {
          credentialType: type as AuthType,
          secretReference: secretReference.value.trim(),
          validFrom: blankToNull(validFrom.value),
          validTo: blankToNull(validTo.value),
          isActive: isActive.value,
        })

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
    :title="isEditing ? 'Edit credential reference' : 'Add credential reference'"
  >
    <form class="space-y-4" data-testid="credential-form" novalidate @submit.prevent="save">
      <Alert
        v-if="errors.formError.value"
        variant="destructive"
        data-testid="credential-form-error"
      >
        {{ errors.formError.value }}
      </Alert>

      <Alert
        v-if="unplaced.length > 0"
        variant="destructive"
        data-testid="credential-unplaced-errors"
      >
        <ul class="list-disc pl-4">
          <li v-for="item in unplaced" :key="item.path">{{ item.path }}: {{ item.message }}</li>
        </ul>
      </Alert>

      <div class="space-y-2">
        <Label for="credential-type">Credential type</Label>
        <Select
          id="credential-type"
          v-model="credentialType"
          data-testid="credential-type"
          placeholder="Choose a type"
          :options="typeOptions"
          :invalid="!!fieldError('credentialType')"
        />
        <p
          v-if="fieldError('credentialType')"
          class="text-xs text-destructive"
          data-testid="credentialType-error"
        >
          {{ fieldError('credentialType') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="credential-secret-reference">Secret reference</Label>
        <!-- type="text", deliberately: see the component comment. -->
        <Input
          id="credential-secret-reference"
          v-model="secretReference"
          type="text"
          data-testid="credential-secret-reference"
          placeholder="vault://kelir/erp/api-key or env://ERP_API_KEY"
          autocomplete="off"
          :invalid="!!fieldError('secretReference')"
          described-by="credential-secret-reference-hint"
        />
        <p
          id="credential-secret-reference-hint"
          class="text-xs text-muted-foreground"
          data-testid="credential-secret-reference-hint"
        >
          Where the secret is kept, not the secret itself: a vault path such as
          <code>vault://kelir/erp/api-key</code> or an environment variable such as
          <code>env://ERP_API_KEY</code>. Never paste a password or key here.
        </p>
        <p
          v-if="fieldError('secretReference')"
          class="text-xs text-destructive"
          data-testid="secretReference-error"
        >
          {{ fieldError('secretReference') }}
        </p>
      </div>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="space-y-2">
          <Label for="credential-valid-from">Valid from</Label>
          <Input
            id="credential-valid-from"
            v-model="validFrom"
            type="date"
            data-testid="credential-valid-from"
            :invalid="!!fieldError('validFrom')"
          />
          <p
            v-if="fieldError('validFrom')"
            class="text-xs text-destructive"
            data-testid="validFrom-error"
          >
            {{ fieldError('validFrom') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="credential-valid-to">Valid to</Label>
          <Input
            id="credential-valid-to"
            v-model="validTo"
            type="date"
            data-testid="credential-valid-to"
            :invalid="!!fieldError('validTo')"
          />
          <p
            v-if="fieldError('validTo')"
            class="text-xs text-destructive"
            data-testid="validTo-error"
          >
            {{ fieldError('validTo') }}
          </p>
        </div>
      </div>

      <div class="flex items-center gap-2">
        <Checkbox id="credential-active" v-model="isActive" data-testid="credential-active" />
        <Label for="credential-active">Active</Label>
      </div>

      <div class="flex justify-end gap-2">
        <Button type="button" variant="outline" :disabled="isSaving" @click="open = false">
          Cancel
        </Button>
        <Button type="submit" :disabled="isSaving" data-testid="save-credential">
          {{ isSaving ? 'Saving…' : isEditing ? 'Save' : 'Create reference' }}
        </Button>
      </div>
    </form>
  </Dialog>
</template>
