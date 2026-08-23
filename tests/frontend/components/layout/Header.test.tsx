import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from '../../../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ logout: vi.fn() }))

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { username: 'alice' }, logout: mocks.logout }),
}))

import Header from '../../../../src/frontend/components/layout/Header'

function CurrentPath() {
  return <div data-testid="current-path">{useLocation().pathname}</div>
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Header', () => {
  it('opens account settings from the avatar', async () => {
    const user = userEvent.setup()
    render(
      <MemoryRouter initialEntries={['/timeline']}>
        <Header onMenuClick={vi.fn()} />
        <CurrentPath />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Open account settings' }))

    expect(screen.getByTestId('current-path').textContent).toBe('/settings')
  })

  it('keeps logout as a separate action', async () => {
    const user = userEvent.setup()
    render(<MemoryRouter><Header onMenuClick={vi.fn()} /></MemoryRouter>)

    await user.click(screen.getByRole('button', { name: 'Logout' }))

    expect(mocks.logout).toHaveBeenCalledOnce()
  })
})
