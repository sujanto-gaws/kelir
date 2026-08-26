<script setup lang="ts">
import { computed } from 'vue'

import type { JfssComponent, JfssLayoutComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssLayoutComponent }>()

/**
 * A single-slot container with an optional heading (JFSS §4.3).
 *
 * **The chrome, not the traversal.** Every layout component in this folder
 * exposes its children through one scoped slot named `group`, invoked once per
 * slot-group it holds — once here, once per column in
 * [`ColumnsLayout`](./ColumnsLayout.vue), once for the active tab in
 * [`TabsLayout`](./TabsLayout.vue). The renderer fills that slot and never
 * needs to know which of §4.3.1's three shapes it is looking at, and no layout
 * component imports the renderer back.
 */
const children = computed<JfssComponent[]>(() => props.component.components ?? [])

/**
 * `grid.columns`, as a class rather than an inline style.
 *
 * A whitelist because Tailwind compiles the classes it can see: an
 * interpolated `grid-cols-${n}` would be a class nobody generated and the
 * layout would silently be one column. Anything outside the range falls back
 * to a single column, which is a legible layout rather than a broken one.
 */
const GRID_CLASSES: Record<number, string> = {
  1: 'grid-cols-1',
  2: 'sm:grid-cols-2',
  3: 'sm:grid-cols-3',
  4: 'sm:grid-cols-4',
}

const gridClass = computed(() => GRID_CLASSES[props.component.grid?.columns ?? 1] ?? 'grid-cols-1')
</script>

<template>
  <section class="space-y-4 rounded-md border border-border p-4">
    <h2 v-if="component.title" class="text-base font-semibold">{{ component.title }}</h2>

    <div class="grid gap-4" :class="gridClass">
      <slot name="group" :components="children" />
    </div>
  </section>
</template>
