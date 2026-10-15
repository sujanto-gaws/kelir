<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { isParams } from './propertyEdits'

/**
 * A rule's `params`, as raw JSON (**D-110**; #688 F2, and the test-engineer
 * campaign's defect 2).
 *
 * **The text is the author's while they type.** It is held here, so the box is
 * never rewritten under the cursor: emptying it does not snap it to `{}`, and a
 * value that parses is not re-indented. It is written only when it parses to
 * what the meta-schema's `advancedRule.params` allows — **a plain object**;
 * an empty box is no parameters, `{}`. Anything else — half-typed JSON, an
 * array, `null`, a number — is marked not valid and not written, so the rule
 * keeps its last good parameters.
 *
 * **The text follows the model only when the model changes from outside**: a
 * change of rule seeding new parameters, a removal shifting the rows. The
 * model's echo of what this box wrote is recognised by value and left alone.
 * On blur, text that was not written gives way to the parameters the rule
 * holds, so what the box shows is what is saved.
 *
 * **It checks the shape alone.** A required parameter that is missing, empty
 * or of the wrong type is valid JSON here; the section flags it under this box
 * (#722).
 */
const props = defineProps<{
  modelValue: Record<string, unknown> | undefined
  testId: string
  label: string
  disabled: boolean
  /** A server message about these parameters (S10.3), shown by the section. */
  invalidByServer: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [params: Record<string, unknown>] }>()

function shown(value: unknown): string {
  return value === undefined ? '' : JSON.stringify(value, null, 2)
}

/** What the text means: the parameters it writes, or `null` when it writes none. */
function meaning(text: string): Record<string, unknown> | null {
  const trimmed = text.trim()

  if (trimmed === '') {
    return {}
  }

  try {
    const value: unknown = JSON.parse(trimmed)

    return isParams(value) ? value : null
  } catch {
    return null
  }
}

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

const text = ref(shown(props.modelValue))
const refused = computed(() => meaning(text.value) === null)

watch(
  () => props.modelValue,
  (value) => {
    // Its own echo, or an outside value equal to what is typed: leave the text.
    if (!sameJson(meaning(text.value), value ?? {})) {
      text.value = shown(value)
    }
  },
)

function onInput(next: string): void {
  text.value = next

  const value = meaning(next)

  if (value !== null && !sameJson(value, props.modelValue ?? {})) {
    emit('update:modelValue', value)
  }
}

function onBlur(): void {
  if (refused.value) {
    text.value = shown(props.modelValue)
  }
}
</script>

<template>
  <textarea
    rows="2"
    class="w-full rounded-md border bg-background p-2 font-mono text-xs sm:col-span-3"
    :class="refused ? 'border-destructive' : ''"
    placeholder='{"pattern": "^[A-Z]+$"}'
    :aria-label="label"
    :aria-invalid="refused || invalidByServer"
    :aria-describedby="refused ? `${testId}-refused` : undefined"
    :value="text"
    :disabled="disabled"
    :data-testid="testId"
    @input="onInput(($event.target as HTMLTextAreaElement).value)"
    @blur="onBlur"
  ></textarea>
  <p
    v-if="refused"
    :id="`${testId}-refused`"
    class="text-xs text-destructive sm:col-span-3"
    :data-testid="`${testId}-refused`"
  >
    Not saved: parameters are a JSON object, such as <code>{"target": "other"}</code>. The rule
    keeps its last valid ones.
  </p>
</template>
