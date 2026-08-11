<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    id?: string
    type?: string
    placeholder?: string
    autocomplete?: string
    required?: boolean
    disabled?: boolean
    invalid?: boolean
    describedBy?: string
    class?: string
  }>(),
  {
    id: undefined,
    type: 'text',
    placeholder: undefined,
    autocomplete: undefined,
    required: false,
    disabled: false,
    invalid: false,
    describedBy: undefined,
    class: undefined,
  },
)

const model = defineModel<string>({ default: '' })

const classes = computed(() =>
  cn(
    'flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm',
    'placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50',
    props.invalid && 'border-destructive',
    props.class,
  ),
)
</script>

<template>
  <input
    :id="id"
    v-model="model"
    :type="type"
    :class="classes"
    :placeholder="placeholder"
    :autocomplete="autocomplete"
    :required="required"
    :disabled="disabled"
    :aria-invalid="invalid"
    :aria-describedby="describedBy"
  />
</template>
