<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ApiError } from '@/api/error'
import { HOME_ROUTE_NAME, safeReturnPath } from '@/router/guards'
import { useAuthStore } from '@/stores/auth'

/**
 * Sign-in form.
 *
 * Failures land in one of three places: against a field (the envelope's
 * validation details), on the form (credentials, rate limit, network), or —
 * never — silently. The wording lives here rather than in the store, per the
 * coding standard: the store hands back the `ApiError` and the page decides
 * what a person reads.
 */
const auth = useAuthStore()
const router = useRouter()
const route = useRoute()

const username = ref('')
const password = ref('')
const submitted = ref(false)
const isSubmitting = ref(false)
const formError = ref('')
const serverFieldErrors = ref<Record<string, string>>({})

/**
 * Tokens are present but the profile behind them could not be fetched.
 *
 * Two different failures send a caller to this page: the server rejected the
 * session, or it could not be reached. The guard cannot tell them apart — it
 * only knows the profile did not load — but the surviving tokens can, because
 * only a rejection clears them.
 *
 * So a transport failure looks like this: the session is intact, and asking for
 * credentials that are still perfectly good would be wrong. Offer the retry
 * instead, and keep the form for whoever genuinely needs it.
 */
const hasUnverifiedSession = computed(() => auth.isAuthenticated && auth.user === null)
const isRetrying = ref(false)

async function retrySession(): Promise<void> {
  isRetrying.value = true
  formError.value = ''

  try {
    if (await auth.ensureProfile()) {
      await router.replace(safeReturnPath(route.query) ?? { name: HOME_ROUTE_NAME })
      return
    }

    // Still here with tokens intact means the server is still unreachable. A
    // rejection would have cleared them, and the form below takes over.
    if (auth.isAuthenticated) {
      formError.value = 'Still could not reach the server. Try again in a moment.'
    }
  } finally {
    isRetrying.value = false
  }
}

/** Abandon the stored session and sign in as somebody else. */
function discardSession(): void {
  auth.clearSession()
  formError.value = ''
}

/** Seconds left on a rate limit; counts down so the button re-enables itself. */
const retryAfter = ref(0)
let retryTimer: ReturnType<typeof setInterval> | undefined

const localErrors = computed<Record<string, string>>(() => {
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

// The server has seen the actual values, so where both have an opinion its
// answer is the one worth showing.
const errors = computed<Record<string, string>>(() => ({
  ...localErrors.value,
  ...serverFieldErrors.value,
}))

const isRateLimited = computed(() => retryAfter.value > 0)
const isBusy = computed(() => isSubmitting.value || isRateLimited.value)

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

function reportFailure(error: unknown): void {
  if (!(error instanceof ApiError)) {
    formError.value = 'Something went wrong. Try again.'
    return
  }

  if (error.isValidation) {
    serverFieldErrors.value = error.fieldErrors()
    // Details that name no field of ours would otherwise vanish.
    const unmatched = Object.keys(serverFieldErrors.value).every(
      (path) => path !== 'username' && path !== 'password',
    )
    formError.value = unmatched ? error.message : ''
    return
  }

  if (error.isRateLimited) {
    // The backend's message carries the wait; repeating it in our own words
    // would only risk contradicting it.
    formError.value = error.message

    if (error.retryAfterSeconds !== undefined) {
      startRetryCountdown(error.retryAfterSeconds)
    }

    return
  }

  if (error.isUnauthorized) {
    // Deliberately generic, matching the backend: saying which half was wrong
    // would confirm whether an account exists.
    formError.value = 'Your username or password is not correct.'
    return
  }

  formError.value = error.message
}

async function submit(): Promise<void> {
  submitted.value = true
  formError.value = ''
  serverFieldErrors.value = {}

  if (Object.keys(localErrors.value).length > 0 || isBusy.value) {
    return
  }

  isSubmitting.value = true

  try {
    await auth.signIn(username.value.trim(), password.value)
    // `replace`, not `push`: the back button should not return to a form the
    // user has already cleared.
    await router.replace(safeReturnPath(route.query) ?? { name: HOME_ROUTE_NAME })
  } catch (error) {
    password.value = ''
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
        <h1 class="text-2xl font-semibold tracking-tight">Kelir</h1>
        <p class="mt-1 text-sm text-muted-foreground">Sign in to continue</p>
      </div>

      <Alert v-if="formError" variant="destructive" class="mb-4">{{ formError }}</Alert>

      <div v-if="hasUnverifiedSession" class="mb-4 rounded-md border border-border bg-card p-4">
        <p class="text-sm font-medium">You are still signed in</p>
        <p class="mt-1 text-sm text-muted-foreground">
          We could not reach the server to confirm your session. Your sign-in has not been lost.
        </p>
        <div class="mt-3 flex gap-2">
          <Button size="sm" :loading="isRetrying" @click="retrySession()">Try again</Button>
          <Button size="sm" variant="ghost" :disabled="isRetrying" @click="discardSession()">
            Sign in as someone else
          </Button>
        </div>
      </div>

      <form v-if="!hasUnverifiedSession" class="space-y-4" novalidate @submit.prevent="submit">
        <div class="space-y-2">
          <Label for="username">Username or email</Label>
          <Input
            id="username"
            v-model="username"
            autocomplete="username"
            :invalid="Boolean(errors.username)"
            :disabled="isSubmitting"
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
            :disabled="isSubmitting"
            described-by="password-error"
          />
          <p v-if="errors.password" id="password-error" class="text-sm text-destructive">
            {{ errors.password }}
          </p>
        </div>

        <Button type="submit" class="w-full" :loading="isSubmitting" :disabled="isRateLimited">
          {{ isRateLimited ? `Try again in ${retryAfter}s` : 'Sign in' }}
        </Button>
      </form>
    </div>
  </div>
</template>
