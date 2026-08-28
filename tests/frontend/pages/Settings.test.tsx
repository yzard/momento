import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ThemeProvider } from '../../../src/frontend/context/ThemeContext'
import Settings from '../../../src/frontend/pages/Settings'

const mocks = vi.hoisted(() => ({
  changePassword: vi.fn(),
  user: { username: 'alice', role: 'user', mustChangePassword: false },
}))
const storedValues = new Map<string, string>()
const testLocalStorage = {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  removeItem: (key: string) => storedValues.delete(key),
  setItem: (key: string, value: string) => storedValues.set(key, value),
}

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: mocks.user, changePassword: mocks.changePassword }),
}))
vi.mock('../../../src/frontend/components/admin/ImportPanel', () => ({
  default: () => <div data-testid="import-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/MetadataPanel', () => ({
  default: () => <div data-testid="metadata-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/AiPanel', () => ({
  default: () => <div data-testid="ai-panel" />,
}))
vi.mock('../../../src/frontend/components/admin/UserManagement', () => ({
  default: () => <div data-testid="user-panel" />,
}))

beforeEach(() => {
  vi.stubGlobal('localStorage', testLocalStorage)
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  mocks.changePassword.mockResolvedValue(undefined)
  mocks.user.role = 'user'
})
afterEach(() => {
  cleanup()
  document.documentElement.classList.remove('dark')
  vi.clearAllMocks()
  vi.unstubAllGlobals()
})

function renderSettings() {
  return render(
    <ThemeProvider>
      <Settings />
    </ThemeProvider>
  )
}

describe('Settings', () => {
  it('offers the server-bundled Android release', () => {
    renderSettings()

    const downloadLink = screen.getByRole('link', {
      name: 'Download Android app',
    })
    expect(downloadLink.getAttribute('href')).toBe('/momento-android.apk')
    expect(downloadLink.getAttribute('download')).toBe('momento-android.apk')
    expect(screen.getByText(/Install the release APK provided by this Momento server/)).toBeTruthy()
  })

  it('attributes the bundled local location data', () => {
    renderSettings()

    const attribution = screen.getByRole('link', { name: 'GeoNames' })
    const license = screen.getByRole('link', { name: 'CC BY 4.0' })
    expect(attribution.getAttribute('href')).toBe('https://www.geonames.org/')
    expect(license.getAttribute('href')).toBe('https://creativecommons.org/licenses/by/4.0/')
    expect(screen.getByText(/Location data adapted from/)).toBeTruthy()
  })

  it('ends the session through the authentication context after changing the password', async () => {
    const user = userEvent.setup()
    renderSettings()

    await user.type(screen.getByLabelText('Current Password'), 'old-password')
    await user.type(screen.getByLabelText('New Password'), 'new-password')
    await user.type(screen.getByLabelText('Confirm New Password'), 'new-password')
    await user.click(screen.getByRole('button', { name: 'Update Password' }))

    await waitFor(() => {
      expect(mocks.changePassword).toHaveBeenCalledWith('old-password', 'new-password')
    })
  })

  it('keeps administrator controls out of regular user settings', () => {
    renderSettings()

    expect(screen.queryByRole('heading', { name: 'Admin' })).toBeNull()
    expect(screen.queryByTestId('import-panel')).toBeNull()
  })

  it('places all administrator controls after the Admin separator', () => {
    mocks.user.role = 'admin'
    renderSettings()

    const adminHeading = screen.getByRole('heading', { name: 'Admin' })
    expect(adminHeading.closest('section')?.className).toContain('border-t')
    expect(screen.getByTestId('import-panel')).toBeTruthy()
    expect(screen.getByTestId('metadata-panel')).toBeTruthy()
    expect(screen.getByTestId('ai-panel')).toBeTruthy()
    expect(screen.getByTestId('user-panel')).toBeTruthy()
    expect(screen.getByText('/data/imports/')).toBeTruthy()
  })

  it('selects and persists the appearance preference', async () => {
    const user = userEvent.setup()
    renderSettings()

    expect(screen.getByRole('button', { name: 'System' }).getAttribute('aria-pressed')).toBe('true')
    await user.click(screen.getByRole('button', { name: 'Dark' }))

    expect(localStorage.getItem('momento-theme')).toBe('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(screen.getByRole('button', { name: 'Dark' }).getAttribute('aria-pressed')).toBe('true')
  })
})
