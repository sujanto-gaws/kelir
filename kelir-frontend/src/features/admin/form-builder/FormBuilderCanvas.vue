<script setup lang="ts">
import { computed } from 'vue'

import ConfirmDialog from '../ConfirmDialog.vue'

import FormCanvasList from './FormCanvasList.vue'
import { useFormBuilder } from './useFormBuilder'

/**
 * The form builder's canvas: the definition's root list, everything nested in
 * it, and the two things the whole canvas shares (#688 C3, D1, D6).
 *
 * - **The live region** says what the last change was — *Added*, *Moved to*,
 *   *Removed* — because a change made from the keyboard is otherwise silent
 *   to someone who cannot see it land.
 * - **The confirmation** for removing a node or slot that holds anything.
 *   Removing one takes its children and their lookup bindings with it, which
 *   is a lot for one key press to do unasked.
 */
const builder = useFormBuilder()

const confirming = computed({
  get: () => builder.pendingRemoval.value !== null,
  set: (open: boolean) => {
    if (!open) {
      builder.pendingRemoval.value = null
    }
  },
})
</script>

<template>
  <section class="space-y-2" aria-label="Form canvas" data-testid="form-canvas">
    <FormCanvasList
      list-path="components"
      :items="builder.definition.value.components"
      kind="root"
    />

    <p class="sr-only" role="status" aria-live="polite" data-testid="canvas-announcement">
      {{ builder.announcement.value }}
    </p>

    <ConfirmDialog
      v-model:open="confirming"
      :title="`Remove ${builder.pendingRemoval.value?.name ?? ''}?`"
      description="It holds components. Removing it removes them too, with any lookup source bound to them."
      confirm-label="Remove"
      @confirm="builder.confirmRemoval()"
    />
  </section>
</template>
