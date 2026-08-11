<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ variant?: 'default' | 'destructive'; class?: string }>(), {
  variant: 'default',
  class: undefined,
})

const classes = computed(() =>
  cn(
    'rounded-md border px-4 py-3 text-sm',
    props.variant === 'destructive'
      ? 'border-destructive/50 bg-destructive/10 text-destructive'
      : 'border-border bg-card text-card-foreground',
    props.class,
  ),
)
</script>

<template>
  <!-- role=alert so a failure is announced, not only shown (NFR-USE-005). -->
  <div :class="classes" role="alert">
    <slot />
  </div>
</template>
