<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { JfssActionComponent } from '@/types/jfss'

/**
 * A button's properties (JFSS §4.5): its `label`. **Its `action` is shown and
 * fixed**: `submit` is the one action the builder offers, and a button loaded
 * with another (`navigate`, `reset`) keeps it, shown read-only. Kept as loaded
 * and not offered: `theme`.
 */
const props = defineProps<{
  component: JfssActionComponent
  disabled: boolean
}>()

const emit = defineEmits<{ update: [component: JfssActionComponent] }>()

const id = computed(() => props.component.id)
</script>

<template>
  <div class="space-y-2">
    <Label :for="`button-label-${id}`">Label</Label>
    <Input
      :id="`button-label-${id}`"
      :model-value="component.label ?? ''"
      :disabled="disabled"
      :data-testid="`button-label-${id}`"
      @update:model-value="emit('update', { ...component, label: String($event) })"
    />
  </div>
  <div class="space-y-2">
    <Label :for="`button-action-${id}`">Action</Label>
    <Input
      :id="`button-action-${id}`"
      :model-value="component.action ?? ''"
      disabled
      :data-testid="`button-action-${id}`"
    />
  </div>
</template>
