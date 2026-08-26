<script setup lang="ts">
import { ref, watch } from 'vue'
import { useRoute } from 'vue-router'

import JfssForm from './JfssForm.vue'
import { getForm } from '@/api/rad'
import { Alert } from '@/components/ui/alert'
import type { Form } from '@/types/rad'

/**
 * A published form definition, opened and filled in (FR-RAD-010, #162).
 *
 * **The first RAD surface in the frontend**, and the page the browser harness
 * is pointed at — #162 AC4 asks for the renderer driven through #153 rather
 * than asserted through component tests alone, and a renderer with no page has
 * nothing to drive.
 *
 * **Nothing about a specific form is here** (AC3). The page fetches by id,
 * hands the definition to the renderer and shows what came back; every label,
 * every column, every required marker is the definition's.
 */
const route = useRoute()

const form = ref<Form | null>(null)
const loading = ref(true)
const failed = ref(false)

/** The last action a button raised, so the harness can see one arrive. */
const lastAction = ref<string | null>(null)

async function load(id: string): Promise<void> {
  loading.value = true
  failed.value = false
  lastAction.value = null

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

watch(() => route.params.id, (id) => load(String(id)), { immediate: true })

/**
 * What a button means.
 *
 * **Recorded and not acted on**, because submit is #164 and the sprint plan
 * splits them on purpose. Showing that the action arrived is the honest
 * intermediate state: a button wired to nothing looks identical to a button
 * that is broken, and this page is what the harness reads.
 */
function onAction(action: string): void {
  lastAction.value = action
}
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

      <JfssForm :definition="form.definition" data-testid="jfss-form" @action="onAction" />

      <p v-if="lastAction" data-testid="form-action" class="text-sm text-muted-foreground">
        Action received: {{ lastAction }} — submitting is not built yet.
      </p>
    </template>
  </div>
</template>
