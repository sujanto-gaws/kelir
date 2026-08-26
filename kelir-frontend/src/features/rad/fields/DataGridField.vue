<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted } from 'vue'

import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { dataComponents, type JfssComponent, type JfssDataComponent } from '@/types/jfss'

/**
 * The renderer, imported lazily to break a cycle.
 *
 * `registry.ts` maps `datagrid` to this file, this file renders its rows
 * through `JfssRenderer.vue`, and the renderer reads the registry — a three
 * module loop. A static import would resolve to `undefined` for whichever
 * module the bundler evaluated first, and the symptom would be an empty grid
 * rather than an error. `defineAsyncComponent` defers the import to first
 * render, by which point every module in the loop is initialised.
 */
const JfssRenderer = defineAsyncComponent(
  () => import('@/features/rad/renderer/JfssRenderer.vue'),
)

const props = defineProps<{ component: JfssDataComponent; modelValue: unknown }>()
const emit = defineEmits<{ (e: 'update:modelValue', value: unknown): void }>()

/**
 * A repeater: rows of the template `components` declares.
 *
 * **`components` on a `data` component is a row template, not a set of
 * siblings** (JFSS §4.3.1) — the same property name a layout container uses for
 * its children, meaning something else. Rendering it as siblings would put one
 * copy of each field on the form instead of one copy per row, and would look
 * almost right on an empty grid.
 *
 * Each row is its own scope: the template's `key`s address properties of the
 * row object, not of the form payload, which is why two components in one
 * document may legitimately share a `key` and why `settings.lookups` binds on
 * `id` instead (**D-23**).
 */
const rows = computed<Record<string, unknown>[]>(() =>
  Array.isArray(props.modelValue) ? (props.modelValue as Record<string, unknown>[]) : [],
)

const template = computed<JfssComponent[]>(() => props.component.components ?? [])

/** A blank row: every template field present, so the payload shape is stable. */
function blankRow(): Record<string, unknown> {
  const row: Record<string, unknown> = {}

  for (const field of dataComponents(template.value)) {
    row[field.key] = field.defaultValue ?? null
  }

  return row
}

/**
 * `sequenceKey`: the 1-based row index, written into a child `key` (§4.2).
 *
 * Recomputed on every write rather than stored once, because removing row 2 of
 * four has to renumber the two below it — a line-item table that reads 1, 3, 4
 * after a deletion is the defect this property exists to prevent.
 */
function withSequence(next: Record<string, unknown>[]): Record<string, unknown>[] {
  const sequenceKey = props.component.sequenceKey

  if (!sequenceKey) {
    return next
  }

  return next.map((row, index) => ({ ...row, [sequenceKey]: index + 1 }))
}

function commit(next: Record<string, unknown>[]): void {
  emit('update:modelValue', withSequence(next))
}

function addRow(): void {
  commit([...rows.value, blankRow()])
}

function removeRow(index: number): void {
  commit(rows.value.filter((_, position) => position !== index))
}

function updateCell(index: number, key: string, value: unknown): void {
  commit(rows.value.map((row, position) => (position === index ? { ...row, [key]: value } : row)))
}

/** `defaultItems`: empty rows to open with, when the grid arrives empty (§4.2). */
onMounted(() => {
  const wanted = props.component.defaultItems ?? 0

  if (wanted > 0 && rows.value.length === 0) {
    commit(Array.from({ length: wanted }, blankRow))
  } else if (props.component.sequenceKey && rows.value.length > 0) {
    // An existing grid arriving without its sequence filled in — an edit of a
    // document stored before the property was added to the definition.
    commit(rows.value)
  }
})
</script>

<template>
  <fieldset class="space-y-3">
    <legend class="text-sm font-medium">
      {{ component.label }}
      <span v-if="component.validation.required" class="text-destructive" aria-hidden="true">*</span>
    </legend>

    <p v-if="component.description" class="text-sm text-muted-foreground">
      {{ component.description }}
    </p>

    <p v-if="rows.length === 0" class="text-sm text-muted-foreground">No rows yet.</p>

    <div
      v-for="(row, index) in rows"
      :key="index"
      class="space-y-3 rounded-md border border-border p-4"
    >
      <div class="flex items-center justify-between">
        <Label class="text-muted-foreground">Row {{ index + 1 }}</Label>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          :disabled="component.readOnly"
          @click="removeRow(index)"
        >
          Remove
        </Button>
      </div>

      <JfssRenderer
        v-for="field in template"
        :key="field.id"
        :component="field"
        :values="row"
        @update:field="(key: string, value: unknown) => updateCell(index, key, value)"
      />
    </div>

    <Button type="button" variant="secondary" :disabled="component.readOnly" @click="addRow">
      Add row
    </Button>
  </fieldset>
</template>
