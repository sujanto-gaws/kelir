<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { createDelegation, listUsers } from '@/api/identity'
import { listDocumentTypes } from '@/api/document-types'
import { useFormErrors } from '@/composables/useFormErrors'
import { useAuthStore } from '@/stores/auth'
import type { Delegation, DelegationScope, User } from '@/types/identity'

/**
 * Open a delegation window (FR-IDM-006, #184).
 *
 * # There is no "from" field, and that is the security property
 *
 * The window is always in the caller's own name. A field here would be the one
 * shape of this feature that escalates: a holder of `identity:delegation:create`
 * could point somebody else's approvals at themselves and the row would look
 * exactly like legitimate cover. The request type on the wire has no
 * `delegatorUserId` either, so this is not a control the screen declines to
 * draw — it is a value the API will not accept.
 *
 * # Two scopes, because the third cannot work
 *
 * `ROLE` is in the column's vocabulary and is refused by the API. A window
 * redirects a task that resolves to a *person*; a task offered to a role has no
 * assignee to redirect, and every other holder is still being offered it. So a
 * `ROLE` window could never match anything the engine looks at, and offering the
 * choice would be drawing a control the product then refuses.
 *
 * # The dates are dates, and the window is half-open
 *
 * `[starts, ends)`. Cover to the end of the month stops at midnight rather than
 * running a day into the next person's, which is what the backend's own
 * predicate says and what the help text below has to match.
 */
const emit = defineEmits<{ saved: [delegation: Delegation] }>()

const open = defineModel<boolean>('open', { default: false })

const auth = useAuthStore()

const delegateUserId = ref('')
const startsAt = ref('')
const endsAt = ref('')
const scope = ref<DelegationScope>('ALL')
const documentTypeId = ref('')
const reason = ref('')

const people = ref<User[]>([])
const documentTypes = ref<{ value: string; label: string }[]>([])
const isSaving = ref(false)
const submitted = ref(false)

const { fieldErrors, formError, report, reset, clearField } = useFormErrors()

const scopeOptions: { value: DelegationScope; label: string }[] = [
  { value: 'ALL', label: 'Everything that would reach me' },
  { value: 'DOCUMENT_TYPE', label: 'One type of document' },
]

const peopleOptions = computed(() =>
  people.value.map((person) => ({ value: person.id, label: person.displayName })),
)

/**
 * Mirrors the backend's rules so a slip costs no round trip. The backend
 * re-checks every one and its answer wins.
 */
const localErrors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (!delegateUserId.value) {
    found.delegateUserId = 'Choose who takes it'
  }

  if (!startsAt.value) {
    found.startsAt = 'Enter when it starts'
  }

  if (!endsAt.value) {
    found.endsAt = 'Enter when it ends'
  } else if (startsAt.value && new Date(endsAt.value) <= new Date(startsAt.value)) {
    found.endsAt = 'It has to end after it starts'
  } else if (new Date(endsAt.value) <= new Date()) {
    // The mistake this catches is a year typed wrong. Stored, it would be cover
    // somebody believes is in place and which routes nothing.
    found.endsAt = 'This window has already ended'
  }

  if (scope.value === 'DOCUMENT_TYPE' && !documentTypeId.value) {
    found.documentTypeId = 'Choose the type it covers'
  }

  return found
})

const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...fieldErrors.value,
}))

async function loadChoices(): Promise<void> {
  try {
    const page = await listUsers({ page: 1, pageSize: 100 })

    people.value = page.items.filter(
      // Not yourself, and nobody who cannot sign in. `ck_delegations_not_self`
      // refuses the first and the service refuses the second; offering either
      // would be a choice the product then declines.
      (person) => person.id !== auth.user?.id && person.status === 'ACTIVE',
    )
  } catch {
    people.value = []
  }

  try {
    const types = await listDocumentTypes({ page: 1, pageSize: 100 })

    documentTypes.value = types.items.map((type) => ({ value: type.id, label: type.name }))
  } catch {
    // A caller who may not read document types can still open an `ALL` window,
    // which is the common case. The narrower choice is what goes missing.
    documentTypes.value = []
  }
}

function resetForm(): void {
  delegateUserId.value = ''
  startsAt.value = ''
  endsAt.value = ''
  scope.value = 'ALL'
  documentTypeId.value = ''
  reason.value = ''
  submitted.value = false
  reset()
}

watch(
  open,
  (isOpen) => {
    if (isOpen) {
      resetForm()
      void loadChoices()
    }
  },
  { immediate: true },
)

async function submit(): Promise<void> {
  submitted.value = true
  reset()

  if (Object.keys(localErrors.value).length > 0) {
    return
  }

  isSaving.value = true

  try {
    const saved = await createDelegation({
      delegateUserId: delegateUserId.value,
      // `datetime-local` has no zone, and the API takes an instant. Converting
      // here means the window means what the person entering it meant — their
      // own clock — rather than whatever the server's happens to be.
      startsAt: new Date(startsAt.value).toISOString(),
      endsAt: new Date(endsAt.value).toISOString(),
      scope: scope.value,
      ...(scope.value === 'DOCUMENT_TYPE' ? { documentTypeId: documentTypeId.value } : {}),
      ...(reason.value.trim() ? { reason: reason.value.trim() } : {}),
    })

    emit('saved', saved)
    open.value = false
  } catch (error) {
    report(error)
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <Dialog
    v-model:open="open"
    title="Delegate my work"
    description="Approvals that would reach you go to somebody else while this window is open. It
                 grants them nothing — they act with their own account, and every decision records
                 both names."
  >
    <form id="delegation-form" class="space-y-4" novalidate @submit.prevent="submit">
      <Alert v-if="formError" variant="destructive">{{ formError }}</Alert>

      <div class="space-y-2">
        <Label for="delegation-delegate">Hand my approvals to</Label>
        <Select
          id="delegation-delegate"
          v-model="delegateUserId"
          :options="peopleOptions"
          placeholder="Choose somebody"
          :disabled="isSaving"
          :invalid="Boolean(errors.delegateUserId)"
          described-by="delegation-delegate-error"
          @update:model-value="clearField('delegateUserId')"
        />
        <p
          v-if="errors.delegateUserId"
          id="delegation-delegate-error"
          class="text-sm text-destructive"
        >
          {{ errors.delegateUserId }}
        </p>
      </div>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="space-y-2">
          <Label for="delegation-starts">From</Label>
          <Input
            id="delegation-starts"
            v-model="startsAt"
            type="datetime-local"
            :disabled="isSaving"
            :invalid="Boolean(errors.startsAt)"
            described-by="delegation-starts-error"
            @update:model-value="clearField('startsAt')"
          />
          <p v-if="errors.startsAt" id="delegation-starts-error" class="text-sm text-destructive">
            {{ errors.startsAt }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="delegation-ends">Until</Label>
          <Input
            id="delegation-ends"
            v-model="endsAt"
            type="datetime-local"
            :disabled="isSaving"
            :invalid="Boolean(errors.endsAt)"
            described-by="delegation-ends-error"
            @update:model-value="clearField('endsAt')"
          />
          <p v-if="errors.endsAt" id="delegation-ends-error" class="text-sm text-destructive">
            {{ errors.endsAt }}
          </p>
          <p v-else class="text-xs text-muted-foreground">It stops at this moment, not after it.</p>
        </div>
      </div>

      <div class="space-y-2">
        <Label for="delegation-scope">Covers</Label>
        <Select
          id="delegation-scope"
          v-model="scope"
          :options="scopeOptions"
          :disabled="isSaving"
        />
        <p class="text-xs text-muted-foreground">
          Work already on your desk stays there either way — hand those over one at a time, from the
          task itself.
        </p>
      </div>

      <div v-if="scope === 'DOCUMENT_TYPE'" class="space-y-2">
        <Label for="delegation-type">Document type</Label>
        <Select
          id="delegation-type"
          v-model="documentTypeId"
          :options="documentTypes"
          placeholder="Choose a type"
          :disabled="isSaving"
          :invalid="Boolean(errors.documentTypeId)"
          described-by="delegation-type-error"
          @update:model-value="clearField('documentTypeId')"
        />
        <p v-if="errors.documentTypeId" id="delegation-type-error" class="text-sm text-destructive">
          {{ errors.documentTypeId }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="delegation-reason">
          Why
          <span class="font-normal text-muted-foreground">(optional)</span>
        </Label>
        <Textarea
          id="delegation-reason"
          v-model="reason"
          :rows="2"
          :disabled="isSaving"
          placeholder="Annual leave"
        />
      </div>
    </form>

    <template #footer>
      <Button variant="outline" :disabled="isSaving" @click="open = false">Cancel</Button>
      <Button type="submit" form="delegation-form" :loading="isSaving">Open the window</Button>
    </template>
  </Dialog>
</template>
