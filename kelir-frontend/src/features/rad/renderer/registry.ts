import type { Component } from 'vue'

import ButtonAction from '../actions/ButtonAction.vue'
import AlertDisplay from '../display/AlertDisplay.vue'
import DividerDisplay from '../display/DividerDisplay.vue'
import TextDisplay from '../display/TextDisplay.vue'
import CheckboxField from '../fields/CheckboxField.vue'
import DataGridField from '../fields/DataGridField.vue'
import DateField from '../fields/DateField.vue'
import LookupField from '../fields/LookupField.vue'
import NumberField from '../fields/NumberField.vue'
import RadioField from '../fields/RadioField.vue'
import SelectField from '../fields/SelectField.vue'
import TextField from '../fields/TextField.vue'
import TextareaField from '../fields/TextareaField.vue'
import ColumnsLayout from '../layout/ColumnsLayout.vue'
import FieldsetLayout from '../layout/FieldsetLayout.vue'
import PanelLayout from '../layout/PanelLayout.vue'
import TabsLayout from '../layout/TabsLayout.vue'
import type { JfssRole } from '@/types/jfss'

/**
 * **Kelir's component vocabulary. This file is the whole of it** (#162 AC1).
 *
 * JFSS §4.4 says so in as many words: *"`type` is an open vocabulary defined by
 * each implementation's component registry"*. The meta-schema enumerates no
 * component types and cannot — it validates `type` as `string` — so nothing
 * upstream decides which types exist. **This does.** The backend's own
 * validator states the same thing from the other side, calling its lookup rule
 * "a registry-level constraint… This is Kelir's registry" for the server half.
 *
 * That makes AC1's *"the list in one place rather than discovered per
 * component"* enforceable rather than aspirational: a type is supported here,
 * declared unsupported here, or it is neither — and
 * [`registry.spec.ts`](./registry.spec.ts) fails when a definition anywhere in
 * the repository uses a type that is neither.
 *
 * **Adding a type is two lines and a component.** Add the entry, add the file.
 * Nothing else in the renderer changes, which is the property that makes the
 * dispatch in [`JfssRenderer.vue`](./JfssRenderer.vue) worth having.
 */

/** What the renderer needs to know about a type beyond which component draws it. */
export interface RegistryEntry {
  /**
   * The role this type belongs to.
   *
   * Recorded because the renderer passes different props to each role, and a
   * definition pairing `role: "data"` with `type: "panel"` would otherwise be
   * handed a panel and told to bind a value to it. The backend accepts such a
   * document — the meta-schema constrains properties by role, not `type` by
   * role — so the mismatch is reachable and is rendered as unsupported.
   */
  role: JfssRole
  component: Component
}

/** Every component type Kelir renders. */
export const SUPPORTED: Readonly<Record<string, RegistryEntry>> = {
  // role: data
  textfield: { role: 'data', component: TextField },
  textarea: { role: 'data', component: TextareaField },
  number: { role: 'data', component: NumberField },
  select: { role: 'data', component: SelectField },
  radio: { role: 'data', component: RadioField },
  checkbox: { role: 'data', component: CheckboxField },
  date: { role: 'data', component: DateField },
  lookup: { role: 'data', component: LookupField },
  datagrid: { role: 'data', component: DataGridField },

  // role: layout — the chrome only; the three child-container shapes JFSS
  // §4.3.1 defines are traversed by whichever container owns them.
  panel: { role: 'layout', component: PanelLayout },
  fieldset: { role: 'layout', component: FieldsetLayout },
  columns: { role: 'layout', component: ColumnsLayout },
  tabs: { role: 'layout', component: TabsLayout },

  // role: display
  heading: { role: 'display', component: TextDisplay },
  paragraph: { role: 'display', component: TextDisplay },
  divider: { role: 'display', component: DividerDisplay },
  alert: { role: 'display', component: AlertDisplay },

  // role: action
  button: { role: 'action', component: ButtonAction },
} as const

/**
 * Types Kelir knows about and does not render yet, each with its reason.
 *
 * **A declared gap, so that an undeclared one is a finding.** Without this
 * list, a type nobody implemented and a type nobody has heard of look identical
 * on screen and identical in the test — which is the "discovered per component"
 * that AC1 names as the failure.
 *
 * A reason here is shown to the person looking at the form, so it says what
 * they can do about it rather than which sprint it is in.
 */
export const NOT_YET_RENDERED: Readonly<Record<string, string>> = {
  steps: 'A stepped form is not available yet; ask for this form to use tabs instead.',
  file: 'File attachments are not part of this form surface yet.',
  signature: 'Signature capture is not available yet.',
} as const

/** Every type this registry has an opinion about, supported or not. */
export function declaredTypes(): string[] {
  return [...Object.keys(SUPPORTED), ...Object.keys(NOT_YET_RENDERED)].sort()
}

/**
 * The entry for a type, or `undefined` when the registry does not know it.
 *
 * Deliberately not falling back to a "text field" or similar: a lookup silently
 * rendered as a free-text box would collect an id nobody can resolve, and would
 * pass every test that only checks the form rendered.
 */
export function resolve(type: string): RegistryEntry | undefined {
  return SUPPORTED[type]
}

/** Why a type is not rendered, when the registry declares it as expected. */
export function declaredGap(type: string): string | undefined {
  return NOT_YET_RENDERED[type]
}
