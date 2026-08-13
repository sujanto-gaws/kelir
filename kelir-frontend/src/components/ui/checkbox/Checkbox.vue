<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    id?: string
    value?: string
    disabled?: boolean
    class?: string
  }>(),
  {
    id: undefined,
    value: undefined,
    disabled: false,
    class: undefined,
  },
)

const model = defineModel<boolean>({ default: false })

const classes = computed(() =>
  cn(
    'size-4 rounded border-input accent-primary',
    'disabled:cursor-not-allowed disabled:opacity-50',
    props.class,
  ),
)
</script>

<template>
  <!-- A real input, not a styled div: keyboard and screen-reader behaviour comes
       free and the browser owns the checked state. -->
  <input
    :id="id"
    v-model="model"
    type="checkbox"
    :class="classes"
    :value="value"
    :disabled="disabled"
  />
</template>
