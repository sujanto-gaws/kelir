<script setup lang="ts">
import { computed } from 'vue'

import { cn } from '@/lib/utils'
import type { JfssDisplayComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDisplayComponent }>()

/**
 * `heading` and `paragraph` — the text-bearing display types (JFSS §4.4).
 *
 * **Rendered as text, never as HTML.** §4.4 describes `content` as "the text or
 * HTML content to render", and the HTML half is declined: a definition is
 * stored data, and `v-html` over stored data is a stored-XSS sink that a form
 * author with `rad:form:create` could reach every reader of every document
 * through. The specification permits HTML; it does not require an
 * implementation to interpret it, and a heading that shows its own markup is a
 * visible defect where an injected script is an invisible one.
 *
 * **A `calculate`-only display renders empty here, and that is #163's.** §4.4
 * lets a display take its text from an expression instead of `content`.
 * Evaluating it is the next issue by the sprint plan's own split, so a
 * definition whose totals are computed shows its labels and not yet its
 * numbers.
 */
const level = computed(() => (props.component.type === 'heading' ? 'h3' : 'p'))

/** `variant` is the definition's styling hint (§4.4), not a semantic level. */
const classes = computed(() =>
  cn(
    props.component.type === 'heading'
      ? 'text-base font-semibold'
      : 'text-sm text-muted-foreground',
    props.component.variant === 'muted' && 'text-muted-foreground',
  ),
)
</script>

<template>
  <component :is="level" :class="classes">{{ component.content ?? '' }}</component>
</template>
