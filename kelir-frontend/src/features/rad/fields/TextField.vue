<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Input } from '@/components/ui/input'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * The HTML input type, taken from JFSS `validation.format`.
 *
 * The browser's own affordances for an email or a URL — the keyboard a phone
 * shows, the built-in hint — come free with the right `type` and cost nothing
 * to opt into. **This is not validation:** an `email` input does not refuse a
 * bad address here, `validation.ts` does that from `validation.format`, and the
 * two read the same property so they cannot disagree about which fields are
 * addresses.
 */
const inputType = computed(() => {
  switch (props.component.validation.format) {
    case 'email':
      return 'email'
    case 'uri':
      return 'url'
    default:
      return 'text'
  }
})

const value = computed({
  get: () => (props.modelValue == null ? '' : String(props.modelValue)),
  set: (next: string) => emit('update:modelValue', next),
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy, invalid }" :component="component">
    <Input
      :id="controlId"
      v-model="value"
      :type="inputType"
      :placeholder="component.placeholder"
      :required="component.validation.required"
      :disabled="component.readOnly"
      :invalid="invalid"
      :described-by="describedBy"
    />
  </FieldShell>
</template>
