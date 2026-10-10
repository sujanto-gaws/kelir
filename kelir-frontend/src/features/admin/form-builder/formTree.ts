import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

import { entryFor, offeredEntry, typeLabel, type NodeIdentity } from './builderRegistry'

/**
 * **The form builder's tree, as pure functions** (#688 C, D, E6).
 *
 * Every structural change the canvas makes is one of the functions below, and
 * **the pointer and the keyboard call the same one** (D5): a drop calls
 * {@link moveNode}, and so do *Move up*, *Move down* and *Move to…*. Each takes
 * a definition and returns a new one, or `null` when it refuses, so **a refused
 * drop leaves the tree as it was** (C1) rather than half-changed.
 *
 * # Addresses
 *
 * A node is addressed by its **path**, in the S10.3 dot-notation the server
 * uses, without the server's `definition.` prefix: `components.0`,
 * `components.0.columns.1.components.2`. A **list** is the path of the array:
 * `components` (the root), `components.0.components` (a panel's children, or a
 * data grid's row template), `components.0.columns.1.components` (a slot).
 * Paths, not ids, because a loaded definition may carry a duplicate `id` and
 * the canvas still has to draw it. Selection and focus follow ids, which a
 * path does not survive a move to.
 *
 * # Nesting (JFSS 2.0.1 §3, §4.3; builder policy where marked)
 *
 * | Parent | Children |
 * |---|---|
 * | root, `panel`, `fieldset` | any role |
 * | `columns`, `tabs` | any role, per slot |
 * | a `datagrid` row template | data and display only (**builder policy, not JFSS**) |
 * | display, action, other data | none |
 *
 * A loaded node that breaks this, or that the builder does not offer, is drawn
 * as an *unsupported* card and kept exactly as loaded (B5): it can be moved and
 * removed, and nothing is drawn or edited inside it.
 */

/** What kind of list a list path addresses. */
export type ListKind = 'root' | 'components' | 'column' | 'tab' | 'rowTemplate'

/** A resolved list: its kind, its owner, and the array (absent for an empty row template). */
export interface ListInfo {
  kind: ListKind
  /** The node that owns it; `null` for the root. */
  owner: JfssComponent | null
  ownerPath: string | null
  /** The slot index, for a column or tab. */
  slot: number | null
  items: JfssComponent[]
}

/** A refusal, with the reason a person reads. */
export type Verdict = { ok: true } | { ok: false; reason: string }

const ROOT = 'components'

// --- Reading -----------------------------------------------------------------

/** A JSON copy: definitions are JSON, and this also reads through a reactive proxy. */
function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

function isIndex(token: string | undefined): token is string {
  return token !== undefined && /^\d+$/.test(token)
}

function slotsOf(node: JfssComponent, key: 'columns' | 'tabs'): unknown[] | undefined {
  const slots = (node as unknown as Record<string, unknown>)[key]

  return Array.isArray(slots) ? slots : undefined
}

function childrenOf(node: JfssComponent): JfssComponent[] | undefined {
  const children = (node as unknown as Record<string, unknown>).components

  return Array.isArray(children) ? (children as JfssComponent[]) : undefined
}

/**
 * Why the canvas will not draw or edit this node, or `null` when it will.
 *
 * **A node is drawn when the builder offers its type, its role agrees, and it
 * has exactly the child shape its type has.** A panel loaded with `tabs`, or a
 * layout with two shapes, is valid JFSS or not, but it is not something this
 * builder made or can edit without guessing; it is kept as it is.
 */
export function unsupportedReason(node: JfssComponent): string | null {
  const entry = entryFor(node.type)

  if (!entry) {
    return `"${node.type}" is not a component type this builder knows.`
  }

  if (!entry.offered) {
    return entry.reason
  }

  if (entry.role !== node.role) {
    return `"${node.type}" is a ${entry.role} type, but this node's role is ${node.role}.`
  }

  const record = node as unknown as Record<string, unknown>
  const present = (['components', 'columns', 'tabs'] as const).filter((key) => key in record)

  switch (entry.children) {
    case 'components':
    case 'columns':
    case 'tabs': {
      if (present.length !== 1 || present[0] !== entry.children) {
        return `A ${entry.label.toLowerCase()} holds its children in \`${entry.children}\` alone.`
      }

      if (entry.children === 'components') {
        return Array.isArray(record.components) ? null : 'Its `components` is not a list.'
      }

      const slots = slotsOf(node, entry.children)
      const wellFormed =
        slots !== undefined &&
        slots.every(
          (slot) =>
            typeof slot === 'object' &&
            slot !== null &&
            Array.isArray((slot as Record<string, unknown>).components) &&
            (entry.children === 'columns' ||
              typeof (slot as Record<string, unknown>).title === 'string'),
        )

      return wellFormed ? null : `Its \`${entry.children}\` slots are not all well formed.`
    }
    case 'rowTemplate':
      if (present.some((key) => key !== 'components')) {
        return 'A data grid holds its row template in `components` alone.'
      }

      return record.components === undefined || Array.isArray(record.components)
        ? null
        : 'Its row template is not a list.'
    case 'none':
      return present.length === 0 ? null : `A ${entry.label.toLowerCase()} holds no children.`
  }
}

/** Why a node may not sit in a list of this kind (builder policy), or `null`. */
export function placementReason(kind: ListKind, node: JfssComponent): string | null {
  if (kind === 'rowTemplate' && node.role !== 'data' && node.role !== 'display') {
    return 'A row template holds data and display components only.'
  }

  return null
}

/** Why the canvas draws this node as an unsupported card where it sits, or `null`. */
export function cardReason(kind: ListKind, node: JfssComponent): string | null {
  return unsupportedReason(node) ?? placementReason(kind, node)
}

/**
 * The list a list path addresses, **only when the canvas draws it**.
 *
 * A path into an unsupported node, into a shape its type does not have, or
 * into a non-container is not a list here, and every mutation below refuses
 * it.
 */
export function listAt(definition: JfssDefinition, listPath: string): ListInfo | null {
  if (listPath === ROOT) {
    return {
      kind: 'root',
      owner: null,
      ownerPath: null,
      slot: null,
      items: definition.components,
    }
  }

  const slotted = /^(.*)\.(columns|tabs)\.(\d+)\.components$/.exec(listPath)
  const plain = slotted ? null : /^(.*)\.components$/.exec(listPath)
  const ownerPath = slotted?.[1] ?? plain?.[1]

  if (ownerPath === undefined) {
    return null
  }

  const located = locate(definition, ownerPath)
  // The owner's own list must be drawn too, all the way up: a list inside an
  // unsupported card is not a list the canvas offers, however well formed.
  const parent = located ? listAt(definition, located.listPath) : null

  if (!located || !parent || cardReason(parent.kind, located.node)) {
    return null
  }

  const { node } = located
  const shape = offeredEntry(node.type)?.children

  if (slotted) {
    const key = slotted[2] as 'columns' | 'tabs'
    const slot = Number(slotted[3])
    const slots = slotsOf(node, key)

    if (shape !== key || !slots || slot >= slots.length) {
      return null
    }

    return {
      kind: key === 'columns' ? 'column' : 'tab',
      owner: node,
      ownerPath,
      slot,
      items: (slots[slot] as { components: JfssComponent[] }).components,
    }
  }

  if (shape === 'components') {
    return { kind: 'components', owner: node, ownerPath, slot: null, items: childrenOf(node) ?? [] }
  }

  if (shape === 'rowTemplate') {
    return {
      kind: 'rowTemplate',
      owner: node,
      ownerPath,
      slot: null,
      items: childrenOf(node) ?? [],
    }
  }

  return null
}

/** A node and the kind of list it sits in, by path; `null` when nothing is there. */
export function locate(
  definition: JfssDefinition,
  nodePath: string,
): { node: JfssComponent; kind: ListKind; listPath: string; index: number } | null {
  const match = /^(.*)\.(\d+)$/.exec(nodePath)

  if (!match) {
    return null
  }

  const [, listPath, at] = match

  // The root is the only list that needs no owner; every other one is
  // resolved through its owner, which is how a path into a non-container
  // fails here rather than reading a property that happens to be there.
  const list = listPath === ROOT ? null : listShapeAt(definition, listPath)
  const items = listPath === ROOT ? definition.components : list?.items
  const kind: ListKind = listPath === ROOT ? 'root' : (list?.kind ?? 'components')
  const node = items?.[Number(at)]

  return node ? { node, kind, listPath, index: Number(at) } : null
}

/**
 * A list by path **whatever its owner is**, for reading. Unlike {@link listAt},
 * it walks into nodes the canvas does not draw, so a node inside one can still
 * be found by its path.
 */
function listShapeAt(
  definition: JfssDefinition,
  listPath: string,
): { items: JfssComponent[]; kind: ListKind } | null {
  const slotted = /^(.*)\.(columns|tabs)\.(\d+)\.components$/.exec(listPath)
  const plain = slotted ? null : /^(.*)\.components$/.exec(listPath)
  const ownerPath = slotted?.[1] ?? plain?.[1]

  if (ownerPath === undefined) {
    return null
  }

  const owner = locate(definition, ownerPath)?.node

  if (!owner) {
    return null
  }

  if (slotted) {
    const slot = slotsOf(owner, slotted[2] as 'columns' | 'tabs')?.[Number(slotted[3])] as
      { components?: unknown } | undefined

    return Array.isArray(slot?.components)
      ? {
          items: slot.components as JfssComponent[],
          kind: slotted[2] === 'columns' ? 'column' : 'tab',
        }
      : null
  }

  const children = childrenOf(owner)

  return children
    ? { items: children, kind: owner.role === 'data' ? 'rowTemplate' : 'components' }
    : null
}

/** The nodes of any list by path, drawn or not; empty when there is none. */
export function itemsAt(definition: JfssDefinition, listPath: string): JfssComponent[] {
  return listPath === ROOT
    ? definition.components
    : (listShapeAt(definition, listPath)?.items ?? [])
}

/** The path of the node that owns a list; `null` for the root. */
export function ownerPathOf(listPath: string): string | null {
  const slot = /^(.*)\.(?:columns|tabs)\.\d+\.components$/.exec(listPath)

  return slot ? slot[1] : (/^(.*)\.components$/.exec(listPath)?.[1] ?? null)
}

/** The path of the first node with this `id`, in document order; `null` when there is none. */
export function pathOf(definition: JfssDefinition, id: string): string | null {
  let found: string | null = null

  walk(definition, (node, path) => {
    if (found === null && node.id === id) {
      found = path
    }
  })

  return found
}

/**
 * Every node in the tree, row templates and unsupported subtrees included,
 * with its path.
 *
 * **The generic walk**: it descends `components`, `columns[].components` and
 * `tabs[].components` on every node whatever its role or type, because what it
 * is for — taken ids and keys, lookup cleanup — must see everything stored,
 * not only what the canvas draws.
 */
export function walk(
  definition: Pick<JfssDefinition, 'components'>,
  visit: (node: JfssComponent, path: string) => void,
): void {
  const descend = (nodes: unknown, listPath: string): void => {
    if (!Array.isArray(nodes)) {
      return
    }

    nodes.forEach((node: JfssComponent, index) => {
      if (typeof node !== 'object' || node === null) {
        return
      }

      const path = `${listPath}.${index}`

      visit(node, path)
      descend(childrenOf(node), `${path}.components`)

      for (const key of ['columns', 'tabs'] as const) {
        slotsOf(node, key)?.forEach((slot, at) =>
          descend(
            (slot as { components?: unknown } | null)?.components,
            `${path}.${key}.${at}.components`,
          ),
        )
      }
    })
  }

  descend(definition.components, ROOT)
}

/** Every `id` in a subtree, the node's own included. */
function subtreeIds(node: JfssComponent): string[] {
  const ids = [node.id]

  walk({ components: [node] }, (child) => {
    if (child !== node) {
      ids.push(child.id)
    }
  })

  return ids
}

// --- Identity (B4) -------------------------------------------------------------

/**
 * A fresh `id` and `key`, unique across **the whole tree**: row templates,
 * slots and unsupported subtrees included.
 *
 * `id` is unique per instance (JFSS §4.1) and is what `settings.lookups` binds
 * on, so a duplicate binds two fields to one source. The flat builder scanned
 * the root alone, which was the whole tree when the tree was flat.
 *
 * Data nodes are `field_N`, which is what the create dialog's first field is
 * and what the flat builder made; everything else is named for its type.
 */
export function freshIdentity(definition: JfssDefinition, type: string): NodeIdentity {
  const taken = new Set<string>()

  walk(definition, (node) => {
    taken.add(node.id)

    const key = (node as { key?: unknown }).key

    if (typeof key === 'string') {
      taken.add(key)
    }
  })

  const prefix = offeredEntry(type)?.role === 'data' ? 'field' : type.replace(/[^A-Za-z0-9_]/g, '_')

  for (let n = 1; ; n += 1) {
    const candidate = `${prefix}_${n}`

    if (!taken.has(candidate)) {
      return { id: candidate, key: candidate }
    }
  }
}

// --- Containment (C1) ----------------------------------------------------------

/**
 * **The one function that decides containment** (C1). Drag, *Add here* and
 * *Move to…* all ask it, and every mutation below asks it again, so nothing
 * that skips the question can change the tree.
 *
 * `from` is the node's current path when it is being moved, so that a
 * container is refused as a destination inside itself.
 */
export function canContain(
  definition: JfssDefinition,
  listPath: string,
  node: JfssComponent,
  from?: string,
): Verdict {
  const list = listAt(definition, listPath)

  if (!list) {
    return { ok: false, reason: 'That is not a place components can go.' }
  }

  if (from !== undefined && (listPath === from || listPath.startsWith(`${from}.`))) {
    return { ok: false, reason: 'A component cannot be moved into itself.' }
  }

  const placement = placementReason(list.kind, node)

  return placement ? { ok: false, reason: placement } : { ok: true }
}

// --- Mutations -------------------------------------------------------------------

/** Inserts a node; `null` when the list refuses it. `index` is clamped to the list. */
export function insertNode(
  definition: JfssDefinition,
  listPath: string,
  index: number,
  node: JfssComponent,
): JfssDefinition | null {
  if (!canContain(definition, listPath, node).ok) {
    return null
  }

  const next = copy(definition)
  const list = listAt(next, listPath) as ListInfo

  if (list.kind === 'rowTemplate' && childrenOf(list.owner as JfssComponent) === undefined) {
    ;(list.owner as unknown as { components: JfssComponent[] }).components = list.items
  }

  list.items.splice(Math.max(0, Math.min(index, list.items.length)), 0, copy(node))

  return next
}

/**
 * A new node of an offered type, placed; `null` when the type is not offered
 * or the list refuses it.
 *
 * **A lookup is bound to a source as it is added**, because the server refuses
 * a lookup with no binding (`LOOKUP_SOURCE_MISSING`), and a palette default
 * that fails its first save is not a default (B4). The author changes the
 * source in the editor.
 */
export function addComponent(
  definition: JfssDefinition,
  type: string,
  listPath: string,
  index?: number,
): { definition: JfssDefinition; node: JfssComponent } | null {
  const entry = offeredEntry(type)

  if (!entry) {
    return null
  }

  const node = entry.create(freshIdentity(definition, type))
  const at = index ?? listAt(definition, listPath)?.items.length ?? 0
  const next = insertNode(definition, listPath, at, node)

  if (!next) {
    return null
  }

  if (type === 'lookup') {
    next.settings = {
      ...(next.settings ?? {}),
      lookups: {
        ...((next.settings?.lookups as Record<string, string>) ?? {}),
        [node.id]: 'supplier',
      },
    }
  }

  return { definition: next, node }
}

/**
 * Moves a node to `toIndex` of a list, **counted after the node has left its
 * own list** — which is what a drop reports, and what makes a move down by one
 * `index + 1` in the same list.
 *
 * `null` when the destination refuses it (C1, C2), including a container into
 * itself or a descendant. **Lookups are not touched**: they are keyed by `id`,
 * and a move changes no `id` (C4).
 */
export function moveNode(
  definition: JfssDefinition,
  nodePath: string,
  toListPath: string,
  toIndex: number,
): JfssDefinition | null {
  const source = locate(definition, nodePath)

  if (!source || !canContain(definition, toListPath, source.node, nodePath).ok) {
    return null
  }

  const next = copy(definition)
  // Both arrays are taken **before** either changes: removing the node can
  // shift every index in the destination's path, and a reference does not
  // shift.
  const from = locate(next, nodePath) as NonNullable<ReturnType<typeof locate>>
  const fromItems = itemsAt(next, from.listPath)
  const to = listAt(next, toListPath) as ListInfo

  if (to.kind === 'rowTemplate' && childrenOf(to.owner as JfssComponent) === undefined) {
    ;(to.owner as unknown as { components: JfssComponent[] }).components = to.items
  }

  const [moved] = fromItems.splice(from.index, 1)

  to.items.splice(Math.max(0, Math.min(toIndex, to.items.length)), 0, moved)

  return next
}

/** Moves a node one place up (`-1`) or down (`1`) in its own list; `null` at the ends. */
export function moveBy(
  definition: JfssDefinition,
  nodePath: string,
  delta: -1 | 1,
): JfssDefinition | null {
  const source = locate(definition, nodePath)

  if (!source) {
    return null
  }

  const length = itemsAt(definition, source.listPath).length
  const target = source.index + delta

  if (target < 0 || target >= length) {
    return null
  }

  return moveNode(definition, nodePath, source.listPath, target)
}

/** Drops `settings.lookups` entries for these ids, leaving `settings` otherwise as it was. */
function dropBindings(definition: JfssDefinition, ids: string[]): void {
  const lookups = definition.settings?.lookups as Record<string, string> | undefined

  if (!lookups || !ids.some((id) => id in lookups)) {
    return
  }

  const remaining = { ...lookups }

  for (const id of ids) {
    delete remaining[id]
  }

  definition.settings = { ...definition.settings, lookups: remaining }
}

/**
 * Removes a node **and every lookup binding in its subtree** (C4).
 *
 * A binding naming no component is refused at save (`domain/jfss.rs`,
 * `LOOKUP_COMPONENT_NOT_FOUND`), so leaving one behind makes the next save
 * fail about a field no longer on screen. The flat builder cleaned up the
 * removed node's own binding; a removed panel takes its children's with it.
 */
export function removeNode(definition: JfssDefinition, nodePath: string): JfssDefinition | null {
  if (!locate(definition, nodePath)) {
    return null
  }

  const next = copy(definition)
  const at = locate(next, nodePath) as NonNullable<ReturnType<typeof locate>>
  const items = itemsAt(next, at.listPath)
  const [removed] = items.splice(at.index, 1)

  dropBindings(next, subtreeIds(removed))

  return next
}

/** Replaces a node in place, as the editor does after every edit. */
export function replaceNode(
  definition: JfssDefinition,
  nodePath: string,
  node: JfssComponent,
): JfssDefinition | null {
  if (!locate(definition, nodePath)) {
    return null
  }

  const next = copy(definition)
  const at = locate(next, nodePath) as NonNullable<ReturnType<typeof locate>>
  const items = itemsAt(next, at.listPath)

  items[at.index] = copy(node)

  return next
}

/** Binds a lookup to a source, or unbinds it with `''` (D-23). */
export function setLookup(definition: JfssDefinition, id: string, source: string): JfssDefinition {
  const next = copy(definition)
  const remaining = { ...((next.settings?.lookups as Record<string, string>) ?? {}) }

  if (source === '') {
    delete remaining[id]
  } else {
    remaining[id] = source
  }

  next.settings = { ...(next.settings ?? {}), lookups: remaining }

  return next
}

// --- Slots (C3) --------------------------------------------------------------------

function slotContainer(definition: JfssDefinition, nodePath: string) {
  const at = locate(definition, nodePath)

  if (!at || !listAt(definition, at.listPath) || cardReason(at.kind, at.node)) {
    return null
  }

  const shape = offeredEntry(at.node.type)?.children

  return shape === 'columns' || shape === 'tabs' ? { ...at, shape } : null
}

/** Adds an empty slot at the end: a column, or a tab titled *Tab N*. */
export function addSlot(definition: JfssDefinition, nodePath: string): JfssDefinition | null {
  if (!slotContainer(definition, nodePath)) {
    return null
  }

  const next = copy(definition)
  const at = slotContainer(next, nodePath) as NonNullable<ReturnType<typeof slotContainer>>
  const slots = slotsOf(at.node, at.shape) as Record<string, unknown>[]

  slots.push(
    at.shape === 'columns'
      ? { components: [] }
      : { title: `Tab ${slots.length + 1}`, components: [] },
  )

  return next
}

/**
 * Removes a slot and every lookup binding inside it (C3, C4); `null` for the
 * last slot, because a container with no slots is a container nothing can be
 * put in.
 */
export function removeSlot(
  definition: JfssDefinition,
  nodePath: string,
  slot: number,
): JfssDefinition | null {
  const at = slotContainer(definition, nodePath)
  const slots = at ? slotsOf(at.node, at.shape) : undefined

  if (!at || !slots || slots.length <= 1 || slot < 0 || slot >= slots.length) {
    return null
  }

  const next = copy(definition)
  const target = slotContainer(next, nodePath) as NonNullable<ReturnType<typeof slotContainer>>
  const [removed] = (
    slotsOf(target.node, target.shape) as { components: JfssComponent[] }[]
  ).splice(slot, 1)

  dropBindings(
    next,
    removed.components.flatMap((child) => subtreeIds(child)),
  )

  return next
}

/** Renames a tab; `null` for a blank title, which JFSS's tab slot requires (C3). */
export function renameSlot(
  definition: JfssDefinition,
  nodePath: string,
  slot: number,
  title: string,
): JfssDefinition | null {
  const at = slotContainer(definition, nodePath)

  if (!at || at.shape !== 'tabs' || title.trim() === '') {
    return null
  }

  const next = copy(definition)
  const target = slotContainer(next, nodePath) as NonNullable<ReturnType<typeof slotContainer>>
  const slots = slotsOf(target.node, 'tabs') as { title: string }[]

  if (!slots[slot]) {
    return null
  }

  slots[slot].title = title

  return next
}

// --- Names and destinations (D3, D6) ---------------------------------------------

/**
 * What a person calls a node: its type's name, and its own text when it has
 * one. Every control on a card is named with it (D6).
 */
export function nodeName(node: JfssComponent): string {
  const record = node as unknown as Record<string, unknown>
  const own = [record.label, record.title, record.content].find(
    (value): value is string => typeof value === 'string' && value.trim() !== '',
  )
  const type = typeLabel(node.type)

  return own ? `${type} “${own.length > 40 ? `${own.slice(0, 39)}…` : own}”` : type
}

/** A list's name, as *Move to…* offers it and the announcements read it. */
export function listName(definition: JfssDefinition, listPath: string): string {
  const list = listAt(definition, listPath)

  if (!list || !list.owner) {
    return 'the form'
  }

  const owner = nodeName(list.owner)

  switch (list.kind) {
    case 'column':
      return `column ${(list.slot ?? 0) + 1} of ${owner}`
    case 'tab': {
      const title = (slotsOf(list.owner, 'tabs')?.[list.slot ?? 0] as { title?: string })?.title
      return `tab “${title ?? ''}” of ${owner}`
    }
    case 'rowTemplate':
      return `the row template of ${owner}`
    default:
      return owner
  }
}

/** Every list the canvas draws, in document order, root first. */
export function drawnLists(definition: JfssDefinition): string[] {
  const lists = [ROOT]

  const visit = (items: JfssComponent[], listPath: string, kind: ListKind): void => {
    items.forEach((node, index) => {
      const path = `${listPath}.${index}`

      if (cardReason(kind, node)) {
        return
      }

      const shape = offeredEntry(node.type)?.children

      if (shape === 'components' || shape === 'rowTemplate') {
        lists.push(`${path}.components`)
        visit(
          childrenOf(node) ?? [],
          `${path}.components`,
          shape === 'components' ? 'components' : 'rowTemplate',
        )
      }

      if (shape === 'columns' || shape === 'tabs') {
        slotsOf(node, shape)?.forEach((slot, at) => {
          const slotPath = `${path}.${shape}.${at}.components`

          lists.push(slotPath)
          visit(
            (slot as { components: JfssComponent[] }).components,
            slotPath,
            shape === 'columns' ? 'column' : 'tab',
          )
        })
      }
    })
  }

  visit(definition.components, ROOT, 'root')

  return lists
}

/**
 * Where *Move to…* may send a node: every drawn list {@link canContain}
 * allows, except the one it is already in, each named by its path (D3).
 */
export function moveDestinations(
  definition: JfssDefinition,
  nodePath: string,
): { listPath: string; label: string }[] {
  const at = locate(definition, nodePath)

  if (!at) {
    return []
  }

  return drawnLists(definition)
    .filter(
      (listPath) =>
        listPath !== at.listPath && canContain(definition, listPath, at.node, nodePath).ok,
    )
    .map((listPath) => ({ listPath, label: listTrail(definition, listPath) }))
}

/** A list named by its ancestry: *Form › Panel “Details” › Column 2 of Columns*. */
export function listTrail(definition: JfssDefinition, listPath: string): string {
  if (listPath === ROOT) {
    return 'Form'
  }

  const trail = ['Form']
  const steps = listPath.split('.')

  // Every proper prefix that addresses a list is an ancestor list.
  for (let end = 2; end < steps.length; end += 1) {
    const prefix = steps.slice(0, end).join('.')

    if (prefix !== ROOT && /components$/.test(prefix) && listAt(definition, prefix)) {
      trail.push(capitalise(listName(definition, prefix)))
    }
  }

  trail.push(capitalise(listName(definition, listPath)))

  return trail.join(' › ')
}

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1)
}

// --- S10.3 paths (E6) -----------------------------------------------------------------

/** Where a server detail lands: a node, and the rest of the path below it. */
export interface ResolvedDetail {
  nodePath: string
  nodeId: string
  /** The path below the node, `''` when the detail is about the node itself. */
  rest: string
}

/**
 * **The server's S10.3 path, resolved to the node it is about** (E6).
 *
 * **The save's 422 prefixes every path with `definition.`.** Measured on
 * 2026-10-10 against a running backend (row 11's probe): a stray property four
 * levels down came back as
 * `definition.components.0.components.0.tabs.0.components.0.columns.1.components.0`,
 * and an unregistered rule in a row template as
 * `definition.components.0.components.0.rules.0`. `domain/jfss.rs`'s
 * `shape_errors`, the rule engine and the lookup check all write it, and no
 * layer between them and the browser strips it. A path without the prefix is
 * therefore not one this resolver claims: it stays in the page's list, where
 * it is still shown.
 *
 * It walks `components.N`, `columns.N.components.M`, `tabs.N.components.M`
 * and a row template's `components.M`, as far as the tree goes, and returns
 * the deepest node reached and what is left of the path. **`stop` ends the
 * walk at a node whose inside the canvas does not draw**, so a message about
 * something inside an unsupported card lands on the card rather than on
 * nothing.
 */
export function resolveDetail(
  definition: JfssDefinition,
  path: string,
  stop: (node: JfssComponent, kind: ListKind) => boolean = () => false,
): ResolvedDetail | null {
  const prefix = 'definition.'

  if (!path.startsWith(prefix)) {
    return null
  }

  const tokens = path.slice(prefix.length).split('.')
  let node: JfssComponent | null = null
  let nodePath = ''
  let kind: ListKind = 'root'
  let at = 0

  for (;;) {
    if (node && stop(node, kind)) {
      break
    }

    const [first, second, third, fourth] = tokens.slice(at, at + 4)

    if (first === 'components' && isIndex(second)) {
      const list: JfssComponent[] | undefined = node ? childrenOf(node) : definition.components
      const child: JfssComponent | undefined = list?.[Number(second)]

      if (!child) {
        break
      }

      kind = node === null ? 'root' : node.role === 'data' ? 'rowTemplate' : 'components'
      nodePath = nodePath === '' ? `components.${second}` : `${nodePath}.components.${second}`
      node = child
      at += 2
    } else if (
      node &&
      (first === 'columns' || first === 'tabs') &&
      isIndex(second) &&
      third === 'components' &&
      isIndex(fourth)
    ) {
      const slot = slotsOf(node, first)?.[Number(second)] as
        { components?: JfssComponent[] } | undefined
      const child: JfssComponent | undefined = Array.isArray(slot?.components)
        ? slot.components[Number(fourth)]
        : undefined

      if (!child) {
        break
      }

      kind = first === 'columns' ? 'column' : 'tab'
      nodePath = `${nodePath}.${first}.${second}.components.${fourth}`
      node = child
      at += 4
    } else {
      break
    }
  }

  if (!node) {
    return null
  }

  return { nodePath, nodeId: node.id, rest: tokens.slice(at).join('.') }
}

/**
 * Splits a refusal's details into those that land on a node, by its `id`, and
 * those that land on none — **and drops none** (E6).
 */
export function placeDetails(
  definition: JfssDefinition,
  details: ValidationDetail[],
): { byNode: Map<string, ValidationDetail[]>; unplaced: ValidationDetail[] } {
  const byNode = new Map<string, ValidationDetail[]>()
  const unplaced: ValidationDetail[] = []

  for (const detail of details) {
    const placed = resolveDetail(definition, detail.path, (node, kind) =>
      Boolean(cardReason(kind, node)),
    )

    if (placed) {
      byNode.set(placed.nodeId, [...(byNode.get(placed.nodeId) ?? []), detail])
    } else {
      unplaced.push(detail)
    }
  }

  return { byNode, unplaced }
}
