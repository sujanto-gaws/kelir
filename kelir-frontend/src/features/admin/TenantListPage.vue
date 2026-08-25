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
import { deleteTenant, listTenants } from '@/api/organization'
import { toApiError } from '@/api/client'
import { usePaginatedList } from '@/composables/usePaginatedList'
import { useAuthStore } from '@/stores/auth'
import { TENANT_STATUS_LABELS, type Tenant } from '@/types/organization'
import ConfirmDialog from './ConfirmDialog.vue'
import TenantFormDialog from './TenantFormDialog.vue'

/**
 * Tenant administration (FR-ORG-001).
 *
 * **This screen is reachable from one tenant only** — the deployment's default
 * one, the tenant the first administrator lives in. The `can()` gates below
 * cannot express that half of the rule, because a permission is all a token
 * carries; the backend checks the caller's tenant as well and answers 403.
 * What keeps the screen from appearing where it would only refuse is that a
 * tenant's own administrator is never granted `organization:tenant:*` in the
 * first place (decision **D-18**).
 */
const auth = useAuthStore()

const canManage = computed(() => auth.can('organization:tenant:manage'))

const tenants = usePaginatedList<Tenant>(listTenants)

const isFormOpen = ref(false)
const editing = ref<Tenant | null>(null)

const confirming = ref<Tenant | null>(null)
const isConfirmOpen = ref(false)
const isDeleting = ref(false)
const deleteError = ref('')

function openCreate(): void {
  editing.value = null
  isFormOpen.value = true
}

function openEdit(tenant: Tenant): void {
  editing.value = tenant
  isFormOpen.value = true
}

function openDelete(tenant: Tenant): void {
  confirming.value = tenant
  deleteError.value = ''
  isConfirmOpen.value = true
}

/**
 * What deleting a tenant actually does, said before it is done.
 *
 * The user count matters here and nowhere else on the screen: this is the
 * number of people who are signed out by the confirmation they are about to
 * give.
 */
const deleteDescription = computed(() => {
  const target = confirming.value

  if (!target) {
    return 'This tenant will be removed.'
  }

  return (
    `${target.name} (${target.tenantCode}) will be removed. ` +
    `Its ${target.userCount} user(s) are signed out and can no longer sign in. ` +
    'Its data is left in place but becomes unreachable.'
  )
})

async function onSaved(): Promise<void> {
  await tenants.refresh()
}

async function confirmDelete(): Promise<void> {
  const target = confirming.value

  if (!target) {
    return
  }

  isDeleting.value = true
  deleteError.value = ''

  try {
    await deleteTenant(target.id)
    isConfirmOpen.value = false
    confirming.value = null
    await tenants.refresh()
  } catch (error) {
    // Includes the 400 for the administering tenant — the server's own wording,
    // not ours.
    deleteError.value = toApiError(error).message
  } finally {
    isDeleting.value = false
  }
}

onMounted(async () => {
  await tenants.load()
})
</script>

<template>
  <section class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-xl font-semibold tracking-tight">Tenants</h2>
        <p class="mt-1 text-sm text-muted-foreground">
          Each tenant is a separate organization on this deployment, with its own users, roles and
          data. A tenant is created together with the administrator who runs it.
        </p>
      </div>

      <Button v-if="canManage" @click="openCreate()">New tenant</Button>
    </div>

    <Alert v-if="tenants.error.value" variant="destructive">
      <p>{{ tenants.error.value }}</p>
      <Button variant="outline" size="sm" class="mt-3" @click="tenants.load()">Try again</Button>
    </Alert>

    <p v-if="tenants.isLoading.value" class="text-sm text-muted-foreground">Loading tenants…</p>

    <template v-else-if="!tenants.error.value">
      <p v-if="tenants.items.value.length === 0" class="text-sm text-muted-foreground">
        No tenants to show.
      </p>

      <Table v-else>
        <TableHeader>
          <TableRow>
            <TableHead>Code</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Users</TableHead>
            <TableHead v-if="canManage" class="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>

        <TableBody>
          <TableRow v-for="tenant in tenants.items.value" :key="tenant.id">
            <TableCell class="font-medium">
              <span class="font-mono text-xs">{{ tenant.tenantCode }}</span>
              <Badge v-if="tenant.isDefault" variant="secondary" class="ml-2"
                >This deployment</Badge
              >
            </TableCell>
            <TableCell>{{ tenant.name }}</TableCell>
            <TableCell>
              <Badge :variant="tenant.status === 'ACTIVE' ? 'default' : 'secondary'">
                {{ TENANT_STATUS_LABELS[tenant.status] }}
              </Badge>
            </TableCell>
            <TableCell>{{ tenant.userCount }}</TableCell>
            <TableCell v-if="canManage" class="text-right">
              <div class="flex justify-end gap-2">
                <Button variant="outline" size="sm" @click="openEdit(tenant)">Edit</Button>
                <Button
                  variant="ghost"
                  size="sm"
                  :disabled="tenant.isDefault"
                  :title="
                    tenant.isDefault
                      ? 'You cannot delete the tenant you administer from'
                      : 'Delete this tenant'
                  "
                  @click="openDelete(tenant)"
                >
                  Delete
                </Button>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>

      <div v-if="tenants.items.value.length > 0" class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">
          Page {{ tenants.page.value }} of {{ tenants.totalPages.value }} ·
          {{ tenants.total.value }} tenants
        </p>
        <div class="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            :disabled="!tenants.hasPrevious.value"
            @click="tenants.goToPage(tenants.page.value - 1)"
          >
            Previous
          </Button>
          <Button
            variant="outline"
            size="sm"
            :disabled="!tenants.hasNext.value"
            @click="tenants.goToPage(tenants.page.value + 1)"
          >
            Next
          </Button>
        </div>
      </div>
    </template>

    <TenantFormDialog v-model:open="isFormOpen" :tenant="editing" @saved="onSaved()" />

    <ConfirmDialog
      v-model:open="isConfirmOpen"
      title="Delete tenant"
      :description="deleteDescription"
      confirm-label="Delete"
      :error="deleteError"
      :pending="isDeleting"
      @confirm="confirmDelete()"
    />
  </section>
</template>
