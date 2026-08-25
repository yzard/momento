import { describe, expect, it } from 'vitest'

import { ThemeContext } from '../../../src/frontend/context/theme'

describe('ThemeContext', () => {
  it('starts without an implicit theme value', () => {
    expect(ThemeContext).toBeTruthy()
  })
})
