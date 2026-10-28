<script setup lang="ts">
import { computed } from 'vue'

import { WORKFLOW_GRAPH_CEILING, type WorkflowGraphCount } from './workflowGraphMapping'

/**
 * What the *Graph* tab shows in place of a graph above the ceiling (#721).
 *
 * **First-load code, and that is the point**: the editor renders this instead
 * of reaching for `WorkflowGraph.vue`, so a declined draft fetches neither
 * dagre nor Vue Flow. It imports nothing but the count's module, which imports
 * neither.
 *
 * **One sentence, announced, not a heading**: `role="status"`, so a screen
 * reader hears it when the tab is opened. It gives the counts in the caption's
 * words and the ceiling from `WORKFLOW_GRAPH_CEILING`, and points to the list,
 * which reads and edits the same workflow.
 */
const props = defineProps<{ count: WorkflowGraphCount }>()

const states = computed(() => {
  const { states, undeclared } = props.count
  const said = `${states} ${states === 1 ? 'state' : 'states'}`

  return undeclared > 0 ? `${said} (${undeclared} not declared)` : said
})

const transitions = computed(() => {
  const { transitions } = props.count

  return `${transitions} ${transitions === 1 ? 'transition' : 'transitions'}`
})
</script>

<template>
  <p class="text-sm text-muted-foreground" role="status" data-testid="workflow-graph-declined">
    This workflow has {{ states }} and {{ transitions }}. The graph shows up to
    {{ WORKFLOW_GRAPH_CEILING.states }} states or {{ WORKFLOW_GRAPH_CEILING.transitions }}
    transitions, whichever is reached first, so it is not drawn. Use the List tab to read and change
    the workflow.
  </p>
</template>
