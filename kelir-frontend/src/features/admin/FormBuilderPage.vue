<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { toApiError } from '@/api/client'
import { createFormRevision, getForm, publishForm, updateForm } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import JfssForm from '@/features/rad/JfssForm.vue'
import { useAuthStore } from '@/stores/auth'
import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'
import type { Form } from '@/types/rad'

import FormBuilderCanvas from './form-builder/FormBuilderCanvas.vue'
import FormBuilderPalette from './form-builder/FormBuilderPalette.vue'
import { locate, pathOf, replaceNode, setLookup } from './form-builder/formTree'
import { createFormBuilder, provideFormBuilder } from './form-builder/useFormBuilder'
import FormComponentEditor from './FormComponentEditor.vue'

/**
 * The form builder (FR-RAD-004, #373; nested since FR-RAD-013, #688).
 *
 * **Three properties this screen holds, and none of them is that it renders.**
 *
 * 1. **A published revision is not edited in place.** [ADR-0027] — a document
 *    pins the revision it was filled against, so editing a published one would
 *    change what an already-submitted document claims to have been. The screen
 *    opens `PUBLISHED` read-only and offers *New revision*, and says why rather
 *    than merely disabling the fields.
 * 2. **The server's verdict is the only verdict.** `rad::domain::engine`
 *    resolves rule names and builds the dependency graph at the write and again
 *    at the publish (**D-67**, [ADR-0035]); JFSS S12.2 asks for a cycle to be
 *    *surfaced in an authoring tool*, and this is that tool — surfacing the
 *    server's answer, not computing a second one. A rule catalogue in the
 *    browser would be a third opinion about a question already answered twice.
 * 3. **What the server refuses is shown against the component it named.** The
 *    S10.3 `path` is dot-notation — `definition.components.0.columns.1.components.2`
 *    — and `formTree.resolveDetail` walks it into the nested node it names, so
 *    a detail lands on that node's card rather than in a banner above forty
 *    fields. One no node claims stays in the list at the top.
 *
 * **The canvas is a tree** (#688, **D-86** A): a palette of what the renderer
 * draws, a recursive canvas that nests panels, fieldsets, columns, tabs and a
 * data grid's row template, by pointer and by keyboard, and the shipped
 * `FormComponentEditor` mounted for the one node selected. The drag-and-drop
 * library is imported by the canvas components alone, which this page alone
 * imports, and the route is lazy, so a session that never opens a builder
 * never fetches it ([ADR-0046]; `check:bundle`).
 *
 * **The preview is the editor's own claim, checked.** It renders the definition
 * being edited through the same `JfssForm` a document uses, so a definition
 * this screen produces is visibly one the renderer draws — which is the half of
 * *writes conformant JFSS* that a meta-schema check cannot show.
 */
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

const canUpdate = computed(() => auth.can('rad:form:update'))
const canPublish = computed(() => auth.can('rad:form:publish'))

const form = ref<Form | null>(null)
const title = ref('')
const definition = ref<JfssDefinition | null>(null)

const isLoading = ref(true)
const isSaving = ref(false)
const loadError = ref('')
const saveError = ref('')
const details = ref<ValidationDetail[]>([])
const saved = ref(false)

/** A published revision is immutable; everything below is read-only over it. */
const isPublished = computed(() => form.value?.status === 'PUBLISHED')
const isReadOnly = computed(() => isPublished.value || !canUpdate.value)

const builder = createFormBuilder({ definition, readOnly: isReadOnly, details })

provideFormBuilder(builder)

/** `settings.lookups`, which is where a lookup's source lives (**D-23**). */
const lookups = computed<Record<string, string>>(
  () => (definition.value?.settings?.lookups as Record<string, string> | undefined) ?? {},
)

/** The node the properties region edits (A2): one at a time. */
const selected = computed<{ path: string; node: JfssComponent } | null>(() => {
  const id = builder.selectedId.value
  const path = definition.value && id ? pathOf(definition.value, id) : null
  const at = definition.value && path ? locate(definition.value, path) : null

  return path && at ? { path, node: at.node } : null
})

/** The details no node claims. A message nobody claims is a message the author never sees. */
const unclaimedDetails = computed(() => builder.unplacedDetails.value)

function replaceSelected(component: JfssComponent): void {
  const current = selected.value

  if (!definition.value || !current || isReadOnly.value) {
    return
  }

  definition.value = replaceNode(definition.value, current.path, component) ?? definition.value
}

function setLookupSource(componentId: string, source: string): void {
  if (!definition.value || isReadOnly.value) {
    return
  }

  definition.value = setLookup(definition.value, componentId, source)
}

/** On load, the first root component is selected (A2). */
function selectFirst(): void {
  builder.select(definition.value?.components[0]?.id ?? null)
}

async function load(): Promise<void> {
  isLoading.value = true
  loadError.value = ''

  try {
    const loaded = await getForm(String(route.params.id))

    form.value = loaded
    title.value = loaded.title
    // A copy: the editor mutates it, and the last saved state stays in `form`
    // so the header keeps telling the truth about what is stored.
    definition.value = structuredClone(loaded.definition)
    selectFirst()
  } catch (failure) {
    loadError.value = toApiError(failure).message
  } finally {
    isLoading.value = false
  }
}

async function save(): Promise<void> {
  if (!form.value || !definition.value) {
    return
  }

  isSaving.value = true
  saveError.value = ''
  details.value = []
  saved.value = false

  try {
    const updated = await updateForm(form.value.id, {
      title: title.value.trim(),
      definition: { ...definition.value, title: title.value.trim() },
    })

    form.value = updated
    // Read back what was stored rather than keeping the local copy: the server
    // is entitled to normalise, and a screen that kept its own version would be
    // showing something no document will ever be filled against.
    definition.value = structuredClone(updated.definition)
    // The selection survives when its node did.
    builder.select(builder.selectedId.value)
    saved.value = true
  } catch (failure) {
    const failed = toApiError(failure)

    saveError.value = failed.message
    details.value = failed.details
  } finally {
    isSaving.value = false
  }
}

async function publish(): Promise<void> {
  if (!form.value) {
    return
  }

  isSaving.value = true
  saveError.value = ''
  details.value = []

  try {
    form.value = await publishForm(form.value.id)
  } catch (failure) {
    const failed = toApiError(failure)

    // **Publish re-validates the stored definition**, so its refusal is about
    // what is in the database rather than what is on screen — which is exactly
    // the case ADR-0035 exists for: a draft written by an older build.
    saveError.value = failed.message
    details.value = failed.details
  } finally {
    isSaving.value = false
  }
}

async function revise(): Promise<void> {
  if (!form.value) {
    return
  }

  isSaving.value = true
  saveError.value = ''

  try {
    const next = await createFormRevision(form.value.id)

    await router.push({ name: 'admin-form-builder', params: { id: next.id } })
    await load()
  } catch (failure) {
    saveError.value = toApiError(failure).message
  } finally {
    isSaving.value = false
  }
}

onMounted(() => {
  void load()
})
</script>

<template>
  <section class="space-y-6">
    <Alert v-if="loadError" variant="destructive" data-testid="load-error">{{ loadError }}</Alert>

    <p v-if="isLoading" class="text-sm text-muted-foreground">Loading…</p>

    <template v-if="form && definition">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 class="text-xl font-semibold tracking-tight">{{ form.formKey }}</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Revision {{ form.revision }} · JFSS {{ form.jfssVersion }}
          </p>
        </div>

        <div class="flex items-center gap-2">
          <Badge :variant="isPublished ? 'default' : 'secondary'">{{ form.status }}</Badge>

          <Button
            v-if="!isPublished && canUpdate"
            :disabled="isSaving"
            data-testid="save-definition"
            @click="save"
          >
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>

          <Button
            v-if="!isPublished && canPublish"
            variant="secondary"
            :disabled="isSaving"
            data-testid="publish-form"
            @click="publish"
          >
            Publish
          </Button>

          <Button
            v-if="isPublished && canUpdate"
            variant="secondary"
            :disabled="isSaving"
            data-testid="new-revision"
            @click="revise"
          >
            New revision
          </Button>
        </div>
      </div>

      <!-- Said rather than merely enforced by disabled inputs: a screen whose
           fields are greyed out with no explanation reads as broken. -->
      <Alert v-if="isPublished" data-testid="published-notice">
        This revision is published and cannot be edited. Documents raised from it pinned this exact
        definition, so changing it would alter what they were filled against. Open a new revision to
        make changes.
      </Alert>

      <Alert v-if="saveError" variant="destructive" data-testid="save-error">
        {{ saveError }}
      </Alert>

      <Alert v-if="saved" data-testid="save-confirmed">
        Saved. What is shown below is what the server stored.
      </Alert>

      <ul v-if="unclaimedDetails.length > 0" class="space-y-1" data-testid="form-errors">
        <li
          v-for="(detail, at) in unclaimedDetails"
          :key="`${detail.path}-${at}`"
          class="text-sm text-destructive"
        >
          <span class="font-mono text-xs">{{ detail.path }}</span> — {{ detail.message }}
        </li>
      </ul>

      <div class="space-y-2">
        <Label for="definition-title">Title</Label>
        <Input
          id="definition-title"
          v-model="title"
          :disabled="isReadOnly"
          data-testid="definition-title"
        />
      </div>

      <div
        class="grid gap-6"
        :class="isReadOnly ? 'lg:grid-cols-2' : 'lg:grid-cols-[12rem_minmax(0,1fr)_minmax(0,1fr)]'"
      >
        <!-- Absent, not disabled, on a read-only canvas (D7). -->
        <FormBuilderPalette v-if="!isReadOnly" />

        <div class="space-y-2">
          <h3 class="text-sm font-semibold">Components</h3>
          <FormBuilderCanvas />
          <p v-if="definition.components.length === 0" class="text-sm text-muted-foreground">
            No components. A form with none is a document nobody can fill in.
          </p>
        </div>

        <div class="space-y-6">
          <section class="space-y-2" aria-labelledby="properties-heading" data-testid="properties">
            <h3 id="properties-heading" class="text-sm font-semibold">Properties</h3>
            <FormComponentEditor
              v-if="selected"
              :key="selected.node.id"
              :component="selected.node"
              :disabled="isReadOnly"
              :details="builder.detailsFor(selected.node.id)"
              :lookup-source="lookups[selected.node.id] ?? ''"
              @update="replaceSelected"
              @update:lookup-source="setLookupSource(selected.node.id, $event)"
            />
            <p v-else class="text-sm text-muted-foreground">
              Select a component on the canvas to edit it.
            </p>
          </section>

          <section class="space-y-2">
            <h3 class="text-sm font-semibold">Preview</h3>
            <div class="rounded-lg border p-4" data-testid="form-preview">
              <!-- The same renderer a document uses. A definition this screen
                   produces that the preview cannot draw is a definition no
                   document could be filled against either. -->
              <JfssForm :definition="definition" />
            </div>
          </section>
        </div>
      </div>
    </template>
  </section>
</template>
