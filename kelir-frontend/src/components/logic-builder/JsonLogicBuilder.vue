<script setup lang="ts">
import { computed, ref, shallowRef, toRaw, watch } from 'vue'

import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/select'

import JsonLogicNode from './JsonLogicNode.vue'
import JsonLogicRawBox from './JsonLogicRawBox.vue'
import {
  applyEdit,
  createNode,
  FAMILY_LABELS,
  familiesIn,
  isComplete,
  isRootRepresentable,
  parseExpression,
  serialise,
  type LogicBuilderContext,
  type LogicEdit,
  type LogicNode,
  type LogicTier,
  type LogicVariable,
  type NodeChoice,
} from './logicTree'

/**
 * The visual JSON Logic builder (FR-RAD-013, FR-WF-018; #686, decision **D-110**).
 *
 * One builder for both of Kelir's expression surfaces: a form's `calculate` and
 * `conditional.logic`, and a workflow transition's `condition`. **What differs
 * between them is input, not code**: the `tier` decides which operators are
 * offered (Calculation Rule Registry §2.1 against §2.5), and `variables` decides
 * which paths are — a form passes its field keys, a workflow its JWSS §6.1
 * context. Nothing is hard-coded.
 *
 * **The rules it keeps:**
 *
 * - **It emits only on an edit.** Mounting, focusing, blurring and expanding
 *   emit nothing, so a definition opened and closed is not a definition changed.
 * - **It never emits a placeholder.** An operand not yet filled in leaves the
 *   last emitted value standing and reports the builder invalid
 *   (`validityChanged`, and the exposed `valid`).
 * - **What it cannot show is kept, not rewritten.** A subtree it cannot
 *   represent is an advanced raw block holding the original value by reference;
 *   a root it cannot represent opens the whole expression in the raw-JSON box,
 *   marked advanced, which never rewrites a value that parses.
 *
 * It evaluates nothing, and imports no evaluator: an evaluation panel, if one
 * is added, goes through `lib/jsonlogic.ts`'s `loadEvaluator` (**D-10**).
 */

const props = withDefaults(
  defineProps<{
    modelValue?: unknown
    tier: LogicTier
    variables: readonly LogicVariable[]
    /** Whether a path outside `variables` may be typed. */
    allowFreePaths?: boolean
    /**
     * Prefixes under which a typed path counts as offered, and draws no
     * warning: a part of the context the host cannot list. Kept for a path
     * either way; this decides only the badge.
     */
    freePrefixes?: readonly string[]
    /** The accessible name of the expression, and the prefix of every control's. */
    label?: string
    disabled?: boolean
  }>(),
  {
    modelValue: undefined,
    allowFreePaths: false,
    freePrefixes: () => [],
    label: 'Expression',
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: unknown]
  validityChanged: [valid: boolean]
}>()

/** Never equal to a value a host could pass, so the first `modelValue` always loads. */
const NOTHING_EMITTED = Symbol('nothing emitted')

let lastEmitted: unknown = NOTHING_EMITTED

const tree = shallowRef<LogicNode | null>(null)
const mode = ref<'visual' | 'raw'>('visual')
/** What the raw box holds: the last value it parsed, or what it opened with. */
const rawHeld = shallowRef<unknown>(undefined)
const rawValid = ref(true)
/** Bumped to remount the raw box when its value comes from outside it. */
const generation = ref(0)

const context = computed<LogicBuilderContext>(() => ({
  tier: props.tier,
  variables: props.variables,
  allowFreePaths: props.allowFreePaths,
  freePrefixes: props.freePrefixes,
  disabled: props.disabled,
}))

/**
 * The host's value without Vue's proxy.
 *
 * A host's `v-model` is usually reactive state, so what arrives is a proxy of
 * the value and every object reached through it is a proxy too. The seam holds
 * opaque subtrees **by reference** and emits untouched nodes as their originals,
 * and a proxy is not the host's value: it would be emitted in its place.
 * Unwrapping the root is enough — the raw object's members are raw.
 */
function raw(value: unknown): unknown {
  return typeof value === 'object' && value !== null ? toRaw(value) : value
}

function load(proxied: unknown): void {
  const value = raw(proxied)

  // Whatever was emitted before is history once a host value is adopted. Kept,
  // it made a redo — the host handing back the value this builder last emitted,
  // after an undo loaded another — look like this builder's own echo, so the
  // tree went on showing the undone expression and the next edit was built
  // from it.
  lastEmitted = NOTHING_EMITTED
  generation.value += 1
  rawValid.value = true

  if (value === undefined) {
    tree.value = null
    mode.value = 'visual'

    return
  }

  const parsed = parseExpression(value, props.tier)

  if (isRootRepresentable(parsed)) {
    tree.value = parsed
    mode.value = 'visual'
  } else {
    tree.value = null
    rawHeld.value = value
    mode.value = 'raw'
  }
}

load(props.modelValue)

/**
 * Whether a value from the host is this builder's last emission coming back.
 *
 * Compared **structurally**, not by identity: a host that clones on write
 * (an undo stack, a store that copies) hands back an equal value that is not
 * the same object, and an identity check would reload the tree on every
 * keystroke — remounting every control under the user and dropping operands
 * not yet filled in. A value too deep for `JSON.stringify` is treated as a
 * change.
 *
 * **An emission is echoed once.** The watcher consumes the match, so a host
 * that later sets an equal value of its own — reopening the definition — is
 * read afresh, and a redo after an undo is read because `load` forgot the
 * emission.
 */
function isOwnEcho(value: unknown): boolean {
  if (lastEmitted === NOTHING_EMITTED) {
    return false
  }

  const incoming = raw(value)

  if (incoming === lastEmitted) {
    return true
  }

  try {
    return JSON.stringify(incoming) === JSON.stringify(lastEmitted)
  } catch {
    return false
  }
}

// A value the host changed is loaded afresh. One this builder just emitted is
// already what the tree says, and reloading it would throw away the tree's
// unfinished operands and the user's place in it.
watch(
  () => props.modelValue,
  (value) => {
    if (isOwnEcho(value)) {
      lastEmitted = NOTHING_EMITTED
    } else {
      load(value)
    }
  },
)

watch(
  () => props.tier,
  () => load(props.modelValue),
)

const valid = computed(() =>
  mode.value === 'raw' ? rawValid.value : tree.value === null || isComplete(tree.value),
)

watch(valid, (now) => emit('validityChanged', now))

defineExpose({ valid })

function emitValue(value: unknown): void {
  lastEmitted = value
  emit('update:modelValue', value)
}

/**
 * Records that the user changed something, emitted or not.
 *
 * Once the tree has changed since the last emission, a value equal to that
 * emission is no longer an echo of what is shown: it is the host setting it,
 * a reset to what was saved, and it is read.
 */
function edited(): void {
  lastEmitted = NOTHING_EMITTED
}

function onEdit(edit: LogicEdit): void {
  if (!tree.value) {
    return
  }

  edited()

  const next = applyEdit(tree.value, edit)

  tree.value = next

  if (isComplete(next)) {
    emitValue(serialise(next))
  }
}

const startOptions = computed(() => [
  { value: 'var', label: 'Variable' },
  ...familiesIn(props.tier).map((family) => ({
    value: family,
    label: FAMILY_LABELS[family],
  })),
])

function start(choice: string): void {
  if (choice === '') {
    return
  }

  const node = createNode(choice as NodeChoice)

  edited()
  tree.value = node

  if (isComplete(node)) {
    emitValue(serialise(node))
  }
}

function clear(): void {
  tree.value = null
  emitValue(undefined)
}

/** Whether the raw box's value can be shown visually: empty, or a representable root. */
const canEditVisually = computed(
  () =>
    rawValid.value &&
    (rawHeld.value === undefined ||
      isRootRepresentable(parseExpression(rawHeld.value, props.tier))),
)

const canEditAsJson = computed(() => tree.value === null || isComplete(tree.value))

/** Switches to the raw box with the value as it stands. Emits nothing: nothing changed. */
function editAsJson(): void {
  rawHeld.value = tree.value === null ? undefined : serialise(tree.value)
  rawValid.value = true
  generation.value += 1
  mode.value = 'raw'
}

function editVisually(): void {
  load(rawHeld.value)
}

function onRawUnparsed(): void {
  edited()
  rawValid.value = false
}

function onRawParsed(value: unknown): void {
  rawHeld.value = value
  rawValid.value = true
  emitValue(value)
}
</script>

<template>
  <div role="group" :aria-label="label" class="space-y-2">
    <template v-if="mode === 'raw'">
      <JsonLogicRawBox
        :key="generation"
        :label="`${label} (JSON)`"
        :model-value="rawHeld"
        allow-empty
        :disabled="disabled"
        @parsed="onRawParsed"
        @unparsed="onRawUnparsed"
      />
      <p v-if="!canEditVisually && rawValid" class="text-xs text-muted-foreground">
        The visual builder cannot show this expression, so it is edited as JSON.
      </p>
      <Button
        variant="outline"
        size="sm"
        :aria-label="`Edit ${label} visually`"
        :disabled="disabled || !canEditVisually"
        @click="editVisually"
      >
        Edit visually
      </Button>
    </template>

    <template v-else>
      <Select
        v-if="tree === null"
        :aria-label="`${label}: kind`"
        class="w-auto"
        model-value=""
        :options="startOptions"
        placeholder="No expression"
        :disabled="disabled"
        @update:model-value="start"
      />
      <JsonLogicNode
        v-else
        :node="tree"
        :path="[]"
        :label="label"
        :context="context"
        :removable="false"
        @edit="onEdit"
      />
      <div class="flex flex-wrap gap-2">
        <Button
          v-if="tree !== null"
          variant="ghost"
          size="sm"
          :aria-label="`Clear ${label}`"
          :disabled="disabled"
          @click="clear"
        >
          Clear expression
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :aria-label="`Edit ${label} as JSON`"
          :disabled="disabled || !canEditAsJson"
          @click="editAsJson"
        >
          Edit as JSON
        </Button>
      </div>
    </template>
  </div>
</template>
