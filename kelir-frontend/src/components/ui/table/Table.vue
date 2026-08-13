<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(defineProps<{ class?: string }>(), {
  class: undefined,
})

const classes = computed(() => cn('w-full caption-bottom text-sm', props.class))
</script>

<template>
  <!-- The scroll container is the wrapper, not the table: a list with many
       columns must not widen the page (SDD §5.3 list rendering). -->
  <div class="w-full overflow-x-auto">
    <table :class="classes">
      <slot />
    </table>
  </div>
</template>
