import { inject, provide, type InjectionKey } from 'vue'

/**
 * The form's lookup bindings, made reachable from the field that needs them.
 *
 * **A lookup field does not carry its own source.** JFSS is frozen at v2.0.1
 * and the meta-schema closes a component with `unevaluatedProperties: false`,
 * so decision **D-23** put the binding in `settings.lookups` — a map from
 * component `id` to master-data source key — which is the one object the
 * specification declares open.
 *
 * That leaves the binding several levels away from the component it binds, and
 * provide/inject rather than a prop is the honest way to close the distance:
 * the alternative threads a `lookups` prop through every layout container and
 * every renderer level so that one field type can read it, which makes every
 * component in the tree carry a property only one of them uses.
 *
 * **The cost D-23 recorded applies here too** — a reader of a definition has to
 * look in two places to see what a lookup field points at, and so does a reader
 * of this code.
 */
export type LookupBindings = Readonly<Record<string, string>>

const LOOKUP_BINDINGS_KEY: InjectionKey<LookupBindings> = Symbol('jfss-lookup-bindings')

/** Called once by the form root, with `definition.settings?.lookups`. */
export function provideLookupBindings(bindings: LookupBindings | undefined): void {
  provide(LOOKUP_BINDINGS_KEY, bindings ?? {})
}

/**
 * The source a lookup component names, or `undefined`.
 *
 * `undefined` is not a case the renderer is expected to hit: the backend
 * refuses a definition whose lookup field has no binding, and one whose binding
 * names a source nobody serves — five refusals, all at save
 * (`domain/jfss.rs`). It is handled anyway because the alternative to handling
 * it is a chooser that opens empty, which is indistinguishable from master data
 * that happens to hold nothing. That is the exact failure the backend's own
 * comment gives as the reason those checks live at save time.
 */
export function useLookupSource(componentId: string): string | undefined {
  return inject(LOOKUP_BINDINGS_KEY, {})[componentId]
}
