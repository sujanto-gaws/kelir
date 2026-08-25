<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError } from '@/api/error'
import { resetPassword } from '@/api/auth'

/** The floor the backend enforces; repeated here only to fail before the round trip. */
const MIN_PASSWORD_LENGTH = 12

/**
 * Redeem a reset link and choose a new password.
 *
 * The token arrives as `?token=` because that is what the emailed link carries.
 * It is a bearer credential for the length of one request, so on success the
 * query is replaced rather than pushed — leaving it in history would keep a
 * live-looking secret in the address bar and in whatever syncs it, even though
 * the server has already spent it.
 *
 * **A rejected token gets one message, not four.** Unknown, expired, already
 * used and malformed are indistinguishable to the caller by design, so the page
 * shows the server's wording and offers a fresh link rather than guessing which
 * of the four happened.
 */
const route = useRoute()
const router = useRouter()

const token = computed(() => {
  const value = route.query.token
  return typeof value === 'string' ? value : ''
})

const newPassword = ref('')
const confirmPassword = ref('')
const submitted = ref(false)
const isSubmitting = ref(false)
const isDone = ref(false)
const formError = ref('')
const serverFieldErrors = ref<Record<string, string>>({})

/** Seconds left on a rate limit; counts down so the button re-enables itself. */
const retryAfter = ref(0)
let retryTimer: ReturnType<typeof setInterval> | undefined

function startRetryCountdown(seconds: number): void {
  clearInterval(retryTimer)
  retryAfter.value = seconds

  retryTimer = setInterval(() => {
    retryAfter.value = Math.max(0, retryAfter.value - 1)

    if (retryAfter.value === 0) {
      clearInterval(retryTimer)
    }
  }, 1_000)
}

onUnmounted(() => clearInterval(retryTimer))

const localErrors = computed<Record<string, string>>(() => {
  if (!submitted.value) {
    return {}
  }

  const found: Record<string, string> = {}

  if (newPassword.value.length < MIN_PASSWORD_LENGTH) {
    found.newPassword = `Use at least ${MIN_PASSWORD_LENGTH} characters`
  }

  if (confirmPassword.value !== newPassword.value) {
    found.confirmPassword = 'Both entries must match'
  }

  return found
})

const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...serverFieldErrors.value,
}))

const isRateLimited = computed(() => retryAfter.value > 0)

function reportFailure(error: unknown): void {
  if (!(error instanceof ApiError)) {
    formError.value = 'Something went wrong. Try again.'
    return
  }

  if (error.isValidation) {
    const details = error.fieldErrors()
    // The shared password validator reports against `password`, which is what
    // it is called everywhere it is also used (user creation, the bootstrap
    // administrator, change-password). Mapping it here keeps the message on the
    // field a person is actually looking at instead of dropping it.
    const { password, ...rest } = details
    serverFieldErrors.value = password === undefined ? rest : { ...rest, newPassword: password }

    // A rejected token has no field on this form, so it belongs on the form.
    formError.value = 'token' in details ? details.token : ''

    if (formError.value === '' && Object.keys(serverFieldErrors.value).length === 0) {
      formError.value = error.message
    }

    return
  }

  if (error.isRateLimited) {
    formError.value = error.message

    if (error.retryAfterSeconds !== undefined) {
      startRetryCountdown(error.retryAfterSeconds)
    }

    return
  }

  formError.value = error.message
}

async function submit(): Promise<void> {
  submitted.value = true
  formError.value = ''
  serverFieldErrors.value = {}

  if (Object.keys(localErrors.value).length > 0 || isSubmitting.value || isRateLimited.value) {
    return
  }

  isSubmitting.value = true

  try {
    await resetPassword({ token: token.value, newPassword: newPassword.value })
    newPassword.value = ''
    confirmPassword.value = ''
    isDone.value = true
    // Drop the spent token out of the address bar and out of history.
    await router.replace({ name: 'reset-password' })
  } catch (error) {
    reportFailure(error)
  } finally {
    isSubmitting.value = false
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-background px-4">
    <div class="w-full max-w-sm">
      <div class="mb-8 text-center">
        <h1 class="text-2xl font-semibold tracking-tight">Choose a new password</h1>
      </div>

      <Alert v-if="formError" variant="destructive" class="mb-4">{{ formError }}</Alert>

      <div v-if="isDone" class="rounded-md border border-border bg-card p-4">
        <p class="text-sm font-medium">Your password is changed</p>
        <p class="mt-1 text-sm text-muted-foreground">
          Everywhere you were signed in has been signed out. Sign in again with the new password.
        </p>
        <RouterLink
          :to="{ name: 'login' }"
          class="mt-3 inline-block text-sm font-medium underline underline-offset-4"
        >
          Go to sign in
        </RouterLink>
      </div>

      <!-- No token at all: the link was truncated, or somebody typed the path.
           There is nothing to submit, so do not offer a form that cannot work. -->
      <div v-else-if="token === ''" class="rounded-md border border-border bg-card p-4">
        <p class="text-sm font-medium">This link is not complete</p>
        <p class="mt-1 text-sm text-muted-foreground">
          Open the link from your email exactly as it was sent, or ask for a new one.
        </p>
        <RouterLink
          :to="{ name: 'forgot-password' }"
          class="mt-3 inline-block text-sm font-medium underline underline-offset-4"
        >
          Ask for a new link
        </RouterLink>
      </div>

      <form v-else class="space-y-4" novalidate @submit.prevent="submit">
        <div class="space-y-2">
          <Label for="new-password">New password</Label>
          <Input
            id="new-password"
            v-model="newPassword"
            type="password"
            autocomplete="new-password"
            :invalid="Boolean(errors.newPassword)"
            :disabled="isSubmitting"
            described-by="new-password-error"
          />
          <p v-if="errors.newPassword" id="new-password-error" class="text-sm text-destructive">
            {{ errors.newPassword }}
          </p>
          <p v-else class="text-sm text-muted-foreground">
            At least {{ MIN_PASSWORD_LENGTH }} characters.
          </p>
        </div>

        <div class="space-y-2">
          <Label for="confirm-password">Confirm new password</Label>
          <Input
            id="confirm-password"
            v-model="confirmPassword"
            type="password"
            autocomplete="new-password"
            :invalid="Boolean(errors.confirmPassword)"
            :disabled="isSubmitting"
            described-by="confirm-password-error"
          />
          <p
            v-if="errors.confirmPassword"
            id="confirm-password-error"
            class="text-sm text-destructive"
          >
            {{ errors.confirmPassword }}
          </p>
        </div>

        <Button type="submit" class="w-full" :loading="isSubmitting" :disabled="isRateLimited">
          {{ isRateLimited ? `Try again in ${retryAfter}s` : 'Set new password' }}
        </Button>

        <p class="text-center text-sm">
          <RouterLink
            :to="{ name: 'forgot-password' }"
            class="font-medium underline underline-offset-4"
          >
            Ask for a new link
          </RouterLink>
        </p>
      </form>
    </div>
  </div>
</template>
