<script setup lang="ts">
import { computed } from 'vue'

import type { LogicVariable } from '@/components/logic-builder/logicTree'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type {
  JfssActionComponent,
  JfssAdvancedRule,
  JfssComponent,
  JfssDataComponent,
  JfssDisplayComponent,
  JfssLayoutComponent,
} from '@/types/jfss'

import ButtonProperties from './ButtonProperties.vue'
import DataFieldProperties from './DataFieldProperties.vue'
import DataGridProperties from './DataGridProperties.vue'
import DisplayProperties from './DisplayProperties.vue'
import ExpressionProperties from './ExpressionProperties.vue'
import LayoutProperties from './LayoutProperties.vue'
import { switchType } from './propertyEdits'
import ValidationRulesSection from './ValidationRulesSection.vue'

/**
 * **The form builder's properties panel**: the one node the canvas has
 * selected, edited (FR-RAD-013, #688 part 2, **D-86** A, **D-110**, **D-116**).
 *
 * It replaces part 1's `FormComponentEditor` and keeps what that did: the type
 * switches, the seeded rule params, the unbound-lookup note, and the server's
 * messages at their field. **Each group of types has a body of its own**, and
 * the panel composes them:
 *
 * | Types | Body |
 * |---|---|
 * | the data types but `datagrid` | `DataFieldProperties`, then expressions and rules |
 * | `datagrid` | `DataGridProperties`, then `conditional` and rules |
 * | `panel`, `fieldset`, `columns`, `tabs` | `LayoutProperties` |
 * | `heading`, `paragraph`, `divider`, `alert` | `DisplayProperties`, then `conditional` |
 * | `button` | `ButtonProperties` |
 *
 * **It never rewrites what it does not show** (AC4). Every edit spreads the
 * node and changes one property, so `defaultValue`, `readOnly`,
 * `calculateMode`, `format`, `enum`, `messages`, `grid`, `theme`, an unknown
 * rule and an expression the builder cannot show all save as they were loaded.
 *
 * **It evaluates nothing** (D-10), and it validates nothing the server
 * validates (ADR-0035): every limit, pattern, key and expression is the
 * server's to refuse, and a refusal is shown at the field it names, and in the
 * list at the top so none is lost (AC9).
 *
 * **Every type here is one the renderer can draw**: the type switch offers
 * data types for data and display types for display, from `SUPPORTED`. A
 * layout node, a button and a data grid show their type read-only.
 */
const props = withDefaults(
  defineProps<{
    component: JfssComponent
    /** Where the node sits, as S10.3 addresses it without `definition.`: `components.0`. */
    path: string
    disabled: boolean
    /** The S10.3 details the page placed on this node. */
    details: ValidationDetail[]
    /** `settings.lookups[component.id]`, when this component is bound to a source. */
    lookupSource: string
    /** The data keys an expression on this node may read (`expressionScope.ts`). */
    scope: readonly LogicVariable[]
    /** The keys more than one data node in the form uses (`formTree.duplicateKeys`). */
    duplicateKeys?: readonly string[]
  }>(),
  { duplicateKeys: () => [] },
)

const emit = defineEmits<{
  update: [component: JfssComponent]
  'update:lookupSource': [source: string]
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

const isData = computed(() => props.component.role === 'data')
const isGrid = computed(() => isData.value && props.component.type === 'datagrid')
const isDisplay = computed(() => props.component.role === 'display')

/** Whether this node's key is one another data node also uses (JFSS §4.2). */
const keyTaken = computed(
  () => isData.value && props.duplicateKeys.includes((props.component as JfssDataComponent).key),
)

/** Data types switch among themselves, display among theirs; the rest are fixed. */
const switchable = computed(() => (isData.value && !isGrid.value) || isDisplay.value)

const types = computed(() =>
  (isData.value ? DATA_TYPES : DISPLAY_TYPES).map((value) => ({ value, label: value })),
)

const heading = computed(() => {
  const node = props.component as unknown as Record<string, unknown>
  const name = isData.value ? node.label || node.key : undefined

  return typeof name === 'string' && name !== '' ? name : props.component.id
})

/** Every message about this node, so nothing the server said is dropped. */
const allMessages = computed(() => props.details.map((detail) => detail.message))

function update(component: JfssComponent): void {
  emit('update', component)
}

function setRules(rules: JfssAdvancedRule[]): void {
  emit('update', { ...(props.component as JfssDataComponent), rules })
}
</script>

<template>
  <div class="rounded-lg border p-4" :data-testid="`editor-${component.id}`">
    <div>
      <p class="text-sm font-medium">{{ heading }}</p>
      <p class="text-xs text-muted-foreground">{{ component.role }} · {{ component.type }}</p>
    </div>

    <!-- Everything the server said about this node, whether or not a field
         below claims it. A detail addressed to a property this panel does not
         offer would otherwise vanish. -->
    <ul
      v-if="allMessages.length > 0"
      class="mt-3 space-y-1"
      :data-testid="`editor-errors-${component.id}`"
    >
      <li v-for="(message, at) in allMessages" :key="at" class="text-xs text-destructive">
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
          @update:model-value="update(switchType(component, String($event)))"
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

      <DataGridProperties
        v-if="isGrid"
        :component="component as JfssDataComponent"
        :node-path="path"
        :disabled="disabled"
        :details="details"
        :key-taken="keyTaken"
        @update="update"
      />
      <DataFieldProperties
        v-else-if="isData"
        :component="component as JfssDataComponent"
        :node-path="path"
        :disabled="disabled"
        :details="details"
        :lookup-source="lookupSource"
        :key-taken="keyTaken"
        @update="update"
        @update:lookup-source="emit('update:lookupSource', $event)"
      />
      <DisplayProperties
        v-else-if="isDisplay"
        :component="component as JfssDisplayComponent"
        :disabled="disabled"
        @update="update"
      />
      <LayoutProperties
        v-else-if="component.role === 'layout'"
        :component="component as JfssLayoutComponent"
        :disabled="disabled"
        @update="update"
      />
      <ButtonProperties
        v-else-if="component.role === 'action'"
        :component="component as JfssActionComponent"
        :disabled="disabled"
        @update="update"
      />

      <ExpressionProperties
        :component="component"
        :node-path="path"
        :scope="scope"
        :disabled="disabled"
        :details="details"
        @update="update"
      />

      <ValidationRulesSection
        v-if="isData"
        :component-id="component.id"
        :node-path="path"
        :rules="(component as JfssDataComponent).rules"
        :disabled="disabled"
        :details="details"
        @update:rules="setRules"
      />
    </div>
  </div>
</template>
