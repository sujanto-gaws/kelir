<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Textarea } from '@/components/ui/textarea'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

const value = computed({
  get: () => (props.modelValue == null ? '' : String(props.modelValue)),
  set: (next: string) => emit('update:modelValue', next),
})
</script>

<template>
  <FieldShell v-slot="{ controlId, describedBy }" :component="component">
    <Textarea
      :id="controlId"
      v-model="value"
      :placeholder="component.placeholder"
      :disabled="component.readOnly"
      :described-by="describedBy"
    />
  </FieldShell>
</template>
