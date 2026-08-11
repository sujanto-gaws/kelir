<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { getOperational } from '@/api/client'
import { ApiError } from '@/api/error'

interface BackendVersion {
  name: string
  version: string
  environment: string
}

/**
 * Placeholder dashboard. The real widgets — pending tasks, recent documents,
 * status chart — are Phase 8 (#FR-RPT).
 *
 * It calls `/version` so the page proves the API client works against a running
 * backend, and so the three async states the coding standard requires (§3.4)
 * have somewhere real to be exercised.
 */
const backend = ref<BackendVersion | null>(null)
const error = ref<ApiError | null>(null)
const isLoading = ref(false)

async function load(): Promise<void> {
  isLoading.value = true
  error.value = null

  try {
    backend.value = await getOperational<BackendVersion>('/version')
  } catch (caught) {
    error.value = caught instanceof ApiError ? caught : null
    backend.value = null
  } finally {
    isLoading.value = false
  }
}

onMounted(load)
</script>

<template>
  <section class="space-y-6">
    <div>
      <h2 class="text-xl font-semibold tracking-tight">Dashboard</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        Widgets arrive in Phase 8. This page currently reports backend connectivity.
      </p>
    </div>

    <div class="rounded-lg border border-border bg-card p-4">
      <p v-if="isLoading" class="text-sm text-muted-foreground">Checking the backend…</p>

      <Alert v-else-if="error" variant="destructive">
        <p class="font-medium">Could not reach the backend</p>
        <p class="mt-1">{{ error.message }}</p>
        <p class="mt-1 text-xs opacity-80">Code: {{ error.code }}</p>
        <Button variant="outline" size="sm" class="mt-3" @click="load()">Try again</Button>
      </Alert>

      <dl v-else-if="backend" class="grid gap-2 text-sm sm:grid-cols-3">
        <div>
          <dt class="text-muted-foreground">Service</dt>
          <dd class="font-medium">{{ backend.name }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Version</dt>
          <dd class="font-medium">{{ backend.version }}</dd>
        </div>
        <div>
          <dt class="text-muted-foreground">Environment</dt>
          <dd class="font-medium">{{ backend.environment }}</dd>
        </div>
      </dl>

      <p v-else class="text-sm text-muted-foreground">No backend information available.</p>
    </div>
  </section>
</template>
