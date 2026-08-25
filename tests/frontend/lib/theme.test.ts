import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { initializeTheme, parseThemePreference, readThemePreference, resolveTheme, writeThemePreference } from '../../../src/frontend/lib/theme'

const storedValues = new Map<string, string>()
const testLocalStorage = {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  removeItem: (key: string) => storedValues.delete(key),
  setItem: (key: string, value: string) => storedValues.set(key, value),
}

beforeEach(() => {
  vi.stubGlobal('localStorage', testLocalStorage)
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false })))
})

afterEach(() => {
  vi.unstubAllGlobals()
  document.documentElement.classList.remove('dark')
})

describe('theme', () => {
  it('normalizes stored preferences and resolves system mode', () => {
    expect(parseThemePreference('dark')).toBe('dark')
    expect(parseThemePreference('invalid')).toBe('system')
    expect(resolveTheme('system', true)).toBe('dark')
    expect(resolveTheme('system', false)).toBe('light')
    expect(resolveTheme('light', true)).toBe('light')
  })

  it('applies the stored theme before rendering', () => {
    localStorage.setItem('momento-theme', 'dark')

    initializeTheme()

    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.style.colorScheme).toBe('dark')
  })

  it('continues with system mode when browser storage is blocked', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => { throw new DOMException('Blocked', 'SecurityError') },
      setItem: () => { throw new DOMException('Blocked', 'SecurityError') },
    })

    expect(readThemePreference()).toBe('system')
    expect(() => writeThemePreference('dark')).not.toThrow()
  })
})
