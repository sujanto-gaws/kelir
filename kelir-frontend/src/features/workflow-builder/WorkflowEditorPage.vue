<script setup lang="ts">
import { computed, onMounted, provide, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Plus, Redo2, Undo2 } from '@lucide/vue'

import { toApiError } from '@/api/client'
import { ApiError } from '@/api/error'
import {
  createWorkflowDefinition,
  createWorkflowRevision,
  deprecateWorkflowDefinition,
  getWorkflowDefinition,
  publishWorkflowDefinition,
  updateWorkflowDefinition,
} from '@/api/workflow'
import FormUnplacedErrors from '@/components/FormUnplacedErrors.vue'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import { useFormErrors } from '@/composables/useFormErrors'
import { useAuthStore } from '@/stores/auth'
import type { WorkflowDefinition } from '@/types/workflow'

import { WORKFLOW_EDITOR } from './editorContext'
import { conditionVariables, STATE_KINDS, starterDefinition, type StateKind } from './jwssRegistry'
import { useWorkflowDeprecation } from './useWorkflowDeprecation'
import { useWorkflowDraft } from './useWorkflowDraft'
import WorkflowDeprecateDialog from './WorkflowDeprecateDialog.vue'
import WorkflowStateCard from './WorkflowStateCard.vue'
import WorkflowTransitionRow from './WorkflowTransitionRow.vue'
import { addressOf, placeVerdict } from './workflowVerdict'

/**
 * The workflow editor (FR-WF-018; #426 AC1–AC5, **D-95** A).
 *
 * **A state list with a transition table under each state.** That is the
 * editor D-95 chose; a read-only graph of the same definition is #687's.
 *
 * **What this screen holds:**
 *
 * 1. **The server's verdict is the only verdict** (AC3). A save is checked
 *    against the meta-schema, the operator tier and JWSS §8, and a publish
 *    again with the tenant's roles (ADR-0019, **D-111**). Every detail comes
 *    back addressed by position in the body; `workflowVerdict` places it on
 *    the state or transition it names, `useFormErrors` holds it, and
 *    `FormUnplacedErrors` lists what nothing on the screen draws (coding
 *    standard §3.4). Nothing here checks a definition itself.
 * 2. **A published revision is not edited in place** (AC5). `ACTIVE` and
 *    `DEPRECATED` open read-only and say why, and the way forward is a new
 *    revision. ~~*Deprecate* joins it after #573's route merges (plan 19 §4
 *    row 6): see the comment in the header's actions.~~ *Deprecate* sits
 *    beside it on an `ACTIVE` revision since 2026-10-10 (#713, plan 19 row 6b,
 *    **D-101** B, **D-108**), and warns first, naming the document types still
 *    bound to the revision (`useWorkflowDeprecation`).
 * 3. **What it writes is JWSS v1.0.0** (AC2): it edits the document in place
 *    of a model of its own, so what it does not edit — guards, actions,
 *    variables, settings, an escalation — goes back as it came.
 */
const auth = useAuthStore()
const route = useRoute()
const router = useRouter()

const isNew = computed(() => route.name === 'admin-workflow-new')

const canCreate = computed(() => auth.can('workflow:definition:create'))
const canUpdate = computed(() => auth.can('workflow:definition:update'))
const canPublish = computed(() => auth.can('workflow:definition:publish'))
const canDeprecate = computed(() => auth.can('workflow:definition:deprecate'))

/** The revision as the server holds it, or `null` for a workflow not yet saved. */
const record = ref<WorkflowDefinition | null>(null)
const draft = useWorkflowDraft(starterDefinition())
const definition = draft.definition

// Loading from the start for an existing revision, so the starter definition
// is never drawn for a moment in its place.
const isLoading = ref(!isNew.value)
const isSaving = ref(false)
const loadError = ref('')
const notice = ref('')

const { fieldErrors, formError, report, reset, clearField } = useFormErrors()

const status = computed(() => record.value?.status ?? 'DRAFT')
const isPublished = computed(() => status.value !== 'DRAFT')
const canEdit = computed(() => (isNew.value ? canCreate.value : canUpdate.value))
/** Whether this caller may change this revision at all: a draft, and the right grant. */
const editable = computed(() => !isPublished.value && canEdit.value)
/**
 * Whether the inputs take edits now. **Not while a save or publish is in
 * flight**: its reply replaces the draft with what the server stored, and its
 * refusal addresses rows by their position in what was sent, so an edit made
 * in the gap would be lost or would move the refusal onto the wrong row.
 */
const readOnly = computed(() => !editable.value || isSaving.value)
/**
 * Whether *Publish* is offered. **`workflow:definition:publish` alone may
 * publish** (the product owner, 2026-10-10), as the backend's
 * `publish_definition` already allows. A caller who can edit
 * publishes what is on screen, saved first; one who cannot publishes the
 * stored draft as it is.
 */
const offerPublish = computed(
  () => canPublish.value && status.value === 'DRAFT' && (editable.value || record.value !== null),
)

/**
 * Whether *Deprecate* is offered: **on an `ACTIVE` revision only**, to a holder
 * of `workflow:definition:deprecate` (**D-108**). A draft is deleted, not
 * deprecated, and nothing writes `ACTIVE` back to a deprecated one.
 */
const offerDeprecate = computed(
  () => canDeprecate.value && record.value !== null && status.value === 'ACTIVE',
)

const deprecation = useWorkflowDeprecation()

/** Which logic builders hold an unfilled operand, by row key and field. */
const validity = ref<Record<string, boolean>>({})

const unfilledExpressions = computed(() => {
  const rows = new Set([...draft.current.value.stateKeys, ...draft.current.value.transitionKeys])

  return Object.entries(validity.value).filter(
    ([key, valid]) => !valid && rows.has(key.slice(0, key.indexOf(':'))),
  ).length
})

provide(WORKFLOW_EDITOR, {
  draft,
  fieldErrors,
  readOnly,
  variables: computed(() => conditionVariables(definition.value)),
  clearField,
  resetErrors: reset,
  setValidity(key, valid) {
    validity.value = { ...validity.value, [key]: valid }
  },
})

/** Transitions out of a state no state declares, which no card would draw. */
const unattached = computed(() => {
  const codes = new Set(definition.value.states.map((state) => state.code))

  return definition.value.transitions
    .map((transition, at) => ({ transition, at, key: draft.current.value.transitionKeys[at] }))
    .filter(({ transition }) => !codes.has(transition.from))
})

/** The request field and the document field that say the same thing, shown as one message. */
function messageFor(...paths: string[]): string {
  return [...new Set(paths.map((path) => fieldErrors.value[path]).filter(Boolean))].join(' ')
}

const ROOT_PLACED = [
  'workflowKey',
  'definition.workflowKey',
  'name',
  'definition.name',
  'description',
  'definition.description',
  'definition.initialState',
  'definition.states',
  'definition.transitions',
]

/** Every path drawn somewhere as the screen is now: the root fields, and each existing row. */
const placed = computed(() =>
  Object.keys(fieldErrors.value).filter((path) => {
    if (ROOT_PLACED.includes(path)) {
      return true
    }

    const address = addressOf(path)

    return (
      address !== null &&
      address.index <
        (address.kind === 'state'
          ? definition.value.states.length
          : definition.value.transitions.length)
    )
  }),
)

const initialOptions = computed(() =>
  definition.value.states.map((state) => ({
    value: state.code,
    label: `${state.name} (${state.code})`,
  })),
)

const kindChoices = Object.entries(STATE_KINDS) as [StateKind, (typeof STATE_KINDS)[StateKind]][]

async function load(id: string): Promise<void> {
  isLoading.value = true
  loadError.value = ''
  notice.value = ''
  reset()

  try {
    const loaded = await getWorkflowDefinition(id)

    record.value = loaded
    draft.load(loaded.definition)
  } catch (failure) {
    loadError.value = toApiError(failure).message
  } finally {
    isLoading.value = false
  }
}

function setRootText(key: 'name' | 'description' | 'workflowKey', value: string): void {
  // A blank description is no description in the JWSS; the request clears the
  // stored column separately (`save`).
  draft.setRoot(key, key === 'description' && value === '' ? undefined : value)
  clearField(key)
  clearField(`definition.${key}`)
}

function setInitial(code: string): void {
  draft.setRoot('initialState', code)
  clearField('definition.initialState')
}

function addState(kind: StateKind): void {
  draft.addState(kind)
  reset()
}

async function undo(): Promise<void> {
  await draft.undo()
  reset()
}

async function redo(): Promise<void> {
  await draft.redo()
  reset()
}

/**
 * Saves the draft, and reads back what the server stored.
 *
 * The request's `name` and `description` are the document's own: the two are
 * one fact, and a column that disagreed with the JWSS it was extracted from
 * would be two.
 *
 * **A cleared description is sent as `''`**, not left out. The `PUT` reads an
 * absent field as *leave it* (`COALESCE` in `repository/definition.rs`), so
 * leaving it out kept the old text in the column while the document had none.
 * The JWSS itself carries no `description` then, which the meta-schema allows;
 * `''` is sent only when there is a stored one to clear, so a workflow that
 * never had one keeps a null column.
 */
async function save(): Promise<boolean> {
  const sent = definition.value
  const description = sent.description ?? (record.value?.description ? '' : undefined)
  const body = { name: sent.name, description, definition: sent }

  reset()
  notice.value = ''
  isSaving.value = true

  try {
    const stored = record.value
      ? await updateWorkflowDefinition(record.value.id, body)
      : await createWorkflowDefinition({ workflowKey: sent.workflowKey, ...body })

    record.value = stored
    draft.load(stored.definition)
    notice.value = 'Saved. What is shown is what the server stored.'

    if (isNew.value) {
      await router.replace({ name: 'admin-workflow-editor', params: { id: stored.id } })
    }

    return true
  } catch (failure) {
    report(placeVerdict(failure, sent))
    await refreshIfPublished(failure, 'save')

    return false
  } finally {
    isSaving.value = false
  }
}

/**
 * After a refusal that means somebody published the revision first, reads its
 * status again, so the screen turns read-only and offers *New revision*.
 * **The draft is kept**: the refused edits stay on screen, read-only, for the
 * author to carry into the new revision by hand.
 *
 * **What says so differs by route.** A save says it as a 422 whose detail is
 * `NOT_A_DRAFT` at `status`. A publish says it as a 409 `CONFLICT`, and a 409
 * is the only thing that route answers when the revision is not a draft and
 * for nothing else (`publish_definition`: *only a draft can be published*, or
 * *was published by another request*). The trigger is that status from that
 * route, not the message, which is prose and not a contract. A publish's 422
 * (a dead role, a rule) or 403 is about the draft or the caller, and reads
 * nothing again.
 *
 * **A deprecation's 409 re-reads the same way** (#713): that route answers 409
 * only for a revision that is not `ACTIVE` (*is a draft*, *is already
 * deprecated*, `deprecate_definition`), so the screen shows what it now is. Its
 * 403 and 404 are about the caller and a revision that is gone, and read
 * nothing again.
 */
async function refreshIfPublished(
  failure: unknown,
  by: 'save' | 'publish' | 'deprecate',
): Promise<void> {
  const published =
    failure instanceof ApiError &&
    (by === 'save'
      ? failure.details.some((detail) => detail.code === 'NOT_A_DRAFT')
      : failure.status === 409)

  if (!published || !record.value) {
    return
  }

  try {
    const now = await getWorkflowDefinition(record.value.id)

    record.value = { ...now, definition: record.value.definition }
  } catch {
    // The refusal already says why; a failed re-read leaves the screen as it was.
  }
}

/**
 * Publishes the stored revision, saving first when the screen holds more.
 *
 * **What is published is what is stored**, so publishing unsaved edits would
 * publish something other than what the author is looking at. A caller who
 * cannot edit has nothing unsaved, and publishes the stored draft as it is.
 */
async function publish(): Promise<void> {
  if (editable.value && (draft.isDirty.value || !record.value) && !(await save())) {
    return
  }

  const target = record.value

  if (!target) {
    return
  }

  reset()
  notice.value = ''
  isSaving.value = true

  try {
    record.value = await publishWorkflowDefinition(target.id)
    draft.load(record.value.definition)
    notice.value = `Published. Revision ${record.value.version} can now be bound to a document type.`
  } catch (failure) {
    report(placeVerdict(failure, draft.definition.value))
    await refreshIfPublished(failure, 'publish')
  } finally {
    isSaving.value = false
  }
}

function askDeprecate(): void {
  if (record.value) {
    deprecation.ask(record.value)
  }
}

/**
 * Deprecates the revision on screen, once its warning is confirmed (#713).
 *
 * **What the route answers is the revision as stored after the change**, read
 * back by `deprecate_definition`, so the screen takes it as publish takes its
 * own: the status turns `DEPRECATED`, the notice says so, and nothing but *New
 * revision* is left to press. Read-only while it is in flight, as a save is.
 */
async function deprecate(): Promise<void> {
  const target = deprecation.confirm()

  if (!target || !record.value) {
    return
  }

  reset()
  notice.value = ''
  isSaving.value = true

  try {
    record.value = await deprecateWorkflowDefinition(target.id)
    draft.load(record.value.definition)
    notice.value = `Deprecated. New documents no longer route to revision ${record.value.version}; approvals already running on it carry on.`
  } catch (failure) {
    formError.value = toApiError(failure).message
    await refreshIfPublished(failure, 'deprecate')
  } finally {
    isSaving.value = false
  }
}

async function revise(): Promise<void> {
  if (!record.value) {
    return
  }

  reset()
  notice.value = ''
  isSaving.value = true

  try {
    const next = await createWorkflowRevision(record.value.id)

    await router.push({ name: 'admin-workflow-editor', params: { id: next.id } })
  } catch (failure) {
    formError.value = toApiError(failure).message
  } finally {
    isSaving.value = false
  }
}

// One component serves `new` and every revision, so a move between them — the
// create's replace, a new revision's push — is a load, unless it is the
// revision already on screen.
watch(
  () => route.params.id,
  (id) => {
    if (typeof id === 'string' && id !== record.value?.id) {
      void load(id)
    }
  },
)

onMounted(() => {
  if (!isNew.value) {
    void load(String(route.params.id))
  }
})
</script>

<template>
  <section class="space-y-6">
    <Alert v-if="loadError" variant="destructive" data-testid="load-error">{{ loadError }}</Alert>

    <p v-if="isLoading" class="text-sm text-muted-foreground">Loading…</p>

    <template v-else-if="!loadError">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 class="text-xl font-semibold tracking-tight">
            {{ record ? record.workflowKey : 'New workflow' }}
          </h2>
          <p class="mt-1 text-sm text-muted-foreground">
            <template v-if="record">
              Revision {{ record.version }} · JWSS {{ record.jwssVersion }}
            </template>
            <template v-else>Not saved yet. It is created as revision 1, in draft.</template>
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <Badge :variant="isPublished ? 'default' : 'secondary'" data-testid="status">
            {{ status }}
          </Badge>

          <template v-if="editable">
            <Button
              variant="ghost"
              size="sm"
              aria-label="Undo"
              :disabled="!draft.canUndo.value || isSaving"
              data-testid="undo"
              @click="undo"
            >
              <Undo2 class="size-4" aria-hidden="true" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              aria-label="Redo"
              :disabled="!draft.canRedo.value || isSaving"
              data-testid="redo"
              @click="redo"
            >
              <Redo2 class="size-4" aria-hidden="true" />
            </Button>
            <Button :disabled="isSaving" data-testid="save-workflow" @click="save">
              {{ isSaving ? 'Saving…' : 'Save' }}
            </Button>
          </template>
          <Button
            v-if="offerPublish"
            variant="secondary"
            :disabled="isSaving"
            data-testid="publish-workflow"
            @click="publish"
          >
            {{ editable && (draft.isDirty.value || !record) ? 'Save and publish' : 'Publish' }}
          </Button>

          <Button
            v-if="isPublished && canCreate"
            variant="secondary"
            :disabled="isSaving"
            data-testid="new-revision"
            @click="revise"
          >
            New revision
          </Button>
          <!-- Deprecate (#713, D-101 B, D-108), beside New revision. -->
          <Button
            v-if="offerDeprecate"
            variant="destructive"
            :disabled="isSaving"
            data-testid="deprecate-workflow"
            @click="askDeprecate"
          >
            Deprecate
          </Button>
        </div>
      </div>

      <!-- AC5: said, not merely enforced by disabled inputs. -->
      <Alert v-if="status === 'ACTIVE'" data-testid="published-notice">
        This revision is published and cannot be edited. Approvals already running execute the
        revision they started against, so changing it would change their rules part-way through. To
        change the workflow, open a new revision: it starts as a draft, and once published it can be
        bound to the document types that should use it.
      </Alert>
      <Alert v-else-if="status === 'DEPRECATED'" data-testid="published-notice">
        This revision is deprecated and cannot be edited. It can no longer be bound to a document
        type; approvals already running on it carry on. Open a new revision to build on it.
      </Alert>
      <Alert v-else-if="!canEdit" data-testid="read-only-notice">
        You can read this workflow but not change it.<template v-if="offerPublish">
          You can publish it as it is stored.</template
        >
      </Alert>

      <Alert v-if="formError" variant="destructive" data-testid="form-error">{{ formError }}</Alert>
      <FormUnplacedErrors
        :field-errors="fieldErrors"
        :placed="placed"
        data-testid="unplaced-errors"
      />
      <Alert v-if="notice" data-testid="notice">{{ notice }}</Alert>
      <Alert v-if="unfilledExpressions > 0 && editable" data-testid="unfilled-expressions">
        {{ unfilledExpressions }}
        {{ unfilledExpressions === 1 ? 'expression has' : 'expressions have' }}
        an operand not filled in. Until it is, what is saved is the last complete version.
      </Alert>

      <div class="grid gap-4 md:grid-cols-2">
        <div class="space-y-1">
          <Label for="workflow-key">Workflow key</Label>
          <Input
            id="workflow-key"
            class="font-mono"
            :model-value="definition.workflowKey"
            :invalid="messageFor('workflowKey', 'definition.workflowKey') !== ''"
            described-by="workflow-key-hint"
            :disabled="readOnly || record !== null"
            data-testid="workflow-key"
            @update:model-value="setRootText('workflowKey', $event)"
          />
          <p id="workflow-key-hint" class="text-xs text-muted-foreground">
            Lower case, digits and underscores. It cannot change once the workflow is saved.
          </p>
          <p
            v-if="messageFor('workflowKey', 'definition.workflowKey')"
            class="text-xs text-destructive"
            data-testid="workflow-key-error"
          >
            {{ messageFor('workflowKey', 'definition.workflowKey') }}
          </p>
        </div>

        <div class="space-y-1">
          <Label for="workflow-name">Name</Label>
          <Input
            id="workflow-name"
            :model-value="definition.name"
            :invalid="messageFor('name', 'definition.name') !== ''"
            :disabled="readOnly"
            data-testid="workflow-name"
            @update:model-value="setRootText('name', $event)"
          />
          <p
            v-if="messageFor('name', 'definition.name')"
            class="text-xs text-destructive"
            data-testid="workflow-name-error"
          >
            {{ messageFor('name', 'definition.name') }}
          </p>
        </div>

        <div class="space-y-1">
          <Label for="workflow-description">Description</Label>
          <Textarea
            id="workflow-description"
            :model-value="definition.description ?? ''"
            :disabled="readOnly"
            @update:model-value="setRootText('description', $event)"
          />
          <p
            v-if="messageFor('description', 'definition.description')"
            class="text-xs text-destructive"
          >
            {{ messageFor('description', 'definition.description') }}
          </p>
        </div>

        <div class="space-y-1">
          <Label for="workflow-initial">Initial state</Label>
          <Select
            id="workflow-initial"
            :model-value="definition.initialState"
            :options="initialOptions"
            placeholder="Choose the state a submitted document enters"
            :invalid="Boolean(fieldErrors['definition.initialState'])"
            :disabled="readOnly"
            data-testid="initial-state"
            @update:model-value="setInitial"
          />
          <p
            v-if="fieldErrors['definition.initialState']"
            class="text-xs text-destructive"
            data-testid="initial-state-error"
          >
            {{ fieldErrors['definition.initialState'] }}
          </p>
        </div>
      </div>

      <div class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h3 class="text-base font-semibold">States</h3>
          <div class="flex flex-wrap gap-2">
            <Button
              v-for="[kind, entry] in kindChoices"
              :key="kind"
              size="sm"
              variant="secondary"
              :disabled="readOnly"
              :data-testid="`add-state-${kind}`"
              @click="addState(kind)"
            >
              <Plus class="mr-1 size-4" aria-hidden="true" />
              {{ entry.label }}
            </Button>
          </div>
        </div>

        <Alert
          v-if="messageFor('definition.states', 'definition.transitions')"
          variant="destructive"
          data-testid="states-error"
        >
          {{ messageFor('definition.states', 'definition.transitions') }}
        </Alert>

        <WorkflowStateCard
          v-for="(key, index) in draft.current.value.stateKeys"
          :key="key"
          :index="index"
        />

        <p v-if="definition.states.length === 0" class="text-sm text-muted-foreground">
          No states. A workflow needs at least two: one to start in and one to finish in.
        </p>

        <div v-if="unattached.length > 0" class="space-y-2" data-testid="unattached-transitions">
          <h4 class="text-sm font-semibold">Transitions out of a state that is not declared</h4>
          <Table>
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
              <WorkflowTransitionRow v-for="row in unattached" :key="row.key" :index="row.at" />
            </TableBody>
          </Table>
        </div>
      </div>

      <WorkflowDeprecateDialog :deprecation="deprecation" @confirm="deprecate" />
    </template>
  </section>
</template>
