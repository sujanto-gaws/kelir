<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import { createDocument } from '@/api/documents'
import { listDocumentTypes } from '@/api/document-types'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { DocumentTypeSummary } from '@/types/document-type'

/**
 * Choose a type, and start a document from it (FR-DOC-001, #167, #172).
 *
 * # This is the screen that closes Sprint 8's exit qualifier
 *
 * That sprint's exit was recorded as met *in parts* because — in the status
 * report's own words — *"the renderer opens a form by form id, and no screen
 * traverses the type-to-form binding that item 5 configures."* Both halves were
 * built and demonstrable and nothing joined them.
 *
 * This screen is the join. It lists document **types**; choosing one creates a
 * document whose form is that type's binding, pinned at creation; and the
 * workspace then renders the form through the document. Nobody types a form id
 * anywhere.
 *
 * # A type with no form is shown and not offered
 *
 * §6.2 permits a type that binds no form — a type is configured before its form
 * exists as often as after — and a document created from one has nothing to
 * render. Hiding such a type would leave an administrator wondering where the
 * type they just made went; offering it would produce a document with an empty
 * workspace. It is listed, greyed, and says which.
 */
const router = useRouter()

const types = ref<DocumentTypeSummary[]>([])
const loading = ref(true)
const problem = ref('')
const creating = ref(false)

const title = ref('')
const chosen = ref<string>('')

/**
 * The types a document may actually be created from.
 *
 * `DEPRECATED` is what an administrator sets to stop new documents being
 * created from a type while its existing ones keep working — the alternative to
 * retiring it, which `delete_type` refuses while documents exist. A chooser that
 * offered deprecated types would defeat the one control that exists for this.
 */
const available = computed(() => types.value.filter((type) => type.status === 'ACTIVE'))

const chosenType = computed(() => available.value.find((type) => type.id === chosen.value))

const ready = computed(
  () => Boolean(chosenType.value?.formId) && title.value.trim() !== '' && !creating.value,
)

onMounted(async () => {
  try {
    const page = await listDocumentTypes({ pageSize: 100 })
    types.value = page.items
  } catch (error) {
    problem.value =
      error instanceof ApiError ? error.message : 'The document types could not be loaded.'
  } finally {
    loading.value = false
  }
})

async function start(): Promise<void> {
  if (!chosenType.value || creating.value) {
    return
  }

  problem.value = ''
  creating.value = true

  try {
    // No form data. A document is created and *then* filled in — the workspace
    // is where the form is, and a chooser that also rendered the form would be
    // the workspace with a different name.
    const created = await createDocument({
      documentTypeId: chosenType.value.id,
      title: title.value.trim(),
    })

    void router.push({ name: 'document', params: { id: created.id } })
  } catch (error) {
    problem.value =
      error instanceof ApiError ? error.message : 'This document could not be created.'
    creating.value = false
  }
}
</script>

<template>
  <section class="mx-auto w-full max-w-2xl space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">New document</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        Choose what you are raising. The form comes from the type.
      </p>
    </div>

    <Alert v-if="problem" variant="destructive" data-testid="new-document-problem">
      {{ problem }}
    </Alert>

    <p v-if="loading" class="text-sm text-muted-foreground">Loading document types…</p>

    <template v-else>
      <p v-if="available.length === 0" class="text-sm text-muted-foreground" data-testid="no-types">
        No document type is active in this tenant yet. An administrator configures one before a
        document can be raised.
      </p>

      <template v-else>
        <fieldset class="space-y-2">
          <legend class="text-sm font-medium">Document type</legend>

          <label
            v-for="type in available"
            :key="type.id"
            class="flex cursor-pointer items-start gap-3 rounded-md border border-border p-3"
            :data-testid="`type-${type.typeCode}`"
          >
            <input
              v-model="chosen"
              type="radio"
              name="document-type"
              :value="type.id"
              :disabled="!type.formId"
              class="mt-1"
            />
            <span>
              <span class="font-medium">{{ type.name }}</span>
              <span class="ml-2 text-xs text-muted-foreground">{{ type.typeCode }}</span>
              <!-- Shown rather than hidden, and said rather than left blank. -->
              <span v-if="!type.formId" class="block text-sm text-muted-foreground">
                This type has no form bound to it yet, so there would be nothing to fill in.
              </span>
            </span>
          </label>
        </fieldset>

        <div class="space-y-2">
          <Label for="new-document-title">Title</Label>
          <Input
            id="new-document-title"
            v-model="title"
            data-testid="new-document-title"
            placeholder="What this document is about"
          />
          <p class="text-sm text-muted-foreground">
            What this is called in every list it appears in. The form's own fields come next.
          </p>
        </div>

        <Button :disabled="!ready" data-testid="create-document" @click="start">
          Create draft
        </Button>
      </template>
    </template>
  </section>
</template>
