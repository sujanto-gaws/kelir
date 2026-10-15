<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

/**
 * A number property of a node: a length limit, a bound, a data grid's
 * `defaultItems` (#688 AC3, AC3b).
 *
 * **What is typed is kept in the box; only a value the property can hold is
 * written.** A length or a row count takes a whole number of 0 or more, a bound
 * any finite number, as the meta-schema says. Anything else is refused with a
 * note beside the box and writes nothing, so the definition keeps its last
 * good value — the meta-schema would refuse `-1` or `2.5` at save, and a
 * half-typed `-` is not a value at all. **Emptied, the property is removed**
 * (`undefined`), never written as `null`.
 *
 * It decides what a well-formed number is, not whether the limit makes sense:
 * that is the server's and the renderer's verdict.
 */
const props = withDefaults(
  defineProps<{
    id: string
    label: string
    modelValue: number | undefined
    /** A whole number of 0 or more, rather than any finite number. */
    count?: boolean
    disabled?: boolean
    /** A server message about this property (S10.3), shown under the box. */
    message?: string
  }>(),
  { count: false, disabled: false, message: undefined },
)

const emit = defineEmits<{ 'update:modelValue': [value: number | undefined] }>()

function shown(value: number | undefined): string {
  return value === undefined ? '' : String(value)
}

const text = ref(shown(props.modelValue))

// A value set from outside — a type switch that removed it, a reload — is
// shown; this box's own write coming back is already what it shows.
watch(
  () => props.modelValue,
  (value) => {
    if (parsed(text.value) !== value) {
      text.value = shown(value)
    }
  },
)

const NUMBER = /^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/
const COUNT = /^\d+$/

/** The number `raw` is, or `null` when it is not one this property can hold. */
function parsed(raw: string): number | undefined | null {
  const trimmed = raw.trim()

  if (trimmed === '') {
    return undefined
  }

  const value = Number(trimmed)

  if (!Number.isFinite(value)) {
    return null
  }

  return (props.count ? COUNT : NUMBER).test(trimmed) ? value : null
}

const refused = computed(() => parsed(text.value) === null)
const noteId = computed(() => `${props.id}-note`)
const describedBy = computed(
  () =>
    [refused.value ? noteId.value : '', props.message ? `${noteId.value}-server` : '']
      .filter((id) => id !== '')
      .join(' ') || undefined,
)

function onInput(raw: string): void {
  text.value = raw

  const value = parsed(raw)

  if (value !== null) {
    emit('update:modelValue', value)
  }
}
</script>

<template>
  <div class="space-y-2">
    <Label :for="id">{{ label }}</Label>
    <Input
      :id="id"
      :model-value="text"
      inputmode="decimal"
      :disabled="disabled"
      :invalid="refused || message !== undefined"
      :described-by="describedBy"
      :data-testid="id"
      @update:model-value="onInput(String($event))"
    />
    <p v-if="refused" :id="noteId" class="text-xs text-destructive" :data-testid="`${id}-refused`">
      {{
        count
          ? 'Enter a whole number of 0 or more. It was not saved.'
          : 'Enter a number. It was not saved.'
      }}
    </p>
    <p v-if="message" :id="`${noteId}-server`" class="text-xs text-destructive">{{ message }}</p>
  </div>
</template>
