<script setup lang="ts">
import { computed } from 'vue'

import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { JfssAdvancedRule } from '@/types/jfss'

import { messageAt } from './propertyEdits'

/**
 * A data field's advanced rules (JFSS §6.2): **a self-contained unit, its test
 * ids unchanged since #373** (#688 AC10): `add-rule-{id}`, `rule-{id}-{at}`,
 * `rule-scope-{id}-{at}`, `rule-params-{id}-{at}` and `remove-rule-{id}-{at}`.
 *
 * **A rule's `params` stay raw JSON** (**D-110**): the visual builder is for
 * expressions, and a rule's parameters are not one. The raw box is where
 * #722's flag lands, after this row; this row adds no `params` check.
 */
const props = defineProps<{
  componentId: string
  /** The node's path, for the server's details (S10.3). */
  nodePath: string
  rules: JfssAdvancedRule[] | undefined
  disabled: boolean
  details: ValidationDetail[]
}>()

const emit = defineEmits<{ 'update:rules': [rules: JfssAdvancedRule[]] }>()

/**
 * The Validation Rule Registry names this build can actually enforce.
 *
 * **The other registered names are deliberately absent.** `passwordStrength`,
 * `async`, `unique`, `exists` and `authorized` are in the registry and
 * `is_registered` accepts them, so a definition carrying one *saves and
 * publishes* — and then every submission against it is refused, because
 * [ADR-0016](../../../../../docs/architectures/adr/0016.%20An%20Unenforceable%20Rule%20Refuses%20Rather%20Than%20Passes.md)
 * makes an unenforceable rule refuse rather than pass. Offering them here would
 * let an author build a form nobody can submit, with nothing on screen saying
 * so until somebody tried.
 *
 * **This list is a chooser, not a validator.** The server resolves every name
 * against its own catalogue at the write and again at the publish (**D-67**),
 * and that verdict is what refuses. A loaded rule outside the list is shown as
 * itself and kept; it is not offered.
 */
const ENFORCEABLE_RULES = ['matchesField', 'notMatchesField', 'regex', 'oneOf', 'notOneOf'] as const

/**
 * Each enforceable rule's parameters, empty, by the names the Validation Rule
 * Registry gives them (§3.1): what *Add rule* and a change of rule write, so
 * the box shows the author what to fill in (schema-guardian M1).
 */
const RULE_PARAMS: Record<string, () => Record<string, unknown>> = {
  matchesField: () => ({ target: '' }),
  notMatchesField: () => ({ target: '' }),
  regex: () => ({ pattern: '' }),
  oneOf: () => ({ values: [] }),
  notOneOf: () => ({ values: [] }),
}

function seededParams(rule: string): Record<string, unknown> {
  return Object.prototype.hasOwnProperty.call(RULE_PARAMS, rule) ? RULE_PARAMS[rule]() : {}
}

function choices(values: readonly string[]): { value: string; label: string }[] {
  return values.map((value) => ({ value, label: value }))
}

const SCOPES = choices(['client', 'server', 'both'])

/** The five, plus a loaded name outside them, shown as itself and not choosable. */
function ruleChoices(name: string): { value: string; label: string; disabled?: boolean }[] {
  const offered = choices(ENFORCEABLE_RULES)

  return (ENFORCEABLE_RULES as readonly string[]).includes(name)
    ? offered
    : [...offered, { value: name, label: `${name} (not offered)`, disabled: true }]
}

const list = computed(() => props.rules ?? [])

/** `params` as text; an absent one shows empty. */
function jsonText(value: unknown): string {
  return value === undefined ? '' : JSON.stringify(value, null, 2)
}

/**
 * Parses a params box, keeping the rule as it is while the text is not JSON
 * yet: half-typed JSON is the normal state of a text box.
 */
function parsed(text: string): { ok: boolean; value: unknown } {
  const trimmed = text.trim()

  if (trimmed === '') {
    return { ok: true, value: undefined }
  }

  try {
    return { ok: true, value: JSON.parse(trimmed) }
  } catch {
    return { ok: false, value: undefined }
  }
}

/**
 * A new rule, **with the `params` and `message` the meta-schema requires**
 * (#688 E7). `{rule, scope}` alone was refused at save: measured on
 * 2026-10-10, a save carrying it came back 422 with `"params" is a required
 * property` and `"message" is a required property`.
 */
function addRule(): void {
  emit('update:rules', [
    ...list.value,
    {
      rule: ENFORCEABLE_RULES[0],
      scope: 'both',
      params: seededParams(ENFORCEABLE_RULES[0]),
      message: 'This value is not valid.',
    },
  ])
}

function updateRule(at: number, changes: Partial<JfssAdvancedRule>): void {
  emit(
    'update:rules',
    list.value.map((rule, index) => (index === at ? { ...rule, ...changes } : rule)),
  )
}

function setRuleParams(at: number, text: string): void {
  const { ok, value } = parsed(text)

  if (ok) {
    // Emptied, the box means no parameters: `{}`, which the meta-schema requires.
    updateRule(at, { params: (value as Record<string, unknown> | undefined) ?? {} })
  }
}

function removeRule(at: number): void {
  emit(
    'update:rules',
    list.value.filter((_, index) => index !== at),
  )
}

/** The server's message about a rule's parameters (AC9). */
function paramsMessage(at: number): string | undefined {
  return messageAt(props.details, props.nodePath, `rules.${at}.params`)
}
</script>

<template>
  <div class="space-y-3 sm:col-span-2" :data-testid="`rules-${componentId}`">
    <div class="flex items-center justify-between">
      <Label>Rules</Label>
      <Button
        size="sm"
        variant="secondary"
        :disabled="disabled"
        :data-testid="`add-rule-${componentId}`"
        @click="addRule"
      >
        Add rule
      </Button>
    </div>

    <div v-for="(rule, at) in list" :key="at" class="grid gap-2 rounded border p-2 sm:grid-cols-3">
      <Select
        :model-value="rule.rule"
        :aria-label="`Rule ${at + 1}`"
        :disabled="disabled"
        :options="ruleChoices(rule.rule)"
        :data-testid="`rule-${componentId}-${at}`"
        @update:model-value="
          updateRule(at, { rule: String($event), params: seededParams(String($event)) })
        "
      />

      <Select
        :model-value="rule.scope"
        :aria-label="`Rule ${at + 1} scope`"
        :disabled="disabled"
        :options="SCOPES"
        :data-testid="`rule-scope-${componentId}-${at}`"
        @update:model-value="updateRule(at, { scope: $event as never })"
      />

      <Button
        size="sm"
        variant="destructive"
        :disabled="disabled"
        :data-testid="`remove-rule-${componentId}-${at}`"
        @click="removeRule(at)"
      >
        Remove
      </Button>

      <textarea
        rows="2"
        class="w-full rounded-md border bg-background p-2 font-mono text-xs sm:col-span-3"
        placeholder='{"pattern": "^[A-Z]+$"}'
        :aria-label="`Rule ${at + 1} parameters (JSON)`"
        :aria-invalid="paramsMessage(at) !== undefined"
        :value="jsonText(rule.params)"
        :disabled="disabled"
        :data-testid="`rule-params-${componentId}-${at}`"
        @input="setRuleParams(at, ($event.target as HTMLTextAreaElement).value)"
      ></textarea>
      <p
        v-if="paramsMessage(at)"
        class="text-xs text-destructive sm:col-span-3"
        :data-testid="`rule-params-error-${componentId}-${at}`"
      >
        {{ paramsMessage(at) }}
      </p>
    </div>

    <p class="text-xs text-muted-foreground">
      The five the registry defines and this build enforces. A rule it cannot enforce refuses the
      submission rather than passing it, so the others are not offered.
    </p>
  </div>
</template>
