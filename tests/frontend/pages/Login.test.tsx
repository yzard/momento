import { render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../../src/frontend/node_modules/react-router-dom'
import { describe, expect, it, vi } from 'vitest'

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ login: vi.fn() }),
}))

import Login from '../../../src/frontend/pages/Login'

describe('Login', () => {
  it('shows the release version without a Momento prefix', () => {
    render(
      <MemoryRouter>
        <Login />
      </MemoryRouter>
    )

    expect(screen.getByText('v1.0.0')).toBeTruthy()
    expect(screen.queryByText('Momento v1.0.0')).toBeNull()
  })
})
