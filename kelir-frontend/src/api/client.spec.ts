import { describe, expect, it } from 'vitest'

import { apiClient } from './client'

describe('apiClient', () => {
  it('targets the versioned api base path', () => {
    // The whole app talks to /api/v1 (FR-API-003); components never call axios
    // directly (coding standard 3.3), so this instance is the only entry point.
    expect(apiClient.defaults.baseURL).toContain('/api/v1')
  })

  it('applies a request timeout so a hung backend cannot block the ui', () => {
    expect(apiClient.defaults.timeout).toBeGreaterThan(0)
  })
})
