<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { ValidationDetail } from '@/types/api'
import type { JfssDataComponent } from '@/types/jfss'

import { messageAt } from './propertyEdits'

/**
 * A data node's `key` and `label`: what every data type, the grid included,
 * shows first. Both are required by the meta-schema, so an emptied box writes
 * the empty string and the server's refusal says why, at the key.
 *
 * **A key another data node uses is flagged here**, and the page refuses to
 * save while one is (JFSS §4.2, `formTree.duplicateKeys`).
 *
 * **A key edit rewrites no expression** (#688 AC6): renaming is out of #688's
 * scope, so a `var` that read the old key keeps reading it and is badged in its
 * builder as not offered.
 */
const props = defineProps<{
  component: JfssDataComponent
  nodePath: string
  disabled: boolean
  details: ValidationDetail[]
  /** Another data node in the form uses this key (JFSS §4.2). */
  keyTaken: boolean
}>()

const emit = defineEmits<{ update: [component: JfssDataComponent] }>()

const id = computed(() => props.component.id)
const keyMessage = computed(() => messageAt(props.details, props.nodePath, 'key'))
</script>

<template>
  <div class="space-y-2">
    <Label :for="`key-${id}`">Key</Label>
    <Input
      :id="`key-${id}`"
      :model-value="component.key"
      :disabled="disabled"
      :invalid="keyMessage !== undefined || keyTaken"
      :described-by="keyTaken ? `key-taken-${id}` : undefined"
      :data-testid="`key-${id}`"
      @update:model-value="emit('update', { ...component, key: String($event) })"
    />
    <p
      v-if="keyTaken"
      :id="`key-taken-${id}`"
      class="text-xs text-destructive"
      :data-testid="`key-taken-${id}`"
    >
      Another component already uses this key. Keys are unique across the form, so it cannot be
      saved until one of them changes.
    </p>
    <p v-if="keyMessage" class="text-xs text-destructive">{{ keyMessage }}</p>
  </div>

  <div class="space-y-2">
    <Label :for="`label-${id}`">Label</Label>
    <Input
      :id="`label-${id}`"
      :model-value="component.label"
      :disabled="disabled"
      :data-testid="`label-${id}`"
      @update:model-value="emit('update', { ...component, label: String($event) })"
    />
  </div>
</template>
