import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import Settings from '../../../src/frontend/pages/Settings'

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: null, refreshUser: vi.fn() }),
}))

vi.mock('../../../src/frontend/api/auth', () => ({
  authApi: { changePassword: vi.fn() },
}))

describe('Settings', () => {
  it('attributes the bundled local location data', () => {
    render(<Settings />)

    const attribution = screen.getByRole('link', { name: 'GeoNames' })
    const license = screen.getByRole('link', { name: 'CC BY 4.0' })
    expect(attribution.getAttribute('href')).toBe('https://www.geonames.org/')
    expect(license.getAttribute('href')).toBe('https://creativecommons.org/licenses/by/4.0/')
    expect(screen.getByText(/Location data adapted from/)).toBeTruthy()
  })
})
