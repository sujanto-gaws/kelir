<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute } from 'vue-router'

import JfssForm from './JfssForm.vue'
import { getForm, submitForm } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import { ApiError } from '@/api/error'
import type { ValidationDetail } from '@/types/api'
import type { Form, FormSubmission } from '@/types/rad'

/**
 * A published form definition, opened, filled in and submitted
 * (FR-RAD-010, #162, #163, #164).
 *
 * **The first RAD surface in the frontend**, and the page the browser harness
 * is pointed at — #162 AC4 asks for the renderer driven through #153 rather
 * than asserted through component tests alone, and a renderer with no page has
 * nothing to drive.
 *
 * **Nothing about a specific form is here** (#162 AC3). The page fetches by id,
 * hands the definition to the renderer and shows what came back; every label,
 * every column, every required marker is the definition's.
 *
 * **What the server sends back is shown, not swallowed** (#164 AC5). JFSS S8.1
 * makes the backend re-evaluate every `calculate` expression and store its own
 * answer, so the payload that comes back from a submission is not necessarily
 * the one that went out. *A form that changes your number without saying so is
 * its own defect* — so where the two differ, this page says which fields moved
 * and what to.
 */
const route = useRoute()

const form = ref<Form | null>(null)
const loading = ref(true)
const failed = ref(false)

/** The submission in flight, so a double press is not two documents. */
const submitting = ref(false)
/** What the server stored, once it has stored something. */
const submission = ref<FormSubmission | null>(null)
/** S10.3 details from a refusal, handed to the renderer to place by `path`. */
const serverViolations = ref<ValidationDetail[]>([])
/**
 * A refusal with nothing to place against a field.
 *
 * A 403, a 409 on an unpublished revision, a dead backend. Surfaced verbatim
 * rather than swallowed, so a refused submit is never a silent no-op — the
 * wording is the backend's, because repeating it in our own words risks
 * contradicting it (coding standard §3.3).
 */
const submitError = ref('')

/** The payload as it left the browser, for comparing against what came back. */
const submitted = ref<Record<string, unknown>>({})

async function load(id: string): Promise<void> {
  loading.value = true
  failed.value = false
  reset()

  try {
    form.value = await getForm(id)
  } catch {
    // The message is chosen here, never composed in the API layer (coding
    // standard §3.3). Which failure it was — 403, 404, a dead backend — is
    // deliberately not distinguished on screen: a reader who may not open this
    // form should not learn from the wording whether it exists.
    failed.value = true
    form.value = null
  } finally {
    loading.value = false
  }
}

function reset(): void {
  submission.value = null
  serverViolations.value = []
  submitError.value = ''
}

watch(
  () => route.params.id,
  (id) => load(String(id)),
  { immediate: true },
)

/**
 * What a button means.
 *
 * Only `submit` does anything here. `reset` and `navigate` are shapes JFSS
 * allows an action to declare and nothing in Kelir binds yet; a page that
 * silently ignored one would look identical to a page whose button is broken,
 * so an unbound action says so.
 */
async function onAction(action: string, values: Record<string, unknown>): Promise<void> {
  if (action !== 'submit') {
    submitError.value = `This form declares a "${action}" button, which is not wired to anything yet.`
    return
  }

  if (!form.value || submitting.value) {
    return
  }

  reset()
  submitting.value = true
  // Every data key, hidden ones included — JFSS S10.1, and S10.1.1 for why the
  // hidden ones go: a conditional that depends on a hidden field would
  // otherwise be decided from different inputs on the two sides.
  submitted.value = { ...values }

  try {
    submission.value = await submitForm(form.value.id, values)
  } catch (error) {
    if (error instanceof ApiError && error.isValidation) {
      // Placed against the fields by `path`, which is the whole reason S10.3
      // names it `path` and not `key`.
      serverViolations.value = error.details
      submitError.value = 'This form was not accepted. The fields below say why.'
    } else if (error instanceof ApiError) {
      submitError.value = error.message
    } else {
      submitError.value = 'Something went wrong. Try again.'
    }
  } finally {
    submitting.value = false
  }
}

/**
 * A value encoded so that two equal values encode identically.
 *
 * **Object keys are sorted, and that is not tidiness.** `serde_json` serializes
 * a map in key order and JavaScript serializes an object in insertion order, so
 * a datagrid row that came back completely unchanged stringifies differently on
 * the two sides — and the comparison below would report every row of every
 * submission as a value the server had altered. Which would be the worst
 * possible failure for this particular banner: the one that is supposed to
 * appear only when something is genuinely wrong, crying wolf on every submit.
 */
function canonical(value: unknown): string {
  return JSON.stringify(value, (_key, inner) => {
    if (typeof inner !== 'object' || inner === null || Array.isArray(inner)) {
      return inner
    }

    return Object.fromEntries(
      Object.entries(inner as Record<string, unknown>).sort(([left], [right]) =>
        left < right ? -1 : left > right ? 1 : 0,
      ),
    )
  })
}

/**
 * The fields the server computed differently from the browser.
 *
 * **Expected to be empty, and shown when it is not.** The two sides run one
 * engine compiled for two runtimes (**D-10**) and `parity/forms.json` holds
 * them to the same answers over whole submissions, so a difference here is a
 * parity defect rather than a routine correction — which is exactly why it is
 * on the screen instead of in a log.
 *
 * Compared only over the keys the stored payload carries: the server discards
 * the values of components it computed as hidden (S10.2), and a discarded field
 * is the pattern working rather than a disagreement.
 *
 * **It compares against what this page sent**, so a request rewritten between
 * the browser and the server is invisible here — correctly. The page has no
 * claim to make about a payload it did not produce; what the server did with
 * one is `kelir-backend/tests/rad_form_submissions.rs`'s and
 * `e2e/tests/a-form-is-submitted.spec.ts`'s.
 */
const corrections = computed(() => {
  const stored = submission.value?.payload

  if (!stored) {
    return []
  }

  return Object.entries(stored)
    .filter(([key, value]) => canonical(value) !== canonical(submitted.value[key]))
    .map(([key, value]) => ({ key, value: JSON.stringify(value) }))
})
</script>

<template>
  <div class="mx-auto w-full max-w-3xl space-y-6 p-6">
    <p v-if="loading" data-testid="form-loading" class="text-sm text-muted-foreground">Loading…</p>

    <Alert v-else-if="failed" variant="destructive" data-testid="form-error">
      This form could not be opened.
    </Alert>

    <template v-else-if="form">
      <header class="space-y-1">
        <h1 class="text-xl font-semibold" data-testid="form-title">{{ form.title }}</h1>
        <p class="text-sm text-muted-foreground">
          Revision {{ form.revision }} · JFSS {{ form.jfssVersion }}
        </p>
      </header>

      <!-- A refusal that no field can carry, and the sentence that sends the
           reader to the ones that can. -->
      <Alert v-if="submitError" variant="destructive" data-testid="submit-error">
        {{ submitError }}
      </Alert>

      <Alert v-if="submission" data-testid="submit-success">
        <p class="font-medium">Submitted.</p>
        <p class="mt-1">
          Stored against revision {{ submission.formRevision }} as <code>{{ submission.id }}</code
          >.
        </p>
      </Alert>

      <!-- The server recomputed something the browser had computed differently.
           This should never appear: both sides run the same pinned engine, and
           `parity/forms.json` fails the build if they stop agreeing. It is on
           the screen because a silent correction is the defect the whole
           Tamper-Proof argument is about. -->
      <Alert v-if="corrections.length" variant="destructive" data-testid="submit-corrections">
        <p class="font-medium">The server stored different values from the ones on screen.</p>
        <ul class="mt-1 list-disc pl-5">
          <li v-for="correction in corrections" :key="correction.key">
            <code>{{ correction.key }}</code> was stored as {{ correction.value }}
          </li>
        </ul>
      </Alert>

      <JfssForm
        :definition="form.definition"
        :server-violations="serverViolations"
        data-testid="jfss-form"
        @action="onAction"
      />
    </template>
  </div>
</template>
