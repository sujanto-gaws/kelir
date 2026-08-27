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
 * **`content` is whatever the definition resolved to, and this component does
 * not know which.** §4.4 lets a display take its text from `calculate` instead
 * of `content`; the renderer evaluates it and hands the result down as
 * `content`, so there is one property here and no branch. A failed calculation
 * arrives as an empty string, which is what construction plan §5.6 asks a
 * failed calculation to look like everywhere.
 *
 * **And it is not formatted.** A computed total renders as the number the
 * expression produced — no currency prefix, no fixed decimals. Formatting
 * decided inside a renderer is the reference implementation's defect and #162
 * AC3's stated anti-pattern.
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
