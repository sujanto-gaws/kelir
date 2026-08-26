<script setup lang="ts">
import { defineAsyncComponent, toRef } from 'vue'

import { provideFieldScope } from '@/features/rad/renderer/useFieldScope'
import type { JfssComponent } from '@/types/jfss'

const props = defineProps<{
  template: JfssComponent[]
  values: Record<string, unknown>
  index: number
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
