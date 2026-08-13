<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'

const props = withDefaults(
  defineProps<{
    scope?: 'col' | 'row'
    class?: string
  }>(),
  {
    scope: 'col',
    class: undefined,
  },
)

const classes = computed(() =>
  cn('h-10 px-3 text-left align-middle text-muted-foreground font-medium', props.class),
)
</script>

<template>
  <!-- scope is explicit so a row header (scope="row") still announces correctly. -->
  <th :scope="scope" :class="classes">
    <slot />
  </th>
</template>
