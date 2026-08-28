export type ThemePreference = 'light' | 'dark' | 'system'
export type ResolvedTheme = 'light' | 'dark'

const THEME_STORAGE_KEY = 'momento-theme'
export const DARK_MEDIA_QUERY = '(prefers-color-scheme: dark)'

function themeStorage(): Storage | null {
  try {
    if (typeof globalThis.localStorage === 'undefined') return null
    return globalThis.localStorage
  } catch (error) {
    if (error instanceof DOMException && error.name === 'SecurityError') return null
    throw error
  }
}

function storageUnavailable(error: unknown): boolean {
  return (
    error instanceof DOMException &&
    (error.name === 'SecurityError' || error.name === 'QuotaExceededError')
  )
}

export function parseThemePreference(value: string | null): ThemePreference {
  if (value === 'light' || value === 'dark' || value === 'system') return value
  return 'system'
}

export function resolveTheme(preference: ThemePreference, systemDark: boolean): ResolvedTheme {
  if (preference === 'system') return systemDark ? 'dark' : 'light'
  return preference
}

export function systemPrefersDark(): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia(DARK_MEDIA_QUERY).matches
}

export function applyTheme(theme: ResolvedTheme): void {
  document.documentElement.classList.toggle('dark', theme === 'dark')
  document.documentElement.style.colorScheme = theme
}

export function readThemePreference(): ThemePreference {
  const storage = themeStorage()
  if (!storage) return 'system'
  try {
    return parseThemePreference(storage.getItem(THEME_STORAGE_KEY))
  } catch (error) {
    if (storageUnavailable(error)) return 'system'
    throw error
  }
}

export function writeThemePreference(preference: ThemePreference): void {
  const storage = themeStorage()
  if (!storage) return
  try {
    storage.setItem(THEME_STORAGE_KEY, preference)
  } catch (error) {
    if (!storageUnavailable(error)) throw error
  }
}

export function initializeTheme(): void {
  applyTheme(resolveTheme(readThemePreference(), systemPrefersDark()))
}
