<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Select } from '@/components/ui/select'
import type { JfssDataComponent, JfssOption } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * The choices, from `options` or from `validation.enum`.
 *
 * JFSS §4.2 makes `options` the property for a `select`, and §5 lets
 * `validation.enum` constrain a value to a set. A definition may carry either.
 * `options` wins where both are present, because it is the one that also
 * carries a label — an enum offers values and a form offers choices, and where
 * an author has written labels they meant them.
 */
const options = computed<JfssOption[]>(() => {
  if (props.component.options) {
    return props.component.options
  }

  return (props.component.validation.enum ?? []).map((value) => ({
    label: String(value),
    value,
  }))
})

/**
 * A native `<select>` carries strings, and JFSS option values are any JSON.
 *
 * So each option is addressed by its **index** on the wire and mapped back to
 * the value the definition wrote. Stringifying the value instead would collapse
 * the number `1` and the string `"1"` onto one option, and a definition is
 * entitled to offer both.
 */
const choices = computed(() =>
  options.value.map((option, index) => ({ value: String(index), label: option.label })),
)

const selectedIndex = computed({
  get: () => {
    const index = options.value.findIndex((option) => option.value === props.modelValue)

    return index === -1 ? '' : String(index)
  },
  set: (next: string) => {
    if (next === '') {
      emit('update:modelValue', null)
      return
    }

    emit('update:modelValue', options.value[Number(next)]?.value ?? null)
  },
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy, invalid }" :component="component">
    <Select
      :id="controlId"
      v-model="selectedIndex"
      :options="choices"
      :placeholder="component.placeholder"
      :disabled="component.readOnly"
      :invalid="invalid"
      :described-by="describedBy"
    />
  </FieldShell>
</template>
