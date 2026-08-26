<script setup lang="ts">
import { reactive, watch } from 'vue'

import JfssRenderer from './renderer/JfssRenderer.vue'
import { provideLookupBindings } from './renderer/useLookupBindings'
import { dataComponents, type JfssDefinition } from '@/types/jfss'

const props = defineProps<{
  definition: JfssDefinition
  /** An existing payload, for a document being re-opened. */
  initialValues?: Record<string, unknown>
}>()

const emit = defineEmits<{
  (e: 'action', action: string, values: Record<string, unknown>): void
  (e: 'change', values: Record<string, unknown>): void
}>()

/**
 * A definition, rendered as a form (#162).
 *
 * **The payload lives here and the renderer is stateless.** One reactive object
 * that every field reads its value out of and writes its value into, which is
 * what makes #163's calculation possible without restructuring anything: a
 * derived field is a value in this object that something else computes.
 *
 * **What this form does not do yet, by the sprint plan's split:** it does not
 * validate, does not evaluate `calculate` or `conditional`, and does not
 * submit. An action reaches the page as an event, so that #164 has somewhere
 * to attach a submit rather than a button that currently does nothing quietly.
 */
const values = reactive<Record<string, unknown>>({})

/**
 * JFSS §4.2.3 Case A: resolve once, on mount.
 *
 * Existing payload first, `defaultValue` second — an edit of a stored document
 * must never have its values replaced by the definition's defaults, which is
 * what the priority table in Case A says and the order below implements.
 *
 * **Cases B and C are #163's.** A field carrying `calculate` resolves
 * differently — `derived` recomputes and always wins, `generated` resolves once
 * and never overwrites a persisted value — and both need an evaluator. Until
 * then such a field behaves as Case A, which shows the stored value rather than
 * a wrong computed one.
 */
function resolveInitialValues(): void {
  for (const key of Object.keys(values)) {
    delete values[key]
  }

  for (const field of dataComponents(props.definition.components)) {
    if (props.initialValues && field.key in props.initialValues) {
      values[field.key] = props.initialValues[field.key]
    } else if (field.defaultValue !== undefined) {
      values[field.key] = field.defaultValue
    } else {
      // Present and empty rather than absent: a key that appears the moment
      // somebody types makes the payload shape depend on what was touched, and
      // S10.1 requires every data key on submission.
      values[field.key] = field.validation.type === 'array' ? [] : null
    }
  }
}

// `immediate` so the first render already has the payload; re-running on a new
// definition matters for a page that switches forms without unmounting.
watch(() => props.definition, resolveInitialValues, { immediate: true, deep: false })

// Lookup bindings are read from `settings` (**D-23**) and reach the fields that
// need them by injection — see `useLookupBindings.ts` for why not a prop.
provideLookupBindings(props.definition.settings?.lookups)

function updateField(key: string, value: unknown): void {
  values[key] = value
  emit('change', { ...values })
}
</script>

<template>
  <!-- `novalidate`: the browser's own bubbles would compete with the messages
       #163 renders from the definition, and would be the only validation on a
       form whose rules have not been built yet — which reads as working. -->
  <form class="space-y-6" novalidate @submit.prevent>
    <JfssRenderer
      v-for="component in definition.components"
      :key="component.id"
      :component="component"
      :values="values"
      @update:field="updateField"
      @action="(action: string) => emit('action', action, { ...values })"
    />
  </form>
</template>
