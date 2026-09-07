import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from '../../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  login: vi.fn(),
  changePassword: vi.fn(),
  user: { username: 'alice', mustChangePassword: true },
}))

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => mocks,
}))

import Login from '../../../src/frontend/pages/Login'

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Login', () => {
  it('removes the login form when the password change form opens', async () => {
    mocks.login.mockResolvedValue(mocks.user)
    const user = userEvent.setup()
    const { container } = render(
      <MemoryRouter>
        <Login />
      </MemoryRouter>
    )
    await user.type(screen.getByLabelText('Username'), 'alice')
    await user.type(screen.getByLabelText('Password'), 'current-password')
    await user.click(screen.getByRole('button', { name: /sign in/i }))
    await screen.findByRole('heading', { name: 'Change Password' })

    expect(screen.queryByLabelText('Password')).toBeNull()
    expect(container.querySelectorAll('form')).toHaveLength(1)
    expect(container.querySelectorAll('input[autocomplete="current-password"]')).toHaveLength(3)
    expect(container.querySelectorAll('input[autocomplete="new-password"]')).toHaveLength(0)
    expect(container.querySelector<HTMLInputElement>('input[autocomplete="username"]')?.value).toBe(
      'alice'
    )
  })

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
