<script setup lang="ts">
import { Plus, Trash2 } from '@lucide/vue'
import { ref, watch } from 'vue'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { JfssOption } from '@/types/jfss'

import { readOptionValue, shownOptionValue } from './propertyEdits'

/**
 * A select's or radio's options, a row each (#790): a label, a value and a
 * Remove, then *Add option*.
 *
 * **An edit replaces only the option its row holds**, by index. Every other
 * option is the object as loaded, so its value keeps its type. A label is
 * stored as typed. A value is read by the field's value type
 * (`readOptionValue`), and **what was typed stays in the box**, as
 * `PropertyNumberInput` keeps it: `1.` is stored as `1` and still shows `1.`.
 */
const props = defineProps<{
  componentId: string
  options: readonly JfssOption[]
  valueType: string
  disabled: boolean
}>()

const emit = defineEmits<{ update: [options: JfssOption[]] }>()

/** The text typed in each row's value box, by index; none for a row not edited. */
const drafts = ref<(string | undefined)[]>([])

// A draft the stored value no longer matches, after a reload or a type
// switch, gives way to the stored value.
watch([() => props.options, () => props.valueType], ([options, type]) => {
  drafts.value = drafts.value.map((draft, at) =>
    draft !== undefined && readOptionValue(draft, type).value === options[at]?.value
      ? draft
      : undefined,
  )
})

function replace(index: number, option: JfssOption): void {
  emit(
    'update',
    props.options.map((current, at) => (at === index ? option : current)),
  )
}

function setValue(index: number, text: string): void {
  drafts.value[index] = text
  replace(index, { ...props.options[index], value: readOptionValue(text, props.valueType).value })
}

function remove(index: number): void {
  drafts.value.splice(index, 1)
  emit(
    'update',
    props.options.filter((_, at) => at !== index),
  )
}

function asText(index: number): boolean {
  const draft = drafts.value[index]

  return draft !== undefined && readOptionValue(draft, props.valueType).asText
}

/** What is said under a value: that it is stored as text, or the type of a value that is not text. */
function note(index: number): string | undefined {
  return asText(index)
    ? 'Stored as text, which does not match the value type.'
    : shownOptionValue(props.options[index].value).hint
}
</script>

<template>
  <div
    role="group"
    :aria-labelledby="`options-${componentId}-heading`"
    class="space-y-2"
    :data-testid="`options-${componentId}`"
  >
    <p :id="`options-${componentId}-heading`" class="text-sm font-medium leading-none">Options</p>
    <div v-for="(option, index) in options" :key="index" class="flex items-start gap-2">
      <Input
        :model-value="option.label"
        :aria-label="`Option ${index + 1} label`"
        placeholder="Label"
        :disabled="disabled"
        :data-testid="`options-${componentId}-${index}-label`"
        @update:model-value="replace(index, { ...option, label: String($event) })"
      />
      <div class="w-full space-y-1">
        <Input
          :model-value="drafts[index] ?? shownOptionValue(option.value).text"
          :aria-label="`Option ${index + 1} value`"
          placeholder="Value"
          :disabled="disabled"
          :invalid="asText(index)"
          :described-by="note(index) && `options-${componentId}-${index}-note`"
          :data-testid="`options-${componentId}-${index}-value`"
          @update:model-value="setValue(index, String($event))"
        />
        <p
          v-if="note(index)"
          :id="`options-${componentId}-${index}-note`"
          :class="['text-xs', asText(index) ? 'text-destructive' : 'text-muted-foreground']"
          :data-testid="`options-${componentId}-${index}-note`"
        >
          {{ note(index) }}
        </p>
      </div>
      <Button
        size="sm"
        variant="ghost"
        class="px-2 text-destructive"
        :aria-label="`Remove option ${index + 1}`"
        :disabled="disabled"
        :data-testid="`options-${componentId}-${index}-remove`"
        @click="remove(index)"
      >
        <Trash2 class="size-4" aria-hidden="true" />
      </Button>
    </div>
    <Button
      size="sm"
      variant="outline"
      :disabled="disabled"
      :data-testid="`options-${componentId}-add`"
      @click="emit('update', [...options, { label: '', value: '' }])"
    >
      <Plus class="mr-1 size-4" aria-hidden="true" />
      Add option
    </Button>
    <p class="text-xs text-muted-foreground">
      The label is shown and the value is saved, read by the field's value type.
    </p>
  </div>
</template>
