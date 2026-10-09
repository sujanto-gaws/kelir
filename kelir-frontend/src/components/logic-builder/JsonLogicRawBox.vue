<script setup lang="ts">
import { ref, useId } from 'vue'

import { Badge } from '@/components/ui/badge'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'

/**
 * The raw-JSON box, marked advanced (#686 C11, C12).
 *
 * It is the box `FormComponentEditor.vue` has today, kept honest in the same
 * two ways. **Half-typed JSON is the normal state of a text box**, so text that
 * does not parse leaves the held value alone and reports itself as unparsed.
 * **A value that parses is emitted as parsed** — never re-indented, re-sorted
 * or normalised — and the text the user typed stays exactly as they typed it.
 *
 * The text is the box's own from the moment it mounts. Its holder remounts it
 * (by `key`) when the value changes from outside, so there is no watcher here
 * that could overwrite what somebody is in the middle of typing.
 */

const props = withDefaults(
  defineProps<{
    modelValue?: unknown
    label: string
    /** Whether an empty box means "no expression". An operand cannot be empty. */
    allowEmpty?: boolean
    disabled?: boolean
  }>(),
  { modelValue: undefined, allowEmpty: false, disabled: false },
)

const emit = defineEmits<{
  parsed: [value: unknown]
  unparsed: [text: string]
}>()

const id = useId()
const hintId = `${id}-hint`

/**
 * The value as text, or `undefined` when it cannot be written out: a value
 * nested deeper than `JSON.stringify` can recurse (past `MAX_VISUAL_DEPTH`, a
 * subtree is held whole). It is still held, and emitted, exactly as it is.
 */
function display(value: unknown): string | undefined {
  if (value === undefined) {
    return ''
  }

  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return undefined
  }
}

const shown = display(props.modelValue)
const unshowable = shown === undefined
const text = ref(shown ?? '')
const invalid = ref(false)

function onInput(next: string): void {
  text.value = next

  const trimmed = next.trim()

  if (trimmed === '' && props.allowEmpty) {
    invalid.value = false
    emit('parsed', undefined)

    return
  }

  try {
    const value: unknown = JSON.parse(trimmed)

    invalid.value = false
    emit('parsed', value)
  } catch {
    invalid.value = true
    emit('unparsed', next)
  }
}
</script>

<template>
  <div class="space-y-1">
    <div class="flex items-center gap-2">
      <Label :for="id">{{ label }}</Label>
      <Badge variant="outline">Advanced</Badge>
    </div>
    <Textarea
      :id="id"
      :model-value="text"
      :rows="4"
      class="font-mono text-xs"
      :invalid="invalid"
      :disabled="disabled || unshowable"
      :described-by="hintId"
      @update:model-value="onInput"
    />
    <p :id="hintId" class="text-xs" :class="invalid ? 'text-destructive' : 'text-muted-foreground'">
      {{
        unshowable
          ? 'Nested too deeply to show as text. It is kept exactly as it is.'
          : invalid
            ? 'Not valid JSON yet. The expression keeps its last valid value until it is.'
            : 'Kept exactly as written. The server checks it when the definition is saved.'
      }}
    </p>
  </div>
</template>
