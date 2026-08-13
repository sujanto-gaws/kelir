<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'
import { badgeVariants, type BadgeVariant } from './variants'

const props = withDefaults(
  defineProps<{
    variant?: BadgeVariant
    class?: string
  }>(),
  {
    variant: 'default',
    class: undefined,
  },
)

const classes = computed(() => cn(badgeVariants({ variant: props.variant }), props.class))
</script>

<template>
  <span :class="classes">
    <slot />
  </span>
</template>
