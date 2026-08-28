import { renderHook } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { ThemeContext, type ThemeContextValue } from '../../../src/frontend/context/theme'
import { useTheme } from '../../../src/frontend/hooks/useTheme'

describe('useTheme', () => {
  it('reads the active theme context', () => {
    const theme: ThemeContextValue = {
      preference: 'dark',
      resolvedTheme: 'dark',
      setPreference: () => undefined,
    }
    const { result } = renderHook(() => useTheme(), {
      wrapper: ({ children }) => (
        <ThemeContext.Provider value={theme}>{children}</ThemeContext.Provider>
      ),
    })

    expect(result.current).toBe(theme)
  })
})
