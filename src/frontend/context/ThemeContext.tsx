import { useEffect, useState, type ReactNode } from 'react'
import {
  applyTheme,
  DARK_MEDIA_QUERY,
  readThemePreference,
  resolveTheme,
  systemPrefersDark,
  writeThemePreference,
  type ThemePreference,
} from '../lib/theme'
import { ThemeContext } from './theme'

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setStoredPreference] = useState<ThemePreference>(readThemePreference)
  const [systemDark, setSystemDark] = useState(systemPrefersDark)
  const resolvedTheme = resolveTheme(preference, systemDark)

  useEffect(() => {
    applyTheme(resolvedTheme)
  }, [resolvedTheme])

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const mediaQuery = window.matchMedia(DARK_MEDIA_QUERY)
    const updateSystemTheme = (event: MediaQueryListEvent) => setSystemDark(event.matches)
    if (typeof mediaQuery.addEventListener === 'function') {
      mediaQuery.addEventListener('change', updateSystemTheme)
      return () => mediaQuery.removeEventListener('change', updateSystemTheme)
    }
    mediaQuery.addListener(updateSystemTheme)
    return () => mediaQuery.removeListener(updateSystemTheme)
  }, [])

  const setPreference = (nextPreference: ThemePreference) => {
    writeThemePreference(nextPreference)
    setStoredPreference(nextPreference)
  }

  return (
    <ThemeContext.Provider value={{ preference, resolvedTheme, setPreference }}>
      {children}
    </ThemeContext.Provider>
  )
}
