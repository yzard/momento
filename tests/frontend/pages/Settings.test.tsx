import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import Settings from '../../../src/frontend/pages/Settings'

const mocks = vi.hoisted(() => ({ changePassword: vi.fn() }))

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: null, changePassword: mocks.changePassword }),
}))

beforeEach(() => mocks.changePassword.mockResolvedValue(undefined))
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Settings', () => {
  it('attributes the bundled local location data', () => {
    render(<Settings />)

    const attribution = screen.getByRole('link', { name: 'GeoNames' })
    const license = screen.getByRole('link', { name: 'CC BY 4.0' })
    expect(attribution.getAttribute('href')).toBe('https://www.geonames.org/')
    expect(license.getAttribute('href')).toBe('https://creativecommons.org/licenses/by/4.0/')
    expect(screen.getByText(/Location data adapted from/)).toBeTruthy()
  })

  it('ends the session through the authentication context after changing the password', async () => {
    const user = userEvent.setup()
    render(<Settings />)

    await user.type(screen.getByLabelText('Current Password'), 'old-password')
    await user.type(screen.getByLabelText('New Password'), 'new-password')
    await user.type(screen.getByLabelText('Confirm New Password'), 'new-password')
    await user.click(screen.getByRole('button', { name: 'Update Password' }))

    await waitFor(() => {
      expect(mocks.changePassword).toHaveBeenCalledWith('old-password', 'new-password')
    })
  })
})
