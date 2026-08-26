<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Input } from '@/components/ui/input'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * **A number field emits a number, or `null` — never a numeric string.**
 *
 * `<input type="number">` hands back a string like every other input, and a
 * payload carrying `"12"` where the definition says `number` is a payload the
 * server has to coerce before it can evaluate anything. It would mostly work:
 * the engine's numeric coercion turns `"12"` into 12. Mostly-working coercion
 * on the boundary between two runtimes is exactly what the operator-parity
 * spike was about, so the conversion happens once, here, where the definition
 * says what the type is.
 *
 * An empty box is `null` rather than `0`: a field nobody has filled in and a
 * field somebody set to zero are different facts, and `required` is what
 * decides whether the first is allowed (#163).
 */
const value = computed({
  get: () => (props.modelValue == null ? '' : String(props.modelValue)),
  set: (next: string) => {
    const trimmed = next.trim()

    if (trimmed === '') {
      emit('update:modelValue', null)
      return
    }

    const parsed = Number(trimmed)

    // A partially typed number — "1e", "-" — parses to NaN. Emitting NaN would
    // put a value in the payload that JSON cannot carry (`JSON.stringify(NaN)`
    // is `null`), so the half-typed state is held as null and the box keeps
    // showing what the user typed.
    emit('update:modelValue', Number.isFinite(parsed) ? parsed : null)
  },
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy }" :component="component">
    <Input
      :id="controlId"
      v-model="value"
      type="number"
      :placeholder="component.placeholder"
      :required="component.validation.required"
      :disabled="component.readOnly"
      :described-by="describedBy"
    />
  </FieldShell>
</template>
