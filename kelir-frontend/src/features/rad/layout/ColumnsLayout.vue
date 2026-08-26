<script setup lang="ts">
import { computed } from 'vue'

import type { JfssColumnSlot, JfssLayoutComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssLayoutComponent }>()

/**
 * Positional slots, one per column (JFSS §4.3, §4.3.1).
 *
 * The `group` slot is invoked **once per column**, which is the whole reason
 * columns cannot be flattened into one list of children: two columns of three
 * fields and one column of six are the same six components in the same order,
 * and only the slot boundaries tell them apart.
 */
const columns = computed<JfssColumnSlot[]>(() => props.component.columns ?? [])

/**
 * The column count is the definition's, capped at what Tailwind has classes for.
 *
 * A definition may declare more columns than four; it renders as four across
 * with the rest wrapping, which is a readable layout rather than a missing one.
 */
const GRID_CLASSES: Record<number, string> = {
  1: 'grid-cols-1',
  2: 'sm:grid-cols-2',
  3: 'sm:grid-cols-3',
  4: 'sm:grid-cols-4',
}

const gridClass = computed(
  () => GRID_CLASSES[Math.min(Math.max(columns.value.length, 1), 4)] ?? 'grid-cols-1',
)
</script>

<template>
  <div class="grid gap-4" :class="gridClass">
    <div v-for="(column, index) in columns" :key="index" class="space-y-4">
      <slot name="group" :components="column.components ?? []" />
    </div>
  </div>
</template>
