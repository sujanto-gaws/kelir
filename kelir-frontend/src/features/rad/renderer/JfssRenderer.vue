<script setup lang="ts">
import { computed } from 'vue'

import { declaredGap, resolve } from './registry'
import UnsupportedComponent from './UnsupportedComponent.vue'
import type {
  JfssActionComponent,
  JfssComponent,
  JfssDataComponent,
  JfssDisplayComponent,
  JfssLayoutComponent,
} from '@/types/jfss'

/**
 * One component of a definition, and everything under it (#162).
 *
 * **Dispatch, and nothing else.** It resolves a type through the
 * [registry](./registry.ts), hands the component the props its role takes, and
 * recurses through whichever child container the node holds. It does not
 * validate, evaluate, or know the name of a single form.
 *
 * **No rules, deliberately** — the sprint plan splits FR-RAD-010 into three
 * issues because *a renderer that is wrong about layout and a renderer that is
 * wrong about arithmetic fail differently and should not be debugged together*.
 * So there is no evaluator import in this file, and there must not be one until
 * #163: `conditional` is read from no component, `calculate` is evaluated for
 * none, and every field is shown. Adding a single `conditional` check here
 * would also quietly put the 588 KB engine on the render path, which is the
 * condition **D-10** accepted its size on.
 *
 * **It is not a second validator** (AC2). The backend refuses a definition that
 * the meta-schema rejects, that carries an unapproved operator, or whose lookup
 * names a source nobody serves — all at save, for the reason it states: *"a
 * definition is written once and rendered thousands of times, and the render
 * path has no good failure"*. What reaches here has been checked. The one thing
 * the backend cannot check is the component `type`, because that vocabulary is
 * this frontend's, and that is exactly the case handled below.
 */
const props = defineProps<{
  component: JfssComponent
  /**
   * The scope this component reads from.
   *
   * The form payload at the top level, and a **row object** inside a datagrid:
   * a repeater's template keys address properties of the row, not of the form
   * (JFSS §4.3.1). Passing a scope rather than the whole payload is what lets
   * the same field components serve both without knowing which they are in.
   */
  values: Record<string, unknown>
}>()

const emit = defineEmits<{
  (e: 'update:field', key: string, value: unknown): void
  (e: 'action', action: string): void
}>()

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

const asData = computed(() => props.component as JfssDataComponent)
const asLayout = computed(() => props.component as JfssLayoutComponent)
const asDisplay = computed(() => props.component as JfssDisplayComponent)
const asAction = computed(() => props.component as JfssActionComponent)
</script>

<template>
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
