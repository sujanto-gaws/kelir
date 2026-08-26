<script setup lang="ts">
import { computed } from 'vue'

import FieldShell from './FieldShell.vue'
import { Label } from '@/components/ui/label'
import { useFieldScope } from '@/features/rad/renderer/useFieldScope'
import type { JfssDataComponent, JfssOption } from '@/types/jfss'

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/** `options` or `validation.enum`, resolved as `SelectField` resolves them. */
const options = computed<JfssOption[]>(() => {
  if (props.component.options) {
    return props.component.options
  }

  return (props.component.validation.enum ?? []).map((value) => ({
    label: String(value),
    value,
  }))
})

/**
 * One `name` shared by every radio in the group, scoped to where it renders.
 *
 * **The browser makes radios mutually exclusive by `name`, not by markup
 * nesting**, so this is the one place where a colliding identifier corrupts
 * data rather than accessibility. §4.1's per-instance uniqueness handles two
 * groups in one definition; it does not handle **one** group rendered once per
 * row of a repeater, where every row would share a name and choosing an option
 * in row two would silently clear row one. The field scope is what separates
 * them (see `useFieldScope.ts`).
 */
const scope = useFieldScope()
const groupName = computed(() => `jfss-${scope.value}${props.component.id}`)

function isSelected(option: JfssOption): boolean {
  return option.value === props.modelValue
}
</script>

<template>
  <FieldShell v-slot="{ describedBy }" :component="component">
    <div
      class="space-y-2"
      role="radiogroup"
      :aria-label="component.label"
      :aria-describedby="describedBy"
    >
      <div
        v-for="(option, index) in options"
        :key="`${groupName}-${index}`"
        class="flex items-center gap-2"
      >
        <input
          :id="`${groupName}-${index}`"
          type="radio"
          class="size-4 border-input accent-primary disabled:cursor-not-allowed disabled:opacity-50"
          :name="groupName"
          :checked="isSelected(option)"
          :disabled="component.readOnly"
          @change="emit('update:modelValue', option.value)"
        />
        <Label :for="`${groupName}-${index}`">{{ option.label }}</Label>
      </div>
    </div>
  </FieldShell>
</template>
