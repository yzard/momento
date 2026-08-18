import { describe, expect, it } from 'vitest'

import { MOMENTO_VERSION } from '../../../src/frontend/lib/version'

describe('MOMENTO_VERSION', () => {
  it('comes from the release version file', () => {
    expect(MOMENTO_VERSION).toBe('1.0.0')
  })
})
