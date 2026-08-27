<script setup lang="ts">
import { computed } from 'vue'

import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import { useFormEvaluation, useValueScope } from '@/features/rad/renderer/useFormEvaluation'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * The one field that does not wear [`FieldShell`](./FieldShell.vue).
 *
 * A checkbox's label belongs beside the box and reads as the thing being
 * agreed to; above it, the box floats under a heading with nothing to say what
 * ticking it means. So the label moves and **the properties it reads do not**:
 * `label`, `validation.required`, `description` and the violation, the same
 * four the shell reads, so #162 AC3 and #163 AC1 both still hold for this type.
 *
 * **`required` on a checkbox refuses nothing**, and that is the specification
 * rather than a gap here. JFSS §5 makes `required` mean *present and non-empty*
 * and `false` is a present value — the backend's `serde_json` sees it that way
 * too, so a client that read it as empty would refuse submissions the server
 * accepts. A box that must be ticked is `validation.enum: [true]`, which
 * `validation.ts` already decides.
 */
const scope = useFieldScope()
const evaluation = useFormEvaluation()
const values = useValueScope()

const controlId = computed(() => `jfss-${scope.value}${props.component.id}`)
const describedBy = computed(() => `${controlId.value}-description`)
const errorId = computed(() => `${controlId.value}-error`)

const violation = computed(() => evaluation?.violationFor(props.component, values.value))

const describedByAttribute = computed(
  () =>
    [
      props.component.description ? describedBy.value : undefined,
      violation.value ? errorId.value : undefined,
    ]
      .filter(Boolean)
      .join(' ') || undefined,
)

const value = computed({
  get: () => props.modelValue === true,
  set: (next: boolean) => emit('update:modelValue', next),
})
</script>

<template>
  <div class="space-y-1.5">
    <div class="flex items-center gap-2">
      <Checkbox
        :id="controlId"
        v-model="value"
        :disabled="component.readOnly"
        :aria-invalid="violation !== undefined"
        :aria-describedby="describedByAttribute"
      />
      <Label :for="controlId">
        {{ component.label }}
        <span v-if="component.validation.required" class="text-destructive" aria-hidden="true"
          >*</span
        >
      </Label>
    </div>

    <p v-if="component.description" :id="describedBy" class="text-sm text-muted-foreground">
      {{ component.description }}
    </p>

    <p v-if="violation" :id="errorId" class="text-sm text-destructive" data-testid="field-error">
      {{ violation.message }}
    </p>
  </div>
</template>
