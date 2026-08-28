import { act, cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ThemeProvider } from '../../../src/frontend/context/ThemeContext'
import { useTheme } from '../../../src/frontend/hooks/useTheme'

const mediaListeners = new Set<(event: MediaQueryListEvent) => void>()
let systemDark = false
let legacyMediaQuery = false
const storedValues = new Map<string, string>()
const testLocalStorage = {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  removeItem: (key: string) => storedValues.delete(key),
  setItem: (key: string, value: string) => storedValues.set(key, value),
}

function ThemeProbe() {
  const { preference, resolvedTheme, setPreference } = useTheme()
  return (
    <div>
      <span>
        {preference}:{resolvedTheme}
      </span>
      <button type="button" onClick={() => setPreference('dark')}>
        Dark
      </button>
    </div>
  )
}

beforeEach(() => {
  systemDark = false
  legacyMediaQuery = false
  mediaListeners.clear()
  vi.stubGlobal('localStorage', testLocalStorage)
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      matches: systemDark,
      media: '(prefers-color-scheme: dark)',
      onchange: null,
      addEventListener: legacyMediaQuery
        ? undefined
        : (_type: string, listener: (event: MediaQueryListEvent) => void) =>
            mediaListeners.add(listener),
      removeEventListener: legacyMediaQuery
        ? undefined
        : (_type: string, listener: (event: MediaQueryListEvent) => void) =>
            mediaListeners.delete(listener),
      addListener: legacyMediaQuery
        ? (listener: (event: MediaQueryListEvent) => void) => mediaListeners.add(listener)
        : vi.fn(),
      removeListener: legacyMediaQuery
        ? (listener: (event: MediaQueryListEvent) => void) => mediaListeners.delete(listener)
        : vi.fn(),
      dispatchEvent: vi.fn(),
    }))
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  document.documentElement.classList.remove('dark')
})

describe('ThemeContext', () => {
  it('persists changes from the provider', async () => {
    const user = userEvent.setup()
    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>
    )

    expect(screen.getByText('system:light')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Dark' }))

    expect(screen.getByText('dark:dark')).toBeTruthy()
    expect(localStorage.getItem('momento-theme')).toBe('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('tracks operating-system changes while using system mode', () => {
    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>
    )

    act(() => {
      mediaListeners.forEach((listener) => listener({ matches: true } as MediaQueryListEvent))
    })

    expect(screen.getByText('system:dark')).toBeTruthy()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('tracks system mode through the legacy media-query listener', () => {
    legacyMediaQuery = true
    render(
      <ThemeProvider>
        <ThemeProbe />
      </ThemeProvider>
    )

    act(() => {
      mediaListeners.forEach((listener) => listener({ matches: true } as MediaQueryListEvent))
    })

    expect(screen.getByText('system:dark')).toBeTruthy()
  })
})
