<script setup lang="ts">
import { computed, ref } from 'vue'

import JsonLogicBuilder from '@/components/logic-builder/JsonLogicBuilder.vue'
import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssConditional } from '@/types/jfss'

import { isExpression, messageAt, withProperty } from './propertyEdits'

/**
 * A node's expressions, built visually (#688 AC5–AC9, **D-110**).
 *
 * **`calculate` and `conditional.logic` each mount the shared
 * `JsonLogicBuilder`**, in the tier the Calculation Rule Registry gives them:
 * `calculate` offers `var` and arithmetic (§2.1; §2.3 forbids comparisons
 * there), `conditional` the whole §2.5 set. An operator outside the tier is
 * held opaque by the builder and saved unchanged: the server's save-time check
 * is the only verdict on it. **Nothing here evaluates** (D-10).
 *
 * **What an expression may read is its scope's data keys** (AC6), from
 * `expressionScope.ts`. A `calculate` is not offered its own key, since the
 * server refuses a calculation that reads itself; a `conditional` is, since a
 * field may decide its own visibility.
 *
 * # The host's write path clones, so the builder is handed a steady value
 *
 * Every write goes through `formTree.replaceNode`, which copies the whole
 * definition (#688 I1). So after **any** edit — the label, a limit — this node's
 * `calculate` arrives as a new object, equal to the old. The builder compares
 * an echo of its own emission by JSON, but a value that changed identity
 * *without* being its echo is a value the host set, and it reloads: an operand
 * the author had not filled in yet would be dropped by a label edit elsewhere
 * in the panel. So each expression is passed on **only when its JSON
 * changes**, and otherwise as the object the builder already has.
 *
 * # An unfinished expression saves its last complete value
 *
 * The builder never emits a placeholder: while an operand is unfilled, the
 * definition holds the last complete expression, and the field says so. Save
 * stays enabled (product owner, 2026-10-15).
 *
 * # What the raw box parses is written only when it is an expression
 *
 * The builder's raw-JSON box emits whatever its text parses to. A form's
 * expression is a JSON Logic object of exactly one operator (the meta-schema's
 * `$defs/jsonLogic`), so `null`, `5`, `[]`, `{}` or an object of two keys is
 * **refused here, in the form's host**, not in the shared builder (a workflow
 * condition may be a literal). The last good value stays, and the field says
 * the text was not saved.
 */
const props = defineProps<{
  component: JfssComponent
  /** The node's path, for the server's details (S10.3). */
  nodePath: string
  /** The data keys in this node's scope (AC6). */
  scope: readonly LogicVariable[]
  disabled: boolean
  details: ValidationDetail[]
}>()

const emit = defineEmits<{ update: [component: JfssComponent] }>()

/** Data nodes calculate; a data grid's value is its rows, so it does not (JFSS §4.3.1). */
const offersCalculate = computed(
  () => props.component.role === 'data' && props.component.type !== 'datagrid',
)

/** `conditional` is offered on data and display nodes, as the flat builder offered it. */
const offersConditional = computed(
  () => props.component.role === 'data' || props.component.role === 'display',
)

const ownKey = computed(() =>
  props.component.role === 'data' ? (props.component as { key: string }).key : undefined,
)

function sameJson(a: unknown, b: unknown): boolean {
  if (a === b) {
    return true
  }

  try {
    return JSON.stringify(a) === JSON.stringify(b)
  } catch {
    return false
  }
}

/** `read()`, but the previous object while its JSON is unchanged. */
function steady<T>(read: () => T) {
  return computed<T>((previous) => {
    const next = read()

    return previous !== undefined && sameJson(previous, next) ? previous : next
  })
}

const record = computed(() => props.component as unknown as Record<string, unknown>)

const calculate = steady(() => record.value.calculate)
const logic = steady(() => props.component.conditional?.logic)

/** A `calculate` is not offered its own key: the server refuses a self-read (engine.rs). */
const calculateVariables = steady(() =>
  props.scope.filter((variable) => variable.path !== ownKey.value),
)
const conditionalVariables = steady(() => [...props.scope])

const calculateUnfinished = ref(false)
const conditionalUnfinished = ref(false)
/** The raw box's last value was not an expression, so it was not written. */
const calculateRefused = ref(false)
const conditionalRefused = ref(false)

const calculateMessage = computed(() => messageAt(props.details, props.nodePath, 'calculate'))
const conditionalMessage = computed(
  () =>
    messageAt(props.details, props.nodePath, 'conditional.logic') ??
    messageAt(props.details, props.nodePath, 'conditional'),
)

const ACTIONS = ['show', 'hide', 'enable', 'disable'].map((value) => ({ value, label: value }))

function emitWith(key: string, value: unknown): void {
  emit('update', withProperty(props.component, key, value))
}

/** Cleared, the property goes (AC7). Anything but an expression is refused (F1). */
function setCalculate(value: unknown): void {
  calculateRefused.value = value !== undefined && !isExpression(value)

  if (!calculateRefused.value) {
    emitWith('calculate', value)
  }
}

/** Cleared, the whole `conditional` goes: an action with no logic is not one (AC7). */
function setLogic(value: unknown): void {
  const existing = props.component.conditional

  conditionalRefused.value = value !== undefined && !isExpression(value)

  if (conditionalRefused.value) {
    return
  }

  emitWith(
    'conditional',
    value === undefined
      ? undefined
      : ({ ...existing, action: existing?.action ?? 'show', logic: value } as JfssConditional),
  )
}

function setAction(action: string): void {
  const existing = props.component.conditional

  if (existing) {
    emitWith('conditional', { ...existing, action: action as JfssConditional['action'] })
  }
}
</script>

<template>
  <div
    v-if="offersCalculate"
    class="space-y-2 sm:col-span-2"
    :data-testid="`calculate-${component.id}`"
  >
    <Label>Calculate</Label>
    <JsonLogicBuilder
      :model-value="calculate"
      tier="calculate"
      :variables="calculateVariables"
      label="Calculate"
      :disabled="disabled"
      :aria-invalid="calculateRefused || undefined"
      :aria-describedby="calculateRefused ? `calculate-refused-${component.id}` : undefined"
      @update:model-value="setCalculate"
      @validity-changed="calculateUnfinished = !$event"
    />
    <p
      v-if="calculateUnfinished"
      class="text-xs text-destructive"
      :data-testid="`calculate-unfinished-${component.id}`"
    >
      Unfinished: an operand is not filled in. Saving keeps the last complete calculation.
    </p>
    <p
      v-if="calculateRefused"
      class="text-xs text-destructive"
      :id="`calculate-refused-${component.id}`"
      role="alert"
      :data-testid="`calculate-refused-${component.id}`"
    >
      Not saved: a calculation is a JSON object with exactly one operator, such as
      <code>{"+": [1, 2]}</code>. The last valid one is kept.
    </p>
    <p
      v-if="calculateMessage"
      class="text-xs text-destructive"
      :data-testid="`calculate-error-${component.id}`"
    >
      {{ calculateMessage }}
    </p>
    <p class="text-xs text-muted-foreground">
      The server recomputes this on every write and overwrites what the browser sent.
    </p>
  </div>

  <div
    v-if="offersConditional"
    class="space-y-2 sm:col-span-2"
    :data-testid="`conditional-${component.id}`"
  >
    <Label :for="`conditional-action-${component.id}`">Conditional</Label>
    <Select
      :id="`conditional-action-${component.id}`"
      class="w-auto"
      :model-value="component.conditional?.action ?? 'show'"
      :disabled="disabled || !component.conditional"
      :options="ACTIONS"
      :data-testid="`conditional-action-${component.id}`"
      @update:model-value="setAction(String($event))"
    />
    <JsonLogicBuilder
      :model-value="logic"
      tier="conditional"
      :variables="conditionalVariables"
      label="Conditional logic"
      :disabled="disabled"
      :aria-invalid="conditionalRefused || undefined"
      :aria-describedby="conditionalRefused ? `conditional-refused-${component.id}` : undefined"
      @update:model-value="setLogic"
      @validity-changed="conditionalUnfinished = !$event"
    />
    <p
      v-if="conditionalUnfinished"
      class="text-xs text-destructive"
      :data-testid="`conditional-unfinished-${component.id}`"
    >
      Unfinished: an operand is not filled in. Saving keeps the last complete condition.
    </p>
    <p
      v-if="conditionalRefused"
      class="text-xs text-destructive"
      :id="`conditional-refused-${component.id}`"
      role="alert"
      :data-testid="`conditional-refused-${component.id}`"
    >
      Not saved: a condition is a JSON object with exactly one operator, such as
      <code>{"var": "approved"}</code>. The last valid one is kept.
    </p>
    <p
      v-if="conditionalMessage"
      class="text-xs text-destructive"
      :data-testid="`conditional-error-${component.id}`"
    >
      {{ conditionalMessage }}
    </p>
    <p class="text-xs text-muted-foreground">
      Re-decided on the server against the whole payload, after calculations run.
    </p>
  </div>
</template>
