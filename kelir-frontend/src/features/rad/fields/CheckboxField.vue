<script setup lang="ts">
import { computed } from 'vue'

import { Checkbox } from '@/components/ui/checkbox'
import { Label } from '@/components/ui/label'
import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import type { JfssDataComponent } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * The one field that does not wear [`FieldShell`](./FieldShell.vue).
 *
 * A checkbox's label belongs beside the box and reads as the thing being
 * agreed to; above it, the box floats under a heading with nothing to say what
 * ticking it means. So the label moves and **the properties it reads do not**:
 * `label`, `validation.required` and `description`, the same three the shell
 * reads, so #162 AC3 still holds for this type.
 */
const scope = useFieldScope()
const controlId = computed(() => `jfss-${scope.value}${props.component.id}`)
const describedBy = computed(() => `${controlId.value}-description`)

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
        :aria-describedby="component.description ? describedBy : undefined"
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
  </div>
</template>
