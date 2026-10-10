<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { Plus } from '@lucide/vue'
import { VueDraggable } from 'vue-draggable-plus'

import { Button } from '@/components/ui/button'
import type { JfssComponent } from '@/types/jfss'

import { offeredEntries } from './builderRegistry'
import FormCanvasNode from './FormCanvasNode.vue'
import { listName, type ListKind } from './formTree'
import { useFormBuilder } from './useFormBuilder'

/**
 * One list of the canvas: the root, a panel's or fieldset's children, a slot,
 * or a data grid's row template (#688 D1, D2).
 *
 * **Lists nest**: each card draws its own lists with this component, so the
 * canvas is as deep as the definition.
 *
 * # Dragging changes nothing by itself
 *
 * `vue-draggable-plus` is handed the list one way, as `model-value`, and its
 * `update:modelValue` is not listened to. It puts the DOM back after Sortable
 * moved it, and the drop is then **reported**, on `end`, as a move from one
 * list path to another: {@link FormBuilder.drop}, which calls the same
 * `moveNode` the keyboard's controls call (D5). A drop the tree refuses
 * therefore changes nothing, and **a list refuses it before it lands** through
 * Sortable's `put`, which asks the same `canContain`.
 *
 * Drags start from the handle only, and a read-only canvas disables them as
 * well as hiding the handle (D7).
 *
 * # Without dragging
 *
 * *Add component* is a button every list has, empty or not, reachable by Tab
 * (D2). It opens the types this list can take, and adding one moves focus to
 * the new node (D6).
 */
const props = defineProps<{
  listPath: string
  items: JfssComponent[]
  kind: ListKind
}>()

const builder = useFormBuilder()

const adding = ref(false)
const chooser = ref<HTMLElement | null>(null)

const name = computed(() => listName(builder.definition.value, props.listPath))

/** The types this list takes, in palette order. */
const choices = computed(() =>
  adding.value
    ? offeredEntries().filter((entry) => builder.canAddTo(entry.type, props.listPath))
    : [],
)

/**
 * Keys by `id`, made unique within the list: a loaded definition may repeat
 * an `id`, and the canvas still draws it.
 */
const keys = computed(() => {
  const seen = new Set<string>()

  return props.items.map((node, index) => {
    const key = seen.has(node.id) ? `${node.id}#${index}` : node.id

    seen.add(node.id)

    return key
  })
})

/** Sortable's `group`: one canvas-wide group, and a `put` that asks the tree. */
const group = computed(() => ({
  name: 'jfss-canvas',
  pull: true,
  put: () => builder.canDropInto(props.listPath),
}))

/** The parts of a Sortable event this list reads. */
interface SortEvent {
  from: HTMLElement
  to: HTMLElement
  oldIndex?: number
  newIndex?: number
  oldDraggableIndex?: number
  newDraggableIndex?: number
}

function onStart(event: SortEvent): void {
  const index = event.oldDraggableIndex ?? event.oldIndex ?? -1
  const node = props.items[index]

  builder.dragging.value = node
    ? { kind: 'move', nodePath: `${props.listPath}.${index}`, node }
    : null
}

/** The drop handler: what a drag reports, as list paths and indexes. */
function onEnd(event: SortEvent): void {
  const fromListPath = event.from?.dataset.listPath
  const toListPath = event.to?.dataset.listPath

  if (!fromListPath || !toListPath) {
    builder.dragging.value = null

    return
  }

  builder.drop({
    fromListPath,
    oldIndex: event.oldDraggableIndex ?? event.oldIndex ?? 0,
    toListPath,
    newIndex: event.newDraggableIndex ?? event.newIndex ?? 0,
  })
}

function toggle(): void {
  adding.value = !adding.value

  if (adding.value) {
    void nextTick(() => chooser.value?.querySelector<HTMLElement>('button')?.focus())
  }
}

function close(): void {
  adding.value = false
  void nextTick(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-list-add]'))
      .find((element) => element.dataset.listAdd === props.listPath)
      ?.focus(),
  )
}

function add(type: string): void {
  adding.value = false
  builder.add(type, props.listPath)
}
</script>

<template>
  <div class="space-y-2">
    <VueDraggable
      :model-value="items"
      tag="ul"
      class="min-h-10 space-y-2 rounded-md border border-dashed border-border p-2"
      :data-list-path="listPath"
      :data-testid="`list-${listPath}`"
      :aria-label="`Components in ${name}`"
      :group="group"
      handle=".canvas-drag-handle"
      :disabled="builder.readOnly.value"
      :animation="150"
      :swap-threshold="0.65"
      :empty-insert-threshold="16"
      @start="onStart"
      @end="onEnd"
    >
      <li v-for="(node, index) in items" :key="keys[index]">
        <FormCanvasNode
          :node="node"
          :node-path="`${listPath}.${index}`"
          :index="index"
          :count="items.length"
          :list-kind="kind"
        />
      </li>
    </VueDraggable>

    <p v-if="items.length === 0" class="px-2 text-xs text-muted-foreground">Nothing here yet.</p>

    <div v-if="!builder.readOnly.value" class="space-y-2">
      <Button
        size="sm"
        variant="outline"
        :aria-expanded="adding"
        :aria-label="`Add component to ${name}`"
        :data-list-add="listPath"
        @click="toggle"
      >
        <Plus class="mr-1 size-4" aria-hidden="true" />
        Add component
      </Button>

      <div
        v-if="adding"
        ref="chooser"
        role="group"
        :aria-label="`Component types ${name} can take`"
        class="flex flex-wrap gap-1"
        @keydown.escape.stop="close"
      >
        <Button
          v-for="entry in choices"
          :key="entry.type"
          size="sm"
          variant="secondary"
          :aria-label="`Add ${entry.label} to ${name}`"
          :data-add-type="entry.type"
          @click="add(entry.type)"
        >
          {{ entry.label }}
        </Button>
        <Button size="sm" variant="ghost" @click="close">Cancel</Button>
      </div>
    </div>
  </div>
</template>
