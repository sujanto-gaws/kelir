<script setup lang="ts">
import { computed, ref } from 'vue'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError } from '@/api/error'
import { requestPasswordReset } from '@/api/auth'

/**
 * Ask for a reset link.
 *
 * **The confirmation is unconditional, and that is the feature.** The backend
 * answers 202 for an unknown identifier, a suspended account, a resend still
 * inside its cooldown, and a mail server that is down — all alike, so that this
 * page cannot be used to find out whether an account exists. So it shows one
 * message on success, and never phrases it as "we sent you an email", which
 * would assert something the server has not told us.
 *
 * Only two things can genuinely fail here: a request we shaped wrongly (422 on
 * an empty identifier, which the local check catches first) and not reaching
 * the server at all. Both say so.
 */
const username = ref('')
const submitted = ref(false)
const isSubmitting = ref(false)
const isSent = ref(false)
const formError = ref('')

const localError = computed(() =>
  submitted.value && username.value.trim() === '' ? 'Enter your username or email address' : '',
)

async function submit(): Promise<void> {
  submitted.value = true
  formError.value = ''

  if (localError.value !== '' || isSubmitting.value) {
    return
  }

  isSubmitting.value = true

  try {
    await requestPasswordReset({ username: username.value.trim() })
    isSent.value = true
  } catch (error) {
    formError.value = error instanceof ApiError ? error.message : 'Something went wrong. Try again.'
  } finally {
    isSubmitting.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-background px-4">
    <div class="w-full max-w-sm">
      <div class="mb-8 text-center">
        <h1 class="text-2xl font-semibold tracking-tight">Reset your password</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          We will email you a link to choose a new one
        </p>
      </div>

      <Alert v-if="formError" variant="destructive" class="mb-4">{{ formError }}</Alert>

      <div v-if="isSent" class="rounded-md border border-border bg-card p-4">
        <p class="text-sm font-medium">Check your email</p>
        <!-- Conditional wording, because the server has not told us whether it
             sent anything, and saying that it did would be both a claim we
             cannot make and an answer to "does this account exist?". -->
        <p class="mt-1 text-sm text-muted-foreground">
          If that username or email belongs to an account, a reset link is on its way. The link is
          good for 30 minutes.
        </p>
        <RouterLink
          :to="{ name: 'login' }"
          class="mt-3 inline-block text-sm font-medium underline underline-offset-4"
        >
          Back to sign in
        </RouterLink>
      </div>

      <form v-else class="space-y-4" novalidate @submit.prevent="submit">
        <div class="space-y-2">
          <Label for="username">Username or email</Label>
          <Input
            id="username"
            v-model="username"
            autocomplete="username"
            :invalid="Boolean(localError)"
            :disabled="isSubmitting"
            described-by="username-error"
            placeholder="you@example.com"
          />
          <p v-if="localError" id="username-error" class="text-sm text-destructive">
            {{ localError }}
          </p>
        </div>

        <Button type="submit" class="w-full" :loading="isSubmitting">Send reset link</Button>

        <p class="text-center text-sm">
          <RouterLink :to="{ name: 'login' }" class="font-medium underline underline-offset-4">
            Back to sign in
          </RouterLink>
        </p>
      </form>
    </div>
  </div>
</template>
