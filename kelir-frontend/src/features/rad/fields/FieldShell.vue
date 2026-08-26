<script setup lang="ts">
import { computed } from 'vue'

import { Label } from '@/components/ui/label'
import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import type { JfssDataComponent } from '@/types/jfss'

/**
 * The label, required marker and help text every data field wears.
 *
 * **One shell so that "comes from the definition" is true in one place**
 * (#162 AC3). A field component that drew its own label would be a field
 * component that could draw a different one, and the marker for `required`
 * would end up meaning something slightly different on each of the nine types.
 *
 * The `id` it gives the control is the component's JFSS `id` under whatever
 * field scope is in force — `jfss-title-field` at the top level of a form,
 * `jfss-row-1-line-no` inside a repeater's second row. **§4.1's per-instance
 * uniqueness is not enough on its own**: a repeater renders one template
 * instance once per row, so the scope is what keeps the ids distinct and the
 * `for`/`id` pairing pointing at the right box (see `useFieldScope.ts`).
 */
const props = defineProps<{ component: JfssDataComponent }>()

const scope = useFieldScope()

/** The id the control carries, and what the label points `for` at. */
const controlId = computed(() => `jfss-${scope.value}${props.component.id}`)

/** The id of the help text, for the control's `aria-describedby`. */
const describedBy = computed(() => `${controlId.value}-description`)
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
