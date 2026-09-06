<script setup lang="ts">
import { ref, watch } from 'vue'

import { getNumberingRule, setNumberingRule } from '@/api/document-types'
import { toApiError } from '@/api/client'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { DocumentTypeSummary, GapPolicy, SequenceScope } from '@/types/document-type'

/**
 * A document type's numbering rule (FR-DTYPE-002; #341 AC1).
 *
 * **Its own dialog because it is its own sub-resource.** A type has one rule or
 * none, so the endpoint is a `PUT` rather than a `POST` that would conflict the
 * second time — and folding it into the type dialog would make creating a type
 * require deciding how its documents are numbered.
 *
 * **A type with no rule is ordinary, not an error.** The read answers 404 and
 * this treats it as *not configured yet*: a rule is set when somebody decides
 * the numbering, which is often after the type exists.
 */
const props = defineProps<{ open: boolean; documentType: DocumentTypeSummary | null }>()
const emit = defineEmits<{ (event: 'close'): void; (event: 'saved'): void }>()

const template = ref('')
const scope = ref<SequenceScope>('YEAR')
const padding = ref('6')
const policy = ref<GapPolicy>('GAPLESS')

const isLoading = ref(false)
const isSaving = ref(false)
const error = ref('')
const details = ref<ValidationDetail[]>([])
/** True once the type is known to have no rule, so the copy can say so. */
const isNew = ref(false)

const SCOPES: { value: SequenceScope; label: string }[] = [
  { value: 'GLOBAL', label: 'Global — one sequence, forever' },
  { value: 'YEAR', label: 'Year — restarts each calendar year' },
  { value: 'MONTH', label: 'Month — restarts each calendar month' },
  { value: 'DEPARTMENT_YEAR', label: 'Department and year — one sequence per department' },
]

const POLICIES: { value: GapPolicy; label: string }[] = [
  { value: 'GAPLESS', label: 'Gapless — a failed submission gives its number back' },
  { value: 'ALLOW_GAPS', label: 'Allow gaps — a failed submission leaves a hole' },
]

function messageFor(path: string): string {
  return details.value.find((detail) => detail.path === path)?.message ?? ''
}

async function load(): Promise<void> {
  const documentType = props.documentType

  if (!documentType) {
    return
  }

  isLoading.value = true
  error.value = ''
  details.value = []
  isNew.value = false

  try {
    const rule = await getNumberingRule(documentType.id)

    template.value = rule.ruleTemplate
    scope.value = rule.sequenceScope
    padding.value = String(rule.sequencePadding)
    policy.value = rule.gapPolicy
  } catch (failure) {
    const failed = toApiError(failure)

    if (failed.status === 404) {
      // Not configured yet, which is a state and not a failure. The defaults
      // below are the ones the backend itself applies.
      isNew.value = true
      template.value = `${documentType.typeCode}-{year}-{sequence}`
      scope.value = 'YEAR'
      padding.value = '6'
      policy.value = 'GAPLESS'
    } else {
      error.value = failed.message
    }
  } finally {
    isLoading.value = false
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      void load()
    }
  },
  { immediate: true },
)

async function save(): Promise<void> {
  const documentType = props.documentType

  if (!documentType) {
    return
  }

  isSaving.value = true
  error.value = ''
  details.value = []

  try {
    await setNumberingRule(documentType.id, {
      ruleTemplate: template.value.trim(),
      sequenceScope: scope.value,
      sequencePadding: Number(padding.value) || undefined,
      gapPolicy: policy.value,
    })

    emit('saved')
  } catch (failure) {
    const failed = toApiError(failure)

    error.value = failed.message
    details.value = failed.details
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <div
    v-if="open && documentType"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    aria-modal="true"
    aria-label="Numbering rule"
    data-testid="numbering-dialog"
  >
    <div class="w-full max-w-xl rounded-lg bg-background p-6 shadow-lg">
      <h3 class="text-lg font-semibold">Numbering rule — {{ documentType.typeCode }}</h3>

      <p v-if="isNew" class="mt-1 text-sm text-muted-foreground" data-testid="numbering-is-new">
        This type has no numbering rule yet. Its documents cannot be submitted until it does.
      </p>

      <Alert v-if="error" variant="destructive" class="mt-4" data-testid="numbering-error">
        {{ error }}
      </Alert>

      <p v-if="isLoading" class="mt-4 text-sm text-muted-foreground">Loading…</p>

      <form v-else class="mt-4 space-y-4" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="rule-template">Template</Label>
          <Input id="rule-template" v-model="template" data-testid="rule-template" />
          <!-- `{sequence}` is required and the backend says so: a template
               without it names every document the same thing, and the unique
               index would refuse the second one at submit — having already
               taken a number. -->
          <p class="text-xs text-muted-foreground">
            <code>{sequence}</code> is required. <code>{year}</code>, <code>{month}</code> and
            <code>{typeCode}</code> are also available.
          </p>
          <p v-if="messageFor('ruleTemplate')" class="text-xs text-destructive">
            {{ messageFor('ruleTemplate') }}
          </p>
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-2">
            <Label for="rule-scope">Sequence restarts</Label>
            <Select id="rule-scope" v-model="scope" data-testid="rule-scope" :options="SCOPES" />
          </div>

          <div class="space-y-2">
            <Label for="rule-padding">Padding</Label>
            <Input id="rule-padding" v-model="padding" type="number" data-testid="rule-padding" />
            <p v-if="messageFor('sequencePadding')" class="text-xs text-destructive">
              {{ messageFor('sequencePadding') }}
            </p>
          </div>
        </div>

        <div class="space-y-2">
          <Label for="rule-policy">If a submission fails</Label>
          <Select id="rule-policy" v-model="policy" data-testid="rule-policy" :options="POLICIES" />
        </div>

        <div class="flex justify-end gap-2">
          <Button type="button" variant="secondary" @click="emit('close')">Cancel</Button>
          <Button type="submit" :disabled="isSaving" data-testid="save-numbering-rule">
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
