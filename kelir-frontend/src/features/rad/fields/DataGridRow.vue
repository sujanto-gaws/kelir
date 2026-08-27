<script setup lang="ts">
import { defineAsyncComponent, toRef } from 'vue'

import { provideFieldScope } from '@/features/rad/renderer/useFieldScope'
import { provideValuePath, provideValueScope } from '@/features/rad/renderer/useFormEvaluation'
import type { JfssComponent } from '@/types/jfss'

const props = defineProps<{
  template: JfssComponent[]
  values: Record<string, unknown>
  index: number
  /** The repeater's own payload `key`, for the S10.3 path its rows sit under. */
  arrayKey: string
}>()

const emit = defineEmits<{ (e: 'update:field', key: string, value: unknown): void }>()

/**
 * One row of a repeater, and the reason it is its own component.
 *
 * **`provide` runs once per component instance, and rows are a `v-for`.** The
 * grid cannot open a scope per row from its own setup, so each row is an
 * instance that opens its own — which is also what makes a nested grid work,
 * since the inner row's scope appends to the outer row's rather than replacing
 * it.
 */
provideFieldScope(() => `row-${props.index}-`)

/**
 * And the row is the scope its template's `key`s address (JFSS §4.3.1).
 *
 * Provided beside the id prefix and for the same reason: a rule or a
 * calculation written in a row template means *this* row's siblings, not the
 * form's. Without it, `matchesField` targeting `unit_price` in row three would
 * compare against a top-level field of that name, which on most forms does not
 * exist — so the rule would compare against `undefined` and quietly hold.
 */
provideValueScope(() => props.values)

/**
 * And the S10.3 path that scope sits at (`line_items.0.`).
 *
 * Separate from the id prefix above because the two address different things —
 * that one a DOM element, this one a place in the payload — and they are spelled
 * differently: `jfss-row-0-line-total` against `line_items.0.line_total`. A
 * server violation arrives keyed by the second, which is why the envelope names
 * the field `path` rather than `key`.
 */
provideValuePath(() => `${props.arrayKey}.${props.index}.`)

/** Lazily imported for the cycle `DataGridField` documents. */
const JfssRenderer = defineAsyncComponent(() => import('@/features/rad/renderer/JfssRenderer.vue'))

// A ref rather than the prop directly, so a row's fields see edits to the row
// object they were handed rather than the one that existed at mount.
const values = toRef(props, 'values')
</script>

<template>
  <JfssRenderer
    v-for="field in template"
    :key="field.id"
    :component="field"
    :values="values"
    @update:field="(key: string, value: unknown) => emit('update:field', key, value)"
  />
</template>
