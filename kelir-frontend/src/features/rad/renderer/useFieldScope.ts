import { computed, inject, provide, type ComputedRef, type InjectionKey } from 'vue'

/**
 * The prefix that makes a rendered field's DOM id unique.
 *
 * **JFSS §4.1 makes `id` unique per component *instance*, and a repeater breaks
 * the assumption that follows from it.** A `datagrid` renders one template
 * instance once per row, so a template carrying `id: "line-no"` produces as
 * many elements wanting `jfss-line-no` as there are rows. Duplicate DOM ids are
 * invalid HTML, and the consequence is not cosmetic: `<label for>` binds to the
 * *first* match, so every row's label points at row one's input. Clicking the
 * third row's label focuses the first row's box, and a screen reader announces
 * the same field however many rows there are.
 *
 * So a scope contributes a prefix, and only a repeater's rows open one. Ids stay
 * **deterministic** — `jfss-row-2-line-no` rather than a generated counter —
 * because the browser harness locates by them and a test that has to discover
 * an id cannot assert one.
 *
 * Provided per row rather than threaded as a prop through every component in
 * between: the prefix concerns the two components that care about it, and a
 * prop would put it on every layout container, every display type and every
 * field that never reads it.
 */
const FIELD_SCOPE_KEY: InjectionKey<ComputedRef<string>> = Symbol('jfss-field-scope')

/**
 * Opens a nested scope, appending to whatever scope is already in force.
 *
 * Appending rather than replacing, because a datagrid inside a datagrid row is
 * a shape JFSS permits: the inner row's fields need the outer row's prefix too,
 * or rows one and two of the inner grid collide exactly as before.
 */
export function provideFieldScope(segment: () => string): void {
  const parent = inject(FIELD_SCOPE_KEY, undefined)

  provide(
    FIELD_SCOPE_KEY,
    computed(() => `${parent?.value ?? ''}${segment()}`),
  )
}

/** The prefix in force here — empty at the top level of a form. */
export function useFieldScope(): ComputedRef<string> {
  return inject(
    FIELD_SCOPE_KEY,
    computed(() => ''),
  )
}
