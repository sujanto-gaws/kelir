<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    id?: string
    placeholder?: string
    rows?: number
    disabled?: boolean
    invalid?: boolean
    describedBy?: string
    class?: string
  }>(),
  {
    id: undefined,
    placeholder: undefined,
    rows: 3,
    disabled: false,
    invalid: false,
    describedBy: undefined,
    class: undefined,
  },
)

const model = defineModel<string>({ default: '' })

const classes = computed(() =>
  cn(
    'flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm',
    'placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50',
    props.invalid && 'border-destructive',
    props.class,
  ),
)
</script>

<template>
  <textarea
    :id="id"
    v-model="model"
    :class="classes"
    :rows="rows"
    :placeholder="placeholder"
    :disabled="disabled"
    :aria-invalid="invalid"
    :aria-describedby="describedBy"
  />
</template>
