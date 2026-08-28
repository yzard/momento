import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('../../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { username: 'admin' }, logout: vi.fn() }),
}))

import Layout from '../../../../src/frontend/components/layout/Layout'

describe('Layout', () => {
  afterEach(cleanup)

  it('starts with the desktop sidebar folded', () => {
    render(
      <MemoryRouter>
        <Layout />
      </MemoryRouter>
    )

    expect(screen.getByRole('button', { name: 'Expand Sidebar' })).toBeTruthy()
  })

  it('opens the mobile drawer with full labels', () => {
    render(
      <MemoryRouter>
        <Layout />
      </MemoryRouter>
    )

    fireEvent.click(screen.getByRole('button', { name: 'Open navigation' }))

    expect(screen.getByRole('button', { name: 'Collapse Sidebar' })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Timeline' })).toBeTruthy()
  })
})
