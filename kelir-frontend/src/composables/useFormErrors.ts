import { ref, toValue, type MaybeRefOrGetter, type Ref } from 'vue'

import { ApiError } from '@/api/error'

/**
 * Routes a rejected API call to the place a user can act on it.
 *
 * Three destinations, in the order the backend makes them available:
 *
 * 1. **Against a field** — a 422 carries `details` whose `path` is already the
 *    camelCase field name (`displayName`), so it binds straight to an input.
 * 2. **Against a field we infer** — a 409 carries no `details` at all. The
 *    backend answers "That username or email address is already in use" without
 *    saying which one collided, and a duplicate is a property of a field, not of
 *    the request. A toast would leave the offending input looking valid, so the
 *    caller declares which fields a given conflict belongs to.
 * 3. **On the form** — everything else: 403 denials, the 400 self-deactivation
 *    guard, network failures. Surfaced verbatim rather than swallowed, so a
 *    refused action is never a silent no-op.
 */
export interface ConflictRule {
  /** Matched against the backend's message. */
  match: RegExp
  /** Fields the message is shown against when it matches. */
  fields: string[]
}

export interface FormErrors {
  /** Errors keyed by field name, for binding to inputs. */
  fieldErrors: Ref<Record<string, string>>
  /** The whole-form message, empty when there is none. */
  formError: Ref<string>
  /** Clear both — call before every submit so a fixed error disappears. */
  reset(): void
  /** Classify a rejected call. */
  report(error: unknown): void
  /** Drop one field's error, for clearing as the user edits it. */
  clearField(field: string): void
}

/**
 * `conflictRules` is resolved at report time, not at setup, because which field
 * a conflict belongs to can depend on the form's mode: a duplicate on create
 * could be either the username or the email, while on edit only the email can
 * have changed.
 */
export function useFormErrors(conflictRules: MaybeRefOrGetter<ConflictRule[]> = []): FormErrors {
  const fieldErrors = ref<Record<string, string>>({})
  const formError = ref('')

  function reset(): void {
    fieldErrors.value = {}
    formError.value = ''
  }

  function clearField(field: string): void {
    if (!(field in fieldErrors.value)) {
      return
    }

    const rest = { ...fieldErrors.value }
    delete rest[field]
    fieldErrors.value = rest
  }

  function report(error: unknown): void {
    reset()

    if (!(error instanceof ApiError)) {
      formError.value = 'Something went wrong. Try again.'
      return
    }

    if (error.isValidation) {
      fieldErrors.value = error.fieldErrors()
      return
    }

    if (error.status === 409) {
      const rule = toValue(conflictRules).find((candidate) => candidate.match.test(error.message))

      if (rule) {
        // The backend's wording is the authority on what went wrong; repeating
        // it in our own words risks contradicting it.
        fieldErrors.value = Object.fromEntries(rule.fields.map((field) => [field, error.message]))
        return
      }
    }

    formError.value = error.message
  }

  return { fieldErrors, formError, reset, report, clearField }
}
