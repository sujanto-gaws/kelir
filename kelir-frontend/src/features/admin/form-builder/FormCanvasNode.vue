<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { ArrowDown, ArrowUp, GripVertical, Plus, Trash2 } from '@lucide/vue'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { JfssComponent } from '@/types/jfss'

import { offeredEntry, typeLabel } from './builderRegistry'
import FormCanvasList from './FormCanvasList.vue'
import { cardReason, nodeName, type ListKind } from './formTree'
import { useFormBuilder } from './useFormBuilder'

/**
 * One node on the canvas, and the lists inside it (#688 B5, C3, D1–D7).
 *
 * **Every drag has a keyboard equivalent on this card** (D5): *Move up*,
 * *Move down*, *Move to…* (which lists only the destinations containment
 * allows, named by their path), and *Remove*, which asks first when the node
 * holds anything (C3). Each control is named with the node's label or type
 * (D6), and none of them is drawn on a read-only canvas (D7).
 *
 * **A node the builder will not edit is an *unsupported* card** (B5): a type
 * it does not offer, a role that disagrees with the type, a shape the type
 * does not have, or a layout node inside a row template. The card says which
 * and why, can be moved and removed, cannot be selected for editing, and
 * draws nothing inside it — so what it holds is saved exactly as loaded.
 *
 * **Columns and tabs manage their slots here** (C3): add, remove (asking
 * first when the slot holds anything), and, for a tab, its title, which JFSS
 * requires and this card will not let go blank.
 */
const props = defineProps<{
  node: JfssComponent
  nodePath: string
  index: number
  count: number
  listKind: ListKind
}>()

const builder = useFormBuilder()

const name = computed(() => nodeName(props.node))
const reason = computed(() => cardReason(props.listKind, props.node))
const shape = computed(() =>
  reason.value ? 'none' : (offeredEntry(props.node.type)?.children ?? 'none'),
)
const selected = computed(() => !reason.value && builder.selectedId.value === props.node.id)
const messages = computed(() => builder.detailsFor(props.node.id))

const record = computed(() => props.node as unknown as Record<string, unknown>)

/** What the card is headed with: the node's own text, else its type's name. */
const heading = computed(() => {
  const own = [record.value.label, record.value.title, record.value.content].find(
    (value): value is string => typeof value === 'string' && value.trim() !== '',
  )

  return own ?? typeLabel(props.node.type)
})

const children = computed(() => (record.value.components as JfssComponent[] | undefined) ?? [])
const slots = computed(() =>
  shape.value === 'columns' || shape.value === 'tabs'
    ? (record.value[shape.value] as { title?: string; components: JfssComponent[] }[])
    : [],
)
const slotNoun = computed(() => (shape.value === 'tabs' ? 'tab' : 'column'))

// --- Move to… --------------------------------------------------------------------

const moving = ref(false)
const destinationsBox = ref<HTMLElement | null>(null)
const destinations = computed(() => (moving.value ? builder.destinations(props.nodePath) : []))

function toggleMove(): void {
  moving.value = !moving.value

  if (moving.value) {
    void nextTick(() => destinationsBox.value?.querySelector<HTMLElement>('button')?.focus())
  }
}

function closeMove(): void {
  moving.value = false
  void nextTick(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-node-id]'))
      .find((card) => card.dataset.nodeId === props.node.id)
      ?.querySelector<HTMLElement>('[data-node-control="move-to"]')
      ?.focus(),
  )
}

function moveTo(listPath: string): void {
  moving.value = false
  builder.move(props.nodePath, listPath, Number.MAX_SAFE_INTEGER, 'move-to')
}

// --- Tab titles ----------------------------------------------------------------------

const titleErrors = ref<Record<number, string>>({})

function renameTab(slot: number, title: string): void {
  if (builder.renameSlot(props.nodePath, slot, title)) {
    const rest = { ...titleErrors.value }

    delete rest[slot]
    titleErrors.value = rest
  } else {
    titleErrors.value = { ...titleErrors.value, [slot]: 'A tab needs a title.' }
  }
}
</script>

<template>
  <article
    :data-node-id="node.id"
    :data-testid="`component-${node.id}`"
    tabindex="-1"
    :aria-label="name"
    class="rounded-lg border bg-card p-3 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    :class="selected ? 'border-primary ring-1 ring-primary' : 'border-border'"
  >
    <div class="flex items-start gap-2">
      <span
        v-if="!builder.readOnly.value"
        class="canvas-drag-handle mt-1 cursor-grab text-muted-foreground"
        :title="`Drag ${name}`"
        :data-testid="`drag-${node.id}`"
        aria-hidden="true"
      >
        <GripVertical class="size-4" />
      </span>

      <button
        v-if="!reason"
        type="button"
        data-node-focus
        class="min-w-0 flex-1 rounded text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        :aria-pressed="selected"
        :aria-label="`Select ${name}`"
        :data-testid="`select-${node.id}`"
        @click="builder.select(node.id)"
      >
        <span class="block truncate text-sm font-medium">{{ heading }}</span>
        <span class="text-xs text-muted-foreground">{{ node.role }} · {{ node.type }}</span>
      </button>

      <div v-else class="min-w-0 flex-1" :data-testid="`unsupported-${node.id}`">
        <p class="text-sm font-medium">Unsupported · {{ node.role }} · {{ node.type }}</p>
        <p class="text-xs text-muted-foreground">
          {{ reason }} It is kept exactly as it was loaded.
        </p>
      </div>

      <div v-if="!builder.readOnly.value" class="flex shrink-0 flex-wrap items-center gap-1">
        <Button
          size="sm"
          variant="ghost"
          class="px-2"
          data-node-control="up"
          :aria-label="`Move ${name} up`"
          :disabled="index === 0"
          :data-testid="`up-${node.id}`"
          @click="builder.moveBy(nodePath, -1)"
        >
          <ArrowUp class="size-4" aria-hidden="true" />
        </Button>
        <Button
          size="sm"
          variant="ghost"
          class="px-2"
          data-node-control="down"
          :aria-label="`Move ${name} down`"
          :disabled="index === count - 1"
          :data-testid="`down-${node.id}`"
          @click="builder.moveBy(nodePath, 1)"
        >
          <ArrowDown class="size-4" aria-hidden="true" />
        </Button>
        <Button
          size="sm"
          variant="ghost"
          class="px-2"
          data-node-control="move-to"
          :aria-label="`Move ${name} to…`"
          :aria-expanded="moving"
          :data-testid="`move-to-${node.id}`"
          @click="toggleMove"
        >
          Move to…
        </Button>
        <Button
          size="sm"
          variant="ghost"
          class="px-2 text-destructive"
          data-node-control="remove"
          :aria-label="`Remove ${name}`"
          :data-testid="`remove-${node.id}`"
          @click="builder.requestRemove(nodePath)"
        >
          <Trash2 class="size-4" aria-hidden="true" />
        </Button>
      </div>
    </div>

    <!-- Everything the server said about this node (E6). -->
    <ul
      v-if="messages.length > 0"
      class="mt-2 space-y-1"
      :data-testid="`component-errors-${node.id}`"
    >
      <li v-for="(detail, at) in messages" :key="at" class="text-xs text-destructive">
        {{ detail.message }}
      </li>
    </ul>

    <div
      v-if="moving"
      ref="destinationsBox"
      role="group"
      :aria-label="`Where to move ${name}`"
      class="mt-2 flex flex-wrap gap-1 rounded-md border border-border p-2"
      @keydown.escape.stop="closeMove"
    >
      <p v-if="destinations.length === 0" class="text-xs text-muted-foreground">
        Nowhere else on this form can take it.
      </p>
      <Button
        v-for="destination in destinations"
        :key="destination.listPath"
        size="sm"
        variant="outline"
        :aria-label="`Move ${name} to ${destination.label}`"
        :data-destination="destination.listPath"
        @click="moveTo(destination.listPath)"
      >
        {{ destination.label }}
      </Button>
      <Button size="sm" variant="ghost" @click="closeMove">Cancel</Button>
    </div>

    <!-- A panel's or fieldset's children, or a data grid's row template. -->
    <div v-if="shape === 'components' || shape === 'rowTemplate'" class="mt-3">
      <p v-if="shape === 'rowTemplate'" class="mb-1 text-xs font-medium text-muted-foreground">
        Row template: data and display components, repeated per row
      </p>
      <FormCanvasList
        :list-path="`${nodePath}.components`"
        :items="children"
        :kind="shape === 'rowTemplate' ? 'rowTemplate' : 'components'"
      />
    </div>

    <!-- Columns and tabs: one list per slot. -->
    <div v-if="shape === 'columns' || shape === 'tabs'" class="mt-3 space-y-3">
      <div :class="shape === 'columns' ? 'grid gap-3 md:grid-cols-2' : 'space-y-3'">
        <section
          v-for="(slot, slotIndex) in slots"
          :key="slotIndex"
          class="space-y-2 rounded-md bg-muted/40 p-2"
          :aria-label="`${slotNoun === 'tab' ? 'Tab' : 'Column'} ${slotIndex + 1} of ${name}`"
        >
          <div class="flex items-end gap-2">
            <p v-if="shape === 'columns'" class="flex-1 text-xs font-medium">
              Column {{ slotIndex + 1 }}
            </p>
            <div v-else class="flex-1 space-y-1">
              <label :for="`tab-title-${node.id}-${slotIndex}`" class="text-xs font-medium">
                Title of tab {{ slotIndex + 1 }}
              </label>
              <Input
                :id="`tab-title-${node.id}-${slotIndex}`"
                :model-value="slot.title ?? ''"
                :disabled="builder.readOnly.value"
                :invalid="Boolean(titleErrors[slotIndex])"
                :aria-label="`Title of tab ${slotIndex + 1} of ${name}`"
                :data-testid="`tab-title-${node.id}-${slotIndex}`"
                @update:model-value="renameTab(slotIndex, String($event))"
              />
              <p v-if="titleErrors[slotIndex]" class="text-xs text-destructive">
                {{ titleErrors[slotIndex] }}
              </p>
            </div>
            <Button
              v-if="!builder.readOnly.value"
              size="sm"
              variant="ghost"
              class="px-2 text-destructive"
              :aria-label="`Remove ${slotNoun} ${slotIndex + 1} of ${name}`"
              :disabled="slots.length <= 1"
              :data-testid="`remove-slot-${node.id}-${slotIndex}`"
              @click="builder.requestRemoveSlot(nodePath, slotIndex)"
            >
              <Trash2 class="size-4" aria-hidden="true" />
            </Button>
          </div>

          <FormCanvasList
            :list-path="`${nodePath}.${shape}.${slotIndex}.components`"
            :items="slot.components"
            :kind="shape === 'columns' ? 'column' : 'tab'"
          />
        </section>
      </div>

      <Button
        v-if="!builder.readOnly.value"
        size="sm"
        variant="outline"
        :aria-label="`Add a ${slotNoun} to ${name}`"
        :data-testid="`add-slot-${node.id}`"
        @click="builder.addSlot(nodePath)"
      >
        <Plus class="mr-1 size-4" aria-hidden="true" />
        Add {{ slotNoun }}
      </Button>
    </div>
  </article>
</template>
