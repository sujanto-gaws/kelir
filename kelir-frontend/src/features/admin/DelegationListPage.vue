<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { endDelegation, listDelegations } from '@/api/identity'
import { toApiError } from '@/api/client'
import { usePaginatedList } from '@/composables/usePaginatedList'
import { useAuthStore } from '@/stores/auth'
import type { Delegation } from '@/types/identity'
import ConfirmDialog from './ConfirmDialog.vue'
import DelegationFormDialog from './DelegationFormDialog.vue'

/**
 * Delegation windows (FR-IDM-006, #184).
 *
 * # Two verbs, and neither of them is "edit"
 *
 * A window is **opened** and **ended**. There is no `identity:delegation:update`
 * in the catalogue and no edit control here, which is the same decision
 * `0005_delegation_tenant_permissions.sql` recorded when it seeded three
 * permissions rather than four: a window is what approvals are being routed by
 * while it stands, and editing one in place would change where work went with
 * nothing saying it had changed. Ending it and opening another leaves both
 * facts on the screen.
 *
 * # Opening one is in your own name; ending one is administrative
 *
 * The form has no "delegator" field. **Nobody hands over another person's
 * authority** — a holder of `identity:delegation:create` who could name somebody
 * else would be able to point the finance director's approvals at themselves,
 * and the row would look exactly like legitimate cover. Reading and ending are
 * not restricted that way, because the row somebody has to be able to find is
 * the one whose owner went on leave without ending it.
 *
 * # "Active" and "routing" are two columns because they are two facts
 *
 * A window ends by being switched off *or* by its end passing. A screen showing
 * only the flag would report finished cover as live, and one showing only the
 * dates could not tell cover that was cancelled from cover that ran its course.
 * Both come off the server, computed in the same statement the routing decision
 * uses, so this screen cannot disagree with the engine about which windows are
 * live.
 */
const auth = useAuthStore()

const canOpen = computed(() => auth.can('identity:delegation:create'))
const canEnd = computed(() => auth.can('identity:delegation:delete'))

const delegations = usePaginatedList<Delegation>(listDelegations)

const isFormOpen = ref(false)

const confirming = ref<Delegation | null>(null)
const isConfirmOpen = ref(false)
const isEnding = ref(false)
const endError = ref('')

function openCreate(): void {
  isFormOpen.value = true
}

function openEnd(delegation: Delegation): void {
  confirming.value = delegation
  endError.value = ''
  isConfirmOpen.value = true
}

/**
 * What ending a window does, said before it is done — including the half people
 * get wrong.
 *
 * Ending it stops work *arriving*. Tasks already handed over stay where they
 * are, for the same reason opening a window does not reach back for tasks
 * already assigned: moving approvals out from under somebody mid-decision, on a
 * schedule nobody triggered, is the failure both halves of that decision avoid.
 */
const endDescription = computed(() => {
  const target = confirming.value

  if (!target) {
    return 'This delegation will stop routing.'
  }

  return (
    `Work for ${target.delegatorDisplayName} stops reaching ` +
    `${target.delegateDisplayName} from now. Tasks already in their hands stay there — ` +
    'those are handed back one at a time, from the task itself. The window stays on this ' +
    'list, so how the last few weeks were routed is still readable.'
  )
})

/** A window's own dates, as a person reads them. */
function period(delegation: Delegation): string {
  return `${new Date(delegation.startsAt).toLocaleDateString()} – ${new Date(
    delegation.endsAt,
  ).toLocaleDateString()}`
}

async function onSaved(): Promise<void> {
  await delegations.refresh()
}

async function confirmEnd(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isEnding.value = true
  endError.value = ''

  try {
    await endDelegation(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await delegations.refresh()
  } catch (error) {
    endError.value = toApiError(error).message
  } finally {
    isEnding.value = false
  }
}

onMounted(async () => {
  await delegations.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Delegations</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          A delegation sends the approvals that would reach one person to somebody else for a
          stretch of time. It grants nothing: the delegate acts with their own account and their own
          permissions, and every decision records both names.
        </p>
      </div>

      <Button v-if="canOpen" data-testid="open-delegation" @click="openCreate()">
        Delegate my work
      </Button>
    </div>

    <Alert v-if="delegations.error.value" variant="destructive">
      <p>{{ delegations.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="delegations.load()">
        Try again
      </Button>
    </Alert>

    <p v-if="delegations.isLoading.value" class="text-sm text-muted-foreground">
      Loading delegations…
    </p>

    <template v-else-if="!delegations.error.value">
      <p
        v-if="delegations.items.value.length === 0"
        class="text-sm text-muted-foreground"
        data-testid="no-delegations"
      >
        Nobody has delegated anything.
      </p>

      <Table v-else>
        <TableHeader>
          <TableRow>
            <TableHead>From</TableHead>
            <TableHead>To</TableHead>
            <TableHead>Covers</TableHead>
            <TableHead>Period</TableHead>
            <TableHead>State</TableHead>
            <TableHead v-if="canEnd" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow
            v-for="delegation in delegations.items.value"
            :key="delegation.id"
            :data-testid="`delegation-${delegation.id}`"
          >
            <TableCell class="font-medium">{{ delegation.delegatorDisplayName }}</TableCell>
            <TableCell>{{ delegation.delegateDisplayName }}</TableCell>
            <TableCell>
              {{ delegation.scope === 'ALL' ? 'Everything' : 'One document type' }}
            </TableCell>
            <TableCell>{{ period(delegation) }}</TableCell>
            <TableCell>
              <!-- Three states, not two. "Routing" is what a person came here to
                   check; "scheduled" is cover that is set up and has not started;
                   "ended" covers both a window that was switched off and one
                   whose time is simply up. -->
              <Badge v-if="delegation.isRouting" data-testid="delegation-state">Routing now</Badge>
              <Badge
                v-else-if="delegation.isActive"
                variant="secondary"
                data-testid="delegation-state"
              >
                Scheduled
              </Badge>
              <Badge v-else variant="outline" data-testid="delegation-state">Ended</Badge>
            </TableCell>
            <TableCell v-if="canEnd" class="text-right">
              <Button
                variant="ghost"
                size="sm"
                :disabled="!delegation.isActive"
                :title="
                  delegation.isActive ? 'Stop this delegation' : 'This delegation has already ended'
                "
                :data-testid="`end-delegation-${delegation.id}`"
                @click="openEnd(delegation)"
              >
                End it
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div
        v-if="delegations.items.value.length > 0"
        class="flex items-center justify-between gap-3"
      >
        <p class="text-sm text-muted-foreground">
          Page {{ delegations.page.value }} of {{ delegations.totalPages.value }} ·
          {{ delegations.total.value }} delegations
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!delegations.hasPrevious.value"
            @click="delegations.goToPage(delegations.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!delegations.hasNext.value"
            @click="delegations.goToPage(delegations.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <DelegationFormDialog v-model:open="isFormOpen" @saved="onSaved()" />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="End delegation"
      :description="endDescription"
      confirm-label="End it"
      :error="endError"
      :pending="isEnding"
      @confirm="confirmEnd()"
    />
  </section>
</template>
