<script setup lang="ts">
import { computed, nextTick, ref, useId } from 'vue'
import { ChevronDown, ChevronRight, Plus, Trash2, TriangleAlert } from 'lucide-vue-next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'

import JsonLogicRawBox from './JsonLogicRawBox.vue'
import {
  canChangeOperator,
  createNode,
  FAMILY_LABELS,
  familiesIn,
  familyOf,
  holeNode,
  isOfferedPath,
  literalNode,
  OPERATOR_FAMILIES,
  OPERATOR_LABELS,
  opaqueNode,
  operandCount,
  summarise,
  varNode,
  type ExpressionOperator,
  type LogicBuilderContext,
  type LogicEdit,
  type LogicNode,
  type NodeChoice,
  type NodePath,
} from './logicTree'

/**
 * One node of the visual JSON Logic builder, and every node below it (#686).
 *
 * It renders what its node is and reports what the user did to it as a
 * {@link LogicEdit}; it never changes the tree itself. `JsonLogicBuilder` owns
 * the tree, applies the edit through the seam, and decides whether there is
 * anything to emit — so the rule that **nothing is emitted except on an edit**
 * lives in one place. Focusing, blurring and expanding a branch change only
 * this component's own view state.
 *
 * Every control is a native one from the shadcn-vue set — a `select`, an
 * `input`, a `button`, a `textarea` — so the whole tree is operated by keyboard
 * alone, in tab order, and every one carries an accessible name built from the
 * node's position: *Expression, operand 2: variable*.
 */

const props = defineProps<{
  node: LogicNode
  path: NodePath
  /** The accessible name of this node; operands extend it. */
  label: string
  context: LogicBuilderContext
  /** Whether the operator above can spare this operand. */
  removable: boolean
}>()

const emit = defineEmits<{ edit: [edit: LogicEdit] }>()

const id = useId()
const messageId = `${id}-message`
const operandsId = `${id}-operands`
const warningId = `${id}-warning`

const isRoot = computed(() => props.path.length === 0)
const expanded = ref(true)
const operandsEl = ref<HTMLElement>()

/** The kind of operand this node is, as the kind chooser names it. */
const kind = computed<string>(() => {
  const node = props.node

  if (node.kind === 'operator') {
    return familyOf(node.op)
  }

  if (node.kind === 'opaque') {
    return 'json'
  }

  if (node.kind === 'hole') {
    return node.slot === 'any' ? '' : node.slot
  }

  if (node.kind === 'literal') {
    return node.value === null ? 'null' : typeof node.value
  }

  return 'var'
})

/**
 * The root is an operator or a variable (B2), so it is not offered a literal.
 * "Advanced" at the root is the builder's *Edit as JSON*, not a choice here.
 */
const kindOptions = computed(() => {
  const operands = [
    { value: 'var', label: 'Variable' },
    ...(isRoot.value
      ? []
      : [
          { value: 'number', label: 'Number' },
          { value: 'string', label: 'Text' },
          { value: 'boolean', label: 'True or false' },
          { value: 'null', label: 'Empty (null)' },
          { value: 'json', label: 'Advanced (JSON)' },
        ]),
  ]

  return [
    ...operands,
    ...familiesIn(props.context.tier).map((family) => ({
      value: family,
      label: FAMILY_LABELS[family],
    })),
  ]
})

const operatorNode = computed(() => (props.node.kind === 'operator' ? props.node : undefined))

const operatorOptions = computed(() => {
  const node = operatorNode.value

  if (!node) {
    return []
  }

  const family: readonly ExpressionOperator[] = OPERATOR_FAMILIES[familyOf(node.op)]

  return family.map((op) => ({
    value: op,
    label: `${OPERATOR_LABELS[op]} (${op})`,
    disabled: !canChangeOperator(node, op),
  }))
})

const bounds = computed(() =>
  operatorNode.value ? operandCount(operatorNode.value.op) : { min: 0, max: 0 },
)

const varPath = computed(() => (props.node.kind === 'var' ? props.node.path : ''))

const offered = computed(() =>
  isOfferedPath(varPath.value, props.context.variables, props.context.freePrefixes),
)

const variableOptions = computed(() => {
  const options = props.context.variables.map((variable) => ({
    value: variable.path,
    label: variable.type
      ? `${variable.label} — ${variable.path} (${variable.type})`
      : `${variable.label} — ${variable.path}`,
  }))

  // A path the host does not offer is kept verbatim and shown as itself (B4).
  return varPath.value !== '' && !offered.value
    ? [...options, { value: varPath.value, label: `${varPath.value} (not offered)` }]
    : options
})

const isHole = computed(() => props.node.kind === 'hole')

const HOLE_MESSAGES: Record<string, string> = {
  any: 'Choose what this operand is.',
  var: 'Choose a variable.',
  number: 'Enter a number.',
  string: 'Type the text.',
  boolean: 'Choose true or false.',
  json: 'Enter valid JSON.',
}

const holeMessage = computed(() =>
  props.node.kind === 'hole' ? HOLE_MESSAGES[props.node.slot] : undefined,
)

function initialNumberText(node: LogicNode): string {
  if (node.kind === 'literal' && typeof node.value === 'number') {
    return String(node.value)
  }

  return node.kind === 'hole' ? (node.text ?? '') : ''
}

/** The number as typed, which may not be a number yet ("1.", "-"). */
const numberText = ref(initialNumberText(props.node))

const NUMBER = /^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/

function replace(node: LogicNode): void {
  emit('edit', { type: 'replace', path: props.path, node })
}

function choose(choice: string): void {
  if (choice === '' || choice === kind.value) {
    return
  }

  numberText.value = ''
  replace(createNode(choice as NodeChoice))
}

function setPath(next: string): void {
  replace(next === '' ? holeNode('var') : varNode(next))
}

function setNumber(next: string): void {
  numberText.value = next

  const trimmed = next.trim()
  const value = Number(trimmed)

  // Finite as well as well-formed: `1e400` is `Infinity`, which JSON writes as `null`.
  replace(
    NUMBER.test(trimmed) && Number.isFinite(value) ? literalNode(value) : holeNode('number', next),
  )
}

function setBoolean(next: string): void {
  replace(next === '' ? holeNode('boolean') : literalNode(next === 'true'))
}

function setOperator(op: string): void {
  emit('edit', { type: 'operator', path: props.path, op: op as ExpressionOperator })
}

/** Moves focus to the kind chooser of operand `at`, or the last one there is. */
async function focusOperand(at: number): Promise<void> {
  await nextTick()

  const operands = operandsEl.value?.querySelectorAll<HTMLElement>(':scope > [role="group"]')

  if (!operands || operands.length === 0) {
    return
  }

  operands[Math.min(at, operands.length - 1)].querySelector<HTMLElement>('select')?.focus()
}

function addOperand(): void {
  const count = operatorNode.value?.args.length ?? 0

  emit('edit', { type: 'add', path: props.path })
  void focusOperand(count)
}

/**
 * Passes an operand's edit up, and after removing one of this node's own
 * operands puts focus on the operand that took its place — the button that had
 * it is gone, and focus must not fall back to the page.
 */
function onOperandEdit(edit: LogicEdit): void {
  emit('edit', edit)

  const own =
    edit.type === 'remove' &&
    edit.path.length === props.path.length + 1 &&
    props.path.every((at, index) => edit.path[index] === at)

  if (own) {
    void focusOperand(edit.path[edit.path.length - 1])
  }
}
</script>

<template>
  <div
    role="group"
    :aria-label="label"
    class="space-y-2 rounded-md border p-2"
    :class="isHole ? 'border-destructive' : 'border-border'"
  >
    <div class="flex flex-wrap items-center gap-2">
      <Select
        :aria-label="`${label}: kind`"
        class="w-auto"
        :model-value="kind"
        :options="kindOptions"
        :placeholder="kind === '' ? 'Choose…' : undefined"
        :invalid="node.kind === 'hole' && node.slot === 'any'"
        :described-by="isHole ? messageId : undefined"
        :disabled="context.disabled"
        @update:model-value="choose"
      />

      <Select
        v-if="operatorNode && operatorOptions.length > 1"
        :aria-label="`${label}: operator`"
        class="w-auto"
        :model-value="operatorNode.op"
        :options="operatorOptions"
        :disabled="context.disabled"
        @update:model-value="setOperator"
      />

      <template v-if="node.kind === 'var' || (node.kind === 'hole' && node.slot === 'var')">
        <Select
          :aria-label="`${label}: variable`"
          class="w-auto"
          :model-value="varPath"
          :options="variableOptions"
          :placeholder="varPath === '' ? 'Choose a variable' : undefined"
          :invalid="isHole"
          :described-by="isHole ? messageId : !offered ? warningId : undefined"
          :disabled="context.disabled"
          @update:model-value="setPath"
        />
        <Input
          v-if="context.allowFreePaths"
          :aria-label="`${label}: variable path`"
          class="w-56 font-mono"
          :model-value="varPath"
          :invalid="isHole"
          :described-by="isHole ? messageId : !offered ? warningId : undefined"
          :disabled="context.disabled"
          @update:model-value="setPath"
        />
        <Badge v-if="node.kind === 'var' && !offered" :id="warningId" variant="destructive">
          <TriangleAlert class="mr-1 size-3" aria-hidden="true" />
          Not in the offered variables
        </Badge>
      </template>

      <Input
        v-else-if="kind === 'number'"
        :aria-label="`${label}: number`"
        class="w-36"
        inputmode="decimal"
        :model-value="numberText"
        :invalid="isHole"
        :described-by="isHole ? messageId : undefined"
        :disabled="context.disabled"
        @update:model-value="setNumber"
      />

      <Input
        v-else-if="kind === 'string'"
        :aria-label="`${label}: text`"
        class="w-56"
        :model-value="node.kind === 'literal' ? String(node.value) : ''"
        :invalid="isHole"
        :described-by="isHole ? messageId : undefined"
        :disabled="context.disabled"
        @update:model-value="replace(literalNode($event))"
      />

      <Select
        v-else-if="kind === 'boolean'"
        :aria-label="`${label}: true or false`"
        class="w-auto"
        :model-value="node.kind === 'literal' ? String(node.value) : ''"
        :options="[
          { value: 'true', label: 'true' },
          { value: 'false', label: 'false' },
        ]"
        :placeholder="isHole ? 'Choose…' : undefined"
        :invalid="isHole"
        :described-by="isHole ? messageId : undefined"
        :disabled="context.disabled"
        @update:model-value="setBoolean"
      />

      <Button
        v-if="operatorNode"
        variant="ghost"
        size="sm"
        :aria-label="`${expanded ? 'Collapse' : 'Expand'} ${label}`"
        :aria-expanded="expanded"
        :aria-controls="operandsId"
        @click="expanded = !expanded"
      >
        <ChevronDown v-if="expanded" class="size-4" aria-hidden="true" />
        <ChevronRight v-else class="size-4" aria-hidden="true" />
      </Button>

      <Button
        v-if="removable"
        variant="ghost"
        size="sm"
        :aria-label="`Remove ${label}`"
        :disabled="context.disabled"
        @click="emit('edit', { type: 'remove', path })"
      >
        <Trash2 class="size-4" aria-hidden="true" />
      </Button>
    </div>

    <p v-if="holeMessage" :id="messageId" class="text-xs text-destructive">{{ holeMessage }}</p>

    <JsonLogicRawBox
      v-if="kind === 'json'"
      :key="node.key"
      :label="`${label} (JSON)`"
      :model-value="node.kind === 'opaque' ? node.value : undefined"
      :disabled="context.disabled"
      @parsed="replace(opaqueNode($event))"
      @unparsed="replace(holeNode('json', $event))"
    />

    <template v-if="operatorNode">
      <div
        v-show="expanded"
        :id="operandsId"
        ref="operandsEl"
        class="space-y-2 border-l-2 border-border pl-3"
      >
        <JsonLogicNode
          v-for="(operand, at) in operatorNode.args"
          :key="operand.key"
          :node="operand"
          :path="[...path, at]"
          :label="`${label}, operand ${at + 1}`"
          :context="context"
          :removable="operatorNode.args.length > bounds.min"
          @edit="onOperandEdit"
        />
        <Button
          v-if="operatorNode.args.length < bounds.max"
          variant="outline"
          size="sm"
          :aria-label="`Add operand to ${label}`"
          :disabled="context.disabled"
          @click="addOperand"
        >
          <Plus class="mr-1 size-4" aria-hidden="true" />
          Add operand
        </Button>
      </div>
      <p v-if="!expanded" class="font-mono text-xs text-muted-foreground">
        {{ summarise(node) }}
      </p>
    </template>
  </div>
</template>
