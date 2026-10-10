import { computed, inject, nextTick, provide, ref, type InjectionKey, type Ref } from 'vue'

import type { ValidationDetail } from '@/types/api'
import type { JfssComponent, JfssDefinition } from '@/types/jfss'

import {
  addComponent,
  addSlot as addSlotTo,
  canContain,
  cardReason,
  itemsAt,
  listAt,
  listName,
  locate,
  moveBy as moveByOne,
  moveDestinations,
  moveNode,
  nodeName,
  pathOf,
  placeDetails,
  removeNode,
  removeSlot as removeSlotFrom,
  renameSlot as renameSlotOf,
  ownerPathOf,
} from './formTree'
import { offeredEntry } from './builderRegistry'

/**
 * **The canvas's state and every change it makes, in one place** (#688 D).
 *
 * The page creates it, the palette and every recursive card inject it. **Each
 * operation here is the one the pointer and the keyboard both reach** (D5): a
 * drop is {@link FormBuilder.drop}, which calls the same `move` that *Move up*,
 * *Move down* and *Move to…* call, and a palette drop is the same `add` the
 * palette's buttons and every list's *Add component* call. Each of those calls
 * one pure function in [`formTree.ts`](./formTree.ts), so a refused change is
 * refused in one place.
 *
 * **Read-only is checked here, not only by hiding controls** (D7): a drop
 * that reached a read-only canvas, or a keyboard path a template forgot to
 * hide, changes nothing.
 *
 * **Focus and the announcement follow every change** (D6): after an add, focus
 * goes to the new node; after a move it stays on the moved node; after a
 * removal it goes to the next sibling, else the previous one, else the
 * container. The live region says *Added*, *Moved to* or *Removed*.
 */

/** What is being dragged, so a list can refuse a drop before it happens. */
export type Dragging =
  { kind: 'move'; nodePath: string; node: JfssComponent } | { kind: 'new'; type: string } | null

/** A removal waiting on the confirmation C3 asks for. */
export interface PendingRemoval {
  nodePath: string
  slot: number | null
  /** What the dialog names. */
  name: string
}

/** A finished drag: the list it left, where, the list it reached, and where. */
export interface Drop {
  fromListPath: string
  oldIndex: number
  toListPath: string
  newIndex: number
}

export interface FormBuilder {
  definition: Ref<JfssDefinition>
  readOnly: Ref<boolean>
  selectedId: Ref<string | null>
  announcement: Ref<string>
  dragging: Ref<Dragging>
  pendingRemoval: Ref<PendingRemoval | null>
  /** The S10.3 details that landed on this node (E6). */
  detailsFor(id: string): ValidationDetail[]
  /** The details that landed on no node, which the page lists (E6). */
  unplacedDetails: Ref<ValidationDetail[]>
  select(id: string | null): void
  /** Where the palette adds, from the selection (D2). */
  paletteTarget: Ref<{ listPath: string; name: string }>
  canAddTo(type: string, listPath: string): boolean
  add(type: string, listPath: string, index?: number): boolean
  move(nodePath: string, toListPath: string, toIndex: number, control?: string): boolean
  moveBy(nodePath: string, delta: -1 | 1): boolean
  destinations(nodePath: string): { listPath: string; label: string }[]
  requestRemove(nodePath: string): void
  requestRemoveSlot(nodePath: string, slot: number): void
  confirmRemoval(): void
  addSlot(nodePath: string): void
  renameSlot(nodePath: string, slot: number, title: string): boolean
  /** Whether the list may take what is being dragged (a Sortable `put`). */
  canDropInto(listPath: string): boolean
  drop(drop: Drop): boolean
  dropNew(type: string, toListPath: string, newIndex: number): boolean
}

const KEY: InjectionKey<FormBuilder> = Symbol('form-builder')

/** Whether a node is one the canvas draws as editable where it sits. */
function selectable(definition: JfssDefinition, id: string): boolean {
  const path = pathOf(definition, id)
  const at = path ? locate(definition, path) : null
  const list = at ? listAt(definition, at.listPath) : null

  return at !== null && list !== null && cardReason(list.kind, at.node) === null
}

/** Whether a node holds anything: removing it then asks first (C3). */
function holdsChildren(node: JfssComponent): boolean {
  const record = node as unknown as Record<string, unknown>
  const lists = [
    record.components,
    ...(['columns', 'tabs'] as const).flatMap((key) =>
      Array.isArray(record[key])
        ? (record[key] as { components?: unknown }[]).map((slot) => slot?.components)
        : [],
    ),
  ]

  return lists.some((list) => Array.isArray(list) && list.length > 0)
}

function focusFirst(selector: string): void {
  void nextTick(() => {
    document.querySelector<HTMLElement>(selector)?.focus()
  })
}

/** The element that takes focus for a node: its select button, or the card itself. */
function focusNode(id: string, control?: string): void {
  void nextTick(() => {
    const card = Array.from(document.querySelectorAll<HTMLElement>('[data-node-id]')).find(
      (element) => element.dataset.nodeId === id,
    )
    const preferred = control
      ? card?.querySelector<HTMLButtonElement>(`[data-node-control="${control}"]`)
      : null
    const target =
      preferred && !preferred.disabled
        ? preferred
        : (card?.querySelector<HTMLElement>('[data-node-focus]') ?? card)

    target?.focus()
  })
}

export function createFormBuilder(options: {
  definition: Ref<JfssDefinition | null>
  readOnly: Ref<boolean>
  details: Ref<ValidationDetail[]>
}): FormBuilder {
  const definition = computed({
    get: () => options.definition.value as JfssDefinition,
    set: (next: JfssDefinition) => {
      options.definition.value = next
    },
  })
  const selectedId = ref<string | null>(null)
  const announcement = ref('')
  const dragging = ref<Dragging>(null)
  const pendingRemoval = ref<PendingRemoval | null>(null)

  const placed = computed(() =>
    options.definition.value
      ? placeDetails(options.definition.value, options.details.value)
      : { byNode: new Map<string, ValidationDetail[]>(), unplaced: options.details.value },
  )

  function announce(message: string): void {
    // Cleared first, so the same words twice in a row are announced twice.
    announcement.value = ''
    void nextTick(() => {
      announcement.value = message
    })
  }

  function select(id: string | null): void {
    selectedId.value = id && selectable(definition.value, id) ? id : null
  }

  const paletteTarget = computed(() => {
    const id = selectedId.value
    const path = id ? pathOf(definition.value, id) : null
    const at = path ? locate(definition.value, path) : null

    if (!path || !at) {
      return { listPath: 'components', name: 'the form' }
    }

    // The selected container, else the container the selected node is in.
    const shape = offeredEntry(at.node.type)?.children
    const own =
      shape === 'components' || shape === 'rowTemplate'
        ? `${path}.components`
        : shape === 'columns' || shape === 'tabs'
          ? `${path}.${shape}.0.components`
          : null
    const listPath = own && listAt(definition.value, own) ? own : at.listPath

    return { listPath, name: listName(definition.value, listPath) }
  })

  /**
   * Whether a list takes a new node of this type: the same `canContain` the
   * add itself asks, on a stand-in of the type's role, without building one.
   */
  function canAddTo(type: string, listPath: string): boolean {
    const entry = offeredEntry(type)

    return (
      !options.readOnly.value &&
      entry !== undefined &&
      canContain(definition.value, listPath, { id: '', role: entry.role, type } as JfssComponent).ok
    )
  }

  function add(type: string, listPath: string, index?: number): boolean {
    if (options.readOnly.value) {
      return false
    }

    const result = addComponent(definition.value, type, listPath, index)

    if (!result) {
      return false
    }

    definition.value = result.definition
    select(result.node.id)
    announce(`Added ${nodeName(result.node)} to ${listName(result.definition, listPath)}.`)
    focusNode(result.node.id)

    return true
  }

  function move(nodePath: string, toListPath: string, toIndex: number, control?: string): boolean {
    if (options.readOnly.value) {
      return false
    }

    const at = locate(definition.value, nodePath)
    const next = at ? moveNode(definition.value, nodePath, toListPath, toIndex) : null

    if (!at || !next) {
      return false
    }

    definition.value = next

    // The destination's path may have shifted when the node left its list;
    // the node's own new path says where it is now.
    const now = pathOf(next, at.node.id)
    const landed = now ? locate(next, now)?.listPath : null

    announce(`Moved ${nodeName(at.node)} to ${listName(next, landed ?? toListPath)}.`)
    focusNode(at.node.id, control)

    return true
  }

  function moveBy(nodePath: string, delta: -1 | 1): boolean {
    const at = locate(definition.value, nodePath)
    const next = at && !options.readOnly.value ? moveByOne(definition.value, nodePath, delta) : null

    if (!at || !next) {
      return false
    }

    definition.value = next
    announce(
      `Moved ${nodeName(at.node)} ${delta < 0 ? 'up' : 'down'} in ${listName(next, at.listPath)}.`,
    )
    focusNode(at.node.id, delta < 0 ? 'up' : 'down')

    return true
  }

  /** Where focus goes after a removal: next sibling, else previous, else the container. */
  function focusAfterRemoval(nodePath: string): () => void {
    const at = locate(definition.value, nodePath)

    if (!at) {
      return () => undefined
    }

    const siblings = itemsAt(definition.value, at.listPath)
    const ownerPath = ownerPathOf(at.listPath)
    const target =
      siblings[at.index + 1] ??
      siblings[at.index - 1] ??
      (ownerPath ? locate(definition.value, ownerPath)?.node : undefined)

    if (!target) {
      return () => focusFirst('[data-list-add="components"]')
    }

    return () => {
      if (selectedId.value === null) {
        select(target.id)
      }

      focusNode(target.id)
    }
  }

  function remove(nodePath: string): boolean {
    const at = locate(definition.value, nodePath)

    if (options.readOnly.value || !at) {
      return false
    }

    const refocus = focusAfterRemoval(nodePath)
    const next = removeNode(definition.value, nodePath)

    if (!next) {
      return false
    }

    definition.value = next

    if (selectedId.value !== null && pathOf(next, selectedId.value) === null) {
      selectedId.value = null
    }

    announce(`Removed ${nodeName(at.node)}.`)
    refocus()

    return true
  }

  function requestRemove(nodePath: string): void {
    const at = locate(definition.value, nodePath)

    if (options.readOnly.value || !at) {
      return
    }

    if (holdsChildren(at.node)) {
      pendingRemoval.value = { nodePath, slot: null, name: nodeName(at.node) }

      return
    }

    remove(nodePath)
  }

  function removeSlotNow(nodePath: string, slot: number): boolean {
    const at = locate(definition.value, nodePath)
    const next =
      at && !options.readOnly.value ? removeSlotFrom(definition.value, nodePath, slot) : null

    if (!at || !next) {
      return false
    }

    const kind = 'columns' in at.node ? 'column' : 'tab'

    definition.value = next

    if (selectedId.value !== null && pathOf(next, selectedId.value) === null) {
      selectedId.value = null
    }

    announce(`Removed ${kind} ${slot + 1} from ${nodeName(at.node)}.`)
    focusNode(at.node.id)

    return true
  }

  function requestRemoveSlot(nodePath: string, slot: number): void {
    const at = locate(definition.value, nodePath)

    if (options.readOnly.value || !at) {
      return
    }

    const record = at.node as unknown as Record<string, { components: unknown[] }[]>
    const slots = record.columns ?? record.tabs ?? []

    if ((slots[slot]?.components.length ?? 0) > 0) {
      pendingRemoval.value = {
        nodePath,
        slot,
        name: `${'columns' in record ? 'column' : 'tab'} ${slot + 1} of ${nodeName(at.node)}`,
      }

      return
    }

    removeSlotNow(nodePath, slot)
  }

  function confirmRemoval(): void {
    const pending = pendingRemoval.value

    pendingRemoval.value = null

    if (!pending) {
      return
    }

    if (pending.slot === null) {
      remove(pending.nodePath)
    } else {
      removeSlotNow(pending.nodePath, pending.slot)
    }
  }

  function addSlot(nodePath: string): void {
    const at = locate(definition.value, nodePath)
    const next = at && !options.readOnly.value ? addSlotTo(definition.value, nodePath) : null

    if (!at || !next) {
      return
    }

    definition.value = next
    announce(`Added a ${'columns' in at.node ? 'column' : 'tab'} to ${nodeName(at.node)}.`)
  }

  function renameSlot(nodePath: string, slot: number, title: string): boolean {
    const next = options.readOnly.value
      ? null
      : renameSlotOf(definition.value, nodePath, slot, title)

    if (!next) {
      return false
    }

    definition.value = next

    return true
  }

  function canDropInto(listPath: string): boolean {
    const current = dragging.value

    if (options.readOnly.value || !current) {
      return false
    }

    return current.kind === 'move'
      ? canContain(definition.value, listPath, current.node, current.nodePath).ok
      : canAddTo(current.type, listPath)
  }

  function drop({ fromListPath, oldIndex, toListPath, newIndex }: Drop): boolean {
    dragging.value = null

    if (fromListPath === toListPath && oldIndex === newIndex) {
      return false
    }

    return move(`${fromListPath}.${oldIndex}`, toListPath, newIndex)
  }

  function dropNew(type: string, toListPath: string, newIndex: number): boolean {
    dragging.value = null

    return add(type, toListPath, newIndex)
  }

  return {
    definition,
    readOnly: options.readOnly,
    selectedId,
    announcement,
    dragging,
    pendingRemoval,
    detailsFor: (id) => placed.value.byNode.get(id) ?? [],
    unplacedDetails: computed(() => placed.value.unplaced),
    select,
    paletteTarget,
    canAddTo,
    add,
    move,
    moveBy,
    destinations: (nodePath) => moveDestinations(definition.value, nodePath),
    requestRemove,
    requestRemoveSlot,
    confirmRemoval,
    addSlot,
    renameSlot,
    canDropInto,
    drop,
    dropNew,
  }
}

export function provideFormBuilder(builder: FormBuilder): void {
  provide(KEY, builder)
}

export function useFormBuilder(): FormBuilder {
  const builder = inject(KEY)

  if (!builder) {
    throw new Error('useFormBuilder() needs a FormBuilderPage above it')
  }

  return builder
}
