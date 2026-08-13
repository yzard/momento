import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ getOcrStatus: vi.fn(), getImageTaggingStatus: vi.fn(), status: vi.fn() }))

vi.mock('../../../../src/frontend/api/ai', () => ({ aiApi: { getOcrStatus: mocks.getOcrStatus, getImageTaggingStatus: mocks.getImageTaggingStatus } }))
vi.mock('../../../../src/frontend/api/deduplicate', () => ({ deduplicateApi: { status: mocks.status } }))

import AiPanel from '../../../../src/frontend/components/admin/AiPanel'

describe('AiPanel', () => {
  beforeEach(() => {
    mocks.getOcrStatus.mockResolvedValue({ completedJobs: 12 })
    mocks.getImageTaggingStatus.mockResolvedValue({ completedJobs: 9 })
    mocks.status.mockResolvedValue({ indexedMedia: 30, candidateComparisons: 44, clustersCreated: 5 })
  })

  afterEach(cleanup)

  it('shows consolidated processing metrics', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={queryClient}><AiPanel /></QueryClientProvider>)
    expect(await screen.findByText('12')).toBeTruthy()
    expect(screen.getByText('9')).toBeTruthy()
    expect(screen.getByText('30')).toBeTruthy()
    expect(screen.getByText('44')).toBeTruthy()
    expect(screen.getByText('5')).toBeTruthy()
    expect(screen.getByText('Processed OCR')).toBeTruthy()
    expect(screen.getByText('Duplicate groups')).toBeTruthy()
  })
})
