<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import DocumentHeader from './DocumentHeader.vue'
import EmptyTab from './EmptyTab.vue'
import {
  getDocument,
  getStatusHistory,
  submitDocument,
  transitionDocument,
  updateDocument,
} from '@/api/documents'
import { getForm } from '@/api/rad'
import { ApiError } from '@/api/error'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import JfssForm from '@/features/rad/JfssForm.vue'
import type { ValidationDetail } from '@/types/api'
import {
  ALLOWED_TRANSITIONS,
  DOCUMENT_STATUS_LABELS,
  type Document,
  type DocumentStatus,
  type StatusHistoryEntry,
} from '@/types/document'
import type { Form } from '@/types/rad'

/**
 * The document detail workspace (FR-DOC-014, #172).
 *
 * **A shell, deliberately.** The tabs Phase 5 and Phase 6 fill — workflow,
 * attachments, comments, the activity timeline — do not exist yet. What lands
 * here is the workspace that holds the form and has somewhere for them to go,
 * so that adding one later is adding a tab rather than rebuilding a screen.
 *
 * **The form is rendered through #162's renderer, unchanged.** A workspace that
 * needed the renderer modified would mean #162 rendered a page rather than a
 * form. What it passes is the document's stored payload as `initialValues` and
 * the *pinned* revision's definition — `document.formId`, never the type's
 * current binding — which is what makes **D-30** visible on a screen: an old
 * document re-renders against the definition it was filled in against.
 *
 * **Refusals are shown as the API returns them** (#172 AC3). A submit refused
 * for `required` lands on the fields through the renderer's S10.3 handling; a
 * submit refused because the type has no numbering rule lands as a message
 * naming what an administrator has to fix. Neither is flattened into
 * "something went wrong", which is the shape that sends a person to support
 * instead of to the field they left blank.
 */
const route = useRoute()
const router = useRouter()

const document = ref<Document | null>(null)
const form = ref<Form | null>(null)
const history = ref<StatusHistoryEntry[]>([])

const loading = ref(true)
const failed = ref(false)

/** In flight, so a double press is not two submissions. */
const busy = ref(false)
/** S10.3 details from a refusal, handed to the renderer to place by `path`. */
const violations = ref<ValidationDetail[]>([])
/** A refusal with nothing to place against a field: a 403, a 409, a 422 about the type. */
const problem = ref('')
/** Something that worked, said plainly. */
const notice = ref('')

const TABS = [
  { key: 'form', label: 'Form' },
  { key: 'history', label: 'History' },
  { key: 'workflow', label: 'Workflow' },
  { key: 'attachments', label: 'Attachments' },
  { key: 'comments', label: 'Comments' },
] as const

type TabKey = (typeof TABS)[number]['key']

const tab = ref<TabKey>('form')

/** A draft is edited; anything else is read. */
const editable = computed(() => document.value?.status === 'DRAFT')

/** The moves the backend will accept from where this document is. */
const transitions = computed<DocumentStatus[]>(() =>
  document.value ? ALLOWED_TRANSITIONS[document.value.status] : [],
)

async function load(id: string): Promise<void> {
  loading.value = true
  failed.value = false
  reset()

  try {
    const loaded = await getDocument(id)
    document.value = loaded

    // The pinned revision, not the type's binding. See the module comment.
    form.value = loaded.formId ? await getForm(loaded.formId) : null
    history.value = await getStatusHistory(id)
  } catch {
    // Which failure it was — 403, 404, a dead backend — is deliberately not
    // distinguished: a reader who may not open this document should not learn
    // from the wording whether it exists.
    failed.value = true
    document.value = null
    form.value = null
  } finally {
    loading.value = false
  }
}

function reset(): void {
  violations.value = []
  problem.value = ''
  notice.value = ''
}

watch(
  () => route.params.id,
  (id) => load(String(id)),
  { immediate: true },
)

/**
 * Turns a refusal into what the screen shows.
 *
 * A validation failure goes to the fields by `path`; everything else is shown
 * in the backend's own words, because repeating it in ours risks contradicting
 * it (coding standard §3.3).
 */
function report(error: unknown): void {
  if (error instanceof ApiError && error.isValidation) {
    violations.value = error.details
    // A refusal whose details name no field of the form — NO_NUMBERING_RULE
    // against `documentTypeId`, say — would otherwise be placed nowhere and
    // seen by nobody, so the message is shown as well.
    problem.value = error.details.map((detail) => detail.message).join(' ')
  } else if (error instanceof ApiError) {
    problem.value = error.message
  } else {
    problem.value = 'Something went wrong. Try again.'
  }
}

/** Saves the draft's form data, and shows what the server stored. */
async function save(values: Record<string, unknown>): Promise<void> {
  if (!document.value || busy.value) {
    return
  }

  reset()
  busy.value = true

  try {
    // What comes back is the server's re-evaluated payload, and that is what is
    // put on screen — a workspace that kept its own copy would be showing a
    // number the stored document does not hold.
    document.value = await updateDocument(document.value.id, { formData: values })
    notice.value = 'Saved.'
  } catch (error) {
    report(error)
  } finally {
    busy.value = false
  }
}

/** Submits the draft, which is where the number comes from. */
async function submit(values: Record<string, unknown>): Promise<void> {
  if (!document.value || busy.value) {
    return
  }

  reset()
  busy.value = true

  try {
    // Saved first, deliberately. The submit re-evaluates and numbers what is
    // *stored*, so submitting without saving would commit the payload as it was
    // before the last keystroke — and the number would be attached to it.
    const saved = await updateDocument(document.value.id, { formData: values })
    document.value = await submitDocument(saved.id)
    history.value = await getStatusHistory(saved.id)
    notice.value = `Submitted as ${document.value.documentNumber}.`
  } catch (error) {
    report(error)
  } finally {
    busy.value = false
  }
}

async function move(status: DocumentStatus): Promise<void> {
  if (!document.value || busy.value) {
    return
  }

  reset()
  busy.value = true

  try {
    await transitionDocument(document.value.id, status)
    document.value = await getDocument(document.value.id)
    history.value = await getStatusHistory(document.value.id)
    notice.value = `Moved to ${DOCUMENT_STATUS_LABELS[status]}.`
  } catch (error) {
    report(error)
  } finally {
    busy.value = false
  }
}

/**
 * What a button on the rendered form means.
 *
 * `submit` is the one the definition's own action declares, and it is routed to
 * the document's submit rather than to a form submission — a document is not a
 * `rad_form_submissions` row, and #164's endpoint would store one beside the
 * document rather than in it.
 */
function onAction(action: string, values: Record<string, unknown>): void {
  if (action === 'submit') {
    void submit(values)
    return
  }

  problem.value = `This form declares a "${action}" button, which is not wired to anything yet.`
}

/** The payload as it stands in the renderer, so Save has something to send. */
const draftValues = ref<Record<string, unknown>>({})

function onChange(values: Record<string, unknown>): void {
  draftValues.value = values
}
</script>

<template>
  <section class="space-y-6">
    <p v-if="loading" data-testid="document-loading" class="text-sm text-muted-foreground">
      Loading…
    </p>

    <Alert v-else-if="failed" variant="destructive" data-testid="document-error">
      This document could not be opened.
    </Alert>

    <template v-else-if="document">
      <DocumentHeader :document="document" />

      <Alert v-if="problem" variant="destructive" data-testid="document-problem">
        {{ problem }}
      </Alert>

      <Alert v-if="notice" data-testid="document-notice">{{ notice }}</Alert>

      <nav class="flex flex-wrap gap-2" aria-label="Document sections">
        <Button
          v-for="candidate in TABS"
          :key="candidate.key"
          :variant="candidate.key === tab ? 'default' : 'outline'"
          size="sm"
          :aria-current="candidate.key === tab ? 'page' : undefined"
          :data-testid="`tab-${candidate.key}`"
          @click="tab = candidate.key"
        >
          {{ candidate.label }}
        </Button>
      </nav>

      <div v-show="tab === 'form'" class="space-y-4">
        <p v-if="!form" class="text-sm text-muted-foreground" data-testid="document-no-form">
          This document's type binds no form, so there is nothing to render.
        </p>

        <template v-else>
          <!-- Read or edit by status (#172 AC1). A `fieldset` rather than a
               per-field flag: every control the renderer can produce is a form
               control, so one element closes all of them and no field component
               has to learn about documents. What it does not yet give is a
               true read-only *presentation* — the values are shown in disabled
               inputs rather than as text, which is legible and not selectable.
               Naming it here is what makes it a known limitation rather than an
               oversight. -->
          <fieldset :disabled="!editable" :data-readonly="!editable" data-testid="document-form">
            <JfssForm
              :definition="form.definition"
              :initial-values="document.formData"
              :server-violations="violations"
              @action="onAction"
              @change="onChange"
            />
          </fieldset>

          <div v-if="editable" class="flex flex-wrap gap-2">
            <Button
              variant="outline"
              :disabled="busy"
              data-testid="save-draft"
              @click="save(draftValues)"
            >
              Save draft
            </Button>
          </div>
        </template>
      </div>

      <div v-show="tab === 'history'" class="space-y-3" data-testid="document-history">
        <ol class="space-y-2">
          <li
            v-for="(entry, index) in history"
            :key="`${entry.changedAt}-${index}`"
            class="rounded-md border border-border p-3 text-sm"
          >
            <p class="font-medium">
              <span v-if="entry.previousStatus">
                {{ DOCUMENT_STATUS_LABELS[entry.previousStatus] }} →
              </span>
              {{ DOCUMENT_STATUS_LABELS[entry.status] }}
            </p>
            <p class="text-muted-foreground">{{ entry.changedAt }}</p>
            <p v-if="entry.reason" class="mt-1">{{ entry.reason }}</p>
          </li>
        </ol>

        <div v-if="transitions.length" class="flex flex-wrap gap-2">
          <Button
            v-for="status in transitions"
            :key="status"
            variant="outline"
            size="sm"
            :disabled="busy"
            :data-testid="`transition-${status}`"
            @click="move(status)"
          >
            {{ DOCUMENT_STATUS_LABELS[status] }}
          </Button>
        </div>
        <p v-else-if="!editable" class="text-sm text-muted-foreground">
          This document has reached the end of its life.
        </p>
      </div>

      <div v-show="tab === 'workflow'" data-testid="panel-workflow">
        <EmptyTab subject="Approvals" arrives="Phase 5" />
      </div>

      <div v-show="tab === 'attachments'" data-testid="panel-attachments">
        <EmptyTab subject="Attachments" arrives="Phase 6" />
      </div>

      <div v-show="tab === 'comments'" data-testid="panel-comments">
        <EmptyTab subject="Comments" arrives="Phase 6" />
      </div>

      <div>
        <Button variant="outline" size="sm" @click="router.push({ name: 'documents' })">
          Back to documents
        </Button>
      </div>
    </template>
  </section>
</template>
