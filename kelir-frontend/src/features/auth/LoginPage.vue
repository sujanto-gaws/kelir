<script setup lang="ts">
import { computed, ref } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

/**
 * Sign-in form — presentation only.
 *
 * Phase 2 (#14) replaces `submit` with a call to the auth store; the field
 * errors below already use the shape `ApiError.fieldErrors()` returns, so
 * wiring it is a substitution rather than a rewrite.
 */
const username = ref('')
const password = ref('')
const submitted = ref(false)

const errors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (username.value.trim() === '') {
    found.username = 'Enter your username or email address'
  }

  if (password.value === '') {
    found.password = 'Enter your password'
  }

  return found
})

const hasErrors = computed(() => Object.keys(errors.value).length > 0)

function submit(): void {
  submitted.value = true
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-background px-4">
    <div class="w-full max-w-sm">
      <div class="mb-8 text-center">
        <h1 class="text-2xl font-semibold tracking-tight">Kelir</h1>
        <p class="mt-1 text-sm text-muted-foreground">Sign in to continue</p>
      </div>

      <form class="space-y-4" novalidate @submit.prevent="submit">
        <div class="space-y-2">
          <Label for="username">Username or email</Label>
          <Input
            id="username"
            v-model="username"
            autocomplete="username"
            :invalid="Boolean(errors.username)"
            described-by="username-error"
            placeholder="you@example.com"
          />
          <p v-if="errors.username" id="username-error" class="text-sm text-destructive">
            {{ errors.username }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="password">Password</Label>
          <Input
            id="password"
            v-model="password"
            type="password"
            autocomplete="current-password"
            :invalid="Boolean(errors.password)"
            described-by="password-error"
          />
          <p v-if="errors.password" id="password-error" class="text-sm text-destructive">
            {{ errors.password }}
          </p>
        </div>

        <Button type="submit" class="w-full">Sign in</Button>
      </form>

      <Alert v-if="submitted && !hasErrors" class="mt-4">
        Authentication is not wired yet — it arrives in Phase 2 with the identity module.
      </Alert>
    </div>
  </div>
</template>
