<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'

import { createDocument } from '@/api/documents'
import { listDocumentTypes } from '@/api/document-types'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SearchSelect, type SearchSource } from '@/components/ui/search-select'
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

const problem = ref('')
const creating = ref(false)

const title = ref('')
const chosen = ref<string>('')

/**
 * The types a document may actually be created from, searched on the server.
 *
 * `DEPRECATED` is what an administrator sets to stop new documents being
 * created from a type while its existing ones keep working — the alternative to
 * retiring it, which `delete_type` refuses while documents exist. A chooser that
 * offered deprecated types would defeat the one control that exists for this.
 *
 * **`ACTIVE` is asked of the server, not applied to a page (#525).** Filtering
 * a hundred rows after they arrived showed fewer than a hundred and could never
 * show the type that sorted past them.
 */
const typeSource: SearchSource<DocumentTypeSummary> = {
  fetch: (query) => listDocumentTypes({ ...query, status: 'ACTIVE' }),
  value: (type) => type.id,
  label: (type) => `${type.name} (${type.typeCode})`,
  // Shown, greyed and explained: see the note on this screen.
  disabled: (type) => !type.formId,
}

// A type with no form cannot be chosen, so a chosen type always has one.
const ready = computed(() => chosen.value !== '' && title.value.trim() !== '' && !creating.value)

async function start(): Promise<void> {
  if (!ready.value) {
    return
  }

  problem.value = ''
  creating.value = true

  try {
    // No form data. A document is created and *then* filled in — the workspace
    // is where the form is, and a chooser that also rendered the form would be
    // the workspace with a different name.
    const created = await createDocument({
      documentTypeId: chosen.value,
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

    <SearchSelect
      id="new-document-type"
      v-model="chosen"
      label="Document type"
      variant="list"
      test-id="document-type"
      search-placeholder="Search by name or code"
      :source="typeSource"
      :option-test-id="(type) => `type-${type.typeCode}`"
    >
      <template #option="{ row: type, label }">
        <template v-if="type">
          <span class="font-medium">{{ type.name }}</span>
          <span class="ml-2 text-xs text-muted-foreground">{{ type.typeCode }}</span>
          <!-- Shown rather than hidden, and said rather than left blank. -->
          <span v-if="!type.formId" class="block text-sm text-muted-foreground">
            This type has no form bound to it yet, so there would be nothing to fill in.
          </span>
        </template>
        <template v-else>{{ label }}</template>
      </template>

      <template #empty>
        <span data-testid="no-types">
          No document type is active in this tenant yet. An administrator configures one before a
          document can be raised.
        </span>
      </template>
    </SearchSelect>

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

    <Button :disabled="!ready" data-testid="create-document" @click="start"> Create draft </Button>
  </section>
</template>
