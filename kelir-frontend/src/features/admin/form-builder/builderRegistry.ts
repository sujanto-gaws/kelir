import type { JfssComponent, JfssRole } from '@/types/jfss'

/**
 * **What the form builder offers, and why it offers nothing else** (#688 B1–B4,
 * **D-86** A).
 *
 * **One entry per type the builder has an opinion about.** An offered entry
 * says what the type is for (`role`), what the palette calls it, which group it
 * sits in, what a fresh one looks like, and where its children live. A
 * withheld entry says why it is withheld, so the canvas can say so on the card
 * of a loaded node it will not edit.
 *
 * **The offered set is the renderer's `SUPPORTED`, exactly**
 * ([`registry.ts`](../../rad/renderer/registry.ts)). A type the builder offers
 * and the renderer cannot draw saves, publishes, and renders as the
 * renderer's *unsupported component* placeholder: a form that passes every
 * check and cannot be filled in. A type the renderer draws and the builder
 * withholds is a gap nobody chose. `builderRegistry.spec.ts` fails on either.
 *
 * **Withheld, and why** (**D-86**):
 * - `steps`, `file` and `signature` are declared by the renderer as not drawn
 *   yet (`NOT_YET_RENDERED`);
 * - `repeater` is the reference builder's, and Kelir's repeating type is
 *   `datagrid`;
 * - `spacer` is in JFSS §4.4's examples and not in the renderer's registry.
 */

/** Where a container's children live (JFSS §4.3, §4.3.1). */
export type ChildShape = 'none' | 'components' | 'columns' | 'tabs' | 'rowTemplate'

/** The palette's four groups, which are the four roles. */
export type PaletteGroup = JfssRole

/** A fresh node's identity, generated against the whole tree. */
export interface NodeIdentity {
  id: string
  /** Data nodes only: the payload key, unique across the tree. */
  key: string
}

export interface OfferedEntry {
  type: string
  offered: true
  role: JfssRole
  label: string
  group: PaletteGroup
  children: ChildShape
  /**
   * A fresh node of this type, **valid against the meta-schema on its own**
   * (B4, E3): every required property present, a container with its one
   * shape, a layout node with no `key`.
   */
  create: (identity: NodeIdentity) => JfssComponent
}

export interface WithheldEntry {
  type: string
  offered: false
  role: JfssRole
  label: string
  /** Shown on the card of a loaded node of this type. */
  reason: string
}

export type BuilderEntry = OfferedEntry | WithheldEntry

function data(
  type: string,
  label: string,
  build: (identity: NodeIdentity) => Record<string, unknown>,
  children: ChildShape = 'none',
): OfferedEntry {
  return {
    type,
    offered: true,
    role: 'data',
    label,
    group: 'data',
    children,
    create: (identity) =>
      ({ id: identity.id, role: 'data', type, ...build(identity) }) as JfssComponent,
  }
}

function layout(
  type: string,
  label: string,
  children: ChildShape,
  build: () => Record<string, unknown>,
): OfferedEntry {
  return {
    type,
    offered: true,
    role: 'layout',
    label,
    group: 'layout',
    children,
    create: ({ id }) => ({ id, role: 'layout', type, ...build() }) as JfssComponent,
  }
}

function display(type: string, label: string, content?: string): OfferedEntry {
  return {
    type,
    offered: true,
    role: 'display',
    label,
    group: 'display',
    children: 'none',
    create: ({ id }) =>
      (content === undefined
        ? { id, role: 'display', type }
        : { id, role: 'display', type, content }) as JfssComponent,
  }
}

function withheld(type: string, role: JfssRole, label: string, reason: string): WithheldEntry {
  return { type, offered: false, role, label, reason }
}

const OPTION = [{ label: 'Option 1', value: 'option_1' }]

/** Every type the builder has an opinion about, in palette order within each group. */
export const BUILDER_REGISTRY: Readonly<Record<string, BuilderEntry>> = {
  // role: data
  textfield: data('textfield', 'Text field', ({ key }) => ({
    key,
    label: 'Text field',
    validation: { type: 'string' },
  })),
  textarea: data('textarea', 'Text area', ({ key }) => ({
    key,
    label: 'Text area',
    validation: { type: 'string' },
  })),
  number: data('number', 'Number', ({ key }) => ({
    key,
    label: 'Number',
    validation: { type: 'number' },
  })),
  date: data('date', 'Date', ({ key }) => ({
    key,
    label: 'Date',
    validation: { type: 'string', format: 'date' },
  })),
  select: data('select', 'Select', ({ key }) => ({
    key,
    label: 'Select',
    validation: { type: 'string' },
    options: OPTION.map((option) => ({ ...option })),
  })),
  radio: data('radio', 'Radio', ({ key }) => ({
    key,
    label: 'Radio',
    validation: { type: 'string' },
    options: OPTION.map((option) => ({ ...option })),
  })),
  checkbox: data('checkbox', 'Checkbox', ({ key }) => ({
    key,
    label: 'Checkbox',
    validation: { type: 'boolean' },
  })),
  // Its source is bound in `settings.lookups` by `addComponent`, not here: the
  // binding is the definition's, keyed by this node's `id` (D-23).
  lookup: data('lookup', 'Lookup', ({ key }) => ({
    key,
    label: 'Lookup',
    validation: { type: 'string' },
  })),
  // JFSS §4.3.1: a row template, repeated per row. `sequenceKey`,
  // `defaultItems` and `uniqueBy` are part 2's (#688 §0).
  datagrid: data(
    'datagrid',
    'Data grid',
    ({ key }) => ({ key, label: 'Data grid', validation: { type: 'array' }, components: [] }),
    'rowTemplate',
  ),

  // role: layout. Exactly one child shape each, and never a `key`.
  panel: layout('panel', 'Panel', 'components', () => ({ title: 'Panel', components: [] })),
  fieldset: layout('fieldset', 'Fieldset', 'components', () => ({
    title: 'Fieldset',
    components: [],
  })),
  columns: layout('columns', 'Columns', 'columns', () => ({
    columns: [{ components: [] }, { components: [] }],
  })),
  tabs: layout('tabs', 'Tabs', 'tabs', () => ({ tabs: [{ title: 'Tab 1', components: [] }] })),

  // role: display
  heading: display('heading', 'Heading', 'Heading'),
  paragraph: display('paragraph', 'Paragraph', 'Text'),
  divider: display('divider', 'Divider'),
  alert: display('alert', 'Alert', 'Alert'),

  // role: action. `submit` is the only action part 1 offers (#688 §0).
  button: {
    type: 'button',
    offered: true,
    role: 'action',
    label: 'Button',
    group: 'action',
    children: 'none',
    create: ({ id }) => ({ id, role: 'action', type: 'button', label: 'Submit', action: 'submit' }),
  },

  // Withheld (B3, D-86).
  steps: withheld(
    'steps',
    'layout',
    'Steps',
    'The renderer does not draw a stepped form yet, so the builder does not offer one.',
  ),
  file: withheld(
    'file',
    'data',
    'File',
    'The renderer does not draw file attachments in a form yet, so the builder does not offer one.',
  ),
  signature: withheld(
    'signature',
    'data',
    'Signature',
    'The renderer does not draw signature capture yet, so the builder does not offer one.',
  ),
  repeater: withheld(
    'repeater',
    'data',
    'Repeater',
    "Kelir repeats rows with a data grid; `repeater` is not one of this renderer's types.",
  ),
  spacer: withheld(
    'spacer',
    'display',
    'Spacer',
    "`spacer` is not one of this renderer's types, so the builder does not offer one.",
  ),
} as const

/** The offered entries, in palette order. */
export function offeredEntries(): OfferedEntry[] {
  return Object.values(BUILDER_REGISTRY).filter((entry): entry is OfferedEntry => entry.offered)
}

/** The palette's groups, in the order an author meets them. */
export const PALETTE_GROUPS: { group: PaletteGroup; label: string }[] = [
  { group: 'data', label: 'Fields' },
  { group: 'layout', label: 'Layout' },
  { group: 'display', label: 'Display' },
  { group: 'action', label: 'Actions' },
]

/**
 * The entry for a type, offered or withheld; `undefined` when the builder has
 * never heard of it. Own properties only, so `constructor` is not a type.
 */
export function entryFor(type: string): BuilderEntry | undefined {
  return Object.prototype.hasOwnProperty.call(BUILDER_REGISTRY, type)
    ? BUILDER_REGISTRY[type]
    : undefined
}

/** The entry the builder offers for a type, or `undefined` when it offers none. */
export function offeredEntry(type: string): OfferedEntry | undefined {
  const entry = entryFor(type)

  return entry?.offered ? entry : undefined
}

/** What the palette and the canvas call a type; the type itself when nobody named it. */
export function typeLabel(type: string): string {
  return entryFor(type)?.label ?? type
}
