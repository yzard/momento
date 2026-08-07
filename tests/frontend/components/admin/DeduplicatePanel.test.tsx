import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  start: vi.fn(),
  cancel: vi.fn(),
  clean: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/deduplicate', () => ({
  deduplicateApi: mocks,
}))

import DeduplicatePanel from '../../../../src/frontend/components/admin/DeduplicatePanel'

function renderPanel() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <DeduplicatePanel />
    </QueryClientProvider>,
  )
}

describe('DeduplicatePanel', () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset()
    mocks.status.mockResolvedValue({
      status: 'idle',
      runId: null,
      trigger: null,
      scheduledFor: null,
      startedAt: null,
      completedAt: null,
      indexedMedia: 0,
      processedMedia: 0,
      candidateComparisons: 0,
      clustersCreated: 0,
      error: null,
      nextScheduledAt: null,
    })
    mocks.start.mockResolvedValue({ message: 'started', status: 'running' })
  })

  afterEach(cleanup)

  it('shows global scan controls and status', async () => {
    renderPanel()

    expect(await screen.findByText('Start scan')).toBeTruthy()
    expect(screen.getByText('Refresh status')).toBeTruthy()
    expect(screen.getByText('Cancel scan')).toBeTruthy()
    expect(screen.getByText('Clean indexes')).toBeTruthy()
    expect(mocks.status).toHaveBeenCalled()
  })

  it('starts a global scan', async () => {
    renderPanel()

    await userEvent.click(await screen.findByText('Start scan'))

    expect(mocks.start).toHaveBeenCalledOnce()
  })
})
