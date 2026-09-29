<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'

import { toApiError } from '@/api/client'
import { getIntegrationLog } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useAuthStore } from '@/stores/auth'
import {
  INTEGRATION_DIRECTION_LABELS,
  INTEGRATION_LOG_STATUS_LABELS,
  type IntegrationLog,
} from '@/types/integration'

import {
  formatDuration,
  formatPayload,
  formatTimestamp,
  isLogId,
  logStatusVariant,
} from './integration-log'

/**
 * One integration log row in full (FR-INT-006, #548): every column, and the
 * request and response payloads pretty-printed.
 *
 * **The payloads are shown as stored.** They were masked when the row was
 * written, and nothing here or behind the route resolves them again (#548
 * AC3). A payload the row does not have says so rather than drawing `null`.
 *
 * **Two links, each behind the permission of the screen it opens**: the
 * system's page needs `integration:external-system:read`, which a log reader
 * need not hold, and the document needs `document:read`. Without it the value
 * is shown as text. The route behind each still decides.
 */
const props = defineProps<{
  /** The row to show; `null` shows nothing. */
  logId: string | null
}>()

const open = defineModel<boolean>('open', { default: false })

const auth = useAuthStore()

const canOpenSystem = computed(() => auth.can('integration:external-system:read'))
const canOpenDocument = computed(() => auth.can('document:read'))

const NOT_FOUND = 'There is no integration log with this id.'

const log = ref<IntegrationLog | null>(null)
const isLoading = ref(false)
const loadError = ref('')
const isNotFound = ref(false)

/** Which load is current, so a slow earlier one cannot land on a later row. */
let latest = 0

async function load(id: string): Promise<void> {
  const ticket = ++latest

  log.value = null
  loadError.value = ''
  isNotFound.value = false

  // The id comes from the URL: one that cannot name a row is not found, and
  // is never sent.
  if (!isLogId(id)) {
    isNotFound.value = true
    loadError.value = NOT_FOUND
    isLoading.value = false

    return
  }

  isLoading.value = true

  try {
    const loaded = await getIntegrationLog(id)

    if (ticket === latest) {
      log.value = loaded
    }
  } catch (error) {
    if (ticket !== latest) {
      return
    }

    const apiError = toApiError(error)
    isNotFound.value = apiError.status === 404
    loadError.value = apiError.message
  } finally {
    if (ticket === latest) {
      isLoading.value = false
    }
  }
}

watch(
  () => [open.value, props.logId] as const,
  ([isOpen, id]) => {
    if (isOpen && id) {
      void load(id)
    } else {
      latest += 1
    }
  },
  { immediate: true },
)

const requestPayload = computed(() => formatPayload(log.value?.requestPayload))
const responsePayload = computed(() => formatPayload(log.value?.responsePayload))

const systemLabel = computed(() => {
  const row = log.value

  if (!row?.externalSystemId) {
    return '—'
  }

  if (row.externalSystemCode && row.externalSystemName) {
    return `${row.externalSystemCode} · ${row.externalSystemName}`
  }

  return row.externalSystemCode ?? row.externalSystemName ?? row.externalSystemId
})
</script>

<template>
  <Dialog v-model:open="open" title="Integration log" class="max-w-3xl">
    <div data-testid="integration-log-detail" class="space-y-4">
      <p v-if="isLoading" class="text-muted-foreground" role="status">Loading the log…</p>

      <Alert v-else-if="loadError" variant="destructive" data-testid="integration-log-detail-error">
        <p>
          {{ isNotFound ? NOT_FOUND : loadError }}
        </p>
        <Button
          v-if="!isNotFound && logId"
          variant="outline"
          size="sm"
          class="mt-3"
          @click="load(logId)"
        >
          Try again
        </Button>
      </Alert>

      <template v-else-if="log">
        <div class="flex flex-wrap items-center gap-2">
          <Badge
            :variant="logStatusVariant(log.status)"
            data-testid="integration-log-detail-status"
          >
            {{ INTEGRATION_LOG_STATUS_LABELS[log.status] }}
          </Badge>
          <span
            v-if="log.statusCode !== null"
            class="font-medium"
            data-testid="integration-log-detail-status-code"
          >
            HTTP {{ log.statusCode }}
          </span>
          <span v-if="log.durationMs !== null" class="text-muted-foreground">
            in {{ formatDuration(log.durationMs) }}
          </span>
        </div>

        <Alert
          v-if="log.errorMessage"
          variant="destructive"
          data-testid="integration-log-detail-error-message"
        >
          {{ log.errorMessage }}
        </Alert>

        <dl class="grid gap-x-4 gap-y-2 sm:grid-cols-[9rem_1fr]">
          <dt class="text-muted-foreground">Log</dt>
          <dd>
            <code class="font-mono text-xs" data-testid="integration-log-detail-id">{{
              log.id
            }}</code>
          </dd>

          <dt class="text-muted-foreground">System</dt>
          <dd data-testid="integration-log-detail-system">
            <RouterLink
              v-if="canOpenSystem && log.externalSystemId"
              :to="{ name: 'admin-external-system', params: { id: log.externalSystemId } }"
              class="text-primary underline-offset-4 hover:underline"
              data-testid="integration-log-detail-system-link"
            >
              {{ systemLabel }}
            </RouterLink>
            <template v-else>{{ systemLabel }}</template>
          </dd>

          <dt class="text-muted-foreground">Direction</dt>
          <dd>
            {{ INTEGRATION_DIRECTION_LABELS[log.direction] }}
            <template v-if="log.integrationType">· {{ log.integrationType }}</template>
          </dd>

          <dt class="text-muted-foreground">Request</dt>
          <dd class="break-all" data-testid="integration-log-detail-request">
            <code class="font-mono text-xs">{{ log.method ?? '—' }} {{ log.endpoint ?? '—' }}</code>
          </dd>

          <dt class="text-muted-foreground">About</dt>
          <dd class="break-all">
            <template v-if="log.entityType">
              {{ log.entityType }}
              <code v-if="log.entityId" class="font-mono text-xs">{{ log.entityId }}</code>
            </template>
            <template v-else>—</template>
          </dd>

          <template v-if="log.documentId">
            <dt class="text-muted-foreground">Document</dt>
            <dd data-testid="integration-log-detail-document">
              <RouterLink
                v-if="canOpenDocument"
                :to="{ name: 'document', params: { id: log.documentId } }"
                class="text-primary underline-offset-4 hover:underline"
                data-testid="integration-log-detail-document-link"
              >
                Open the document
              </RouterLink>
              <code v-else class="font-mono text-xs">{{ log.documentId }}</code>
            </dd>
          </template>

          <dt class="text-muted-foreground">Correlation id</dt>
          <dd class="break-all">
            <code class="font-mono text-xs">{{ log.correlationId ?? '—' }}</code>
          </dd>

          <dt class="text-muted-foreground">Started</dt>
          <dd>{{ formatTimestamp(log.startedAt) }}</dd>

          <dt class="text-muted-foreground">Completed</dt>
          <dd>{{ formatTimestamp(log.completedAt) }}</dd>
        </dl>

        <div class="space-y-1">
          <p class="text-muted-foreground">Request payload</p>
          <pre
            v-if="requestPayload !== null"
            class="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border bg-muted p-3 font-mono text-xs"
            data-testid="integration-log-detail-request-payload"
            >{{ requestPayload }}</pre>
          <p
            v-else
            class="text-xs text-muted-foreground"
            data-testid="integration-log-detail-no-request"
          >
            None was recorded.
          </p>
        </div>

        <div class="space-y-1">
          <p class="text-muted-foreground">Response payload</p>
          <pre
            v-if="responsePayload !== null"
            class="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border bg-muted p-3 font-mono text-xs"
            data-testid="integration-log-detail-response-payload"
            >{{ responsePayload }}</pre>
          <p
            v-else
            class="text-xs text-muted-foreground"
            data-testid="integration-log-detail-no-response"
          >
            None was recorded. A call refused before anything was sent, or that got no answer, has
            no response.
          </p>
        </div>

        <p class="text-xs text-muted-foreground">
          Secrets were masked when this row was written. It is shown as stored.
        </p>
      </template>
    </div>

    <template #footer>
      <Button variant="outline" data-testid="integration-log-detail-close" @click="open = false">
        Close
      </Button>
    </template>
  </Dialog>
</template>
