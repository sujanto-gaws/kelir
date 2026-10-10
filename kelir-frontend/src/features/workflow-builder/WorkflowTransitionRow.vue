<script setup lang="ts">
import { computed } from 'vue'
import { ArrowDown, ArrowUp, Trash2 } from 'lucide-vue-next'

import JsonLogicBuilder from '@/components/logic-builder/JsonLogicBuilder.vue'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Select } from '@/components/ui/select'
import { TableCell, TableRow } from '@/components/ui/table'
import type { AssignmentRule, JwssTransition, TransitionAction } from '@/types/workflow'

import AssignmentRuleEditor from './AssignmentRuleEditor.vue'
import { drawnRuleFields } from './assignmentRule'
import { useWorkflowEditorContext } from './editorContext'
import { CONDITION_FREE_PREFIXES, optionsOf, TRANSITION_ACTIONS } from './jwssRegistry'
import { rowErrors } from './workflowVerdict'

/**
 * One transition: its action, target, who may take it, whether it needs a
 * comment, and its condition (JWSS §4; #426 AC1, AC3, AC4).
 *
 * Two table rows: the edge, and under it its condition and any message the
 * server addressed to the transition as a whole. **Every message under this
 * transition's path is shown here** — under the input it names, or listed at
 * the top when it names none this row draws — so AC3's *at the transition it
 * names* holds for whatever the server sends.
 *
 * **`requiresComment` is offered on every edge, `AUTO` included** (AC4). S12
 * refuses it on an `AUTO` edge, and that refusal is the server's: it comes back
 * at `…requiresComment` and is shown under this checkbox, where the author set
 * it, rather than hidden by a control that silently could not be ticked.
 */
const props = defineProps<{
  /** Its position in `transitions`, which is how the server addresses it. */
  index: number
}>()

const editor = useWorkflowEditorContext()

const transition = computed(() => editor.draft.definition.value.transitions[props.index])
const rowKey = computed(() => editor.draft.current.value.transitionKeys[props.index])
const path = computed(() => `definition.transitions.${props.index}`)
const id = computed(() => `transition-${rowKey.value}`)
const label = computed(() => `Transition ${props.index + 1}`)

const errors = computed(() => rowErrors(editor.fieldErrors.value, 'transition', props.index))

/** The fields this row draws a message under, as it is drawn now. */
const drawn = computed(
  () =>
    new Set([
      'to',
      'action',
      'requiresComment',
      'condition',
      ...drawnRuleFields(transition.value.allowedBy).map((field) =>
        field === '' ? 'allowedBy' : `allowedBy.${field}`,
      ),
    ]),
)

/** Messages about the transition itself, or a field of it this row has no input for. */
const rowMessages = computed(() =>
  Object.entries(errors.value)
    .filter(([field]) => !drawn.value.has(field))
    .map(([field, message]) => ({ field, message })),
)

/** `allowedBy`'s messages, relative to the rule, for its editor. */
const ruleErrors = computed(() => {
  const found: Record<string, string> = {}

  for (const [field, message] of Object.entries(errors.value)) {
    if (field === 'allowedBy') {
      found[''] = message
    } else if (field.startsWith('allowedBy.')) {
      found[field.slice('allowedBy.'.length)] = message
    }
  }

  return found
})

const stateOptions = computed(() => {
  const states = editor.draft.definition.value.states
  const options = states.map((state) => ({
    value: state.code,
    label: `${state.name} (${state.code})`,
  }))

  // A target no state declares is shown as itself, so S2's refusal has something to point at.
  return states.some((state) => state.code === transition.value.to)
    ? options
    : [...options, { value: transition.value.to, label: `${transition.value.to} (not declared)` }]
})

const actionOptions = optionsOf(TRANSITION_ACTIONS)

/** Whether a neighbour out of the same state is there to swap with. */
function canMove(direction: -1 | 1): boolean {
  const transitions = editor.draft.definition.value.transitions

  for (let at = props.index + direction; at >= 0 && at < transitions.length; at += direction) {
    if (transitions[at].from === transition.value.from) {
      return true
    }
  }

  return false
}

function change(next: JwssTransition, field: string): void {
  editor.draft.replaceTransition(props.index, next, `${rowKey.value}:${field}`)
  editor.clearField(`${path.value}.${field}`)
}

function setTo(to: string): void {
  change({ ...transition.value, to }, 'to')
}

function setAction(action: string): void {
  change({ ...transition.value, action: action as TransitionAction }, 'action')
}

function setRequiresComment(required: boolean): void {
  const next: JwssTransition = { ...transition.value, requiresComment: required }

  // `false` is JWSS's default; leaving it out writes the same edge.
  if (!required) {
    delete next.requiresComment
  }

  change(next, 'requiresComment')
}

function setAllowedBy(rule: AssignmentRule | undefined, field: string): void {
  const next: JwssTransition = { ...transition.value, allowedBy: rule }

  if (rule === undefined) {
    delete next.allowedBy
  }

  editor.draft.replaceTransition(props.index, next, `${rowKey.value}:allowedBy.${field}`)
  editor.clearField(`${path.value}.allowedBy`)
  editor.clearField(`${path.value}.allowedBy.${field}`)
}

/**
 * The logic builder's `v-model` write. The value it emitted is stored as it
 * came — no clone — so its echo is recognised and the tree it is editing,
 * unfilled operands included, is not reloaded under the user.
 */
function setCondition(condition: unknown): void {
  const next: JwssTransition = { ...transition.value, condition }

  if (condition === undefined) {
    delete next.condition
  }

  change(next, 'condition')
}

function remove(): void {
  editor.draft.removeTransition(props.index)
  editor.resetErrors()
}

function move(direction: -1 | 1): void {
  editor.draft.moveTransition(props.index, direction)
  editor.resetErrors()
}
</script>

<template>
  <TableRow :data-testid="`transition-${index}`" class="align-top">
    <TableCell class="align-top">
      <Select
        :id="`${id}-action`"
        :aria-label="`${label}: action`"
        :model-value="transition.action"
        :options="actionOptions"
        :invalid="Boolean(errors.action)"
        :described-by="`${id}-action-hint`"
        :disabled="editor.readOnly.value"
        @update:model-value="setAction"
      />
      <p :id="`${id}-action-hint`" class="mt-1 text-xs text-muted-foreground">
        {{ TRANSITION_ACTIONS[transition.action]?.firedBy }}
      </p>
      <p v-if="errors.action" class="text-xs text-destructive">{{ errors.action }}</p>
    </TableCell>

    <TableCell class="align-top">
      <Select
        :id="`${id}-to`"
        :aria-label="`${label}: to`"
        :model-value="transition.to"
        :options="stateOptions"
        :invalid="Boolean(errors.to)"
        :described-by="errors.to ? `${id}-to-message` : undefined"
        :disabled="editor.readOnly.value"
        @update:model-value="setTo"
      />
      <p v-if="errors.to" :id="`${id}-to-message`" class="mt-1 text-xs text-destructive">
        {{ errors.to }}
      </p>
    </TableCell>

    <TableCell class="min-w-56 align-top">
      <AssignmentRuleEditor
        :id="`${id}-allowed-by`"
        :model-value="transition.allowedBy"
        :label="`${label} allowed by`"
        :errors="ruleErrors"
        :variables="editor.variables.value"
        allow-none
        :disabled="editor.readOnly.value"
        @change="setAllowedBy"
        @validity-changed="editor.setValidity(`${rowKey}:allowedBy`, $event)"
      />
    </TableCell>

    <TableCell class="align-top">
      <label class="flex items-center gap-2 text-sm" :for="`${id}-comment`">
        <Checkbox
          :id="`${id}-comment`"
          :model-value="transition.requiresComment === true"
          :aria-invalid="Boolean(errors.requiresComment)"
          :aria-describedby="errors.requiresComment ? `${id}-comment-message` : undefined"
          :disabled="editor.readOnly.value"
          :data-testid="`requires-comment-${index}`"
          @update:model-value="setRequiresComment"
        />
        Requires a comment
      </label>
      <p
        v-if="errors.requiresComment"
        :id="`${id}-comment-message`"
        class="mt-1 text-xs text-destructive"
        :data-testid="`requires-comment-error-${index}`"
      >
        {{ errors.requiresComment }}
      </p>
    </TableCell>

    <TableCell class="whitespace-nowrap text-right align-top">
      <Button
        variant="ghost"
        size="sm"
        :aria-label="`Move ${label} up`"
        :disabled="editor.readOnly.value || !canMove(-1)"
        @click="move(-1)"
      >
        <ArrowUp class="size-4" aria-hidden="true" />
      </Button>
      <Button
        variant="ghost"
        size="sm"
        :aria-label="`Move ${label} down`"
        :disabled="editor.readOnly.value || !canMove(1)"
        @click="move(1)"
      >
        <ArrowDown class="size-4" aria-hidden="true" />
      </Button>
      <Button
        variant="ghost"
        size="sm"
        :aria-label="`Remove ${label}`"
        :disabled="editor.readOnly.value"
        @click="remove"
      >
        <Trash2 class="size-4" aria-hidden="true" />
      </Button>
    </TableCell>
  </TableRow>

  <TableRow :data-testid="`transition-${index}-detail`">
    <TableCell colspan="5" class="space-y-2 pt-0">
      <Alert
        v-if="rowMessages.length > 0"
        variant="destructive"
        :data-testid="`transition-${index}-messages`"
      >
        <ul class="list-disc pl-4">
          <li v-for="item in rowMessages" :key="item.field">{{ item.message }}</li>
        </ul>
      </Alert>

      <div class="space-y-1">
        <p class="text-xs font-medium">Condition</p>
        <JsonLogicBuilder
          :model-value="transition.condition"
          tier="conditional"
          :variables="editor.variables.value"
          allow-free-paths
          :free-prefixes="CONDITION_FREE_PREFIXES"
          :label="`${label} condition`"
          :disabled="editor.readOnly.value"
          @update:model-value="setCondition"
          @validity-changed="editor.setValidity(`${rowKey}:condition`, $event)"
        />
        <p v-if="errors.condition" class="text-xs text-destructive">{{ errors.condition }}</p>
        <p v-else-if="transition.condition === undefined" class="text-xs text-muted-foreground">
          None: this edge is taken for its action unless a sibling’s condition holds first (S7).
        </p>
      </div>
    </TableCell>
  </TableRow>
</template>
