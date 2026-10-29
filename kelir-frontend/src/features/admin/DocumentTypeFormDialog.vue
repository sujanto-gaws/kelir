<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { toApiError } from '@/api/client'
import { createDocumentType, getDocumentType, updateDocumentType } from '@/api/document-types'
import { ApiError } from '@/api/error'
import { getForm, getList, listForms, listLists } from '@/api/rad'
import { getWorkflowDefinition, listWorkflowDefinitions } from '@/api/workflow'
import FormUnplacedErrors from '@/components/FormUnplacedErrors.vue'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SearchSelect, type SearchSource } from '@/components/ui/search-select'
import { Select } from '@/components/ui/select'
import { useFormErrors } from '@/composables/useFormErrors'
import {
  DOCUMENT_TYPE_WORKFLOWS_CHANGED,
  type CreateDocumentTypeRequest,
  type DocumentType,
  type DocumentTypeStatus,
  type GovernedEntityType,
  type SecurityLevel,
  type UpdateDocumentTypeRequest,
  type WorkflowBinding,
  type WorkflowBindingRequest,
} from '@/types/document-type'
import type { FormSummary, ListSummary } from '@/types/rad'
import type { WorkflowDefinitionSummary } from '@/types/workflow'

/**
 * Creating and editing a document type (FR-RAD-008, FR-DTYPE-001..004; #341).
 *
 * **This is the screen D-64 was accepted against the absence of.** SRS §9
 * criterion 4 — *administrators can configure document types* — was recorded as
 * met over the API, on the stated basis that this dialog was scheduled. It
 * covers what the API covers: the type's own fields, the published form it
 * renders, the list its documents appear on, the published workflows it routes
 * to, the master-data record it governs, and its status.
 *
 * **`retentionPolicyId` is the one column not offered, and that is deliberate.**
 * Nothing in this build creates a retention policy, reads one or acts on one —
 * the column has waited since `0015` for the surface that will — so the field
 * would be a UUID box for a record that cannot exist. Offering it would invite
 * an administrator to invent an id and believe a retention was configured.
 *
 * **The bindings are chosen, not typed.** A form id and a workflow id are
 * UUIDs; asking an administrator to paste one is asking them to make a mistake
 * that reads as a broken document type later. Both choosers list only what can
 * actually be bound — a `PUBLISHED` form, an `ACTIVE` workflow — because the
 * backend refuses the rest and a chooser that offered them would be offering a
 * 422. **The status is asked of the server and the choosers search** (#525):
 * filtering one page after it arrived hid whatever sorted past it.
 *
 * **The numbering rule is its own dialog**, as it is its own sub-resource: a
 * type has one or it does not, and folding it in here would make creating a
 * type require deciding how its documents are numbered.
 */
const props = defineProps<{ open: boolean; editing: DocumentType | null }>()
const emit = defineEmits<{ (event: 'close'): void; (event: 'saved'): void }>()

const typeCode = ref('')
const name = ref('')
const description = ref('')
const category = ref('')
const formId = ref('')
const listId = ref('')
const targetEntity = ref<GovernedEntityType | ''>('')
const status = ref<DocumentTypeStatus>('DRAFT')
const securityLevel = ref<SecurityLevel>('INTERNAL')
const workflowId = ref('')
/** The bindings the type had when the dialog opened, in the server's order. */
const storedBindings = ref<WorkflowBinding[]>([])
/** The status, form and list of the read the fields hold, from opening or Reload (#742). */
const storedFields = ref<Pick<DocumentType, 'status' | 'formId' | 'listId'>>({
  status: 'DRAFT',
  formId: null,
  listId: null,
})

const isSaving = ref(false)
const { fieldErrors, formError, report, reset } = useFormErrors()

/**
 * Whether the server refused a save because the bindings changed since they
 * were read (#728). It stays up until a reload succeeds or the dialog opens
 * again: until then `storedBindings` is known to be stale.
 */
const workflowsChanged = ref(false)
const isReloading = ref(false)
/** Why the last reload failed, empty when it did not. */
const reloadError = ref('')

const isEditing = computed(() => props.editing !== null)

/**
 * The bindings past the first, which the chooser does not show (#718).
 *
 * **Above none, the chooser is disabled.** It shows one revision; with several
 * bound, a change could mean any of them, and guessing would rewrite a routing
 * the administrator cannot see from here. So the chooser shows the first, says
 * how many more there are, and an edit leaves all of them as they are.
 */
const furtherBindings = computed(() => Math.max(storedBindings.value.length - 1, 0))

const STATUSES: { value: DocumentTypeStatus; label: string }[] = [
  { value: 'DRAFT', label: 'Draft' },
  { value: 'ACTIVE', label: 'Active' },
  { value: 'DEPRECATED', label: 'Deprecated' },
]

/**
 * The master-data records a document type can govern
 * (`master_data::domain::governance::GovernedEntity`).
 *
 * **Only what this build implements.** The column is free text with no `CHECK`,
 * and a value the backend does not recognise makes the type govern *nothing* —
 * a silent no-op. Two named choices and a blank cannot author that state.
 */
const GOVERNED_ENTITIES: { value: GovernedEntityType | ''; label: string }[] = [
  { value: '', label: 'Nothing — an ordinary document type' },
  { value: 'PARTY', label: 'Party — a change to a supplier, customer or employee' },
  { value: 'FACILITY', label: 'Facility' },
]

const SECURITY_LEVELS: { value: SecurityLevel; label: string }[] = [
  { value: 'PUBLIC', label: 'Public' },
  { value: 'INTERNAL', label: 'Internal' },
  { value: 'CONFIDENTIAL', label: 'Confidential' },
  { value: 'RESTRICTED', label: 'Restricted' },
]

/**
 * Where `check_workflow_bindings` names the chooser's revision. The dialog sends
 * at most one binding, so a refusal of what it sent is always the first.
 */
const WORKFLOW_PATH = 'workflows.0.workflowDefinitionId'

/**
 * The paths a message is shown under (#716); a detail at any other path is
 * listed above the form by `FormUnplacedErrors`, so a refused save always says
 * why. Each chooser is drawn in every mode, so the list does not change with it.
 */
const PLACED = ['typeCode', 'name', 'targetEntityType', 'formId', 'listId', WORKFLOW_PATH]

/** The field-level message for one path, if the server named it. */
function messageFor(path: string): string {
  return fieldErrors.value[path] ?? ''
}

/** What describes the workflow chooser: why it is disabled, and why it was refused. */
const workflowDescribedBy = computed(
  () =>
    [
      furtherBindings.value > 0 ? 'type-workflow-further' : '',
      messageFor(WORKFLOW_PATH) ? 'type-workflow-message' : '',
    ]
      .filter((id) => id)
      .join(' ') || undefined,
)

/**
 * What may be bound, each searched on the server.
 *
 * **A chooser that cannot load does not close the dialog.** It says so under
 * itself and the rest of the form still works: a type is configurable without a
 * form, and the three reads want three different permissions — a caller who may
 * configure types but not read list definitions gets a usable dialog rather
 * than a refusal.
 */
const formSource: SearchSource<FormSummary> = {
  fetch: (query) => listForms({ ...query, status: 'PUBLISHED' }),
  value: (form) => form.id,
  label: (form) => `${form.title} (r${form.revision})`,
  // The form a type is already bound to, when no search has named it.
  resolve: getForm,
}

const listSource: SearchSource<ListSummary> = {
  // A `DEPRECATED` list still renders, but binding a type to one is
  // configuring a screen somebody has already retired.
  fetch: (query) => listLists({ ...query, status: 'ACTIVE' }),
  value: (list) => list.id,
  label: (list) => `${list.title} (${list.listKey})`,
  resolve: getList,
}

const workflowSource: SearchSource<WorkflowDefinitionSummary> = {
  fetch: (query) => listWorkflowDefinitions({ ...query, status: 'ACTIVE' }),
  value: (workflow) => workflow.id,
  label: workflowLabel,
  // The revision a type is already bound to, when no search has named it (#625):
  // one sorting past the first page, or one deprecated since it was bound.
  resolve: getWorkflowDefinition,
}

/**
 * How a revision reads as a choice.
 *
 * **A deprecated one says so.** Only an `ACTIVE` revision is searched, so the
 * one place a deprecated revision shows is a type still bound to it — whose
 * submissions are refused until it is rebound (#713). Its name alone would
 * read as a working binding, and would not say why no search finds it again.
 */
function workflowLabel(workflow: WorkflowDefinitionSummary): string {
  return workflow.status === 'DEPRECATED'
    ? `${workflow.name} (r${workflow.version}, deprecated)`
    : `${workflow.name} (r${workflow.version})`
}

/**
 * Reads the stored governed entity back into the chooser.
 *
 * **An unknown value reads as blank rather than being kept.** The column is
 * free text and a row can hold something this build does not implement; showing
 * it as a selected option would claim a governance that does not happen.
 */
function governedEntityOf(value: string | null | undefined): GovernedEntityType | '' {
  return value === 'PARTY' || value === 'FACILITY' ? value : ''
}

/**
 * Puts a read of the type, or nothing for a new one, into every field.
 *
 * **All at once, after the read arrived.** It is the one place the fields,
 * `storedBindings` and the chooser are set, so a reload whose read fails
 * leaves them as they were rather than half the server's (#728).
 */
function fill(editing: DocumentType | null): void {
  reset()
  workflowsChanged.value = false
  reloadError.value = ''

  typeCode.value = editing?.typeCode ?? ''
  name.value = editing?.name ?? ''
  description.value = editing?.description ?? ''
  category.value = editing?.category ?? ''
  formId.value = editing?.formId ?? ''
  listId.value = editing?.listId ?? ''
  targetEntity.value = governedEntityOf(editing?.targetEntityType)
  status.value = editing?.status ?? 'DRAFT'
  securityLevel.value = editing?.defaultSecurityLevel ?? 'INTERNAL'
  // Guarded, because a dialog that throws while opening renders nothing
  // at all — including the error that would have said why.
  storedBindings.value = editing?.workflows ?? []
  workflowId.value = storedBindings.value[0]?.workflowDefinitionId ?? ''
  storedFields.value = {
    status: status.value,
    formId: editing?.formId ?? null,
    listId: editing?.listId ?? null,
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      fill(props.editing)
    }
  },
  { immediate: true },
)

/** A blank optional field is `null` rather than an empty string. */
function orNull(value: string): string | null {
  const trimmed = value.trim()

  return trimmed === '' ? null : trimmed
}

/**
 * A binding as the wire carries it. Named rather than spread: the endpoint
 * denies an unknown field, and these five are all a binding is on the wire.
 */
function onTheWire(binding: WorkflowBinding): WorkflowBindingRequest {
  return {
    workflowDefinitionId: binding.workflowDefinitionId,
    conditionExpression: binding.conditionExpression,
    priority: binding.priority,
    validFrom: binding.validFrom,
    validTo: binding.validTo,
  }
}

/**
 * The `workflows` a save sends, or `undefined` to leave them off the wire.
 *
 * **Sending the array replaces the stored set** (`repository::replace_workflows`),
 * unless the set it read, sent beside it as `expectedWorkflows`, is stale (#728);
 * and leaving it off leaves the set alone — so an edit sends it only when the
 * chooser moved (#718). Until then a rename rebuilt the set from the chooser
 * and deleted every binding past the first, along with the first one's
 * priority, condition and window.
 *
 * When it does move, only the revision changes: the binding goes back with the
 * four fields the dialog does not author as they were read. Those are a routing
 * decision, and a workflow designer is FR-RAD-011's. Clearing the chooser sends
 * `[]`, which unbinds.
 */
function workflowsToSend(): WorkflowBindingRequest[] | undefined {
  const first = storedBindings.value[0]
  const chosen = workflowId.value

  if (
    isEditing.value &&
    (furtherBindings.value > 0 || chosen === (first?.workflowDefinitionId ?? ''))
  ) {
    return undefined
  }

  if (chosen === '') {
    return []
  }

  if (!first) {
    return [{ workflowDefinitionId: chosen }]
  }

  return [{ ...onTheWire(first), workflowDefinitionId: chosen }]
}

/**
 * The `status`, `formId` and `listId` a save sends: on an edit, only those that
 * differ from what the dialog read; the rest are absent rather than `undefined`.
 *
 * **Each is written as sent** (`update_type`), and leaving one off leaves it
 * alone — so an edit sends one only when the administrator changed it (#742),
 * as `workflows` is sent only when the chooser moved (#718). Until then every
 * save sent all three back as read, and one from a stale read undid a
 * deprecation, or a rebinding, another administrator had made since. A change
 * the administrator did make is sent and is last-writer-wins, as before;
 * clearing a bound form or list sends `null`, which unbinds it.
 */
function fieldsToSend(): Pick<CreateDocumentTypeRequest, 'status' | 'formId' | 'listId'> {
  const chosen = {
    status: status.value,
    formId: orNull(formId.value),
    listId: orNull(listId.value),
  }

  if (!isEditing.value) {
    return chosen
  }

  const read = storedFields.value
  const changed: Pick<CreateDocumentTypeRequest, 'status' | 'formId' | 'listId'> = {}

  if (chosen.status !== read.status) {
    changed.status = chosen.status
  }

  if (chosen.formId !== read.formId) {
    changed.formId = chosen.formId
  }

  if (chosen.listId !== read.listId) {
    changed.listId = chosen.listId
  }

  return changed
}

async function save(): Promise<void> {
  isSaving.value = true
  reset()

  const body: CreateDocumentTypeRequest = {
    typeCode: typeCode.value.trim(),
    name: name.value.trim(),
    description: orNull(description.value),
    category: orNull(category.value),
    ...fieldsToSend(),
    targetEntityType: targetEntity.value === '' ? null : targetEntity.value,
    defaultSecurityLevel: securityLevel.value,
  }

  const workflows = workflowsToSend()

  // Absent rather than `undefined`, so what leaves the stored set alone is not
  // left to how the client serialises.
  if (workflows !== undefined) {
    body.workflows = workflows
  }

  try {
    if (props.editing) {
      // `typeCode` may not change — a delegation scopes itself to it and an
      // integration names a type by it — so an edit sends everything else,
      // the status, form and list only as far as they changed.
      const editable = { ...body, typeCode: undefined }

      delete editable.typeCode

      // #728 (D-117): a replacement names the set it replaces, so a binding
      // another administrator saved since this dialog read the type is
      // refused rather than deleted. Every binding read, all five fields as
      // read, `[]` included; never without `workflows`, which is a 422.
      const request: UpdateDocumentTypeRequest =
        workflows === undefined
          ? editable
          : { ...editable, expectedWorkflows: storedBindings.value.map(onTheWire) }

      await updateDocumentType(props.editing.id, request)
    } else {
      await createDocumentType(body)
    }

    emit('saved')
  } catch (failure) {
    // Told apart by the code (coding standard §2.3): this route answers other
    // 409s, and only this one is mended by a reload.
    if (failure instanceof ApiError && failure.code === DOCUMENT_TYPE_WORKFLOWS_CHANGED) {
      workflowsChanged.value = true
    } else {
      report(failure)
    }
  } finally {
    isSaving.value = false
  }
}

/**
 * Re-reads the type after that refusal, and starts the edit over from it.
 *
 * **Unsaved edits are discarded**, which the alert says before Reload is
 * pressed: they were made against bindings that are no longer there. A read
 * that fails changes nothing, so the alert stays up and a later save still
 * sends the read the dialog holds, which the server refuses again.
 */
async function reload(): Promise<void> {
  const editing = props.editing

  if (!editing) {
    return
  }

  isReloading.value = true
  reloadError.value = ''

  try {
    const current = await getDocumentType(editing.id)

    // Not if it closed, or opened on another type, while the read was out.
    if (props.open && props.editing?.id === editing.id) {
      fill(current)
    }
  } catch (failure) {
    reloadError.value = `The document type could not be reloaded: ${toApiError(failure).message}`
  } finally {
    isReloading.value = false
  }
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    role="dialog"
    aria-modal="true"
    :aria-label="isEditing ? 'Edit document type' : 'New document type'"
    data-testid="document-type-dialog"
  >
    <div class="max-h-full w-full max-w-2xl overflow-y-auto rounded-lg bg-background p-6 shadow-lg">
      <h3 class="text-lg font-semibold">
        {{ isEditing ? 'Edit document type' : 'New document type' }}
      </h3>

      <!-- #728: the bindings changed since this dialog read them. Its own
           alert, because unlike the others it offers the way out. -->
      <Alert
        v-if="workflowsChanged"
        variant="destructive"
        class="mt-4"
        data-testid="document-type-workflows-changed"
      >
        <p data-testid="document-type-workflows-changed-message">
          Another administrator changed the workflow bindings of this document type since this
          dialog opened. Nothing was saved. Reload brings in the current bindings and discards your
          unsaved edits.
        </p>
        <Button
          type="button"
          variant="secondary"
          class="mt-3"
          :disabled="isReloading || isSaving"
          data-testid="reload-document-type"
          @click="reload"
        >
          {{ isReloading ? 'Reloading…' : 'Reload' }}
        </Button>
      </Alert>

      <Alert
        v-if="reloadError"
        variant="destructive"
        class="mt-4"
        data-testid="document-type-reload-error"
      >
        {{ reloadError }}
      </Alert>

      <Alert v-if="formError" variant="destructive" class="mt-4" data-testid="document-type-error">
        {{ formError }}
      </Alert>

      <FormUnplacedErrors
        :field-errors="fieldErrors"
        :placed="PLACED"
        class="mt-4"
        data-testid="document-type-unplaced-errors"
      />

      <form class="mt-4 grid gap-4 sm:grid-cols-2" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="type-code">Type code</Label>
          <Input
            id="type-code"
            v-model="typeCode"
            data-testid="type-code"
            :disabled="isEditing"
            placeholder="PURCHASE_REQUISITION"
            :invalid="Boolean(messageFor('typeCode'))"
            :described-by="messageFor('typeCode') ? 'type-code-message' : undefined"
          />
          <!-- Immutable once created: a delegation scopes itself to it and an
               integration names a type by it. Disabled rather than hidden, so an
               administrator can still read what it is. -->
          <p v-if="isEditing" class="text-xs text-muted-foreground">
            A type code cannot change once documents and delegations refer to it.
          </p>
          <p
            v-if="messageFor('typeCode')"
            id="type-code-message"
            class="text-xs text-destructive"
            data-testid="type-code-message"
          >
            {{ messageFor('typeCode') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="type-name">Name</Label>
          <Input
            id="type-name"
            v-model="name"
            data-testid="type-name"
            :invalid="Boolean(messageFor('name'))"
            :described-by="messageFor('name') ? 'type-name-message' : undefined"
          />
          <p
            v-if="messageFor('name')"
            id="type-name-message"
            class="text-xs text-destructive"
            data-testid="type-name-message"
          >
            {{ messageFor('name') }}
          </p>
        </div>

        <div class="space-y-2 sm:col-span-2">
          <Label for="type-description">Description</Label>
          <Input id="type-description" v-model="description" data-testid="type-description" />
        </div>

        <div class="space-y-2">
          <Label for="type-category">Category</Label>
          <Input
            id="type-category"
            v-model="category"
            data-testid="type-category"
            placeholder="PROCUREMENT"
          />
        </div>

        <div class="space-y-2">
          <Label for="type-status">Status</Label>
          <Select id="type-status" v-model="status" data-testid="type-status" :options="STATUSES" />
        </div>

        <div class="space-y-2">
          <SearchSelect
            id="type-form"
            v-model="formId"
            label="Form"
            test-id="type-form"
            placeholder="No form"
            search-placeholder="Search by title or key"
            :source="formSource"
            :invalid="Boolean(messageFor('formId'))"
            :described-by="messageFor('formId') ? 'type-form-message' : undefined"
          />
          <p
            v-if="messageFor('formId')"
            id="type-form-message"
            class="text-xs text-destructive"
            data-testid="type-form-message"
          >
            {{ messageFor('formId') }}
          </p>
          <!-- Only published revisions: a draft is rewritten in place, so a
               document pinning one would render against a definition that no
               longer means what it meant. -->
          <p class="text-xs text-muted-foreground">Published revisions only.</p>
        </div>

        <div class="space-y-2">
          <SearchSelect
            id="type-list"
            v-model="listId"
            label="List"
            test-id="type-list"
            placeholder="No list"
            search-placeholder="Search by title or key"
            :source="listSource"
            :invalid="Boolean(messageFor('listId'))"
            :described-by="messageFor('listId') ? 'type-list-message' : undefined"
          />
          <p
            v-if="messageFor('listId')"
            id="type-list-message"
            class="text-xs text-destructive"
            data-testid="type-list-message"
          >
            {{ messageFor('listId') }}
          </p>
          <!-- The list a document of this type appears on — the renderer #340
               built. A type with none is not on a configured list; it is still
               on `/documents`. -->
          <p class="text-xs text-muted-foreground">
            Where documents of this type are listed. Active lists only.
          </p>
        </div>

        <div class="space-y-2 sm:col-span-2">
          <Label for="type-governs">Governs a master-data record</Label>
          <Select
            id="type-governs"
            v-model="targetEntity"
            data-testid="type-governs"
            :options="GOVERNED_ENTITIES"
            :invalid="Boolean(messageFor('targetEntityType'))"
            :described-by="messageFor('targetEntityType') ? 'type-governs-message' : undefined"
          />
          <!-- Stated because it changes what submitting a document *does*: a
               governing type parks its record at pending-approval rather than
               writing it (ADR-0033). -->
          <p class="text-xs text-muted-foreground">
            A document of a governing type carries a proposed change, and the record it names waits
            for the approval rather than changing when the document is submitted.
          </p>
          <p
            v-if="messageFor('targetEntityType')"
            id="type-governs-message"
            class="text-xs text-destructive"
            data-testid="type-governs-message"
          >
            {{ messageFor('targetEntityType') }}
          </p>
        </div>

        <div class="space-y-2">
          <SearchSelect
            id="type-workflow"
            v-model="workflowId"
            label="Workflow"
            test-id="type-workflow"
            placeholder="No workflow"
            search-placeholder="Search by name or key"
            :source="workflowSource"
            :disabled="furtherBindings > 0"
            :invalid="Boolean(messageFor(WORKFLOW_PATH))"
            :described-by="workflowDescribedBy"
          />
          <!-- #716: a revision retired between this chooser listing it and the
               save is refused here, and the server's reason belongs under it. -->
          <p
            v-if="messageFor(WORKFLOW_PATH)"
            id="type-workflow-message"
            class="text-xs text-destructive"
            data-testid="type-workflow-message"
          >
            {{ messageFor(WORKFLOW_PATH) }}
          </p>
          <p class="text-xs text-muted-foreground">
            Documents of this type route through it when submitted.
          </p>
          <!-- #718: the chooser shows one binding, so with several it cannot
               say which a change would mean. Saving leaves every one alone. -->
          <p
            v-if="furtherBindings > 0"
            id="type-workflow-further"
            class="text-xs text-muted-foreground"
            data-testid="type-workflow-further"
          >
            {{
              furtherBindings === 1
                ? '1 further binding is not edited here.'
                : `${furtherBindings} further bindings are not edited here.`
            }}
            Saving keeps every binding as it is.
          </p>
        </div>

        <div class="space-y-2">
          <Label for="type-security">Default security level</Label>
          <Select
            id="type-security"
            v-model="securityLevel"
            data-testid="type-security"
            :options="SECURITY_LEVELS"
          />
        </div>

        <div class="flex justify-end gap-2 sm:col-span-2">
          <Button type="button" variant="secondary" @click="emit('close')">Cancel</Button>
          <Button
            type="submit"
            :disabled="isSaving || isReloading"
            data-testid="save-document-type"
          >
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
