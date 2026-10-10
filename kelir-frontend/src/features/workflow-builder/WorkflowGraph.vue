<script setup lang="ts">
import { computed, nextTick, useId, watch } from 'vue'
import {
  BaseEdge,
  Handle,
  MarkerType,
  Position,
  VueFlow,
  useVueFlow,
  type Edge,
  type Node,
} from '@vue-flow/core'
import { ControlButton, Controls } from '@vue-flow/controls'
import { Maximize, Minus, Plus } from 'lucide-vue-next'
import '@vue-flow/core/dist/style.css'
import '@vue-flow/controls/dist/style.css'

import { cn } from '@/lib/utils'
import type { JwssDefinition } from '@/types/workflow'

import {
  workflowGraphOf,
  type WorkflowGraphEdge,
  type WorkflowGraphNode,
} from './workflowGraphMapping'
import {
  layoutWorkflowGraph,
  NODE_HEIGHT,
  NODE_WIDTH,
  pathThrough,
  type EdgeRoute,
} from './workflowGraphLayout'

/**
 * A read-only graph of one workflow definition (#687, **D-95** A).
 *
 * **A view of the draft, never an editor of it.** It takes the definition as a
 * prop and emits nothing: dragging, connecting and selecting are off, so it
 * cannot be mistaken for a canvas, and nothing it computes — above all, no
 * position — goes back into the definition. The list beside it is where a
 * workflow is edited, and the list is this picture's text alternative.
 *
 * **Laid out on every load**, by `workflowGraphLayout`, and again whenever the
 * definition changes: an undo, a redo, or the server's copy after a save. No
 * layout is stored, so `jwss-meta-v1.0.0.json` is unchanged.
 *
 * **Off the first-load path** ([ADR-0046]): this component is the only module
 * that imports `@vue-flow/*` and `@dagrejs/dagre`, and the editor reaches it
 * through `defineAsyncComponent`. `npm run check:bundle` fails the build when
 * either library, or Vue Flow's stylesheets, reach a first-load chunk.
 *
 * **Zoom and fit by keyboard**: the controls are buttons with names, so Tab
 * reaches them and Enter presses them. A pointer pans and zooms as Vue Flow
 * allows.
 *
 * **Named, and not operable, to a screen reader**: each state is named with
 * its marks and each transition by what its edge draws, and Vue Flow's keyboard
 * help (select, move with the arrows, delete) is off, since none of it is true
 * here.
 *
 * [ADR-0046]: ../../../../docs/architectures/adr/0046.%20The%20Builders%20Drag%20with%20Vue%20Draggable%20Plus%20and%20Draw%20with%20Vue%20Flow%20and%20Dagre,%20Off%20the%20First-Load%20Path.md
 */
const props = defineProps<{ definition: JwssDefinition }>()

const flowId = `workflow-graph-${useId()}`
const { fitView, zoomIn, zoomOut } = useVueFlow(flowId)

const graph = computed(() => workflowGraphOf(props.definition))
const layout = computed(() => layoutWorkflowGraph(graph.value))

const nodes = computed<Node<WorkflowGraphNode>[]>(() =>
  graph.value.nodes.map((node) => ({
    id: node.id,
    type: 'state',
    position: layout.value.nodes.get(node.id) ?? { x: 0, y: 0 },
    data: node,
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
    ariaLabel: nodeLabel(node),
  })),
)

/** An edge as drawn: the transition, and dagre's route for it. */
type RoutedEdge = WorkflowGraphEdge & { route: EdgeRoute }

const edges = computed<Edge<RoutedEdge>[]>(() => {
  const names = new Map(graph.value.nodes.map((node) => [node.id, node.name]))

  return graph.value.edges.map((edge) => ({
    id: edge.id,
    type: 'transition',
    source: edge.source,
    target: edge.target,
    markerEnd: MarkerType.ArrowClosed,
    class: cn('workflow-graph-edge', edge.conditional && 'workflow-graph-edge--conditional'),
    ariaLabel: edgeLabel(edge, names),
    data: {
      ...edge,
      route: layout.value.edges.get(edge.id) ?? { points: [], label: { x: 0, y: 0 } },
    },
  }))
})

const summary = computed(() => {
  const states = graph.value.nodes.filter((node) => node.declared).length
  const undeclared = graph.value.nodes.length - states
  const transitions = graph.value.edges.length

  return [
    `${states} ${states === 1 ? 'state' : 'states'}`,
    `${transitions} ${transitions === 1 ? 'transition' : 'transitions'}`,
    undeclared > 0 ? `${undeclared} not declared` : null,
  ]
    .filter(Boolean)
    .join(', ')
})

function nodeLabel(node: WorkflowGraphNode): string {
  const marks = [
    node.initial ? 'initial state' : null,
    node.final ? 'final state' : null,
    node.declared ? null : 'not declared by any state',
  ].filter(Boolean)

  return [`${node.name} (${node.code})`, ...marks].join(', ')
}

/**
 * A transition in words: what its edge draws, by the names its states show.
 * Without it Vue Flow names an edge by the graph's internal ids, and the
 * edge's `role="img"` hides the drawn label from assistive technology.
 */
function edgeLabel(edge: WorkflowGraphEdge, names: Map<string, string>): string {
  const marker = edge.conditional
    ? 'if a condition holds'
    : edge.fallback
      ? 'otherwise, when no condition holds'
      : null

  return [edge.actionLabel, `from ${names.get(edge.source)} to ${names.get(edge.target)}`, marker]
    .filter(Boolean)
    .join(', ')
}

function fit(): void {
  void fitView({ padding: 0.15 })
}

// Re-laid out on every change of the definition; fitted again once drawn.
watch(nodes, async () => {
  await nextTick()
  fit()
})
</script>

<template>
  <figure class="space-y-2" data-testid="workflow-graph">
    <figcaption class="text-sm text-muted-foreground" data-testid="workflow-graph-summary">
      {{ summary }}. Read-only: change the workflow in the list.
    </figcaption>

    <div class="h-[32rem] rounded-md border bg-card">
      <VueFlow
        :id="flowId"
        :nodes="nodes"
        :edges="edges"
        :nodes-draggable="false"
        :nodes-connectable="false"
        :elements-selectable="false"
        :edges-updatable="false"
        :nodes-focusable="false"
        :edges-focusable="false"
        :zoom-on-double-click="false"
        :delete-key-code="null"
        disable-keyboard-a11y
        :min-zoom="0.2"
        :max-zoom="2"
        fit-view-on-init
        @nodes-initialized="fit"
      >
        <template #node-state="{ data }">
          <div
            :class="
              cn(
                'flex h-full flex-col justify-center rounded-md border-2 bg-background px-3 py-1 text-left',
                data.initial && 'border-primary',
                data.final && 'border-double border-4',
                !data.declared && 'border-dashed border-destructive',
              )
            "
            :style="{ width: `${NODE_WIDTH}px`, height: `${NODE_HEIGHT}px` }"
            :data-testid="`graph-node-${data.code}`"
          >
            <Handle type="target" :position="Position.Top" :connectable="false" />
            <div class="flex items-center gap-1">
              <span class="truncate text-sm font-medium">{{ data.name }}</span>
              <span
                v-if="data.initial"
                class="rounded bg-primary px-1 text-[10px] font-semibold text-primary-foreground uppercase"
              >
                Start
              </span>
              <span
                v-if="data.final"
                class="rounded bg-secondary px-1 text-[10px] font-semibold text-secondary-foreground uppercase"
              >
                End
              </span>
            </div>
            <span class="truncate font-mono text-xs text-muted-foreground">
              {{ data.declared ? data.code : `${data.code || '(blank)'} · not declared` }}
            </span>
            <span v-if="data.taskName" class="truncate text-xs text-muted-foreground">
              Task: {{ data.taskName }}
            </span>
            <Handle type="source" :position="Position.Bottom" :connectable="false" />
          </div>
        </template>

        <template #edge-transition="{ id, data, markerEnd }">
          <BaseEdge
            :id="id"
            :path="pathThrough(data.route.points)"
            :marker-end="markerEnd"
            :label="data.label"
            :label-x="data.route.label.x"
            :label-y="data.route.label.y"
            label-show-bg
            :label-bg-padding="[4, 2]"
            :label-bg-border-radius="4"
          />
        </template>

        <Controls :show-zoom="false" :show-fit-view="false" :show-interactive="false">
          <ControlButton
            aria-label="Zoom in"
            title="Zoom in"
            data-testid="graph-zoom-in"
            @click="zoomIn()"
          >
            <Plus aria-hidden="true" />
          </ControlButton>
          <ControlButton
            aria-label="Zoom out"
            title="Zoom out"
            data-testid="graph-zoom-out"
            @click="zoomOut()"
          >
            <Minus aria-hidden="true" />
          </ControlButton>
          <ControlButton
            aria-label="Fit the workflow to the view"
            title="Fit to view"
            data-testid="graph-fit"
            @click="fit"
          >
            <Maximize aria-hidden="true" />
          </ControlButton>
        </Controls>
      </VueFlow>
    </div>
  </figure>
</template>

<style scoped>
/* The handles exist so edges have ends; nothing connects to them. */
:deep(.vue-flow__handle) {
  opacity: 0;
  pointer-events: none;
}

:deep(.vue-flow__edge-path) {
  stroke: var(--color-muted-foreground);
  stroke-width: 1.5;
}

/* A transition with a condition is dashed, beside its label's marker. */
:deep(.workflow-graph-edge--conditional .vue-flow__edge-path) {
  stroke-dasharray: 6 4;
}

:deep(.vue-flow__edge-text) {
  fill: var(--color-foreground);
  font-size: 11px;
}

:deep(.vue-flow__edge-textbg) {
  fill: var(--color-background);
}

:deep(.vue-flow__node-state) {
  padding: 0;
  border: 0;
  background: transparent;
}
</style>
