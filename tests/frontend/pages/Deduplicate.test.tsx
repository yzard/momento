import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  groups: vi.fn(),
  status: vi.fn(),
  role: 'user' as 'admin' | 'user',
}))

vi.mock('../../../src/frontend/api/deduplicate', () => ({
  deduplicateApi: {
    groups: mocks.groups,
    status: mocks.status,
    start: vi.fn(),
    cancel: vi.fn(),
    clean: vi.fn(),
  },
}))

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { id: 7, username: 'viewer', role: mocks.role } }),
}))

vi.mock('../../../src/frontend/utils/batcher', () => ({
  batchLoader: { load: vi.fn() },
}))

import Deduplicate from '../../../src/frontend/pages/Deduplicate'

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <Deduplicate />
    </QueryClientProvider>,
  )
}

describe('Deduplicate page', () => {
  beforeEach(() => {
    mocks.role = 'user'
    mocks.groups.mockReset()
    mocks.status.mockReset()
    mocks.groups.mockResolvedValue({ groups: [], nextCursor: null, hasMore: false })
    mocks.status.mockResolvedValue({ status: 'idle', runId: null })
  })

  afterEach(cleanup)

  it('shows user groups without administrator controls', async () => {
    renderPage()

    expect(await screen.findByText('No duplicate groups')).toBeTruthy()
    expect(screen.queryByText('Start scan')).toBeNull()
    expect(mocks.status).not.toHaveBeenCalled()
  })

  it('does not expose scan controls to administrators on the utility page', async () => {
    mocks.role = 'admin'
    renderPage()

    expect(await screen.findByText('No duplicate groups')).toBeTruthy()
    expect(screen.queryByText('Start scan')).toBeNull()
    expect(mocks.status).not.toHaveBeenCalled()
  })
})
