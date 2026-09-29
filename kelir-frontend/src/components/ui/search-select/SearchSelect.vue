<script setup lang="ts" generic="Row, Model extends string | string[] = string">
import { computed, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'

import { toApiError } from '@/api/client'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { SEARCH_SELECT_PAGE_SIZE, type KnownOption, type SearchSource } from './types'

/**
 * A chooser over a server list that can reach every row (#525).
 *
 * # Why it searches instead of listing
 *
 * Every chooser before this one read a single page of a hundred rows and, for
 * some, filtered it by status after it arrived. Whatever sorted past the
 * hundredth row could not be picked, and a status filter applied to a page
 * showed fewer than a page with nothing saying so. This asks the server for
 * the first page of what matches the search — status included — and says when
 * there is more than it shows.
 *
 * # What stays on the screen
 *
 * A stored or chosen value that the current results do not contain is still
 * shown, by its label: a field holding a perfectly good id must not read as
 * "nothing chosen" because somebody typed a different search. The label comes
 * from a row seen earlier, from `known`, or from `source.resolve`, in that
 * order.
 *
 * # Presentations
 *
 * `select` (the default) is a native select, which the platform already makes
 * keyboard-accessible. `list` is a radio group, for a chooser whose options
 * need a sentence each. `multiple` is a checkbox group. All three are real
 * inputs; nothing here is a styled div pretending to be one.
 */
const props = withDefaults(
  defineProps<{
    /** The control's id. The search box is `${id}-search`. */
    id: string
    label: string
    source: SearchSource<Row>
    known?: KnownOption[]
    variant?: 'select' | 'list'
    multiple?: boolean
    /** The blank choice of the `select` presentation, which is how a choice is cleared. */
    placeholder?: string
    searchPlaceholder?: string
    disabled?: boolean
    invalid?: boolean
    describedBy?: string
    /** `data-testid` of the control; the search box and status line derive theirs from it. */
    testId?: string
    /** `data-testid` of one row's choice, in the `list` and `multiple` presentations. */
    optionTestId?: (row: Row) => string
    debounceMs?: number
  }>(),
  {
    known: () => [],
    variant: 'select',
    multiple: false,
    placeholder: undefined,
    searchPlaceholder: 'Search…',
    disabled: false,
    invalid: false,
    describedBy: undefined,
    testId: undefined,
    optionTestId: undefined,
    debounceMs: 250,
  },
)

const model = defineModel<Model>({ required: true })

const emit = defineEmits<{
  /** The row behind a single choice the person just made, or undefined when cleared. */
  pick: [row: Row | undefined]
}>()

defineSlots<{
  /** One choice in the `list` presentation. `row` is absent for a value outside the results. */
  option?(props: { row: Row | undefined; label: string; disabled: boolean }): unknown
  /** Said when nothing can be chosen and nothing was searched for. */
  empty?(props: Record<string, never>): unknown
}>()

interface Choice {
  value: string
  label: string
  disabled: boolean
  row: Row | undefined
}

/** The label a value outside the results falls back to when nothing names it. */
const UNNAMED = 'The current choice, outside these results'

const search = ref('')
const rows = shallowRef<Row[]>([])
const total = ref(0)
const loading = ref(false)
const problem = ref('')
const loadedOnce = ref(false)

/** Every label seen for a value, so a choice keeps its name after the search moves on. */
const seen = reactive(new Map<string, string>())
const resolving = new Set<string>()

const searchId = computed(() => `${props.id}-search`)
const statusId = computed(() => `${props.id}-status`)
const describedBy = computed(() =>
  [props.describedBy, statusId.value].filter((part) => part).join(' '),
)

const selected = computed<string[]>(() => {
  const value = model.value as string | string[]

  if (Array.isArray(value)) {
    return value
  }

  return value ? [value] : []
})

const visible = computed(() => rows.value.filter((row) => !props.source.exclude?.(row)))

function labelOf(value: string): string {
  return seen.get(value) ?? props.known.find((option) => option.value === value)?.label ?? UNNAMED
}

const choices = computed<Choice[]>(() => {
  const found = visible.value.map((row) => ({
    value: props.source.value(row),
    label: props.source.label(row),
    disabled: props.source.disabled?.(row) ?? false,
    row,
  }))
  const present = new Set(found.map((choice) => choice.value))
  const outside = selected.value
    .filter((value) => !present.has(value))
    .map((value) => ({ value, label: labelOf(value), disabled: false, row: undefined }))

  // First, so a choice made earlier is where the eye lands rather than
  // somewhere below a hundred others.
  return [...outside, ...found]
})

const status = computed(() => {
  if (loading.value) {
    return loadedOnce.value ? 'Searching…' : 'Loading…'
  }

  if (problem.value) {
    return ''
  }

  if (total.value > rows.value.length) {
    return `Showing ${rows.value.length} of ${total.value} — refine your search to find the rest.`
  }

  if (visible.value.length === 0 && search.value.trim() !== '') {
    return `Nothing matches “${search.value.trim()}”.`
  }

  return ''
})

let latest = 0
let pending: ReturnType<typeof setTimeout> | undefined

/**
 * Asks for the first page of what matches.
 *
 * **Only the latest request's answer is kept.** Two searches in flight can
 * answer out of order, and the slower, older one must not overwrite what the
 * person is now looking at.
 */
async function load(): Promise<void> {
  clearTimeout(pending)
  pending = undefined

  const ticket = ++latest

  loading.value = true
  problem.value = ''

  try {
    const page = await props.source.fetch({
      search: search.value.trim(),
      pageSize: SEARCH_SELECT_PAGE_SIZE,
    })

    if (ticket !== latest) {
      return
    }

    for (const row of page.items) {
      seen.set(props.source.value(row), props.source.label(row))
    }

    rows.value = page.items
    total.value = page.meta.total
  } catch (error) {
    if (ticket !== latest) {
      return
    }

    // Said here and not rethrown: one chooser that cannot reach its list must
    // not take down a form whose other fields are fine.
    rows.value = []
    total.value = 0
    problem.value = toApiError(error).message
  } finally {
    if (ticket === latest) {
      loading.value = false
      loadedOnce.value = true
    }
  }
}

/**
 * Names a stored value no search has returned, when the source can: once while
 * a read is in flight or has succeeded, and again at the next need after one
 * failed, so a passing outage does not leave the fallback label for good.
 */
function resolveUnnamed(): void {
  const resolve = props.source.resolve

  if (!resolve) {
    return
  }

  for (const value of selected.value) {
    const named = seen.has(value) || props.known.some((option) => option.value === value)

    if (named || resolving.has(value)) {
      continue
    }

    resolving.add(value)
    resolve(value)
      .then((row) => {
        seen.set(value, props.source.label(row))
      })
      .catch(() => {
        // Forgotten, so the next change of selection asks again.
        resolving.delete(value)
        // Until then the fallback label stands. The value is still chosen, and saving
        // sends it back unchanged; the server decides whether it still binds.
      })
  }
}

watch(search, () => {
  clearTimeout(pending)
  pending = setTimeout(() => void load(), props.debounceMs)
})

// After a load too: a value the first page names needs no extra read.
watch([selected, loadedOnce], () => {
  if (loadedOnce.value) {
    resolveUnnamed()
  }
})

onMounted(load)

onBeforeUnmount(() => {
  clearTimeout(pending)
  // An answer arriving after the chooser is gone is nobody's to show.
  latest += 1
})

function choose(value: string): void {
  model.value = value as Model
  emit(
    'pick',
    visible.value.find((row) => props.source.value(row) === value),
  )
}

function toggle(value: string, on: boolean): void {
  const next = on
    ? [...new Set([...selected.value, value])]
    : selected.value.filter((candidate) => candidate !== value)

  model.value = next as Model
}

const single = computed({
  get: () => selected.value[0] ?? '',
  set: (value: string) => choose(value),
})

const selectOptions = computed(() =>
  choices.value.map((choice) => ({
    value: choice.value,
    label: choice.label,
    disabled: choice.disabled,
  })),
)

function testIdOf(choice: Choice): string | undefined {
  return choice.row !== undefined ? props.optionTestId?.(choice.row) : undefined
}

defineExpose({ reload: load })
</script>

<template>
  <div class="space-y-2">
    <Label v-if="!multiple && variant === 'select'" :for="id">{{ label }}</Label>

    <Input
      :id="searchId"
      v-model="search"
      type="search"
      autocomplete="off"
      :placeholder="searchPlaceholder"
      :disabled="disabled"
      :aria-label="`Search ${label}`"
      :aria-controls="id"
      :data-testid="testId && `${testId}-search`"
      @keydown.enter.prevent="load()"
    />

    <Select
      v-if="!multiple && variant === 'select'"
      :id="id"
      v-model="single"
      :options="selectOptions"
      :placeholder="placeholder"
      :disabled="disabled"
      :invalid="invalid"
      :described-by="describedBy"
      :data-testid="testId"
    />

    <fieldset
      v-else
      :id="id"
      class="space-y-2"
      :aria-describedby="describedBy"
      :aria-invalid="invalid"
      :data-testid="testId"
    >
      <legend class="text-sm font-medium leading-none">{{ label }}</legend>

      <div
        v-if="choices.length > 0"
        class="max-h-64 space-y-2 overflow-y-auto rounded-md border border-border p-3"
      >
        <template v-if="multiple">
          <div
            v-for="choice in choices"
            :key="choice.value"
            class="flex items-center gap-2"
            :data-testid="testIdOf(choice)"
          >
            <Checkbox
              :id="`${id}-${choice.value}`"
              :model-value="selected.includes(choice.value)"
              :disabled="disabled || choice.disabled"
              @update:model-value="toggle(choice.value, $event)"
            />
            <Label :for="`${id}-${choice.value}`" class="font-normal">{{ choice.label }}</Label>
          </div>
        </template>

        <template v-else>
          <label
            v-for="choice in choices"
            :key="choice.value"
            class="flex cursor-pointer items-start gap-3 rounded-md border border-border p-3"
            :data-testid="testIdOf(choice)"
          >
            <input
              type="radio"
              class="mt-1"
              :name="`${id}-choice`"
              :value="choice.value"
              :checked="single === choice.value"
              :disabled="disabled || choice.disabled"
              @change="choose(choice.value)"
            />
            <span>
              <slot
                name="option"
                :row="choice.row"
                :label="choice.label"
                :disabled="choice.disabled"
              >
                {{ choice.label }}
              </slot>
            </span>
          </label>
        </template>
      </div>
    </fieldset>

    <p
      v-if="problem"
      class="text-sm text-destructive"
      role="alert"
      :data-testid="testId && `${testId}-error`"
    >
      These choices could not be loaded: {{ problem }}
    </p>

    <p
      :id="statusId"
      class="text-xs text-muted-foreground"
      aria-live="polite"
      :data-testid="testId && `${testId}-status`"
    >
      <template v-if="status">{{ status }}</template>
      <slot
        v-else-if="!problem && loadedOnce && choices.length === 0 && search.trim() === ''"
        name="empty"
      >
        There is nothing to choose from yet.
      </slot>
    </p>
  </div>
</template>
