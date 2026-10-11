<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ArrowDown, ArrowUp, Plus, Trash2 } from '@lucide/vue'

import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { DocumentStatus } from '@/types/document'
import type { AssignmentRule, JwssState, JwssTask } from '@/types/workflow'

import AssignmentRuleEditor from './AssignmentRuleEditor.vue'
import { drawnRuleFields } from './assignmentRule'
import { useWorkflowEditorContext } from './editorContext'
import {
  DOCUMENT_STATUS_LABELS,
  optionsOf,
  TASK_PRIORITIES,
  TASK_TYPES,
  taskKeyFor,
} from './jwssRegistry'
import WorkflowTransitionRow from './WorkflowTransitionRow.vue'
import { rowErrors } from './workflowVerdict'

/**
 * One state, with the table of transitions out of it under it (JWSS §3, §4;
 * #426 AC1, AC3).
 *
 * **Every message under this state's path is shown on this card**: under the
 * input it names, or listed at its top when it names none the card draws. That
 * includes S6's *cannot be reached* and *no final state is reachable*, which
 * the server reports for the whole list and names this state in, and which
 * `workflowVerdict.placeDetails` moves here.
 *
 * **The code is renamed on commit**, not per keystroke: the initial state and
 * every transition naming the old code follow it, and a rename that passed
 * through another state's code on the way would have taken that state's
 * transitions with it.
 */
const props = defineProps<{
  /** Its position in `states`, which is how the server addresses it. */
  index: number
}>()

const editor = useWorkflowEditorContext()

const definition = computed(() => editor.draft.definition.value)
const state = computed(() => definition.value.states[props.index])
const rowKey = computed(() => editor.draft.current.value.stateKeys[props.index])
const path = computed(() => `definition.states.${props.index}`)
const id = computed(() => `state-${rowKey.value}`)
const label = computed(() => `State ${state.value.code || props.index + 1}`)
const isInitial = computed(() => definition.value.initialState === state.value.code)

const errors = computed(() => rowErrors(editor.fieldErrors.value, 'state', props.index))

const drawn = computed(() => {
  const fields = ['code', 'name', 'mapsToDocumentStatus', 'isFinal', 'task']

  if (state.value.task) {
    fields.push(
      'task.taskDefinitionKey',
      'task.taskName',
      'task.taskType',
      'task.dueInHours',
      'task.priority',
      ...drawnRuleFields(state.value.task.assignment).map((field) =>
        field === '' ? 'task.assignment' : `task.assignment.${field}`,
      ),
    )
  }

  return new Set(fields)
})

const stateMessages = computed(() =>
  Object.entries(errors.value)
    .filter(([field]) => !drawn.value.has(field))
    .map(([field, message]) => ({ field, message })),
)

const assignmentErrors = computed(() => {
  const found: Record<string, string> = {}

  for (const [field, message] of Object.entries(errors.value)) {
    if (field === 'task.assignment') {
      found[''] = message
    } else if (field.startsWith('task.assignment.')) {
      found[field.slice('task.assignment.'.length)] = message
    }
  }

  return found
})

/** The transitions out of this state, by their position in `transitions`. */
const outgoing = computed(() =>
  definition.value.transitions
    .map((transition, at) => ({
      transition,
      at,
      key: editor.draft.current.value.transitionKeys[at],
    }))
    .filter(({ transition }) => transition.from === state.value.code),
)

/**
 * Whether unticking the task would drop more than the task's name and
 * assignment: an escalation, which has no field, or a due time or priority,
 * which are easy to miss. Said beside the checkbox, so it is not silent.
 */
const carriesUnshown = computed(() => {
  const task = state.value.task

  return (
    !editor.readOnly.value &&
    task !== undefined &&
    (task.escalation !== undefined || task.dueInHours !== undefined || task.priority !== undefined)
  )
})

const statusOptions = (Object.keys(DOCUMENT_STATUS_LABELS) as DocumentStatus[]).map((value) => ({
  value,
  label: DOCUMENT_STATUS_LABELS[value],
}))
const taskTypeOptions = optionsOf(TASK_TYPES)
const priorityOptions = optionsOf(TASK_PRIORITIES)

const codeText = ref(state.value.code)

watch(
  () => state.value.code,
  (code) => {
    codeText.value = code
  },
)

function change(next: JwssState, field: string): void {
  editor.draft.replaceState(props.index, next, `${rowKey.value}:${field}`)
  editor.clearField(`${path.value}.${field}`)
}

function commitCode(): void {
  if (codeText.value === state.value.code) {
    return
  }

  editor.draft.renameState(props.index, codeText.value)
  editor.clearField(`${path.value}.code`)
}

function setFinal(isFinal: boolean): void {
  const next: JwssState = { ...state.value, isFinal }

  if (!isFinal) {
    delete next.isFinal
  }

  change(next, 'isFinal')
}

function setHasTask(hasTask: boolean): void {
  const next = { ...state.value }

  if (hasTask) {
    next.task = {
      taskDefinitionKey: taskKeyFor(state.value.code),
      taskName: state.value.name,
      assignment: { assigneeType: 'ROLE', roleCode: '' },
    }
  } else {
    delete next.task
  }

  change(next, 'task')
}

function changeTask(task: JwssTask, field: string): void {
  change({ ...state.value, task }, `task.${field}`)
}

function setTaskText(field: 'taskDefinitionKey' | 'taskName', value: string): void {
  if (state.value.task) {
    changeTask({ ...state.value.task, [field]: value }, field)
  }
}

/** An optional choice left at its default is left out, which is the same task. */
function setTaskChoice(field: 'taskType' | 'priority', value: string): void {
  if (!state.value.task) {
    return
  }

  const task = { ...state.value.task, [field]: value } as JwssTask

  if (value === '') {
    delete task[field]
  }

  changeTask(task, field)
}

/**
 * A number when the text is one, and the text itself when it is not, so the
 * server refuses it at this field rather than the editor dropping it silently.
 */
function setDueInHours(text: string): void {
  if (!state.value.task) {
    return
  }

  // Typed wider than JWSS for this one write: text that is not a number is
  // sent as typed, and the meta-schema's `type: number` refuses it here.
  const task: Omit<JwssTask, 'dueInHours'> & { dueInHours?: number | string } = {
    ...state.value.task,
  }
  const trimmed = text.trim()

  if (trimmed === '') {
    delete task.dueInHours
  } else {
    task.dueInHours = Number.isFinite(Number(trimmed)) ? Number(trimmed) : trimmed
  }

  changeTask(task as JwssTask, 'dueInHours')
}

function setAssignment(rule: AssignmentRule | undefined, field: string): void {
  if (!state.value.task || rule === undefined) {
    return
  }

  editor.draft.replaceState(
    props.index,
    { ...state.value, task: { ...state.value.task, assignment: rule } },
    `${rowKey.value}:task.assignment.${field}`,
  )
  editor.clearField(`${path.value}.task.assignment`)
  editor.clearField(`${path.value}.task.assignment.${field}`)
}

function makeInitial(): void {
  editor.draft.setRoot('initialState', state.value.code)
  editor.clearField('definition.initialState')
}

function addTransition(): void {
  editor.draft.addTransition(state.value.code)
  editor.resetErrors()
}

function remove(): void {
  editor.draft.removeState(props.index)
  editor.resetErrors()
}

function move(direction: -1 | 1): void {
  editor.draft.moveState(props.index, direction)
  editor.resetErrors()
}
</script>

<template>
  <section
    class="space-y-4 rounded-lg border p-4"
    :aria-label="label"
    :data-testid="`state-${index}`"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex items-center gap-2">
        <h4 class="font-mono text-sm font-semibold">{{ state.code || '(no code)' }}</h4>
        <Badge v-if="isInitial" variant="secondary">Initial</Badge>
        <Badge v-if="state.isFinal" variant="outline">Final</Badge>
      </div>
      <div class="flex items-center gap-1">
        <Button
          v-if="!isInitial"
          variant="ghost"
          size="sm"
          :disabled="editor.readOnly.value"
          :data-testid="`make-initial-${index}`"
          @click="makeInitial"
        >
          Make initial
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :aria-label="`Move ${label} up`"
          :disabled="editor.readOnly.value || index === 0"
          @click="move(-1)"
        >
          <ArrowUp class="size-4" aria-hidden="true" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :aria-label="`Move ${label} down`"
          :disabled="editor.readOnly.value || index === definition.states.length - 1"
          @click="move(1)"
        >
          <ArrowDown class="size-4" aria-hidden="true" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          :aria-label="`Remove ${label}`"
          :disabled="editor.readOnly.value"
          :data-testid="`remove-state-${index}`"
          @click="remove"
        >
          <Trash2 class="size-4" aria-hidden="true" />
        </Button>
      </div>
    </div>

    <Alert
      v-if="stateMessages.length > 0"
      variant="destructive"
      :data-testid="`state-${index}-messages`"
    >
      <ul class="list-disc pl-4">
        <li v-for="item in stateMessages" :key="item.field">{{ item.message }}</li>
      </ul>
    </Alert>

    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <div class="space-y-1">
        <Label :for="`${id}-code`">Code</Label>
        <Input
          :id="`${id}-code`"
          class="font-mono"
          :model-value="codeText"
          :invalid="Boolean(errors.code)"
          :described-by="errors.code ? `${id}-code-message` : undefined"
          :disabled="editor.readOnly.value"
          :data-testid="`state-code-${index}`"
          @update:model-value="codeText = $event"
          @change="commitCode"
        />
        <p v-if="errors.code" :id="`${id}-code-message`" class="text-xs text-destructive">
          {{ errors.code }}
        </p>
      </div>

      <div class="space-y-1">
        <Label :for="`${id}-name`">Name</Label>
        <Input
          :id="`${id}-name`"
          :model-value="state.name"
          :invalid="Boolean(errors.name)"
          :described-by="errors.name ? `${id}-name-message` : undefined"
          :disabled="editor.readOnly.value"
          :data-testid="`state-name-${index}`"
          @update:model-value="change({ ...state, name: $event }, 'name')"
        />
        <p v-if="errors.name" :id="`${id}-name-message`" class="text-xs text-destructive">
          {{ errors.name }}
        </p>
      </div>

      <div class="space-y-1">
        <Label :for="`${id}-status`">Document status</Label>
        <Select
          :id="`${id}-status`"
          :model-value="state.mapsToDocumentStatus"
          :options="statusOptions"
          :invalid="Boolean(errors.mapsToDocumentStatus)"
          :described-by="errors.mapsToDocumentStatus ? `${id}-status-message` : undefined"
          :disabled="editor.readOnly.value"
          @update:model-value="
            change(
              { ...state, mapsToDocumentStatus: $event as DocumentStatus },
              'mapsToDocumentStatus',
            )
          "
        />
        <p
          v-if="errors.mapsToDocumentStatus"
          :id="`${id}-status-message`"
          class="text-xs text-destructive"
        >
          {{ errors.mapsToDocumentStatus }}
        </p>
      </div>

      <div class="space-y-1 self-end pb-2">
        <label class="flex items-center gap-2 text-sm" :for="`${id}-final`">
          <Checkbox
            :id="`${id}-final`"
            :model-value="state.isFinal === true"
            :aria-invalid="Boolean(errors.isFinal)"
            :disabled="editor.readOnly.value"
            @update:model-value="setFinal"
          />
          Final — ends the workflow
        </label>
        <p v-if="errors.isFinal" class="text-xs text-destructive">{{ errors.isFinal }}</p>
      </div>
    </div>

    <div class="space-y-3 rounded-md bg-muted/40 p-3">
      <label class="flex items-center gap-2 text-sm font-medium" :for="`${id}-has-task`">
        <Checkbox
          :id="`${id}-has-task`"
          :model-value="state.task !== undefined"
          :aria-invalid="Boolean(errors.task)"
          :disabled="editor.readOnly.value"
          :data-testid="`state-has-task-${index}`"
          @update:model-value="setHasTask"
        />
        Entering this state creates a task
      </label>
      <p v-if="errors.task" class="text-xs text-destructive">{{ errors.task }}</p>
      <p
        v-if="carriesUnshown"
        class="text-xs text-muted-foreground"
        :data-testid="`state-task-unshown-${index}`"
      >
        Unticking this also removes the stored escalation, due hours and priority.
        <em>Undo</em> restores them.
      </p>

      <div v-if="state.task" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <div class="space-y-1">
          <Label :for="`${id}-task-name`">Task name</Label>
          <Input
            :id="`${id}-task-name`"
            :model-value="state.task.taskName"
            :invalid="Boolean(errors['task.taskName'])"
            :disabled="editor.readOnly.value"
            @update:model-value="setTaskText('taskName', $event)"
          />
          <p v-if="errors['task.taskName']" class="text-xs text-destructive">
            {{ errors['task.taskName'] }}
          </p>
        </div>

        <div class="space-y-1">
          <Label :for="`${id}-task-key`">Task key</Label>
          <Input
            :id="`${id}-task-key`"
            class="font-mono"
            :model-value="state.task.taskDefinitionKey"
            :invalid="Boolean(errors['task.taskDefinitionKey'])"
            :disabled="editor.readOnly.value"
            @update:model-value="setTaskText('taskDefinitionKey', $event)"
          />
          <p v-if="errors['task.taskDefinitionKey']" class="text-xs text-destructive">
            {{ errors['task.taskDefinitionKey'] }}
          </p>
        </div>

        <div class="space-y-1">
          <Label :for="`${id}-task-type`">Task type</Label>
          <Select
            :id="`${id}-task-type`"
            :model-value="state.task.taskType ?? ''"
            :options="taskTypeOptions"
            placeholder="Approval (the default)"
            :invalid="Boolean(errors['task.taskType'])"
            :disabled="editor.readOnly.value"
            @update:model-value="setTaskChoice('taskType', $event)"
          />
          <p v-if="errors['task.taskType']" class="text-xs text-destructive">
            {{ errors['task.taskType'] }}
          </p>
        </div>

        <div class="space-y-1">
          <Label :for="`${id}-assignment-type`">Assigned to</Label>
          <AssignmentRuleEditor
            :id="`${id}-assignment`"
            :model-value="state.task.assignment"
            :label="`${label} task assigned to`"
            :errors="assignmentErrors"
            :variables="editor.variables.value"
            :disabled="editor.readOnly.value"
            @change="setAssignment"
            @validity-changed="editor.setValidity(`${rowKey}:assignment`, $event)"
          />
        </div>

        <div class="space-y-1">
          <Label :for="`${id}-due`">Due in hours</Label>
          <Input
            :id="`${id}-due`"
            inputmode="decimal"
            :model-value="state.task.dueInHours === undefined ? '' : String(state.task.dueInHours)"
            :invalid="Boolean(errors['task.dueInHours'])"
            :disabled="editor.readOnly.value"
            @update:model-value="setDueInHours"
          />
          <p v-if="errors['task.dueInHours']" class="text-xs text-destructive">
            {{ errors['task.dueInHours'] }}
          </p>
        </div>

        <div class="space-y-1">
          <Label :for="`${id}-priority`">Priority</Label>
          <Select
            :id="`${id}-priority`"
            :model-value="state.task.priority ?? ''"
            :options="priorityOptions"
            placeholder="Normal (the default)"
            :invalid="Boolean(errors['task.priority'])"
            :disabled="editor.readOnly.value"
            @update:model-value="setTaskChoice('priority', $event)"
          />
          <p v-if="errors['task.priority']" class="text-xs text-destructive">
            {{ errors['task.priority'] }}
          </p>
        </div>
      </div>

      <!-- FR-WF-010 is unscheduled, so there is no field for an escalation;
           one already stored is kept as it is and said to be inert. -->
      <p v-if="state.task?.escalation" class="text-xs text-muted-foreground">
        This task declares an escalation, which is kept as stored. Nothing executes it: escalation
        is not scheduled (JWSS §3.1).
      </p>
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <h5 class="text-sm font-semibold">Transitions out of {{ state.code || 'this state' }}</h5>
        <Button
          v-if="!state.isFinal"
          size="sm"
          variant="secondary"
          :disabled="editor.readOnly.value"
          :data-testid="`add-transition-${index}`"
          @click="addTransition"
        >
          <Plus class="mr-1 size-4" aria-hidden="true" />
          Add transition
        </Button>
      </div>

      <Table v-if="outgoing.length > 0">
        <TableHeader>
          <TableRow>
            <TableHead>Action</TableHead>
            <TableHead>To</TableHead>
            <TableHead>Allowed by</TableHead>
            <TableHead>Comment</TableHead>
            <TableHead class="text-right">Order</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <WorkflowTransitionRow v-for="row in outgoing" :key="row.key" :index="row.at" />
        </TableBody>
      </Table>

      <p v-else class="text-sm text-muted-foreground">
        {{
          state.isFinal
            ? 'A final state has no transitions out of it.'
            : 'No transitions. A document that reaches this state cannot leave it.'
        }}
      </p>
    </div>
  </section>
</template>
