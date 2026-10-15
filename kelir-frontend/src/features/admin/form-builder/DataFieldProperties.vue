<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { JfssDataComponent, JfssValidation } from '@/types/jfss'
import type { LookupSource } from '@/types/rad'

import DataIdentityFields from './DataIdentityFields.vue'
import { PLACEHOLDER_TYPES, switchValueType, withProperty } from './propertyEdits'
import ValidationLimitsFields from './ValidationLimitsFields.vue'

/**
 * A data field's own properties (#688 part 2): every data type but the data
 * grid, which has `DataGridProperties`.
 *
 * Its key, label, **description** and, where the renderer draws one, a
 * **placeholder** (**D-116**, `PLACEHOLDER_TYPES`), value type, `required`,
 * **the limits a text box or a number can use** (D-116, `offeredLimits`), and, by type, a select's or
 * radio's options and a lookup's master-data source. Kept as loaded and not
 * offered: `defaultValue`, `readOnly`, `calculateMode`, `format`, `enum`,
 * `messages`, and a placeholder on a type that draws none.
 */
const props = defineProps<{
  component: JfssDataComponent
  /** The node's path, for the server's details (S10.3). */
  nodePath: string
  disabled: boolean
  details: ValidationDetail[]
  /** `settings.lookups[component.id]`, when this component is bound to a source. */
  lookupSource: string
}>()

const emit = defineEmits<{
  update: [component: JfssDataComponent]
  'update:lookupSource': [source: string]
}>()

/** The four master-data sources `LookupSource` allows (**D-23**, FR-RAD-007). */
const LOOKUP_SOURCES: LookupSource[] = ['supplier', 'customer', 'employee', 'facility']

function choices(values: readonly string[]): { value: string; label: string }[] {
  return values.map((value) => ({ value, label: value }))
}

const VALUE_TYPES = choices(['string', 'number', 'integer', 'boolean', 'array', 'object'])
const SOURCE_CHOICES = [{ value: '', label: 'Choose a source…' }, ...choices(LOOKUP_SOURCES)]

const id = computed(() => props.component.id)

function patch(key: string, value: unknown): void {
  emit('update', withProperty(props.component, key, value))
}

function setValidation(validation: JfssValidation): void {
  emit('update', { ...props.component, validation })
}

const offersPlaceholder = computed(() => PLACEHOLDER_TYPES.includes(props.component.type))

/** `select` and `radio` are the two types whose choices come from the definition. */
const hasOptions = computed(
  () => props.component.type === 'select' || props.component.type === 'radio',
)

/**
 * Options as `label=value` lines, which is the shape an author types.
 *
 * A line with no `=` is a label that is its own value, because that is what
 * somebody writing `Draft` in a box means.
 */
const optionLines = computed({
  get: () =>
    (props.component.options ?? [])
      .map((option) => `${option.label}=${String(option.value ?? '')}`)
      .join('\n'),
  set: (text: string) => {
    const options = text
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
      .map((line) => {
        const at = line.indexOf('=')

        return at === -1
          ? { label: line, value: line }
          : { label: line.slice(0, at).trim(), value: line.slice(at + 1).trim() }
      })

    emit('update', { ...props.component, options })
  },
})
</script>

<template>
  <DataIdentityFields
    :component="component"
    :node-path="nodePath"
    :disabled="disabled"
    :details="details"
    @update="emit('update', $event)"
  />

  <div v-if="offersPlaceholder" class="space-y-2">
    <Label :for="`placeholder-${id}`">Placeholder</Label>
    <Input
      :id="`placeholder-${id}`"
      :model-value="component.placeholder ?? ''"
      :disabled="disabled"
      :data-testid="`placeholder-${id}`"
      @update:model-value="patch('placeholder', String($event))"
    />
  </div>

  <div class="space-y-2">
    <Label :for="`description-${id}`">Description</Label>
    <Input
      :id="`description-${id}`"
      :model-value="component.description ?? ''"
      :disabled="disabled"
      :data-testid="`description-${id}`"
      @update:model-value="patch('description', String($event))"
    />
  </div>

  <div class="space-y-2">
    <Label :for="`vtype-${id}`">Value type</Label>
    <Select
      :id="`vtype-${id}`"
      :model-value="component.validation.type"
      :disabled="disabled"
      :options="VALUE_TYPES"
      :data-testid="`vtype-${id}`"
      @update:model-value="setValidation(switchValueType(component.validation, $event as never))"
    />
  </div>

  <div class="flex items-end gap-2">
    <input
      :id="`required-${id}`"
      type="checkbox"
      class="h-4 w-4"
      :checked="component.validation.required === true"
      :disabled="disabled"
      :data-testid="`required-${id}`"
      @change="
        setValidation({
          ...component.validation,
          required: ($event.target as HTMLInputElement).checked,
        })
      "
    />
    <Label :for="`required-${id}`">Required</Label>
  </div>

  <ValidationLimitsFields
    :component-id="id"
    :component-type="component.type"
    :node-path="nodePath"
    :validation="component.validation"
    :disabled="disabled"
    :details="details"
    @update:validation="setValidation"
  />

  <div v-if="hasOptions" class="space-y-2 sm:col-span-2">
    <Label :for="`options-${id}`">Options</Label>
    <textarea
      :id="`options-${id}`"
      v-model="optionLines"
      rows="3"
      class="w-full rounded-md border bg-background p-2 font-mono text-xs"
      :disabled="disabled"
      :data-testid="`options-${id}`"
    ></textarea>
    <p class="text-xs text-muted-foreground">
      One per line, <code>label=value</code>. A line with no <code>=</code> is its own value.
    </p>
  </div>

  <div v-if="component.type === 'lookup'" class="space-y-2 sm:col-span-2">
    <Label :for="`lookup-${id}`">Master data source</Label>
    <Select
      :id="`lookup-${id}`"
      :model-value="lookupSource"
      :disabled="disabled"
      :options="SOURCE_CHOICES"
      :data-testid="`lookup-${id}`"
      @update:model-value="emit('update:lookupSource', String($event))"
    />
    <!-- The binding lives in `settings.lookups`, keyed by this component's
         `id`, because JFSS is frozen and a `dataSource` property on the
         component would make the document non-conformant (D-23). -->
    <p
      v-if="lookupSource === ''"
      class="text-xs text-destructive"
      :data-testid="`lookup-unbound-${id}`"
    >
      Choose a master data source: a lookup with none is refused when the form is saved.
    </p>
    <p class="text-xs text-muted-foreground">
      Options come from master data the person filling the form may already read.
    </p>
  </div>
</template>
