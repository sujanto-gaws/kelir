<script setup lang="ts">
import { Alert } from '@/components/ui/alert'
import type { JfssComponent } from '@/types/jfss'

defineProps<{
  component: JfssComponent
  /** Why this type is not rendered, when the registry says so. */
  reason?: string
}>()

/**
 * What a component type Kelir does not render looks like.
 *
 * **Visible, and naming the type.** `type` is an open vocabulary (JFSS §4.4),
 * so the backend cannot refuse a definition for using one this frontend has no
 * component for — it refuses shapes, operators and lookup sources, and a
 * component type is none of the three. That means an unrendered type reaches
 * here by design rather than by mistake, and the only question is what it looks
 * like when it does.
 *
 * Rendering nothing was the alternative and it is the worse one: a form missing
 * a field looks exactly like a form that never had it, and the person who
 * notices is the one whose submitted document turns out to be missing the
 * amount. That is the same reasoning the backend gives for refusing an unknown
 * lookup source at save — *"the field renders, the chooser opens, and it is
 * empty, which is indistinguishable from master data that happens to hold
 * nothing"*.
 *
 * A definition that reaches this in production is a finding, not a design. The
 * registry's not-yet-rendered list is where the expected cases are declared, so
 * that an *unexpected* one is distinguishable from a planned gap.
 */
</script>

<template>
  <Alert variant="destructive">
    <p class="font-medium">This form uses a component Kelir cannot show yet.</p>
    <p class="mt-1">
      <code>{{ component.type }}</code>
      <span v-if="reason"> — {{ reason }}</span>
    </p>
  </Alert>
</template>
