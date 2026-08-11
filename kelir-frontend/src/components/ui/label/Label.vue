<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ for?: string; class?: string }>(), {
  for: undefined,
  class: undefined,
})

const classes = computed(() => cn('text-sm font-medium leading-none', props.class))
</script>

<template>
  <label :for="props.for" :class="classes">
    <slot />
  </label>
</template>
