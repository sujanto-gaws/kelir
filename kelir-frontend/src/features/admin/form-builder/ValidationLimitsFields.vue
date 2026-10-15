<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { ValidationDetail } from '@/types/api'
import type { JfssValidation } from '@/types/jfss'

import PropertyNumberInput from './PropertyNumberInput.vue'
import { limitConflicts, messageAt, offeredLimits, withProperty, type Limit } from './propertyEdits'

/**
 * A data field's validation limits (#688 AC3b, **D-116**).
 *
 * **It offers the limits the field's `validation.type` can carry, and no
 * other**: lengths and a pattern for a string, bounds for a number or an
 * integer. A limit the type cannot carry that was loaded anyway is kept, not
 * shown, until the value type is switched (`switchValueType`).
 *
 * **It evaluates nothing** (D-10). A `pattern` is a string the author types:
 * this panel never compiles it, so a pattern the server's engine refuses comes
 * back as an S10.3 detail at `validation.pattern` and is shown under the box
 * (#604 and #624 are the server's and the renderer's). A lower bound above its
 * upper one is flagged beside the two, and still saved as typed.
 */
const props = defineProps<{
  componentId: string
  /** The node's component type: only a text box and a number are offered limits. */
  componentType: string
  /** The node's path, for the server's details (S10.3). */
  nodePath: string
  validation: JfssValidation
  disabled: boolean
  details: ValidationDetail[]
}>()

const emit = defineEmits<{ 'update:validation': [validation: JfssValidation] }>()

const offered = computed(() => offeredLimits(props.componentType, props.validation.type))
const conflicts = computed(() => limitConflicts(props.validation))
/** A lower bound above its upper one, each flagged only where its two fields are offered. */
const lengthConflict = computed(() => conflicts.value.length && offered.value.includes('minLength'))
const rangeConflict = computed(() => conflicts.value.range && offered.value.includes('minimum'))

const LABELS: Record<Limit, string> = {
  minLength: 'Minimum length',
  maxLength: 'Maximum length',
  pattern: 'Pattern',
  minimum: 'Minimum',
  maximum: 'Maximum',
}

const TEST_IDS: Record<Limit, string> = {
  minLength: 'min-length',
  maxLength: 'max-length',
  pattern: 'pattern',
  minimum: 'minimum',
  maximum: 'maximum',
}

function idOf(limit: Limit): string {
  return `${TEST_IDS[limit]}-${props.componentId}`
}

function messageFor(limit: Limit): string | undefined {
  return messageAt(props.details, props.nodePath, `validation.${limit}`)
}

function set(limit: Limit, value: unknown): void {
  emit('update:validation', withProperty(props.validation, limit, value))
}

function numberOf(limit: Limit): number | undefined {
  const value = props.validation[limit]

  return typeof value === 'number' ? value : undefined
}
</script>

<template>
  <div
    v-if="offered.length > 0"
    class="grid gap-4 sm:col-span-2 sm:grid-cols-2"
    :data-testid="`limits-${componentId}`"
  >
    <template v-for="limit in offered" :key="limit">
      <div v-if="limit === 'pattern'" class="space-y-2 sm:col-span-2">
        <Label :for="idOf(limit)">{{ LABELS[limit] }}</Label>
        <Input
          :id="idOf(limit)"
          class="font-mono"
          :model-value="validation.pattern ?? ''"
          :disabled="disabled"
          :invalid="messageFor(limit) !== undefined"
          :data-testid="idOf(limit)"
          @update:model-value="set('pattern', String($event))"
        />
        <p
          v-if="messageFor(limit)"
          class="text-xs text-destructive"
          :data-testid="`${idOf(limit)}-error`"
        >
          {{ messageFor(limit) }}
        </p>
        <p v-else class="text-xs text-muted-foreground">
          A regular expression found anywhere in the value; start it with ^ and end it with $ to
          match the whole value. The server checks it when the form is saved.
        </p>
      </div>

      <PropertyNumberInput
        v-else
        :id="idOf(limit)"
        :label="LABELS[limit]"
        :model-value="numberOf(limit)"
        :count="limit === 'minLength' || limit === 'maxLength'"
        :disabled="disabled"
        :message="messageFor(limit)"
        @update:model-value="set(limit, $event)"
      />
    </template>

    <p
      v-if="lengthConflict"
      class="text-xs text-destructive sm:col-span-2"
      :data-testid="`limits-conflict-length-${componentId}`"
    >
      The minimum length is greater than the maximum length, so no value can pass. It is saved as
      typed.
    </p>
    <p
      v-if="rangeConflict"
      class="text-xs text-destructive sm:col-span-2"
      :data-testid="`limits-conflict-range-${componentId}`"
    >
      The minimum is greater than the maximum, so no value can pass. It is saved as typed.
    </p>
  </div>
</template>
