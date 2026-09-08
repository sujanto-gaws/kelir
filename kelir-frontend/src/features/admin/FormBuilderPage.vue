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

import FormComponentEditor from './FormComponentEditor.vue'

/**
 * The form builder (FR-RAD-004; #373).
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
 *    S10.3 `path` is dot-notation — `components.2.rules.0` — so a detail lands
 *    on the component it is about rather than in a banner above forty fields.
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

const components = computed<JfssComponent[]>(() => definition.value?.components ?? [])

/** `settings.lookups`, which is where a lookup's source lives (**D-23**). */
const lookups = computed<Record<string, string>>(
  () => (definition.value?.settings?.lookups as Record<string, string> | undefined) ?? {},
)

/**
 * The details that address one component, by its position in `components`.
 *
 * The server addresses a component by index — `components.2.…` — so this is a
 * prefix match on that index rather than on the component's `id`. A detail
 * whose path names no component stays in the page-level list below, because a
 * message nobody claims is a message the author never sees.
 */
function detailsFor(index: number): ValidationDetail[] {
  return details.value.filter((detail) => detail.path.startsWith(`components.${index}`))
}

const unclaimedDetails = computed(() =>
  details.value.filter((detail) => !/^components\.\d+/.test(detail.path)),
)

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
  } catch (failure) {
    loadError.value = toApiError(failure).message
  } finally {
    isLoading.value = false
  }
}

function nextComponentId(): string {
  // `id` is unique per component *instance* (JFSS §4.1) and `settings.lookups`
  // binds on it, so a duplicate would bind two components to one source.
  const taken = new Set(components.value.map((component) => component.id))

  for (let n = components.value.length + 1; ; n += 1) {
    const candidate = `field_${n}`

    if (!taken.has(candidate)) {
      return candidate
    }
  }
}

function addField(): void {
  if (!definition.value) {
    return
  }

  const id = nextComponentId()

  definition.value.components = [
    ...components.value,
    {
      id,
      role: 'data',
      type: 'textfield',
      key: id,
      label: 'New field',
      validation: { type: 'string' },
    },
  ]
}

function addDisplay(): void {
  if (!definition.value) {
    return
  }

  const id = nextComponentId()

  definition.value.components = [
    ...components.value,
    { id, role: 'display', type: 'paragraph', content: 'Text' },
  ]
}

function replaceComponent(index: number, component: JfssComponent): void {
  if (!definition.value) {
    return
  }

  definition.value.components = components.value.map((existing, at) =>
    at === index ? component : existing,
  )
}

function moveComponent(index: number, direction: -1 | 1): void {
  const target = index + direction

  if (!definition.value || target < 0 || target >= components.value.length) {
    return
  }

  const next = [...components.value]
  const [moved] = next.splice(index, 1)

  next.splice(target, 0, moved)
  definition.value.components = next
}

/**
 * Removes a component, and its lookup binding with it.
 *
 * **A binding naming no component is refused at save** (`rad::domain::jfss`),
 * so leaving the entry behind would make the next save fail with a message
 * about a field that is no longer on screen.
 */
function removeComponent(index: number): void {
  if (!definition.value) {
    return
  }

  const [removed] = components.value.slice(index, index + 1)

  definition.value.components = components.value.filter((_, at) => at !== index)

  if (removed && definition.value.settings?.lookups) {
    const remaining = { ...(definition.value.settings.lookups as Record<string, string>) }

    delete remaining[removed.id]
    definition.value.settings = { ...definition.value.settings, lookups: remaining }
  }
}

function setLookupSource(componentId: string, source: string): void {
  if (!definition.value) {
    return
  }

  const remaining = { ...lookups.value }

  if (source === '') {
    delete remaining[componentId]
  } else {
    remaining[componentId] = source
  }

  definition.value.settings = { ...(definition.value.settings ?? {}), lookups: remaining }
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
        <li v-for="detail in unclaimedDetails" :key="detail.path" class="text-sm text-destructive">
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

      <div class="grid gap-6 lg:grid-cols-2">
        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <h3 class="text-sm font-semibold">Components</h3>
            <div class="space-x-2">
              <Button
                size="sm"
                variant="secondary"
                :disabled="isReadOnly"
                data-testid="add-field"
                @click="addField"
              >
                Add field
              </Button>
              <Button
                size="sm"
                variant="secondary"
                :disabled="isReadOnly"
                data-testid="add-text"
                @click="addDisplay"
              >
                Add text
              </Button>
            </div>
          </div>

          <FormComponentEditor
            v-for="(component, index) in components"
            :key="component.id"
            :component="component"
            :index="index"
            :count="components.length"
            :disabled="isReadOnly"
            :details="detailsFor(index)"
            :lookup-source="lookups[component.id] ?? ''"
            @update="replaceComponent(index, $event)"
            @update:lookup-source="setLookupSource(component.id, $event)"
            @move="moveComponent(index, $event)"
            @remove="removeComponent(index)"
          />

          <p v-if="components.length === 0" class="text-sm text-muted-foreground">
            No components. A form with none is a document nobody can fill in.
          </p>
        </div>

        <div class="space-y-2">
          <h3 class="text-sm font-semibold">Preview</h3>
          <div class="rounded-lg border p-4" data-testid="form-preview">
            <!-- The same renderer a document uses. A definition this screen
                 produces that the preview cannot draw is a definition no
                 document could be filled against either. -->
            <JfssForm :definition="definition" />
          </div>
        </div>
      </div>
    </template>
  </section>
</template>
