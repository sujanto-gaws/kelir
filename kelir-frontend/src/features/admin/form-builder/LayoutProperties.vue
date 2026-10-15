<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { JfssLayoutComponent } from '@/types/jfss'

import { withProperty } from './propertyEdits'

/**
 * A layout node's properties (JFSS §4.3): a panel's or fieldset's `title`,
 * which is optional and goes when cleared, and
 * nothing for columns and tabs, whose slots are added, removed and titled on
 * the canvas. Kept as loaded and not offered: `grid` and `conditional`.
 */
const props = defineProps<{
  component: JfssLayoutComponent
  disabled: boolean
}>()

const emit = defineEmits<{ update: [component: JfssLayoutComponent] }>()

const id = computed(() => props.component.id)
const hasTitle = computed(
  () => props.component.type === 'panel' || props.component.type === 'fieldset',
)
const slotNote = computed(() =>
  props.component.type === 'tabs'
    ? 'Its tabs are added, removed and titled on the canvas.'
    : 'Its columns are added and removed on the canvas.',
)
</script>

<template>
  <div v-if="hasTitle" class="space-y-2 sm:col-span-2">
    <Label :for="`title-${id}`">Title</Label>
    <Input
      :id="`title-${id}`"
      :model-value="component.title ?? ''"
      :disabled="disabled"
      :data-testid="`title-${id}`"
      @update:model-value="emit('update', withProperty(component, 'title', String($event)))"
    />
  </div>

  <p v-else class="text-xs text-muted-foreground sm:col-span-2">{{ slotNote }}</p>
</template>
