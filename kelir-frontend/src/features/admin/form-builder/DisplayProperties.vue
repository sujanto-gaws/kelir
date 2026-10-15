<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { JfssDisplayComponent } from '@/types/jfss'

import { lacksText } from './builderRegistry'
import { withProperty } from './propertyEdits'

/**
 * A display node's properties (JFSS §4.4): its `content`, which goes when
 * cleared — and a text-bearing type left with neither `content` nor
 * `calculate` is flagged here and refused at save (`lacksText`) — and an
 * alert's
 * **`variant`** (**D-116**). A divider draws no text, so it has neither.
 *
 * Kept as loaded and not offered: a heading's or paragraph's `variant`, and a
 * display node's `calculate`.
 */
const props = defineProps<{
  component: JfssDisplayComponent
  disabled: boolean
}>()

const emit = defineEmits<{ update: [component: JfssDisplayComponent] }>()

const id = computed(() => props.component.id)

/** Display types that render text; a divider has none (JFSS §4.4). */
const hasContent = computed(() => props.component.type !== 'divider')

/** Text-bearing, with neither text nor a calculation: JFSS §4.4 refuses it. */
const missing = computed(() => lacksText(props.component))

/**
 * An alert's variants: **the two looks the renderer draws**, by JFSS §4.4's
 * own examples. `AlertDisplay.vue` draws `warning` and `error` as destructive
 * and every other value as the default, so `info` and `warning` are the whole
 * of what a choice here changes; offering `success` or `error` as well would
 * be offering a setting that does nothing new. A loaded variant outside these
 * two is kept as loaded, and shown as itself, not choosable.
 */
const VARIANTS = ['info', 'warning']

const variant = computed(() => props.component.variant ?? '')

const variantChoices = computed(() => [
  { value: '', label: 'Default' },
  ...VARIANTS.map((value) => ({ value, label: value })),
  ...(variant.value !== '' && !VARIANTS.includes(variant.value)
    ? [{ value: variant.value, label: `${variant.value} (not offered)`, disabled: true }]
    : []),
])
</script>

<template>
  <div v-if="hasContent" class="space-y-2 sm:col-span-2">
    <Label :for="`content-${id}`">Content</Label>
    <Input
      :id="`content-${id}`"
      :model-value="component.content ?? ''"
      :disabled="disabled"
      :invalid="missing"
      :described-by="missing ? `content-missing-${id}` : undefined"
      :data-testid="`content-${id}`"
      @update:model-value="emit('update', withProperty(component, 'content', String($event)))"
    />
    <p
      v-if="missing"
      :id="`content-missing-${id}`"
      class="text-xs text-destructive"
      :data-testid="`content-missing-${id}`"
    >
      A {{ component.type }} needs text: it cannot be saved without content or a calculation (JFSS
      §4.4).
    </p>
  </div>

  <div v-if="component.type === 'alert'" class="space-y-2">
    <Label :for="`variant-${id}`">Variant</Label>
    <Select
      :id="`variant-${id}`"
      :model-value="variant"
      :disabled="disabled"
      :options="variantChoices"
      :data-testid="`variant-${id}`"
      @update:model-value="emit('update', withProperty(component, 'variant', String($event)))"
    />
  </div>
</template>
