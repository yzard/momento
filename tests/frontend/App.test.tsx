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
vi.mock('../../src/frontend/pages/Timeline', () => ({
  default: ({ mediaType, classification }: { mediaType: string | null; classification: string | null }) => (
    <div data-testid="timeline-route" data-media-type={mediaType ?? 'all'} data-classification={classification ?? 'all'} />
  ),
}))

import App from '../../src/frontend/App'

describe('App Places routes', () => {
  afterEach(cleanup)

  it.each(['/places', '/places/paris-france'])('renders Places at %s', (path) => {
    render(<MemoryRouter initialEntries={[path]}><App /></MemoryRouter>)
    expect(screen.getByText('Places route')).toBeTruthy()
  })
})

describe('App timeline routes', () => {
  afterEach(cleanup)

  it.each([
    ['/timeline/screenshots', 'image', 'screenshot'],
    ['/timeline/documents', 'image', 'document'],
    ['/timeline/photos', 'image', 'all'],
  ])('renders the timeline filters for %s', (path, mediaType, classification) => {
    render(<MemoryRouter initialEntries={[path]}><App /></MemoryRouter>)

    const timeline = screen.getByTestId('timeline-route')
    expect(timeline.getAttribute('data-media-type')).toBe(mediaType)
    expect(timeline.getAttribute('data-classification')).toBe(classification)
  })

  it('redirects the retired admin route to account settings', () => {
    render(<MemoryRouter initialEntries={['/admin']}><App /></MemoryRouter>)

    expect(screen.getByRole('heading', { name: 'Account Settings' })).toBeTruthy()
  })
})
