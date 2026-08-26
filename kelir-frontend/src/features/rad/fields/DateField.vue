<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Input } from '@/components/ui/input'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * `date`, `time` or `date-time`, from `validation.format`.
 *
 * The native control's value is already the ISO-8601 shape JFSS §5's formats
 * name, so what the user picks is what the payload carries — no parsing, no
 * locale, and nothing that renders differently on a machine set to
 * `dd/mm/yyyy`. A picker with its own format is a picker that eventually
 * submits `03/04/2026` and leaves the server to guess the month.
 */
const inputType = computed(() => {
  switch (props.component.validation.format) {
    case 'time':
      return 'time'
    case 'date-time':
      return 'datetime-local'
    default:
      return 'date'
  }
})

const value = computed({
  get: () => (props.modelValue == null ? '' : String(props.modelValue)),
  set: (next: string) => emit('update:modelValue', next === '' ? null : next),
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy, invalid }" :component="component">
    <Input
      :id="controlId"
      v-model="value"
      :type="inputType"
      :required="component.validation.required"
      :disabled="component.readOnly"
      :invalid="invalid"
      :described-by="describedBy"
    />
  </FieldShell>
</template>
