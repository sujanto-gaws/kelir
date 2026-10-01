import { ref } from 'vue'
import { describe, expect, it } from 'vitest'

import { unplacedErrors, useFormErrors } from './useFormErrors'
import { ApiError } from '@/api/error'

const duplicateRule = { match: /already in use/i, fields: ['username', 'email'] }

describe('useFormErrors', () => {
  it('binds validation details to their fields by path', () => {
    const errors = useFormErrors()

    errors.report(
      new ApiError('VALIDATION_ERROR', 'Validation failed', 422, [
        {
          path: 'displayName',
          rule: 'required',
          code: 'REQUIRED',
          message: 'Display name is required',
        },
        {
          path: 'email',
          rule: 'format',
          code: 'INVALID_FORMAT',
          message: 'Enter a valid email address',
        },
      ]),
    )

    expect(errors.fieldErrors.value).toEqual({
      displayName: 'Display name is required',
      email: 'Enter a valid email address',
    })
    expect(errors.formError.value).toBe('')
  })

  it('puts a matching conflict on the fields it belongs to', () => {
    // The backend's 409 carries no details, so the field is inferred here or the
    // duplicate ends up as a message detached from the input that caused it.
    const errors = useFormErrors([duplicateRule])

    errors.report(new ApiError('CONFLICT', 'That username or email address is already in use', 409))

    expect(errors.fieldErrors.value).toEqual({
      username: 'That username or email address is already in use',
      email: 'That username or email address is already in use',
    })
    expect(errors.formError.value).toBe('')
  })

  it('falls back to the form when a conflict matches no rule', () => {
    const errors = useFormErrors([duplicateRule])

    errors.report(new ApiError('CONFLICT', 'Something else entirely', 409))

    expect(errors.fieldErrors.value).toEqual({})
    expect(errors.formError.value).toBe('Something else entirely')
  })

  it('resolves the rules at report time, not at setup', () => {
    // Which field a duplicate belongs to depends on the form's mode, and the
    // mode can change while the composable lives.
    const isEditing = ref(false)
    const errors = useFormErrors(() => [
      { match: /already in use/i, fields: isEditing.value ? ['email'] : ['username', 'email'] },
    ])

    isEditing.value = true
    errors.report(new ApiError('CONFLICT', 'That username or email address is already in use', 409))

    expect(Object.keys(errors.fieldErrors.value)).toEqual(['email'])
  })

  it('surfaces a denial on the form so it is never a silent no-op', () => {
    const errors = useFormErrors()

    errors.report(
      new ApiError('FORBIDDEN', 'You do not have permission to perform this action', 403),
    )

    expect(errors.formError.value).toBe('You do not have permission to perform this action')
    expect(errors.fieldErrors.value).toEqual({})
  })

  it('surfaces the self-deactivation refusal verbatim', () => {
    const errors = useFormErrors()

    errors.report(new ApiError('BAD_REQUEST', 'You cannot deactivate your own account', 400))

    expect(errors.formError.value).toBe('You cannot deactivate your own account')
  })

  it('does not leak anything it cannot classify', () => {
    const errors = useFormErrors()

    errors.report(new TypeError('window is not defined'))

    expect(errors.formError.value).toBe('Something went wrong. Try again.')
    expect(errors.fieldErrors.value).toEqual({})
  })

  it('clears the previous result on every report', () => {
    const errors = useFormErrors()

    errors.report(
      new ApiError('VALIDATION_ERROR', 'Validation failed', 422, [
        { path: 'email', rule: 'format', code: 'INVALID_FORMAT', message: 'Bad email' },
      ]),
    )
    errors.report(new ApiError('FORBIDDEN', 'Denied', 403))

    // A fixed field error must not linger under an unrelated failure.
    expect(errors.fieldErrors.value).toEqual({})
    expect(errors.formError.value).toBe('Denied')
  })

  it('drops one field as the user edits it', () => {
    const errors = useFormErrors([duplicateRule])

    errors.report(new ApiError('CONFLICT', 'That username or email address is already in use', 409))
    errors.clearField('username')

    expect(Object.keys(errors.fieldErrors.value)).toEqual(['email'])
  })

  it('resets both destinations', () => {
    const errors = useFormErrors()

    errors.report(new ApiError('FORBIDDEN', 'Denied', 403))
    errors.reset()

    expect(errors.formError.value).toBe('')
    expect(errors.fieldErrors.value).toEqual({})
  })
})

describe('unplacedErrors', () => {
  const fieldErrors = {
    name: 'Too long',
    colour: 'Unknown field',
    'lines.2.sku': 'No such product',
  }

  it('returns the details whose path the form does not place, in order', () => {
    expect(unplacedErrors(fieldErrors, ['name'])).toEqual([
      { path: 'colour', message: 'Unknown field' },
      { path: 'lines.2.sku', message: 'No such product' },
    ])
  })

  it('matches a path whole, so a row path is not placed by its prefix', () => {
    expect(unplacedErrors(fieldErrors, ['name', 'colour', 'lines'])).toEqual([
      { path: 'lines.2.sku', message: 'No such product' },
    ])
  })

  it('returns nothing when every detail is placed, or there is none', () => {
    expect(unplacedErrors(fieldErrors, ['name', 'colour', 'lines.2.sku'])).toEqual([])
    expect(unplacedErrors({}, ['name'])).toEqual([])
  })

  it('lists a 422 with details and nothing for a 422 without, or a denial', () => {
    const errors = useFormErrors()
    const detail = { path: 'colour', rule: 'unknown', code: 'UNKNOWN_FIELD', message: 'Unknown' }

    errors.report(new ApiError('VALIDATION_ERROR', 'Validation failed', 422, [detail]))
    expect(unplacedErrors(errors.fieldErrors.value, ['name'])).toHaveLength(1)

    // No detail to place or list: the message is the form's, never dropped.
    errors.report(new ApiError('VALIDATION_ERROR', 'Validation failed', 422))
    expect(unplacedErrors(errors.fieldErrors.value, ['name'])).toEqual([])
    expect(errors.formError.value).toBe('Validation failed')

    errors.report(new ApiError('FORBIDDEN', 'Denied', 403, [detail]))
    expect(unplacedErrors(errors.fieldErrors.value, ['name'])).toEqual([])
    expect(errors.formError.value).toBe('Denied')
  })
})
