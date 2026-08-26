<script setup lang="ts">
import { computed } from 'vue'

import { declaredGap, resolve } from './registry'
import UnsupportedComponent from './UnsupportedComponent.vue'
import { useFormEvaluation } from './useFormEvaluation'
import type {
  JfssActionComponent,
  JfssComponent,
  JfssDataComponent,
  JfssDisplayComponent,
  JfssLayoutComponent,
} from '@/types/jfss'

/**
 * One component of a definition, and everything under it (#162, #163).
 *
 * **Dispatch, and nothing else.** It resolves a type through the
 * [registry](./registry.ts), hands the component the props its role takes, and
 * recurses through whichever child container the node holds. It does not
 * validate, does not evaluate, and does not know the name of a single form.
 *
 * **Where #163 touched it, it touched only the props.** The evaluator lives in
 * [`useFormEvaluation`](./useFormEvaluation.ts) and is reached here through
 * three questions — is this component rendered, does its control accept input,
 * and what text does a display show. The answers arrive as ordinary props, so
 * no field component learned about JSON Logic and the sprint plan's split
 * survived construction: *a renderer that is wrong about layout and a renderer
 * that is wrong about arithmetic fail differently and should not be debugged
 * together*.
 *
 * **It is not a second validator** (#162 AC2). The backend refuses a definition
 * that the meta-schema rejects, that carries an unapproved operator, or whose
 * lookup names a source nobody serves — all at save, for the reason it states:
 * *"a definition is written once and rendered thousands of times, and the
 * render path has no good failure"*. What reaches here has been checked. The
 * one thing the backend cannot check is the component `type`, because that
 * vocabulary is this frontend's, and that is exactly the case handled below.
 */
const props = defineProps<{
  component: JfssComponent
  /**
   * The scope this component reads from.
   *
   * The form payload at the top level, and a **row object** inside a datagrid:
   * a repeater's template keys address properties of the row, not of the form
   * (JFSS §4.3.1). Passing a scope rather than the whole payload is what lets
   * the same field components serve both without knowing which they are in, and
   * it is the same scope every expression on this component is evaluated
   * against.
   */
  values: Record<string, unknown>
}>()

const emit = defineEmits<{
  (e: 'update:field', key: string, value: unknown): void
  (e: 'action', action: string): void
}>()

const evaluation = useFormEvaluation()

const entry = computed(() => resolve(props.component.type))

/**
 * Whether the registry's role for this type matches the definition's.
 *
 * A definition may pair `role: "data"` with `type: "panel"` and the backend
 * will store it — the meta-schema constrains which *properties* each role may
 * carry, not which types belong to which role. Rendering it would mean handing
 * a container a `modelValue`, so the mismatch is shown as unsupported instead.
 */
const renderable = computed(
  () => entry.value !== undefined && entry.value.role === props.component.role,
)

const gap = computed(() => declaredGap(props.component.type))

/**
 * JFSS §7, applied by removing the component rather than hiding it.
 *
 * **A hidden field's value is still submitted** (S10.1.1), and that stays true
 * because the payload is built from the *definition* in
 * [`JfssForm`](../JfssForm.vue) and not from what happens to be mounted. So
 * unmounting costs nothing the specification wants kept, and it buys the thing
 * a `hidden` attribute does not: no duplicate DOM ids while a branch is closed,
 * and nothing in the accessibility tree that a sighted reader cannot see.
 *
 * This differs from an inactive tab, which stays mounted and merely `hidden` —
 * a tab is a place the reader may go, and a closed conditional is a branch
 * their answers took them away from.
 */
const visible = computed(() => evaluation?.isVisible(props.component, props.values) ?? true)

/**
 * The data component as rendered, which is the definition's plus what the rules
 * decided.
 *
 * `readOnly` is resolved here rather than in each of the nine field components:
 * S4.2.3 Case B makes a `derived` field read-only whether or not the definition
 * says so, and §7 lets a `conditional` disable one. Both are the same fact from
 * the field's point of view — this control does not accept input — and a field
 * that had to ask two sources would be a field that could disagree with itself.
 */
const asData = computed<JfssDataComponent>(() => {
  const component = props.component as JfssDataComponent

  if (evaluation?.isEditable(component, props.values) === false) {
    return { ...component, readOnly: true }
  }

  return component
})

const asLayout = computed(() => props.component as JfssLayoutComponent)

/**
 * The display component as rendered, with §4.4's `calculate` resolved to text.
 *
 * A display takes its content from `content` **or** from an expression, and
 * resolving which here keeps `TextDisplay` and `AlertDisplay` unaware that
 * either is a possibility.
 */
const asDisplay = computed<JfssDisplayComponent>(() => {
  const component = props.component as JfssDisplayComponent

  if (!evaluation) {
    return component
  }

  return { ...component, content: evaluation.displayContent(component, props.values) }
})

const asAction = computed(() => props.component as JfssActionComponent)
</script>

<template>
  <template v-if="visible">
    <!-- Unknown to the registry, or known-and-not-yet-built: both are visible,
         and `reason` is what distinguishes a planned gap from a surprise. -->
    <UnsupportedComponent v-if="!renderable" :component="component" :reason="gap" />

    <!-- role: data — one value, bound by `key` within the current scope. -->
    <component
      :is="entry!.component"
      v-else-if="component.role === 'data'"
      :component="asData"
      :model-value="values[asData.key]"
      @update:model-value="(value: unknown) => emit('update:field', asData.key, value)"
    />

    <!-- role: layout — the container owns its child shape and yields each
         slot-group in turn, so §4.3.1's three shapes are traversed without this
         file knowing which one it has. -->
    <component :is="entry!.component" v-else-if="component.role === 'layout'" :component="asLayout">
      <template #group="{ components }">
        <JfssRenderer
          v-for="child in components"
          :key="child.id"
          :component="child"
          :values="values"
          @update:field="(key: string, value: unknown) => emit('update:field', key, value)"
          @action="(action: string) => emit('action', action)"
        />
      </template>
    </component>

    <!-- role: display — reads the definition, writes nothing. -->
    <component
      :is="entry!.component"
      v-else-if="component.role === 'display'"
      :component="asDisplay"
    />

    <!-- role: action — the action string travels to the form root, which decides
         what it means. -->
    <component
      :is="entry!.component"
      v-else
      :component="asAction"
      @action="(action: string) => emit('action', action)"
    />
  </template>
</template>
