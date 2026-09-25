<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { createIntegrationEndpoint, updateIntegrationEndpoint } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { useFormErrors } from '@/composables/useFormErrors'
import { HTTP_METHODS, type HttpMethod, type IntegrationEndpoint } from '@/types/integration'

import { blankToNull, unplacedErrors } from './form-errors'

/**
 * Adding and editing one endpoint of a system (FR-INT-001, #520).
 *
 * Under the system's own `:update`, as the product owner answered: an endpoint
 * has no permissions of its own and no page of its own. Retiring one is a
 * status change the section offers as its own button, so this form edits what
 * the endpoint *is* and leaves whether it is in use alone.
 */
const props = defineProps<{ systemId: string; editing: IntegrationEndpoint | null }>()
const emit = defineEmits<{ saved: [endpoint: IntegrationEndpoint] }>()

const open = defineModel<boolean>('open', { default: false })

const isEditing = computed(() => props.editing !== null)

const endpointCode = ref('')
const name = ref('')
const method = ref<HttpMethod>('GET')
const path = ref('')
const description = ref('')
const isSaving = ref(false)

/** A duplicate code within the system is a 409 with no details. */
const errors = useFormErrors(() =>
  isEditing.value ? [] : [{ match: /./, fields: ['endpointCode'] }],
)

const PLACED = ['endpointCode', 'name', 'method', 'path', 'description'] as const
const unplaced = computed(() => unplacedErrors(errors.fieldErrors.value, PLACED))

const METHOD_OPTIONS = HTTP_METHODS.map((value) => ({ value, label: value }))

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
    endpointCode.value = props.editing?.endpointCode ?? ''
    name.value = props.editing?.name ?? ''
    method.value = props.editing?.method ?? 'GET'
    path.value = props.editing?.path ?? ''
    description.value = props.editing?.description ?? ''
  },
  { immediate: true },
)

async function save(): Promise<void> {
  isSaving.value = true
  errors.reset()

  try {
    const saved = props.editing
      ? await updateIntegrationEndpoint(props.systemId, props.editing.id, {
          name: name.value.trim(),
          method: method.value,
          path: path.value.trim(),
          description: blankToNull(description.value),
        })
      : await createIntegrationEndpoint(props.systemId, {
          endpointCode: endpointCode.value.trim(),
          name: name.value.trim(),
          method: method.value,
          path: path.value.trim(),
          description: blankToNull(description.value) ?? undefined,
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
  <Dialog v-model:open="open" :title="isEditing ? 'Edit endpoint' : 'Add endpoint'">
    <form class="space-y-4" data-testid="endpoint-form" novalidate @submit.prevent="save">
      <Alert v-if="errors.formError.value" variant="destructive" data-testid="endpoint-form-error">
        {{ errors.formError.value }}
      </Alert>

      <Alert
        v-if="unplaced.length > 0"
        variant="destructive"
        data-testid="endpoint-unplaced-errors"
      >
        <ul class="list-disc pl-4">
          <li v-for="item in unplaced" :key="item.path">{{ item.path }}: {{ item.message }}</li>
        </ul>
      </Alert>

      <div class="space-y-2">
        <Label for="endpoint-code">Endpoint code</Label>
        <Input
          id="endpoint-code"
          v-model="endpointCode"
          data-testid="endpoint-code"
          placeholder="CREATE_PURCHASE_ORDER"
          autocomplete="off"
          :disabled="isEditing"
          :invalid="!!fieldError('endpointCode')"
        />
        <p
          v-if="fieldError('endpointCode')"
          class="text-xs text-destructive"
          data-testid="endpointCode-error"
        >
          {{ fieldError('endpointCode') }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="endpoint-name">Name</Label>
        <Input
          id="endpoint-name"
          v-model="name"
          data-testid="endpoint-name"
          placeholder="Create purchase order"
          :invalid="!!fieldError('name')"
        />
        <p v-if="fieldError('name')" class="text-xs text-destructive" data-testid="name-error">
          {{ fieldError('name') }}
        </p>
      </div>

      <div class="grid gap-4 sm:grid-cols-[8rem_1fr]">
        <div class="space-y-2">
          <Label for="endpoint-method">Method</Label>
          <Select
            id="endpoint-method"
            v-model="method"
            data-testid="endpoint-method"
            :options="METHOD_OPTIONS"
            :invalid="!!fieldError('method')"
          />
          <p
            v-if="fieldError('method')"
            class="text-xs text-destructive"
            data-testid="method-error"
          >
            {{ fieldError('method') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="endpoint-path">Path</Label>
          <Input
            id="endpoint-path"
            v-model="path"
            data-testid="endpoint-path"
            placeholder="/purchase-orders"
            autocomplete="off"
            :invalid="!!fieldError('path')"
          />
          <p class="text-xs text-muted-foreground">Relative to the system's base URL.</p>
          <p v-if="fieldError('path')" class="text-xs text-destructive" data-testid="path-error">
            {{ fieldError('path') }}
          </p>
        </div>
      </div>

      <div class="space-y-2">
        <Label for="endpoint-description">Description</Label>
        <Textarea
          id="endpoint-description"
          v-model="description"
          data-testid="endpoint-description"
          :invalid="!!fieldError('description')"
        />
      </div>

      <div class="flex justify-end gap-2">
        <Button type="button" variant="outline" :disabled="isSaving" @click="open = false">
          Cancel
        </Button>
        <Button type="submit" :disabled="isSaving" data-testid="save-endpoint">
          {{ isSaving ? 'Saving…' : isEditing ? 'Save' : 'Create endpoint' }}
        </Button>
      </div>
    </form>
  </Dialog>
</template>
