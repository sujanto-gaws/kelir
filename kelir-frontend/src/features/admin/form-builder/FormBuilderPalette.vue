<script setup lang="ts">
import { computed } from 'vue'
import { GripVertical } from '@lucide/vue'
import { VueDraggable } from 'vue-draggable-plus'

import { Button } from '@/components/ui/button'

import { PALETTE_GROUPS, offeredEntries, type PaletteGroup } from './builderRegistry'
import { useFormBuilder } from './useFormBuilder'

/**
 * The palette: every type the builder offers, which is every type the
 * renderer draws (#688 B2), grouped by role.
 *
 * **A button adds to the selected container**, or to the container of the
 * selected node, or to the form when nothing is selected (D2); the line under
 * the heading says which. A type the target cannot take — a panel into a row
 * template — is disabled there rather than refused after the click.
 *
 * **Its entries can also be dragged onto the canvas**, from their handle, as
 * a copy (Sortable's `pull: 'clone'`). The drop is reported as an add, and
 * goes through the same `add` the buttons call.
 */
const builder = useFormBuilder()

const target = computed(() => builder.paletteTarget.value)

function entriesOf(group: PaletteGroup) {
  return offeredEntries().filter((entry) => entry.group === group)
}

interface SortEvent {
  from: HTMLElement
  to: HTMLElement
  item: HTMLElement
  newIndex?: number
  newDraggableIndex?: number
}

const GROUP = { name: 'jfss-canvas', pull: 'clone' as const, put: false }

function onStart(event: SortEvent): void {
  const type = event.item?.dataset.paletteType

  builder.dragging.value = type ? { kind: 'new', type } : null
}

function onEnd(event: SortEvent): void {
  const type = event.item?.dataset.paletteType
  const toListPath = event.to?.dataset.listPath

  if (!type || !toListPath || event.to === event.from) {
    builder.dragging.value = null

    return
  }

  builder.dropNew(type, toListPath, event.newDraggableIndex ?? event.newIndex ?? 0)
}
</script>

<template>
  <aside class="space-y-4" aria-labelledby="palette-heading" data-testid="form-palette">
    <div>
      <h3 id="palette-heading" class="text-sm font-semibold">Components</h3>
      <p class="text-xs text-muted-foreground" data-testid="palette-target">
        Adds to {{ target.name }}
      </p>
    </div>

    <section v-for="group in PALETTE_GROUPS" :key="group.group" class="space-y-1">
      <h4 class="text-xs font-medium uppercase tracking-wide text-muted-foreground">
        {{ group.label }}
      </h4>

      <VueDraggable
        :model-value="entriesOf(group.group)"
        tag="ul"
        class="space-y-1"
        :group="GROUP"
        :sort="false"
        handle=".palette-drag-handle"
        @start="onStart"
        @end="onEnd"
      >
        <li
          v-for="entry in entriesOf(group.group)"
          :key="entry.type"
          :data-palette-type="entry.type"
          class="flex items-center gap-1"
        >
          <span
            class="palette-drag-handle cursor-grab text-muted-foreground"
            :title="`Drag ${entry.label} onto the canvas`"
            aria-hidden="true"
          >
            <GripVertical class="size-4" />
          </span>
          <Button
            size="sm"
            variant="secondary"
            class="flex-1 justify-start"
            :aria-label="`Add ${entry.label} to ${target.name}`"
            :disabled="!builder.canAddTo(entry.type, target.listPath)"
            :data-testid="`palette-${entry.type}`"
            @click="builder.add(entry.type, target.listPath)"
          >
            {{ entry.label }}
          </Button>
        </li>
      </VueDraggable>
    </section>
  </aside>
</template>
