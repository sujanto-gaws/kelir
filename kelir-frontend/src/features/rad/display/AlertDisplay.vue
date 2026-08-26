<script setup lang="ts">
import { computed } from 'vue'

import { Alert } from '@/components/ui/alert'
import type { JfssDisplayComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDisplayComponent }>()

/**
 * `alert` — a callout the definition places (JFSS §4.4).
 *
 * `variant` maps to the two the UI kit has. Anything else falls back to the
 * default rather than passing an unknown variant through: a form author writing
 * `variant: "success"` gets a neutral callout, not an unstyled one.
 */
const variant = computed(() =>
  props.component.variant === 'warning' || props.component.variant === 'error'
    ? 'destructive'
    : 'default',
)
</script>

<template>
  <!-- Text, not HTML, for the reason `TextDisplay` gives. -->
  <Alert :variant="variant">{{ component.content ?? '' }}</Alert>
</template>
