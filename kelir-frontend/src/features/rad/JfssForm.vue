<script setup lang="ts">
import { computed, reactive, toRef, watch } from 'vue'

import JfssRenderer from './renderer/JfssRenderer.vue'
import {
  createFormEvaluation,
  provideFormEvaluation,
  provideValueScope,
} from './renderer/useFormEvaluation'
import { provideLookupBindings } from './renderer/useLookupBindings'
import { Alert } from '@/components/ui/alert'
import type { ValidationDetail } from '@/types/api'
import { dataComponents, type JfssDefinition } from '@/types/jfss'

const props = defineProps<{
  definition: JfssDefinition
  /** An existing payload, for a document being re-opened. */
  initialValues?: Record<string, unknown>
  /**
   * What the server said about the last submission (JFSS S10.3, #164).
   *
   * A prop rather than a method the page calls, so the messages a field shows
   * are a function of what the page holds — a form whose errors were pushed
   * into it can disagree with the page about whether a submission failed, and
   * only one of the two is on screen.
   *
   * Every one of these is addressed by dot-notation `path`, which is why the
   * envelope names the field `path` and not `key`: `line_items.2.quantity` is
   * not a key.
   */
  serverViolations?: ValidationDetail[]
}>()

const emit = defineEmits<{
  (e: 'action', action: string, values: Record<string, unknown>): void
  (e: 'change', values: Record<string, unknown>): void
}>()

/**
 * A definition, rendered as a form that evaluates its own rules (#162, #163).
 *
 * **The payload lives here and the renderer is stateless.** One reactive object
 * that every field reads its value out of and writes its value into — which is
 * what made #163 possible without restructuring anything, because a derived
 * field is a value in this object that something else computes.
 *
 * **Submitting is the page's** (#164). What is here is whether a submit may
 * happen at all — validation's question — and where the server's answer is
 * shown when it turns out that it may not.
 */
const values = reactive<Record<string, unknown>>({})

/**
 * The rules, running over the payload above.
 *
 * Created here rather than in the page, because the payload is here: an
 * evaluation that had to be handed a payload from outside would be an
 * evaluation two components could disagree about.
 */
const evaluation = createFormEvaluation(toRef(props, 'definition'), values)

provideFormEvaluation(evaluation)

// The scope a top-level `key` addresses is the payload itself. A datagrid's
// rows open their own (see `DataGridRow.vue`), which is what lets one field
// component serve both without knowing which it is in.
provideValueScope(() => values)

/**
 * JFSS §4.2.3, as the decision procedure it is written as rather than a list.
 *
 * **Case A — `calculate` absent.** Existing payload first, `defaultValue`
 * second. An edit of a stored document must never have its values replaced by
 * the definition's defaults.
 *
 * **Cases B and C — `calculate` present.** Neither is resolved here, and the
 * `defaultValue` branch is skipped for both. Case B's computed value wins over
 * every other source, so seeding one would be seeding something guaranteed to
 * be overwritten; Case C ranks `calculate` **above** `defaultValue`, so seeding
 * the default first would make the expression unreachable — the field would be
 * non-null before anything evaluated it. Both are
 * [`useFormEvaluation`](./renderer/useFormEvaluation.ts)'s, which applies the
 * default itself as Case C's third priority.
 */
function resolveInitialValues(): void {
  for (const key of Object.keys(values)) {
    delete values[key]
  }

  for (const field of dataComponents(props.definition.components)) {
    const computedField = field.calculate !== undefined

    if (props.initialValues && field.key in props.initialValues) {
      values[field.key] = props.initialValues[field.key]
    } else if (field.defaultValue !== undefined && !computedField) {
      values[field.key] = field.defaultValue
    } else {
      // Present and empty rather than absent: a key that appears the moment
      // somebody types makes the payload shape depend on what was touched, and
      // S10.1 requires every data key on submission.
      values[field.key] = field.validation.type === 'array' ? [] : null
    }
  }
}

// `immediate` so the first render already has the payload; re-running on a new
// definition matters for a page that switches forms without unmounting.
watch(() => props.definition, resolveInitialValues, { immediate: true, deep: false })

/**
 * The server's answer to the last submission, handed to the evaluation so a
 * field can find the message that is about it.
 *
 * `immediate` because a page may mount already holding one — a submission
 * refused, the route re-entered — and a message that appeared only on the
 * *second* answer would be a message nobody saw the first time.
 */
watch(
  () => props.serverViolations,
  (details) => evaluation.reportServerViolations(details ?? []),
  { immediate: true },
)

// Lookup bindings are read from `settings` (**D-23**) and reach the fields that
// need them by injection — see `useLookupBindings.ts` for why not a prop.
provideLookupBindings(props.definition.settings?.lookups)

function updateField(key: string, value: unknown): void {
  values[key] = value
}

/**
 * `change`, emitted after the rules have run rather than as the value is set.
 *
 * `flush: 'post'` is the whole of it. The calculation pass is a `pre` watcher,
 * so emitting inside `updateField` would hand a listener a payload whose
 * derived fields are one tick behind — a `grand_total` from before the row that
 * just changed, which is precisely the number a listener would be watching for.
 * Emitting after the flush also means a value the rules computed reaches
 * listeners at all; a value that only ever changed because something else
 * computed it never passes through `updateField`.
 */
watch(values, () => emit('change', { ...values }), { deep: true, flush: 'post' })

/**
 * What a button means, and the one part of it that is this issue's.
 *
 * **`submit` is gated and every other action passes through.** The page decides
 * what submitting *does* (#164); whether the payload is fit to submit is what
 * the definition's rules answer, and answering it here is what makes #163 AC1's
 * per-field messages appear where a person can act on them. A `reset` or a
 * `navigate` is not gated — refusing to let somebody leave a form because a
 * field they never reached is empty would be a worse form than one with no
 * validation at all.
 *
 * The messages stay hidden until this runs. Construction plan §5.6 argues that
 * for calculations and the argument is the same here: a form that greets its
 * user with red boxes it drew itself has told them nothing.
 */
function onAction(action: string): void {
  if (action === 'submit' && !evaluation.reveal()) {
    return
  }

  emit('action', action, { ...values })
}

/** Surfaced as top-level refs so the template reads them without `.value`. */
const defect = evaluation.defect

/** Registry rules the definition asks for that this side does not decide. */
const undecided = computed(() =>
  evaluation.undecided.value.filter((rule) => rule.scope !== 'server'),
)
</script>

<template>
  <!-- `novalidate`: the browser's own bubbles would compete with the messages
       the definition supplies, and they would say something different — the
       browser knows `required`, and knows nothing about `matchesField`. -->
  <form class="space-y-6" novalidate @submit.prevent>
    <!-- A defect in the definition, not in what anybody typed (#163 AC3). It is
         shown rather than thrown so the rest of the form still renders, and it
         makes the form invalid so nothing is reported as checked that was not. -->
    <Alert v-if="defect" variant="destructive" data-testid="form-defect">
      <p class="font-medium">This form carries a rule Kelir cannot apply.</p>
      <p class="mt-1">{{ defect }}</p>
    </Alert>

    <!-- A rule the registry defines and this side does not decide. Named, for
         the reason `UnsupportedComponent` is: a check that quietly did not run
         is indistinguishable from a check that passed. `server`-scoped rules
         are filtered out above — those are not a gap, they are §3.3 working. -->
    <Alert v-for="rule in undecided" :key="rule.rule" data-testid="form-undecided">
      <p class="font-medium">
        <code>{{ rule.rule }}</code> is checked when this form is submitted, not as you type.
      </p>
      <p class="mt-1">{{ rule.reason }}.</p>
    </Alert>

    <JfssRenderer
      v-for="component in definition.components"
      :key="component.id"
      :component="component"
      :values="values"
      @update:field="updateField"
      @action="onAction"
    />
  </form>
</template>
