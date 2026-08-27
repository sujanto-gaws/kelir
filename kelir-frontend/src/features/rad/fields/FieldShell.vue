<script setup lang="ts">
import { computed } from 'vue'

import { Label } from '@/components/ui/label'
import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import { useFormEvaluation, useValueScope } from '@/features/rad/renderer/useFormEvaluation'
import type { JfssDataComponent } from '@/types/jfss'

/**
 * The label, required marker, help text and message every data field wears.
 *
 * **One shell so that "comes from the definition" is true in one place**
 * (#162 AC3). A field component that drew its own label would be a field
 * component that could draw a different one, and the marker for `required`
 * would end up meaning something slightly different on each of the nine types.
 * #163 added the message to the same list for the same reason: nine components
 * deciding what a violation looks like is nine chances for one of them to
 * decide it looks like nothing.
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
const evaluation = useFormEvaluation()

/**
 * The record this field's `key` addresses — the payload, or one datagrid row.
 *
 * Injected rather than passed down, so a rule inside a row template is decided
 * against that row: `matchesField` targeting `unit_price` means the same row's
 * unit price, not a top-level field that happens to share the name.
 */
const values = useValueScope()

/** The id the control carries, and what the label points `for` at. */
const controlId = computed(() => `jfss-${scope.value}${props.component.id}`)

/** The id of the help text, for the control's `aria-describedby`. */
const describedBy = computed(() => `${controlId.value}-description`)

/** The id of the message, so a screen reader hears why the box is refused. */
const errorId = computed(() => `${controlId.value}-error`)

const violation = computed(() => evaluation?.violationFor(props.component, values.value))

/**
 * Both ids when both are present.
 *
 * `aria-describedby` takes a list, and dropping the help text as soon as
 * something goes wrong would remove the sentence that explains how to put it
 * right — which is the moment it is most wanted.
 */
const describedByAttribute = computed(
  () =>
    [
      props.component.description ? describedBy.value : undefined,
      violation.value ? errorId.value : undefined,
    ]
      .filter(Boolean)
      .join(' ') || undefined,
)
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

    <slot
      :control-id="controlId"
      :described-by="describedByAttribute"
      :invalid="violation !== undefined"
    />

    <p v-if="component.description" :id="describedBy" class="text-sm text-muted-foreground">
      {{ component.description }}
    </p>

    <!-- The definition's own words where it supplies them: `validation.messages`
         per keyword (§5) and `rule.message` per advanced rule (§6.2). The
         fallbacks in `validation.ts` are for the keywords a definition leaves
         unspoken. -->
    <p v-if="violation" :id="errorId" class="text-sm text-destructive" data-testid="field-error">
      {{ violation.message }}
    </p>
  </div>
</template>
