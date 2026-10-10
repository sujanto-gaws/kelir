<script setup lang="ts">
import { computed } from 'vue'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type { JfssAdvancedRule, JfssComponent, JfssDataComponent } from '@/types/jfss'
import type { LookupSource } from '@/types/rad'

/**
 * One component of a form definition, edited (#373 AC1); since #688, the one
 * the canvas has selected.
 *
 * **It edits only what the node's role allows** (#688 A3): a data node its
 * key, label, value type, options, rules and expressions; a display node its
 * content; a panel or fieldset its `title`; a button its `label`, with the
 * action fixed at `submit`; columns and tabs nothing, since their slots are
 * managed on the canvas. **Moving and removing are the canvas's** (A4), so
 * this component no longer knows where its node sits.
 *
 * **The type is switched within its kind**: one data type for another
 * (`datagrid` excluded, whose row template another type cannot hold), one
 * display type for another. A layout node, a button and a data grid show
 * their type and do not change it.
 *
 * **Every type here is one the renderer can draw.** `SUPPORTED` in
 * `features/rad/renderer/registry.ts` is the list, and offering a type outside
 * it would produce a definition that saves, publishes, and then renders as the
 * registry's own *unsupported component* placeholder — a form that passes every
 * check and cannot be filled in.
 *
 * **Expressions are JSON Logic in a text box, and that is a decision rather
 * than a shortcut** ([construction plan 11](../../../../projects/planning/11.%20Sprint%2015%20RAD%20Authoring%20Construction%20Plan.md)
 * §8). A visual expression builder is the part of this screen with no ceiling;
 * the bar #373 sets is that a form can be authored without `curl`, and a text
 * field the server validates clears it. What the server says about a malformed
 * expression is shown against the component that carries it.
 */
const props = defineProps<{
  component: JfssComponent
  disabled: boolean
  /** The S10.3 details whose `path` addresses this component. */
  details: ValidationDetail[]
  /** `settings.lookups[component.id]`, when this component is bound to a source. */
  lookupSource: string
}>()

const emit = defineEmits<{
  (event: 'update', component: JfssComponent): void
  (event: 'update:lookupSource', source: string): void
}>()

/** The data types the renderer draws, in the order an author meets them. */
const DATA_TYPES = [
  'textfield',
  'textarea',
  'number',
  'date',
  'select',
  'radio',
  'checkbox',
  'lookup',
] as const

/** The display types the renderer draws. */
const DISPLAY_TYPES = ['heading', 'paragraph', 'divider', 'alert'] as const

/** The four master-data sources `LookupSource` allows (**D-23**, FR-RAD-007). */
const LOOKUP_SOURCES: LookupSource[] = ['supplier', 'customer', 'employee', 'facility']

/**
 * The Validation Rule Registry names this build can actually enforce.
 *
 * **The other four registered names are deliberately absent.**
 * `passwordStrength`, `async`, `unique`, `exists` and `authorized` are in the
 * registry and `is_registered` accepts them, so a definition carrying one
 * *saves and publishes* — and then every submission against it is refused,
 * because [ADR-0016](../../../../docs/architectures/adr/0016.%20An%20Unenforceable%20Rule%20Refuses%20Rather%20Than%20Passes.md)
 * makes an unenforceable rule refuse rather than pass. Offering them here would
 * let an author build a form nobody can submit, with nothing on screen saying
 * so until somebody tried.
 *
 * **This list is a chooser, not a validator.** The server resolves every name
 * against its own catalogue at the write and again at the publish (**D-67**),
 * and that verdict is what refuses — a name typed past this list is caught
 * there rather than here. The cost of the duplication is stated: if the
 * registry gains an enforceable rule, this array is a second place to change.
 */
const ENFORCEABLE_RULES = ['matchesField', 'notMatchesField', 'regex', 'oneOf', 'notOneOf'] as const

const isData = computed(() => props.component.role === 'data')
const isDisplay = computed(() => props.component.role === 'display')
const isGrid = computed(() => isData.value && props.component.type === 'datagrid')
const data = computed(() => props.component as JfssDataComponent)
const record = computed(() => props.component as unknown as Record<string, unknown>)

/** A panel or fieldset: its `title` is all this editor writes. */
const hasTitle = computed(
  () => props.component.type === 'panel' || props.component.type === 'fieldset',
)
const isButton = computed(() => props.component.role === 'action')
/** Display types that render text; a divider has none (JFSS §4.4). */
const hasContent = computed(() => isDisplay.value && props.component.type !== 'divider')
/** `conditional` is offered where the flat builder offered it: data and display. */
const hasConditional = computed(() => isData.value || isDisplay.value)

/** Columns and tabs: nothing to edit here, since their slots live on the canvas. */
const hasSlots = computed(
  () => props.component.type === 'columns' || props.component.type === 'tabs',
)
const slotNote = computed(() =>
  props.component.type === 'tabs'
    ? 'Its tabs are added, removed and titled on the canvas.'
    : 'Its columns are added and removed on the canvas.',
)

/** Data types switch among themselves, display among theirs; the rest are fixed. */
const switchable = computed(() => (isData.value && !isGrid.value) || isDisplay.value)

const types = computed(() =>
  (isData.value ? DATA_TYPES : DISPLAY_TYPES).map((value) => ({ value, label: value })),
)

/** `Select` takes `{ value, label }` pairs rather than slotted options. */
function choices(values: readonly string[]): { value: string; label: string }[] {
  return values.map((value) => ({ value, label: value }))
}

const VALUE_TYPES = choices(['string', 'number', 'integer', 'boolean', 'array', 'object'])
const SCOPES = choices(['client', 'server', 'both'])
const ACTIONS = choices(['show', 'hide', 'enable', 'disable'])
const RULE_CHOICES = choices(ENFORCEABLE_RULES)
const SOURCE_CHOICES = [{ value: '', label: 'Choose a source…' }, ...choices(LOOKUP_SOURCES)]

/** The server's message for a property of this component, if it named one. */
function messageFor(suffix: string): string | undefined {
  return props.details.find((detail) => detail.path.endsWith(suffix))?.message
}

/** Every message about this component, so nothing the server said is dropped. */
const allMessages = computed(() => props.details.map((detail) => detail.message))

function patch(changes: Record<string, unknown>): void {
  emit('update', { ...props.component, ...changes } as JfssComponent)
}

/** `select` and `radio` are the two types whose choices come from the definition. */
const hasOptions = computed(() => data.value.type === 'select' || data.value.type === 'radio')

/**
 * Options as `label=value` lines, which is the shape an author types.
 *
 * A line with no `=` is a label that is its own value, because that is what
 * somebody writing `Draft` in a box means.
 */
const optionLines = computed({
  get: () =>
    (data.value.options ?? [])
      .map((option) => `${option.label}=${String(option.value ?? '')}`)
      .join('\n'),
  set: (text: string) => {
    const options = text
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
      .map((line) => {
        const at = line.indexOf('=')

        return at === -1
          ? { label: line, value: line }
          : { label: line.slice(0, at).trim(), value: line.slice(at + 1).trim() }
      })

    patch({ options })
  },
})

/** `calculate` and `conditional.logic` as text; empty clears the property. */
function jsonText(value: unknown): string {
  return value === undefined ? '' : JSON.stringify(value, null, 2)
}

/**
 * Parses an expression box, keeping the raw text when it is not JSON yet.
 *
 * **Half-typed JSON is the normal state of a text box**, so a parse failure
 * leaves the component alone rather than clearing the property — the
 * alternative deletes an author's expression as they type the second character
 * of it. What the server thinks of the finished expression is the verdict that
 * matters, and it arrives on save.
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

function setCalculate(text: string): void {
  const { ok, value } = parsed(text)

  if (ok) {
    patch({ calculate: value })
  }
}

function setConditional(text: string): void {
  const { ok, value } = parsed(text)

  if (!ok) {
    return
  }

  patch({
    conditional:
      value === undefined
        ? undefined
        : { action: props.component.conditional?.action ?? 'show', logic: value },
  })
}

function setConditionalAction(action: string): void {
  const existing = props.component.conditional

  if (!existing) {
    return
  }

  emit('update', {
    ...props.component,
    conditional: { ...existing, action: action as typeof existing.action },
  })
}

/**
 * A new rule, **with the `params` and `message` the meta-schema requires**
 * (#688 E7). `{rule, scope}` alone was refused at save: measured on
 * 2026-10-10, a save carrying it came back 422 with `"params" is a required
 * property` and `"message" is a required property`, and with both present it
 * was stored.
 */
function addRule(): void {
  const rules: JfssAdvancedRule[] = [
    ...(data.value.rules ?? []),
    { rule: ENFORCEABLE_RULES[0], scope: 'both', params: {}, message: 'This value is not valid.' },
  ]

  patch({ rules })
}

function updateRule(at: number, changes: Partial<JfssAdvancedRule>): void {
  const rules = (data.value.rules ?? []).map((rule, index) =>
    index === at ? { ...rule, ...changes } : rule,
  )

  patch({ rules })
}

function setRuleParams(at: number, text: string): void {
  const { ok, value } = parsed(text)

  if (ok) {
    // Emptied, the box means no parameters: `{}`, which the meta-schema requires.
    updateRule(at, { params: (value as Record<string, unknown> | undefined) ?? {} })
  }
}

function removeRule(at: number): void {
  patch({ rules: (data.value.rules ?? []).filter((_, index) => index !== at) })
}
</script>

<template>
  <div class="rounded-lg border p-4" :data-testid="`editor-${component.id}`">
    <div>
      <p class="text-sm font-medium">
        {{ isData ? data.label || data.key || component.id : component.id }}
      </p>
      <p class="text-xs text-muted-foreground">{{ component.role }} · {{ component.type }}</p>
    </div>

    <!-- Everything the server said about this component, whether or not a
         field below claims it. A detail addressed to a property this editor
         does not offer would otherwise vanish. -->
    <ul
      v-if="allMessages.length > 0"
      class="mt-3 space-y-1"
      :data-testid="`editor-errors-${component.id}`"
    >
      <li v-for="message in allMessages" :key="message" class="text-xs text-destructive">
        {{ message }}
      </li>
    </ul>

    <div class="mt-4 grid gap-4 sm:grid-cols-2">
      <div class="space-y-2">
        <Label :for="`type-${component.id}`">Type</Label>
        <Select
          v-if="switchable"
          :id="`type-${component.id}`"
          :model-value="component.type"
          :disabled="disabled"
          :options="types"
          :data-testid="`type-${component.id}`"
          @update:model-value="patch({ type: String($event) })"
        />
        <!-- A layout node, a button and a data grid keep their type: what
             they hold, or do, is not something another type has. -->
        <Input
          v-else
          :id="`type-${component.id}`"
          :model-value="component.type"
          disabled
          :data-testid="`type-${component.id}`"
        />
      </div>

      <template v-if="isData">
        <div class="space-y-2">
          <Label :for="`key-${component.id}`">Key</Label>
          <Input
            :id="`key-${component.id}`"
            :model-value="data.key"
            :disabled="disabled"
            :data-testid="`key-${component.id}`"
            @update:model-value="patch({ key: String($event) })"
          />
          <p v-if="messageFor('.key')" class="text-xs text-destructive">
            {{ messageFor('.key') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label :for="`label-${component.id}`">Label</Label>
          <Input
            :id="`label-${component.id}`"
            :model-value="data.label"
            :disabled="disabled"
            :data-testid="`label-${component.id}`"
            @update:model-value="patch({ label: String($event) })"
          />
        </div>

        <div v-if="!isGrid" class="space-y-2">
          <Label :for="`vtype-${component.id}`">Value type</Label>
          <Select
            :id="`vtype-${component.id}`"
            :model-value="data.validation.type"
            :disabled="disabled"
            :options="VALUE_TYPES"
            :data-testid="`vtype-${component.id}`"
            @update:model-value="
              patch({ validation: { ...data.validation, type: $event as never } })
            "
          />
        </div>

        <div class="flex items-end gap-2">
          <input
            :id="`required-${component.id}`"
            type="checkbox"
            class="h-4 w-4"
            :checked="data.validation.required === true"
            :disabled="disabled"
            :data-testid="`required-${component.id}`"
            @change="
              patch({
                validation: {
                  ...data.validation,
                  required: ($event.target as HTMLInputElement).checked,
                },
              })
            "
          />
          <Label :for="`required-${component.id}`">Required</Label>
        </div>

        <div v-if="hasOptions" class="space-y-2 sm:col-span-2">
          <Label :for="`options-${component.id}`">Options</Label>
          <textarea
            :id="`options-${component.id}`"
            v-model="optionLines"
            rows="3"
            class="w-full rounded-md border bg-background p-2 font-mono text-xs"
            :disabled="disabled"
            :data-testid="`options-${component.id}`"
          ></textarea>
          <p class="text-xs text-muted-foreground">
            One per line, <code>label=value</code>. A line with no <code>=</code> is its own value.
          </p>
        </div>

        <div v-if="data.type === 'lookup'" class="space-y-2 sm:col-span-2">
          <Label :for="`lookup-${component.id}`">Master data source</Label>
          <Select
            :id="`lookup-${component.id}`"
            :model-value="lookupSource"
            :disabled="disabled"
            :options="SOURCE_CHOICES"
            :data-testid="`lookup-${component.id}`"
            @update:model-value="emit('update:lookupSource', String($event))"
          />
          <!-- The binding lives in `settings.lookups`, keyed by this
               component's `id`, because JFSS is frozen and a `dataSource`
               property on the component would make the document
               non-conformant (D-23). -->
          <p class="text-xs text-muted-foreground">
            Options come from master data the person filling the form may already read.
          </p>
        </div>

        <div v-if="!isGrid" class="space-y-2 sm:col-span-2">
          <Label :for="`calculate-${component.id}`">Calculate (JSON Logic)</Label>
          <textarea
            :id="`calculate-${component.id}`"
            rows="3"
            class="w-full rounded-md border bg-background p-2 font-mono text-xs"
            :value="jsonText(data.calculate)"
            :disabled="disabled"
            :data-testid="`calculate-${component.id}`"
            @input="setCalculate(($event.target as HTMLTextAreaElement).value)"
          ></textarea>
          <p class="text-xs text-muted-foreground">
            The server recomputes this on every write and overwrites what the browser sent.
          </p>
        </div>
      </template>

      <div v-if="hasContent" class="space-y-2 sm:col-span-2">
        <Label :for="`content-${component.id}`">Content</Label>
        <Input
          :id="`content-${component.id}`"
          :model-value="String(record.content ?? '')"
          :disabled="disabled"
          :data-testid="`content-${component.id}`"
          @update:model-value="patch({ content: String($event) })"
        />
      </div>

      <div v-if="hasTitle" class="space-y-2 sm:col-span-2">
        <Label :for="`title-${component.id}`">Title</Label>
        <Input
          :id="`title-${component.id}`"
          :model-value="String(record.title ?? '')"
          :disabled="disabled"
          :data-testid="`title-${component.id}`"
          @update:model-value="patch({ title: String($event) })"
        />
      </div>

      <template v-if="isButton">
        <div class="space-y-2">
          <Label :for="`button-label-${component.id}`">Label</Label>
          <Input
            :id="`button-label-${component.id}`"
            :model-value="String(record.label ?? '')"
            :disabled="disabled"
            :data-testid="`button-label-${component.id}`"
            @update:model-value="patch({ label: String($event) })"
          />
        </div>
        <div class="space-y-2">
          <Label :for="`button-action-${component.id}`">Action</Label>
          <!-- `submit` is the one action part 1 offers. One loaded with
               another is shown and kept, not offered. -->
          <Input
            :id="`button-action-${component.id}`"
            :model-value="String(record.action ?? '')"
            disabled
            :data-testid="`button-action-${component.id}`"
          />
        </div>
      </template>

      <p v-if="hasSlots" class="text-xs text-muted-foreground sm:col-span-2">
        {{ slotNote }}
      </p>

      <div v-if="hasConditional" class="space-y-2 sm:col-span-2">
        <Label :for="`conditional-${component.id}`">Conditional (JSON Logic)</Label>
        <div class="flex gap-2">
          <Select
            :model-value="component.conditional?.action ?? 'show'"
            :disabled="disabled || !component.conditional"
            :options="ACTIONS"
            :data-testid="`conditional-action-${component.id}`"
            @update:model-value="setConditionalAction(String($event))"
          />
        </div>
        <textarea
          :id="`conditional-${component.id}`"
          rows="3"
          class="w-full rounded-md border bg-background p-2 font-mono text-xs"
          :value="jsonText(component.conditional?.logic)"
          :disabled="disabled"
          :data-testid="`conditional-${component.id}`"
          @input="setConditional(($event.target as HTMLTextAreaElement).value)"
        ></textarea>
        <p class="text-xs text-muted-foreground">
          Re-decided on the server against the whole payload, after calculations run.
        </p>
      </div>

      <div v-if="isData" class="space-y-3 sm:col-span-2">
        <div class="flex items-center justify-between">
          <Label>Rules</Label>
          <Button
            size="sm"
            variant="secondary"
            :disabled="disabled"
            :data-testid="`add-rule-${component.id}`"
            @click="addRule"
          >
            Add rule
          </Button>
        </div>

        <div
          v-for="(rule, at) in data.rules ?? []"
          :key="at"
          class="grid gap-2 rounded border p-2 sm:grid-cols-3"
        >
          <Select
            :model-value="rule.rule"
            :disabled="disabled"
            :options="RULE_CHOICES"
            :data-testid="`rule-${component.id}-${at}`"
            @update:model-value="updateRule(at, { rule: String($event) })"
          />

          <Select
            :model-value="rule.scope"
            :disabled="disabled"
            :options="SCOPES"
            :data-testid="`rule-scope-${component.id}-${at}`"
            @update:model-value="updateRule(at, { scope: $event as never })"
          />

          <Button
            size="sm"
            variant="destructive"
            :disabled="disabled"
            :data-testid="`remove-rule-${component.id}-${at}`"
            @click="removeRule(at)"
          >
            Remove
          </Button>

          <textarea
            rows="2"
            class="w-full rounded-md border bg-background p-2 font-mono text-xs sm:col-span-3"
            placeholder='{"pattern": "^[A-Z]+$"}'
            :value="jsonText(rule.params)"
            :disabled="disabled"
            :data-testid="`rule-params-${component.id}-${at}`"
            @input="setRuleParams(at, ($event.target as HTMLTextAreaElement).value)"
          ></textarea>
        </div>

        <p class="text-xs text-muted-foreground">
          The five the registry defines and this build enforces. A rule it cannot enforce refuses
          the submission rather than passing it, so the others are not offered.
        </p>
      </div>
    </div>
  </div>
</template>
