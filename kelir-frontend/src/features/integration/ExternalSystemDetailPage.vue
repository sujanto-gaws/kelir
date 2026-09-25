<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, useRoute } from 'vue-router'

import { toApiError } from '@/api/client'
import {
  activateExternalSystem,
  deactivateExternalSystem,
  getExternalSystem,
} from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/features/admin/ConfirmDialog.vue'
import { useAuthStore } from '@/stores/auth'
import {
  AUTH_TYPE_LABELS,
  EXTERNAL_SYSTEM_STATUS_LABELS,
  EXTERNAL_SYSTEM_TYPE_LABELS,
  type ExternalSystem,
} from '@/types/integration'

import ExternalSystemFormDialog from './ExternalSystemFormDialog.vue'
import IntegrationCredentialSection from './IntegrationCredentialSection.vue'
import IntegrationEndpointSection from './IntegrationEndpointSection.vue'

/**
 * One external system, with its endpoints and credential references
 * (FR-INT-001, #520; architectures/03 §3.1).
 *
 * **Each action is offered only with its own permission**: edit with
 * `integration:external-system:update`, and activate and deactivate with
 * `:deactivate` — turning a system on or off is one permission, and the only
 * way into or out of `INACTIVE`. Endpoints ride on the system's permissions;
 * credential references have their own, and their section is absent for a
 * caller who may not read them.
 *
 * There is no delete. A system is deactivated, and stays listed.
 */
const route = useRoute()
const auth = useAuthStore()

const canUpdate = computed(() => auth.can('integration:external-system:update'))
const canToggle = computed(() => auth.can('integration:external-system:deactivate'))

const systemId = computed(() => String(route.params.id ?? ''))

const system = ref<ExternalSystem | null>(null)
const isLoading = ref(true)
const loadError = ref('')
const isNotFound = ref(false)

const isEditOpen = ref(false)

type Toggle = 'activate' | 'deactivate'

const toggling = ref<Toggle | null>(null)
const isConfirmOpen = ref(false)
const isToggling = ref(false)
const toggleError = ref('')

const isInactive = computed(() => system.value?.status === 'INACTIVE')

const confirmCopy = computed(() => {
  const code = system.value?.systemCode ?? 'This system'

  return toggling.value === 'activate'
    ? {
        title: 'Activate external system',
        description: `${code} will be available to integrations again.`,
        confirmLabel: 'Activate',
      }
    : {
        title: 'Deactivate external system',
        description: `${code} will no longer be called by any integration. It stays registered and can be activated again.`,
        confirmLabel: 'Deactivate',
      }
})

async function load(): Promise<void> {
  isLoading.value = true
  loadError.value = ''
  isNotFound.value = false

  try {
    system.value = await getExternalSystem(systemId.value)
  } catch (failure) {
    const error = toApiError(failure)

    system.value = null
    isNotFound.value = error.status === 404
    loadError.value = error.message
  } finally {
    isLoading.value = false
  }
}

function startToggle(action: Toggle): void {
  toggling.value = action
  toggleError.value = ''
  isConfirmOpen.value = true
}

async function toggle(): Promise<void> {
  const current = system.value

  if (!current || !toggling.value) {
    return
  }

  isToggling.value = true
  toggleError.value = ''

  try {
    // The response is the system as it now stands, so the page shows the
    // server's status rather than the one it asked for.
    system.value =
      toggling.value === 'activate'
        ? await activateExternalSystem(current.id)
        : await deactivateExternalSystem(current.id)
    isConfirmOpen.value = false
    toggling.value = null
  } catch (failure) {
    toggleError.value = toApiError(failure).message
  } finally {
    isToggling.value = false
  }
}

function afterEdit(saved: ExternalSystem): void {
  system.value = saved
}

function statusVariant(): 'default' | 'secondary' | 'outline' {
  const status = system.value?.status

  if (status === 'ACTIVE') {
    return 'default'
  }

  return status === 'MAINTENANCE' ? 'outline' : 'secondary'
}

const retrySummary = computed(() => {
  const policy = system.value?.retryPolicy ?? {}
  const parts: string[] = []

  if (policy.maxRetries !== undefined) parts.push(`${policy.maxRetries} retries`)
  if (policy.initialDelaySeconds !== undefined)
    parts.push(`${policy.initialDelaySeconds}s initial delay`)
  if (policy.backoffMultiplier !== undefined) parts.push(`×${policy.backoffMultiplier} backoff`)
  if (policy.deadLetterAfterAttempts !== undefined)
    parts.push(`dead-letter after ${policy.deadLetterAfterAttempts} attempts`)

  return parts.length > 0 ? parts.join(', ') : 'Defaults'
})

watch(systemId, () => void load(), { immediate: true })
</script>

<template>
  <section class="space-y-8">
    <RouterLink
      :to="{ name: 'admin-external-systems' }"
      class="text-sm text-muted-foreground hover:underline"
    >
      ← External systems
    </RouterLink>

    <p v-if="isLoading" class="text-sm text-muted-foreground">Loading external system…</p>

    <Alert v-else-if="loadError" variant="destructive" data-testid="external-system-error">
      <p>{{ isNotFound ? 'This external system does not exist.' : loadError }}</p>
      <Button v-if="!isNotFound" variant="outline" size="sm" class="mt-3" @click="load()">
        Try again
      </Button>
    </Alert>

    <template v-else-if="system">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="space-y-1">
          <h2 class="text-xl font-semibold tracking-tight" data-testid="external-system-name">
            {{ system.systemName }}
          </h2>
          <p class="flex items-center gap-2 text-sm text-muted-foreground">
            <code data-testid="external-system-code">{{ system.systemCode }}</code>
            <Badge :variant="statusVariant()" data-testid="external-system-status">
              {{ EXTERNAL_SYSTEM_STATUS_LABELS[system.status] }}
            </Badge>
          </p>
        </div>

        <div class="flex flex-wrap gap-2">
          <Button
            v-if="canUpdate"
            variant="secondary"
            data-testid="edit-external-system"
            @click="isEditOpen = true"
          >
            Edit
          </Button>
          <Button
            v-if="canToggle && isInactive"
            data-testid="activate-external-system"
            @click="startToggle('activate')"
          >
            Activate
          </Button>
          <Button
            v-if="canToggle && !isInactive"
            variant="destructive"
            data-testid="deactivate-external-system"
            @click="startToggle('deactivate')"
          >
            Deactivate
          </Button>
        </div>
      </div>

      <dl class="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-[12rem_1fr]">
        <dt class="text-muted-foreground">Type</dt>
        <dd data-testid="external-system-type">
          {{ system.systemType ? EXTERNAL_SYSTEM_TYPE_LABELS[system.systemType] : '—' }}
        </dd>

        <dt class="text-muted-foreground">Base URL</dt>
        <dd data-testid="external-system-base-url" class="break-all">
          {{ system.baseUrl ?? '—' }}
        </dd>

        <dt class="text-muted-foreground">Authentication</dt>
        <dd data-testid="external-system-auth-type">
          {{ system.authType ? AUTH_TYPE_LABELS[system.authType] : '—' }}
        </dd>

        <dt class="text-muted-foreground">Timeout</dt>
        <dd data-testid="external-system-timeout">{{ system.timeoutSeconds }} seconds</dd>

        <dt class="text-muted-foreground">Retry policy</dt>
        <dd data-testid="external-system-retry-policy">{{ retrySummary }}</dd>

        <dt class="text-muted-foreground">Description</dt>
        <dd data-testid="external-system-description" class="whitespace-pre-line">
          {{ system.description ?? '—' }}
        </dd>
      </dl>

      <IntegrationEndpointSection :system-id="system.id" :can-update="canUpdate" />

      <IntegrationCredentialSection :system-id="system.id" />

      <ExternalSystemFormDialog
        v-if="canUpdate"
        v-model:open="isEditOpen"
        :editing="system"
        @saved="afterEdit"
      />

      <ConfirmDialog
        v-model:open="isConfirmOpen"
        :title="confirmCopy.title"
        :description="confirmCopy.description"
        :confirm-label="confirmCopy.confirmLabel"
        :error="toggleError"
        :pending="isToggling"
        @confirm="toggle()"
      />
    </template>
  </section>
</template>
