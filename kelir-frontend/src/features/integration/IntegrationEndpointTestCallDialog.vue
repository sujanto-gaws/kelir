<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'

import { toApiError } from '@/api/client'
import { testCallIntegrationEndpoint } from '@/api/integration'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useAuthStore } from '@/stores/auth'
import type { IntegrationEndpoint, TestCallResponse } from '@/types/integration'

import { formatDuration } from './integration-log'
import { describeTestCallFailure, type TestCallFailure } from './test-call-outcome'

/**
 * Calls one endpoint once and shows what came back (FR-INT-002, #547).
 *
 * **Four states, one at a time**: the confirmation, because the call is real —
 * the endpoint's method, with the system's credential — the wait, the answer,
 * or the failure. An answer is not a success: a `500` from the system is an
 * answer, shown `FAILED` with its code. A failure is a call that was refused
 * before anything was sent (422) or got no answer (502, 504), and it names its
 * integration log row as the answer does.
 *
 * The dialog runs the request itself, unlike `ConfirmDialog`, because what it
 * shows afterwards is the result rather than a closed dialog.
 *
 * **The log id opens the row** in the integration log (#548) for a caller who
 * holds `integration:log:read`, and is plain text for anybody else: calling an
 * endpoint and reading the log are separate grants.
 */
const props = defineProps<{
  systemId: string
  endpoint: IntegrationEndpoint | null
  /** The system's `timeoutSeconds`, which bounds the call and the wait. */
  timeoutSeconds: number
}>()

const open = defineModel<boolean>('open', { default: false })

const auth = useAuthStore()

const canOpenLog = computed(() => auth.can('integration:log:read'))

type Phase = 'confirm' | 'pending' | 'answered' | 'failed'

const phase = ref<Phase>('confirm')
const result = ref<TestCallResponse | null>(null)
const failure = ref<TestCallFailure | null>(null)

/**
 * Which run the dialog is showing. A call still running when the dialog is
 * closed, or run again, must not land its result on a later one.
 */
let run = 0

/** GET is the one method a system is expected not to act on. */
const isWrite = computed(() => (props.endpoint?.method ?? 'GET') !== 'GET')

const isRedirect = computed(() => {
  const code = result.value?.statusCode ?? 0

  return code >= 300 && code < 400
})

watch(
  open,
  (isOpen) => {
    run += 1

    if (isOpen) {
      phase.value = 'confirm'
      result.value = null
      failure.value = null
    }
  },
  { immediate: true },
)

async function call(): Promise<void> {
  const endpoint = props.endpoint

  if (!endpoint) {
    return
  }

  run += 1
  const mine = run

  phase.value = 'pending'
  result.value = null
  failure.value = null

  try {
    const answer = await testCallIntegrationEndpoint(
      props.systemId,
      endpoint.id,
      props.timeoutSeconds,
    )

    if (mine !== run) {
      return
    }

    result.value = answer
    phase.value = 'answered'
  } catch (error) {
    if (mine !== run) {
      return
    }

    failure.value = describeTestCallFailure(toApiError(error))
    phase.value = 'failed'
  }
}
</script>

<template>
  <Dialog v-model:open="open" title="Test call" class="max-w-2xl">
    <div data-testid="test-call-dialog" :data-phase="phase" class="space-y-4">
      <template v-if="endpoint && phase === 'confirm'">
        <p>
          Kelir will send one
          <code class="font-mono text-xs">{{ endpoint.method }}</code>
          request, with no body, to
          <code class="font-mono text-xs break-all">{{ endpoint.path }}</code>
          on this system, using its active credential. The call and its answer are logged.
        </p>
        <Alert v-if="isWrite" data-testid="test-call-write-warning">
          This is a real {{ endpoint.method }}. The system may act on it as it would on any other
          request.
        </Alert>
      </template>

      <p
        v-else-if="phase === 'pending'"
        class="text-muted-foreground"
        role="status"
        data-testid="test-call-pending"
      >
        Calling {{ endpoint?.endpointCode }}… This can take up to {{ timeoutSeconds }} seconds.
      </p>

      <template v-else-if="phase === 'answered' && result">
        <div class="flex flex-wrap items-center gap-2">
          <Badge
            :variant="result.status === 'SUCCESS' ? 'default' : 'destructive'"
            data-testid="test-call-status"
          >
            {{ result.status === 'SUCCESS' ? 'Success' : 'Failed' }}
          </Badge>
          <span class="font-medium" data-testid="test-call-status-code">
            HTTP {{ result.statusCode }}
          </span>
          <span class="text-muted-foreground" data-testid="test-call-duration">
            in {{ formatDuration(result.durationMs) }}
          </span>
        </div>

        <p v-if="result.status === 'FAILED'" class="text-muted-foreground">
          The system answered, but not with a 2xx.
          <template v-if="isRedirect">Redirects are not followed.</template>
        </p>

        <dl class="grid gap-x-4 gap-y-2 sm:grid-cols-[6rem_1fr]">
          <dt class="text-muted-foreground">Request</dt>
          <dd class="break-all" data-testid="test-call-request">
            <code class="font-mono text-xs">{{ result.method }} {{ result.url }}</code>
          </dd>
          <dt class="text-muted-foreground">Log</dt>
          <dd>
            <RouterLink
              v-if="canOpenLog"
              :to="{ name: 'admin-integration-logs', query: { log: result.logId } }"
              class="text-primary underline-offset-4 hover:underline"
              data-testid="test-call-log-link"
            >
              <code class="font-mono text-xs" data-testid="test-call-log-id">{{
                result.logId
              }}</code>
            </RouterLink>
            <code v-else class="font-mono text-xs" data-testid="test-call-log-id">{{
              result.logId
            }}</code>
          </dd>
        </dl>

        <div class="space-y-1">
          <p class="text-muted-foreground">Response body</p>
          <pre
            class="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border bg-muted p-3 font-mono text-xs"
            data-testid="test-call-body"
            >{{ result.bodyPreview || '(empty)' }}</pre>
          <p
            v-if="result.bodyTruncated"
            class="text-xs text-muted-foreground"
            data-testid="test-call-truncated"
          >
            Truncated: only the first 2,048 characters of the body are shown.
          </p>
          <p class="text-xs text-muted-foreground">
            Any secret Kelir sent is masked in the body. Headers are not shown.
          </p>
        </div>
      </template>

      <template v-else-if="phase === 'failed' && failure">
        <Alert variant="destructive" data-testid="test-call-failure" :data-kind="failure.kind">
          <p class="font-medium" data-testid="test-call-failure-title">{{ failure.title }}</p>
          <p v-if="failure.explanation" class="mt-1" data-testid="test-call-explanation">
            {{ failure.explanation }}
          </p>
        </Alert>

        <dl class="grid gap-x-4 gap-y-2 sm:grid-cols-[6rem_1fr]">
          <dt class="text-muted-foreground">Code</dt>
          <dd>
            <code class="font-mono text-xs" data-testid="test-call-failure-code">{{
              failure.code
            }}</code>
          </dd>
          <dt class="text-muted-foreground">Details</dt>
          <dd data-testid="test-call-failure-message">{{ failure.message }}</dd>
          <template v-if="failure.logId">
            <dt class="text-muted-foreground">Log</dt>
            <dd>
              <RouterLink
                v-if="canOpenLog"
                :to="{ name: 'admin-integration-logs', query: { log: failure.logId } }"
                class="text-primary underline-offset-4 hover:underline"
                data-testid="test-call-log-link"
              >
                <code class="font-mono text-xs" data-testid="test-call-log-id">{{
                  failure.logId
                }}</code>
              </RouterLink>
              <code v-else class="font-mono text-xs" data-testid="test-call-log-id">{{
                failure.logId
              }}</code>
            </dd>
          </template>
        </dl>
      </template>
    </div>

    <template #footer>
      <Button
        variant="outline"
        :disabled="phase === 'pending'"
        data-testid="test-call-close"
        @click="open = false"
      >
        {{ phase === 'confirm' ? 'Cancel' : 'Close' }}
      </Button>
      <Button
        :loading="phase === 'pending'"
        :disabled="!endpoint"
        data-testid="test-call-run"
        @click="call()"
      >
        {{ phase === 'confirm' || phase === 'pending' ? 'Run test call' : 'Run again' }}
      </Button>
    </template>
  </Dialog>
</template>
