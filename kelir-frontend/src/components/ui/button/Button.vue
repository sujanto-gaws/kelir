<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'
import { buttonVariants, type ButtonSize, type ButtonVariant } from './variants'

const props = withDefaults(
  defineProps<{
    variant?: ButtonVariant
    size?: ButtonSize
    type?: 'button' | 'submit' | 'reset'
    disabled?: boolean
    loading?: boolean
    class?: string
  }>(),
  {
    variant: 'default',
    size: 'default',
    type: 'button',
    disabled: false,
    loading: false,
    class: undefined,
  },
)

// A loading button stays disabled so a slow request cannot be submitted twice.
const isDisabled = computed(() => props.disabled || props.loading)

const classes = computed(() =>
  cn(buttonVariants({ variant: props.variant, size: props.size }), props.class),
)
</script>

<template>
  <button :type="type" :class="classes" :disabled="isDisabled" :aria-busy="loading">
    <span
      v-if="loading"
      class="mr-2 size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
      aria-hidden="true"
    />
    <slot />
  </button>
</template>
