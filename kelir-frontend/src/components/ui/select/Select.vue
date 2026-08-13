<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    id?: string
    options: { value: string; label: string }[]
    placeholder?: string
    disabled?: boolean
    invalid?: boolean
    describedBy?: string
    class?: string
  }>(),
  {
    id: undefined,
    placeholder: undefined,
    disabled: false,
    invalid: false,
    describedBy: undefined,
    class: undefined,
  },
)

const model = defineModel<string>({ default: '' })

// Same box as Input so the two line up on a shared form row.
const classes = computed(() =>
  cn(
    'flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm',
    'disabled:cursor-not-allowed disabled:opacity-50',
    props.invalid && 'border-destructive',
    props.class,
  ),
)
</script>

<template>
  <!-- Native select: the platform already gives us a keyboard-accessible,
       mobile-friendly picker that no hand-rolled popup matches. -->
  <select
    :id="id"
    v-model="model"
    :class="classes"
    :disabled="disabled"
    :aria-invalid="invalid"
    :aria-describedby="describedBy"
  >
    <option v-if="placeholder !== undefined" value="">{{ placeholder }}</option>
    <option v-for="option in options" :key="option.value" :value="option.value">
      {{ option.label }}
    </option>
  </select>
</template>
