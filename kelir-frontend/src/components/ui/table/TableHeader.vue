<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ class?: string }>(), {
  class: undefined,
})

const classes = computed(() => cn('border-b border-border', props.class))
</script>

<template>
  <thead :class="classes">
    <slot />
  </thead>
</template>
