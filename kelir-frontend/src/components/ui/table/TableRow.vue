<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ class?: string }>(), {
  class: undefined,
})

const classes = computed(() =>
  cn('border-b border-border transition-colors hover:bg-secondary/50', props.class),
)
</script>

<template>
  <tr :class="classes">
    <slot />
  </tr>
</template>
