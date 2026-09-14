<script setup lang="ts">
import { computed } from 'vue'
import { Direction, Orientation } from '@unovis/ts'
import { VisAxis, VisGroupedBar, VisXYContainer } from '@unovis/vue'

import { DOCUMENT_STATUS_LABELS } from '@/types/document'
import type { DocumentStatusCount } from '@/types/reporting'

/**
 * The caller's documents by status, drawn (FR-RPT-004, #447).
 *
 * **A picture of numbers the card already states in words.** Unovis draws with
 * d3 into an SVG sized by a `ResizeObserver`, which a screen reader cannot read
 * as data and jsdom does not render at all — so the counts are text in
 * `DashboardPage.vue` beside this, and this is `aria-hidden`. The trade is some
 * repetition on screen, against a chart being the only place a number lives.
 *
 * **Never imported directly.** The page reaches it through
 * `defineAsyncComponent`, because Unovis brings d3 and the dashboard is the
 * route every session lands on. `scripts/check-bundle-split.mjs` fails the
 * build when that edge becomes a static one.
 *
 * **In the dashboard feature rather than `src/components/`**, because it is
 * shaped around `DocumentStatusCount` and nothing else draws a chart.
 * `components/ui` holds shadcn-vue's generated primitives; a second chart is
 * the point at which a shared one earns its place, and not before.
 */
const props = defineProps<{
  /** The server's rows, in the server's order. Nothing here sorts them. */
  counts: DocumentStatusCount[]
}>()

/** Each bar sits at its row's position, so the order drawn is the order sent. */
function barPosition(_row: DocumentStatusCount, index: number): number {
  return index
}

function barLength(row: DocumentStatusCount): number {
  return row.count
}

/**
 * **One colour, from the theme.** Ten statuses of one measure are one series,
 * and ten colours would read as ten kinds of thing. The token is the one
 * `.dark` redefines, so the bars follow the theme without a branch here.
 */
const barColor = 'var(--color-primary)'

/** One tick per row, so every status is labelled and none is skipped for space. */
const statusTicks = computed(() => props.counts.map((_row, index) => index))

function statusLabel(tick: number | Date): string {
  const row = props.counts[Number(tick)]

  return row ? DOCUMENT_STATUS_LABELS[row.status] : ''
}

/** A count is whole documents, so the axis never labels half of one. */
function countLabel(tick: number | Date): string {
  return Number.isInteger(tick) ? String(tick) : ''
}
</script>

<template>
  <!--
    The axis and grid colours are Unovis's own CSS variables pointed at the
    theme's tokens, for the reason `barColor` gives.
  -->
  <div
    aria-hidden="true"
    class="[--vis-axis-domain-color:var(--color-border)] [--vis-axis-grid-color:var(--color-border)] [--vis-axis-tick-color:var(--color-border)] [--vis-axis-tick-label-color:var(--color-muted-foreground)] [--vis-font-family:inherit]"
    data-testid="status-chart"
  >
    <VisXYContainer :data="counts" :height="counts.length * 28" :y-direction="Direction.South">
      <VisGroupedBar
        :x="barPosition"
        :y="barLength"
        :color="barColor"
        :orientation="Orientation.Horizontal"
        :rounded-corners="4"
      />
      <VisAxis type="x" :tick-format="countLabel" :grid-line="true" />
      <VisAxis type="y" :tick-values="statusTicks" :tick-format="statusLabel" :grid-line="false" />
    </VisXYContainer>
  </div>
</template>
