<script setup lang="ts">
import { computed } from 'vue'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { JfssDataComponent } from '@/types/jfss'

import DataIdentityFields from './DataIdentityFields.vue'
import PropertyNumberInput from './PropertyNumberInput.vue'
import {
  messageAt,
  rowKeys,
  sequenceKeys,
  toggleUniqueBy,
  uniqueByKeys,
  withProperty,
} from './propertyEdits'

/**
 * A data grid's properties (#688 AC3; JFSS §4.2, §4.3.1, §5).
 *
 * Its key, label, description and `required`, and the three a grid has alone:
 *
 * - **`sequenceKey`**: one of the row template's data keys, filled with each
 *   row's 1-based number — **never one with a `calculate`** (JFSS S12.7), since
 *   each would overwrite the other;
 * - **`defaultItems`**: the empty rows a new grid opens with, a whole number of
 *   0 or more;
 * - **`validation.uniqueBy`**: one or more of the row's keys, whose combination
 *   no two rows may repeat. **It keeps its loaded shape**: a string stays a
 *   string until a second key is added (`toggleUniqueBy`).
 *
 * **A loaded value that names no row field, or a `sequenceKey` that names a
 * calculated one, is kept and flagged, never dropped**: the row template may be mid-edit on the canvas, and the server is
 * the verdict. `validation.type` stays `array`, the row template is edited on
 * the canvas, and a grid does not `calculate`.
 */
const props = defineProps<{
  component: JfssDataComponent
  /** The node's path, for the server's details (S10.3). */
  nodePath: string
  disabled: boolean
  details: ValidationDetail[]
  /** Another data node in the form uses this node's key (JFSS §4.2). */
  keyTaken: boolean
}>()

const emit = defineEmits<{ update: [component: JfssDataComponent] }>()

const id = computed(() => props.component.id)
const keys = computed(() => rowKeys(props.component))

function patch(key: string, value: unknown): void {
  emit('update', withProperty(props.component, key, value))
}

// --- sequenceKey -------------------------------------------------------------

const sequenceKey = computed(() => props.component.sequenceKey ?? '')
/** The row fields a row number may fill: those that do not `calculate` (S12.7). */
const sequenceable = computed(() => sequenceKeys(props.component))
const sequenceUnknown = computed(
  () => sequenceKey.value !== '' && !keys.value.includes(sequenceKey.value),
)
const sequenceCalculated = computed(
  () =>
    sequenceKey.value !== '' &&
    keys.value.includes(sequenceKey.value) &&
    !sequenceable.value.includes(sequenceKey.value),
)

const sequenceChoices = computed(() => [
  { value: '', label: 'None' },
  ...sequenceable.value.map((key) => ({ value: key, label: key })),
  ...(sequenceUnknown.value || sequenceCalculated.value
    ? [
        {
          value: sequenceKey.value,
          label: `${sequenceKey.value} (${sequenceUnknown.value ? 'not a row field' : 'calculated'})`,
          disabled: true,
        },
      ]
    : []),
])

// --- uniqueBy ----------------------------------------------------------------

const uniqueBy = computed(() => uniqueByKeys(props.component.validation.uniqueBy))

/** The row keys, then any loaded key that names none, so it can be seen and unchecked. */
const uniqueChoices = computed(() => [
  ...keys.value.map((key) => ({ key, known: true })),
  ...uniqueBy.value
    .filter((key) => !keys.value.includes(key))
    .map((key) => ({ key, known: false })),
])

function setUnique(key: string, on: boolean): void {
  emit('update', {
    ...props.component,
    validation: withProperty(
      props.component.validation,
      'uniqueBy',
      toggleUniqueBy(props.component.validation.uniqueBy, key, on),
    ),
  })
}
</script>

<template>
  <DataIdentityFields
    :component="component"
    :node-path="nodePath"
    :disabled="disabled"
    :details="details"
    :key-taken="keyTaken"
    @update="emit('update', $event)"
  />

  <div class="space-y-2 sm:col-span-2">
    <Label :for="`description-${id}`">Description</Label>
    <Input
      :id="`description-${id}`"
      :model-value="component.description ?? ''"
      :disabled="disabled"
      :data-testid="`description-${id}`"
      @update:model-value="patch('description', String($event))"
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
        emit('update', {
          ...component,
          validation: {
            ...component.validation,
            required: ($event.target as HTMLInputElement).checked,
          },
        })
      "
    />
    <Label :for="`required-${id}`">Required</Label>
  </div>

  <div class="space-y-2">
    <Label :for="`sequence-key-${id}`">Row number field</Label>
    <Select
      :id="`sequence-key-${id}`"
      :model-value="sequenceKey"
      :disabled="disabled"
      :options="sequenceChoices"
      :invalid="sequenceUnknown || sequenceCalculated"
      :data-testid="`sequence-key-${id}`"
      @update:model-value="patch('sequenceKey', String($event))"
    />
    <p
      v-if="sequenceUnknown"
      class="text-xs text-destructive"
      :data-testid="`sequence-key-unknown-${id}`"
    >
      “{{ sequenceKey }}” is not a field of the row, so no row is numbered. It is kept until it is
      changed.
    </p>
    <p
      v-else-if="sequenceCalculated"
      class="text-xs text-destructive"
      :data-testid="`sequence-key-calculated-${id}`"
    >
      “{{ sequenceKey }}” calculates its own value, and a row number would overwrite it (JFSS
      S12.7). It is kept until it is changed.
    </p>
    <p v-else class="text-xs text-muted-foreground">
      A field of the row that is filled with the row’s number, from 1. A field that calculates is
      not offered.
    </p>
  </div>

  <PropertyNumberInput
    :id="`default-items-${id}`"
    label="Rows to start with"
    :model-value="component.defaultItems"
    count
    :disabled="disabled"
    :message="messageAt(details, nodePath, 'defaultItems')"
    @update:model-value="patch('defaultItems', $event)"
  />

  <fieldset class="space-y-2 sm:col-span-2" :data-testid="`unique-by-${id}`">
    <legend class="text-sm font-medium">Unique by</legend>
    <p v-if="uniqueChoices.length === 0" class="text-xs text-muted-foreground">
      Add fields to the row on the canvas to choose which must not repeat.
    </p>
    <div v-for="choice in uniqueChoices" :key="choice.key" class="flex items-center gap-2">
      <input
        :id="`unique-by-${id}-${choice.key}`"
        type="checkbox"
        class="h-4 w-4"
        :checked="uniqueBy.includes(choice.key)"
        :disabled="disabled"
        :data-testid="`unique-by-${id}-${choice.key}`"
        @change="setUnique(choice.key, ($event.target as HTMLInputElement).checked)"
      />
      <Label :for="`unique-by-${id}-${choice.key}`">{{ choice.key }}</Label>
      <span
        v-if="!choice.known"
        class="text-xs text-destructive"
        :data-testid="`unique-by-unknown-${id}-${choice.key}`"
      >
        not a field of the row; kept until unchecked
      </span>
    </div>
    <p v-if="messageAt(details, nodePath, 'validation.uniqueBy')" class="text-xs text-destructive">
      {{ messageAt(details, nodePath, 'validation.uniqueBy') }}
    </p>
    <p v-else-if="uniqueChoices.length > 0" class="text-xs text-muted-foreground">
      No two rows may repeat the combination of the fields checked.
    </p>
  </fieldset>
</template>
