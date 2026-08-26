<script setup lang="ts">
import { Label } from '@/components/ui/label'
import type { JfssDataComponent } from '@/types/jfss'

/**
 * The label, required marker and help text every data field wears.
 *
 * **One shell so that "comes from the definition" is true in one place**
 * (#162 AC3). A field component that drew its own label would be a field
 * component that could draw a different one, and the marker for `required`
 * would end up meaning something slightly different on each of the nine types.
 *
 * The `id` it gives the control is the component's JFSS `id`, which §4.1
 * guarantees is unique per instance — so `for`/`id` pairing is correct by
 * construction rather than by a generated counter that has to stay unique.
 */
const props = defineProps<{ component: JfssDataComponent }>()

/** The id the control carries, and what the label points `for` at. */
const controlId = `jfss-${props.component.id}`

/** The id of the help text, for the control's `aria-describedby`. */
const describedBy = `jfss-${props.component.id}-description`
</script>

<template>
  <div class="space-y-1.5">
    <Label :for="controlId">
      {{ component.label }}
      <!-- The marker is the definition's `validation.required`, not a guess
           from the presence of a rule: JFSS §5 makes `required` the keyword
           that decides it, and a form whose asterisks disagree with what it
           refuses is worse than one with no asterisks. `aria-hidden` because
           the control itself carries `required`, and a screen reader announcing
           both says "required required". -->
      <span v-if="component.validation.required" class="text-destructive" aria-hidden="true"
        >*</span
      >
    </Label>

    <slot :control-id="controlId" :described-by="component.description ? describedBy : undefined" />

    <p v-if="component.description" :id="describedBy" class="text-sm text-muted-foreground">
      {{ component.description }}
    </p>
  </div>
</template>
