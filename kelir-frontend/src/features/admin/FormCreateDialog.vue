<script setup lang="ts">
import { ref, watch } from 'vue'

import { toApiError } from '@/api/client'
import { createForm } from '@/api/rad'
import FormUnplacedErrors from '@/components/FormUnplacedErrors.vue'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useFormErrors } from '@/composables/useFormErrors'
import type { Form } from '@/types/rad'

/**
 * Creating a form definition (#373 AC1).
 *
 * **A key, a title, and one field.** The alternative — creating an empty
 * definition — was rejected: JFSS requires `components`, and a form with none
 * is a document nobody can fill in. Seeding one text field means the thing that
 * exists after this dialog is a form that renders, which is what the builder
 * then edits.
 *
 * **The `formKey` is fixed here and nowhere else.** It is the identity a
 * document pins and what `document_types.form_id` is chosen by, so
 * `UpdateFormRequest` has no field for it and the builder does not offer one.
 */
const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ (event: 'close'): void; (event: 'created', form: Form): void }>()

const formKey = ref('')
const title = ref('')

const isSaving = ref(false)
const { fieldErrors, formError, report, reset } = useFormErrors()

/**
 * The paths a message is shown under (#641). A detail at any other path, such
 * as the seeded `definition` or an unknown field, is listed above the form by
 * `FormUnplacedErrors`, so a refused create always says why.
 */
const PLACED = ['formKey', 'title']

watch(
  () => props.open,
  (open) => {
    if (open) {
      formKey.value = ''
      title.value = ''
      reset()
    }
  },
)

/** The server's message for one field, by the S10.3 `path` it addressed. */
function messageFor(path: string): string | undefined {
  return fieldErrors.value[path]
}

async function save(): Promise<void> {
  isSaving.value = true
  reset()

  const key = formKey.value.trim()

  try {
    const form = await createForm({
      formKey: key,
      title: title.value.trim(),
      definition: {
        // JFSS §2: `formId` is the definition's own identity and Kelir keys it
        // to the form key, which is what the renderer and every stored
        // submission refer to it by.
        formId: key,
        version: '2.0.1',
        title: title.value.trim(),
        components: [
          {
            id: 'field_1',
            role: 'data',
            type: 'textfield',
            key: 'field_1',
            label: 'Field 1',
            validation: { type: 'string' },
          },
        ],
      },
    })

    emit('created', form)
  } catch (failure) {
    // A 422's details go under their inputs or into the list; anything else,
    // a 409 for a taken key included, is the one message above the form.
    report(toApiError(failure))
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    aria-modal="true"
    aria-label="New form"
    data-testid="form-create-dialog"
  >
    <div class="w-full max-w-lg rounded-lg bg-background p-6 shadow-lg">
      <h3 class="text-lg font-semibold">New form</h3>

      <Alert v-if="formError" variant="destructive" class="mt-4" data-testid="form-create-error">
        {{ formError }}
      </Alert>

      <FormUnplacedErrors
        :field-errors="fieldErrors"
        :placed="PLACED"
        class="mt-4"
        data-testid="form-create-unplaced-errors"
      />

      <form class="mt-4 space-y-4" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="form-key">Key</Label>
          <Input
            id="form-key"
            v-model="formKey"
            data-testid="form-key"
            placeholder="purchase_requisition"
          />
          <p class="text-xs text-muted-foreground">
            The identity a document pins. It cannot change once documents exist.
          </p>
          <p v-if="messageFor('formKey')" class="text-xs text-destructive">
            {{ messageFor('formKey') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="form-title">Title</Label>
          <Input id="form-title" v-model="title" data-testid="form-title" />
          <p v-if="messageFor('title')" class="text-xs text-destructive">
            {{ messageFor('title') }}
          </p>
        </div>

        <div class="flex justify-end gap-2 pt-2">
          <Button type="button" variant="secondary" @click="emit('close')">Cancel</Button>
          <Button type="submit" :disabled="isSaving" data-testid="save-form">
            {{ isSaving ? 'Creating…' : 'Create' }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
