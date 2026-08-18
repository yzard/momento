import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../src/frontend/node_modules/react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('../../src/frontend/context/AuthContext', () => ({ AuthProvider: ({ children }: { children: React.ReactNode }) => children }))
vi.mock('../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({
    user: { id: 1, username: 'user', role: 'user' },
    isAuthenticated: true,
    isLoading: false,
    logout: vi.fn(),
  }),
}))
vi.mock('../../src/frontend/pages/Places', () => ({ default: () => <div>Places route</div> }))

import App from '../../src/frontend/App'

describe('App Places routes', () => {
  afterEach(cleanup)

  it.each(['/places', '/places/paris-france'])('renders Places at %s', (path) => {
    render(<MemoryRouter initialEntries={[path]}><App /></MemoryRouter>)
    expect(screen.getByText('Places route')).toBeTruthy()
  })
})
