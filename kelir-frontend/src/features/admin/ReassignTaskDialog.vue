<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Select } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { toApiError } from '@/api/client'
import { listRoles, listUsers } from '@/api/identity'
import { reassignTask } from '@/api/tasks'
import { useFormErrors } from '@/composables/useFormErrors'
import { unplacedErrors } from '@/features/integration/form-errors'
import type { Page, PageQuery } from '@/types/api'
import type { OpenTaskNeedingRole, Role, User } from '@/types/identity'
import type { ReassignTarget, WorkflowTask } from '@/types/workflow'

/**
 * An administrator reassigns an open task (FR-WF-017, #512, ADR-0042).
 *
 * Opened from the open tasks a role is needed by (User Manual §11.2), listed on
 * a refused role delete or from a role nobody holds (#508), by a caller holding `workflow:task:reassign`; the list itself needs only
 * `identity:role:delete`, so `RoleListPage` decides whether the action is drawn.
 *
 * # A user or a role, never both
 *
 * The task is then held by the user, or offered unclaimed to the role's holders.
 * The two are one choice, so the form holds one target at a time and switching
 * drops the other: the request can never name both, which the server refuses.
 * Choosing nobody is caught before sending, as a presence check (coding
 * standard §3.4); everything else is the server's to decide.
 *
 * # The server says who can decide it
 *
 * A target that could not decide the task is refused as `TARGET_CANNOT_DECIDE`,
 * and a dead one as `ASSIGNMENT_UNRESOLVED`, each on the field that named it.
 * Neither is predicted here: whether a user satisfies an edge's `allowedBy` is
 * the decision's own check, and a second copy of it would drift. A task decided
 * meanwhile is a 409, shown on the form.
 *
 * **The role the task is being moved away from is not offered.** Reassigning it
 * to the role whose delete it blocks would change nothing.
 */
const props = defineProps<{
  task: OpenTaskNeedingRole | null
  /** The role the task is moved away from, left out of the choices. */
  fromRole?: Role | null
}>()

const emit = defineEmits<{ reassigned: [task: WorkflowTask] }>()

const open = defineModel<boolean>('open', { default: false })

type TargetKind = 'role' | 'user'

/** The paths this form renders an input for; any other detail is listed. */
const PLACED = ['userId', 'roleCode', 'comment'] as const

const kind = ref<TargetKind>('role')
const userId = ref('')
const roleCode = ref('')
const comment = ref('')

const users = ref<User[]>([])
const roles = ref<Role[]>([])
const choicesError = ref('')
const isLoadingChoices = ref(false)
const isSaving = ref(false)
const submitted = ref(false)

const { fieldErrors, formError, report, reset, clearField } = useFormErrors()

const userOptions = computed(() =>
  users.value.map((user) => ({
    value: user.id,
    label: `${user.displayName} (${user.username})`,
  })),
)

const roleOptions = computed(() =>
  roles.value
    .filter((role) => role.roleCode !== props.fromRole?.roleCode)
    .map((role) => ({ value: role.roleCode, label: `${role.name} (${role.roleCode})` })),
)

/** The presence check: a target has to be chosen before anything is sent. */
const localErrors = computed<Record<string, string>>(() => {
  const found: Record<string, string> = {}

  if (!submitted.value) {
    return found
  }

  if (kind.value === 'user' && !userId.value) {
    found.userId = 'Choose the user who holds it next'
  }

  if (kind.value === 'role' && !roleCode.value) {
    found.roleCode = 'Choose the role it is offered to next'
  }

  return found
})

const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...fieldErrors.value,
}))

/**
 * Details for a path no input on screen reads. The target field not chosen is
 * not rendered, so a detail addressed to it is listed rather than lost.
 */
const unplaced = computed(() =>
  unplacedErrors(
    fieldErrors.value,
    PLACED.filter((path) => path !== (kind.value === 'user' ? 'roleCode' : 'userId')),
  ),
)

/**
 * Every page of a list, not the first hundred.
 *
 * The identity lists have no search and page at a hundred, so a first page
 * alone would leave out whoever sorts past it, with nothing saying so.
 */
async function readEvery<T>(fetcher: (query: PageQuery) => Promise<Page<T>>): Promise<T[]> {
  const items: T[] = []

  for (let page = 1; ; page += 1) {
    const result = await fetcher({ page, pageSize: 100 })

    items.push(...result.items)

    if (result.items.length === 0 || items.length >= result.meta.total) {
      return items
    }
  }
}

async function loadChoices(): Promise<void> {
  isLoadingChoices.value = true
  choicesError.value = ''

  try {
    const [everyUser, everyRole] = await Promise.all([readEvery(listUsers), readEvery(listRoles)])

    // A deactivated user is not a live target; the server would refuse them.
    users.value = everyUser.filter((user) => user.status === 'ACTIVE')
    roles.value = everyRole
  } catch (error) {
    users.value = []
    roles.value = []
    choicesError.value = toApiError(error).message
  } finally {
    isLoadingChoices.value = false
  }
}

function resetForm(): void {
  kind.value = 'role'
  userId.value = ''
  roleCode.value = ''
  comment.value = ''
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

// One target at a time: the one not chosen is dropped, with its error.
watch(kind, (next) => {
  if (next === 'user') {
    roleCode.value = ''
    clearField('roleCode')
  } else {
    userId.value = ''
    clearField('userId')
  }
})

function target(): ReassignTarget {
  return kind.value === 'user' ? { userId: userId.value } : { roleCode: roleCode.value }
}

async function submit(): Promise<void> {
  const task = props.task

  submitted.value = true
  reset()

  if (!task || Object.keys(localErrors.value).length > 0) {
    return
  }

  isSaving.value = true

  try {
    const reassigned = await reassignTask(task.id, target(), comment.value)

    emit('reassigned', reassigned)
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
    :title="`Reassign ${task?.taskRef ?? 'the task'}`"
    description="Give the task to a user, who then holds it, or to a role, whose holders are offered
                 it unclaimed. It keeps its place in the workflow."
  >
    <form id="reassign-task-form" class="space-y-4" novalidate @submit.prevent="submit">
      <Alert v-if="formError" variant="destructive" data-testid="reassign-task-error">
        {{ formError }}
      </Alert>

      <Alert v-if="choicesError" variant="destructive">
        The users and roles could not be listed: {{ choicesError }}
        <Button variant="outline" size="sm" class="mt-3" type="button" @click="loadChoices()">
          Try again
        </Button>
      </Alert>

      <Alert v-if="unplaced.length > 0" variant="destructive" data-testid="reassign-task-unplaced">
        <ul class="list-disc pl-4">
          <li v-for="item in unplaced" :key="item.path">{{ item.path }}: {{ item.message }}</li>
        </ul>
      </Alert>

      <fieldset class="space-y-2">
        <legend class="text-sm font-medium">Reassign to</legend>
        <div class="flex gap-6">
          <label class="flex cursor-pointer items-center gap-2">
            <input
              v-model="kind"
              type="radio"
              name="reassign-task-kind"
              value="role"
              :disabled="isSaving"
            />
            A role
          </label>
          <label class="flex cursor-pointer items-center gap-2">
            <input
              v-model="kind"
              type="radio"
              name="reassign-task-kind"
              value="user"
              :disabled="isSaving"
            />
            A user
          </label>
        </div>
      </fieldset>

      <div v-if="kind === 'role'" class="space-y-2">
        <Label for="reassign-task-role">Role</Label>
        <Select
          id="reassign-task-role"
          v-model="roleCode"
          :options="roleOptions"
          :placeholder="isLoadingChoices ? 'Loading the roles…' : 'Choose a role'"
          :disabled="isSaving || isLoadingChoices"
          :invalid="Boolean(errors.roleCode)"
          described-by="reassign-task-role-error"
          @update:model-value="clearField('roleCode')"
        />
        <p v-if="errors.roleCode" id="reassign-task-role-error" class="text-sm text-destructive">
          {{ errors.roleCode }}
        </p>
      </div>

      <div v-else class="space-y-2">
        <Label for="reassign-task-user">User</Label>
        <Select
          id="reassign-task-user"
          v-model="userId"
          :options="userOptions"
          :placeholder="isLoadingChoices ? 'Loading the users…' : 'Choose a user'"
          :disabled="isSaving || isLoadingChoices"
          :invalid="Boolean(errors.userId)"
          described-by="reassign-task-user-error"
          @update:model-value="clearField('userId')"
        />
        <p v-if="errors.userId" id="reassign-task-user-error" class="text-sm text-destructive">
          {{ errors.userId }}
        </p>
      </div>

      <div class="space-y-2">
        <Label for="reassign-task-comment">
          Why
          <span class="font-normal text-muted-foreground">(optional)</span>
        </Label>
        <Textarea
          id="reassign-task-comment"
          v-model="comment"
          :rows="2"
          :disabled="isSaving"
          :invalid="Boolean(errors.comment)"
          described-by="reassign-task-comment-error"
          @update:model-value="clearField('comment')"
        />
        <p v-if="errors.comment" id="reassign-task-comment-error" class="text-sm text-destructive">
          {{ errors.comment }}
        </p>
        <p v-else class="text-xs text-muted-foreground">Kept in the task's history.</p>
      </div>
    </form>

    <template #footer>
      <Button variant="outline" :disabled="isSaving" @click="open = false">Cancel</Button>
      <Button type="submit" form="reassign-task-form" :loading="isSaving">Reassign</Button>
    </template>
  </Dialog>
</template>
