import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter } from '../../src/frontend/node_modules/react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const storedValues = new Map<string, string>()
const testLocalStorage = {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  removeItem: (key: string) => storedValues.delete(key),
  setItem: (key: string, value: string) => storedValues.set(key, value),
}

vi.mock('../../src/frontend/context/AuthContext', () => ({
  AuthProvider: ({ children }: { children: React.ReactNode }) => children,
}))
vi.mock('../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({
    user: { id: 1, username: 'user', role: 'user' },
    isAuthenticated: true,
    isLoading: false,
    logout: vi.fn(),
  }),
}))
vi.mock('../../src/frontend/pages/Places', () => ({
  default: () => <div>Places route</div>,
}))
vi.mock('../../src/frontend/pages/Timeline', () => ({
  default: ({
    mediaType,
    classification,
  }: {
    mediaType: string | null
    classification: string | null
  }) => (
    <div
      data-testid="timeline-route"
      data-media-type={mediaType ?? 'all'}
      data-classification={classification ?? 'all'}
    />
  ),
}))

import App from '../../src/frontend/App'

beforeEach(() => {
  vi.stubGlobal('localStorage', testLocalStorage)
  localStorage.clear()
  document.documentElement.classList.remove('dark')
})

afterEach(() => {
  cleanup()
  document.documentElement.classList.remove('dark')
  vi.unstubAllGlobals()
})

describe('App Places routes', () => {
  it.each(['/places', '/places/paris-france'])('renders Places at %s', async (path) => {
    render(
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    )
    expect(await screen.findByText('Places route')).toBeTruthy()
  })
})

describe('App timeline routes', () => {
  it.each([
    ['/timeline/screenshots', 'image', 'screenshot'],
    ['/timeline/documents', 'image', 'document'],
    ['/timeline/photos', 'image', 'all'],
  ])('renders the timeline filters for %s', async (path, mediaType, classification) => {
    render(
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    )

    const timeline = await screen.findByTestId('timeline-route')
    expect(timeline.getAttribute('data-media-type')).toBe(mediaType)
    expect(timeline.getAttribute('data-classification')).toBe(classification)
  })

  it('redirects the retired admin route to account settings', async () => {
    render(
      <MemoryRouter initialEntries={['/admin']}>
        <App />
      </MemoryRouter>
    )

    expect(await screen.findByRole('heading', { name: 'Account Settings' })).toBeTruthy()
  })
})

describe('App theme routes', () => {
  it('applies a stored dark theme to the login route', async () => {
    localStorage.setItem('momento-theme', 'dark')

    render(
      <MemoryRouter initialEntries={['/login']}>
        <App />
      </MemoryRouter>
    )

    expect(await screen.findByRole('heading', { name: 'Sign In' })).toBeTruthy()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })
})
