<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { createDocumentType, updateDocumentType } from '@/api/document-types'
import { toApiError } from '@/api/client'
import { getForm, getList, listForms, listLists } from '@/api/rad'
import { getWorkflowDefinition, listWorkflowDefinitions } from '@/api/workflow'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SearchSelect, type SearchSource } from '@/components/ui/search-select'
import { Select } from '@/components/ui/select'
import type { ValidationDetail } from '@/types/api'
import type {
  CreateDocumentTypeRequest,
  DocumentType,
  DocumentTypeStatus,
  GovernedEntityType,
  SecurityLevel,
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

const isSaving = ref(false)
const error = ref('')
const details = ref<ValidationDetail[]>([])

const isEditing = computed(() => props.editing !== null)

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

/** The field-level message for one path, if the server named it. */
function messageFor(path: string): string {
  return details.value.find((detail) => detail.path === path)?.message ?? ''
}

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

watch(
  () => props.open,
  (open) => {
    if (!open) {
      return
    }

    error.value = ''
    details.value = []

    const editing = props.editing

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
    workflowId.value = editing?.workflows?.[0]?.workflowDefinitionId ?? ''
  },
  { immediate: true },
)

/** A blank optional field is `null` rather than an empty string. */
function orNull(value: string): string | null {
  const trimmed = value.trim()

  return trimmed === '' ? null : trimmed
}

async function save(): Promise<void> {
  isSaving.value = true
  error.value = ''
  details.value = []

  const body: CreateDocumentTypeRequest = {
    typeCode: typeCode.value.trim(),
    name: name.value.trim(),
    description: orNull(description.value),
    category: orNull(category.value),
    formId: orNull(formId.value),
    listId: orNull(listId.value),
    targetEntityType: targetEntity.value === '' ? null : targetEntity.value,
    defaultSecurityLevel: securityLevel.value,
    status: status.value,
    // One binding, which is what a screen can usefully author: the priority and
    // condition that make several of them meaningful are a routing decision,
    // and a workflow designer is FR-RAD-011's. Sending the array replaces the
    // stored set, so clearing the chooser unbinds.
    workflows: workflowId.value ? [{ workflowDefinitionId: workflowId.value }] : [],
  }

  try {
    if (props.editing) {
      // `typeCode` may not change — a delegation scopes itself to it and an
      // integration names a type by it — so an edit sends everything else.
      const editable = { ...body, typeCode: undefined }

      delete editable.typeCode
      await updateDocumentType(props.editing.id, editable)
    } else {
      await createDocumentType(body)
    }

    emit('saved')
  } catch (failure) {
    const failed = toApiError(failure)

    error.value = failed.message
    details.value = failed.details
  } finally {
    isSaving.value = false
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

      <Alert v-if="error" variant="destructive" class="mt-4" data-testid="document-type-error">
        {{ error }}
      </Alert>

      <form class="mt-4 grid gap-4 sm:grid-cols-2" @submit.prevent="save">
        <div class="space-y-2">
          <Label for="type-code">Type code</Label>
          <Input
            id="type-code"
            v-model="typeCode"
            data-testid="type-code"
            :disabled="isEditing"
            placeholder="PURCHASE_REQUISITION"
          />
          <!-- Immutable once created: a delegation scopes itself to it and an
               integration names a type by it. Disabled rather than hidden, so an
               administrator can still read what it is. -->
          <p v-if="isEditing" class="text-xs text-muted-foreground">
            A type code cannot change once documents and delegations refer to it.
          </p>
          <p v-if="messageFor('typeCode')" class="text-xs text-destructive">
            {{ messageFor('typeCode') }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="type-name">Name</Label>
          <Input id="type-name" v-model="name" data-testid="type-name" />
          <p v-if="messageFor('name')" class="text-xs text-destructive">
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
          />
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
          />
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
          />
          <!-- Stated because it changes what submitting a document *does*: a
               governing type parks its record at pending-approval rather than
               writing it (ADR-0033). -->
          <p class="text-xs text-muted-foreground">
            A document of a governing type carries a proposed change, and the record it names waits
            for the approval rather than changing when the document is submitted.
          </p>
          <p v-if="messageFor('targetEntityType')" class="text-xs text-destructive">
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
          />
          <p class="text-xs text-muted-foreground">
            Documents of this type route through it when submitted.
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
          <Button type="submit" :disabled="isSaving" data-testid="save-document-type">
            {{ isSaving ? 'Saving…' : 'Save' }}
          </Button>
        </div>
      </form>
    </div>
  </div>
</template>
