<script setup lang="ts">
import { computed } from 'vue'

import JsonLogicBuilder from '@/components/logic-builder/JsonLogicBuilder.vue'
import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { Input } from '@/components/ui/input'
import { Select } from '@/components/ui/select'
import type { AssigneeType, AssignmentRule } from '@/types/workflow'

import { readRule, retype } from './assignmentRule'
import { ASSIGNEE_TYPES, CONDITION_FREE_PREFIXES } from './jwssRegistry'

/**
 * One JWSS assignment rule: a task's `assignment`, or a transition's
 * `allowedBy` (JWSS §5).
 *
 * **It offers the four types Kelir resolves**, and shows the other two only
 * when a stored rule already uses one, so it reads as what it is; the server
 * refuses both at save, at this field (§5.3). An `EXPRESSION` rule's
 * expression is edited in the shared logic builder, in the conditional tier.
 *
 * It emits the whole rule on every edit (`change`), with the field that
 * changed, so the host can make consecutive keystrokes in one field a single
 * undo step. An untouched shorthand string is never rewritten.
 */
const props = withDefaults(
  defineProps<{
    modelValue: AssignmentRule | string | undefined
    /** The accessible name of the rule, and the prefix of each control's. */
    label: string
    id: string
    /** Messages by field relative to the rule: `''`, `roleCode`, `expression`… */
    errors: Record<string, string>
    variables: readonly LogicVariable[]
    /** Whether *no rule* is a choice: an `AUTO` edge has none (S5). */
    allowNone?: boolean
    disabled?: boolean
  }>(),
  { allowNone: false, disabled: false },
)

const emit = defineEmits<{
  change: [value: AssignmentRule | undefined, field: string]
  validityChanged: [valid: boolean]
}>()

const read = computed(() => readRule(props.modelValue))
const rule = computed(() => (read.value.kind === 'rule' ? read.value.rule : undefined))
const assigneeType = computed(() => rule.value?.assigneeType ?? '')

const typeOptions = computed(() => [
  ...(props.allowNone ? [{ value: '', label: 'No one — an automatic edge' }] : []),
  ...(Object.keys(ASSIGNEE_TYPES) as AssigneeType[])
    .filter((type) => ASSIGNEE_TYPES[type].resolved || type === assigneeType.value)
    .map((type) => ({ value: type, label: ASSIGNEE_TYPES[type].label })),
])

/** The message under the type chooser: one about the rule as a whole, or about its type. */
const typeMessage = computed(() =>
  [props.errors[''], props.errors.assigneeType].filter(Boolean).join(' '),
)

function setType(next: string): void {
  emit('change', next === '' ? undefined : retype(rule.value, next as AssigneeType), 'assigneeType')
}

function setField(field: 'roleCode' | 'userId' | 'departmentScope', value: string): void {
  if (!rule.value) {
    return
  }

  const next: AssignmentRule = { ...rule.value, [field]: value }

  // An optional scope left blank is no scope, not an empty one.
  if (field === 'departmentScope' && value === '') {
    delete next.departmentScope
  }

  emit('change', next, field)
}

function setExpression(value: unknown): void {
  if (!rule.value) {
    return
  }

  const next: AssignmentRule = { ...rule.value }

  if (value === undefined) {
    delete next.expression
  } else {
    next.expression = value
  }

  emit('change', next, 'expression')
}
</script>

<template>
  <div class="space-y-2">
    <Select
      :id="`${id}-type`"
      :aria-label="`${label}: who`"
      :model-value="assigneeType"
      :options="typeOptions"
      :placeholder="allowNone ? undefined : 'Choose who'"
      :invalid="typeMessage !== ''"
      :described-by="typeMessage ? `${id}-type-message` : undefined"
      :disabled="disabled"
      @update:model-value="setType"
    />
    <p v-if="read.kind === 'unreadable'" class="text-xs text-muted-foreground">
      Stored as <code>{{ read.text }}</code
      >, which is not a shorthand JWSS names.
    </p>
    <p v-if="typeMessage" :id="`${id}-type-message`" class="text-xs text-destructive">
      {{ typeMessage }}
    </p>

    <template
      v-if="rule && (rule.assigneeType === 'ROLE' || rule.assigneeType === 'DEPARTMENT_ROLE')"
    >
      <Input
        :id="`${id}-role`"
        :aria-label="`${label}: role code`"
        placeholder="Role code"
        :model-value="rule.roleCode ?? ''"
        :invalid="Boolean(errors.roleCode)"
        :described-by="errors.roleCode ? `${id}-role-message` : undefined"
        :disabled="disabled"
        @update:model-value="setField('roleCode', $event)"
      />
      <p v-if="errors.roleCode" :id="`${id}-role-message`" class="text-xs text-destructive">
        {{ errors.roleCode }}
      </p>
    </template>

    <template v-if="rule?.assigneeType === 'DEPARTMENT_ROLE'">
      <Input
        :id="`${id}-scope`"
        :aria-label="`${label}: department`"
        placeholder="REQUESTED_DEPARTMENT, OWNER_DEPARTMENT or a code"
        :list="`${id}-scopes`"
        :model-value="rule.departmentScope ?? ''"
        :invalid="Boolean(errors.departmentScope)"
        :described-by="errors.departmentScope ? `${id}-scope-message` : undefined"
        :disabled="disabled"
        @update:model-value="setField('departmentScope', $event)"
      />
      <datalist :id="`${id}-scopes`">
        <option value="REQUESTED_DEPARTMENT" />
        <option value="OWNER_DEPARTMENT" />
      </datalist>
      <p v-if="errors.departmentScope" :id="`${id}-scope-message`" class="text-xs text-destructive">
        {{ errors.departmentScope }}
      </p>
    </template>

    <template v-if="rule?.assigneeType === 'USER'">
      <Input
        :id="`${id}-user`"
        :aria-label="`${label}: user id`"
        placeholder="User id"
        :model-value="rule.userId ?? ''"
        :invalid="Boolean(errors.userId)"
        :described-by="errors.userId ? `${id}-user-message` : undefined"
        :disabled="disabled"
        @update:model-value="setField('userId', $event)"
      />
      <p v-if="errors.userId" :id="`${id}-user-message`" class="text-xs text-destructive">
        {{ errors.userId }}
      </p>
    </template>

    <template v-if="rule?.assigneeType === 'EXPRESSION'">
      <JsonLogicBuilder
        :model-value="rule.expression"
        tier="conditional"
        :variables="variables"
        allow-free-paths
        :free-prefixes="CONDITION_FREE_PREFIXES"
        :label="`${label}: expression`"
        :disabled="disabled"
        @update:model-value="setExpression"
        @validity-changed="emit('validityChanged', $event)"
      />
      <p v-if="errors.expression" class="text-xs text-destructive">{{ errors.expression }}</p>
    </template>
  </div>
</template>
