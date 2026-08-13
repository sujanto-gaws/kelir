<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ class?: string }>(), {
  class: undefined,
})

const classes = computed(() => cn('px-3 py-2 align-middle', props.class))
</script>

<template>
  <td :class="classes">
    <slot />
  </td>
</template>
