import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getTimelineMarkers: vi.fn(),
  listTimeline: vi.fn(),
}))

vi.mock('../../../src/frontend/api/media', () => ({
  mediaApi: {
    getTimelineMarkers: mocks.getTimelineMarkers,
    listTimeline: mocks.listTimeline,
  },
}))

import { useTimelineMarkers, useTimelineWindow } from '../../../src/frontend/hooks/useTimeline'

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  }
}

describe('timeline classification queries', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getTimelineMarkers.mockResolvedValue({ markers: [] })
    mocks.listTimeline.mockResolvedValue({
      groups: [],
      nextCursor: null,
      previousCursor: null,
      hasOlder: false,
      hasNewer: false,
    })
  })

  it('isolates marker queries by classification and propagates normalized search', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const wrapper = createWrapper(queryClient)
    const screenshotHook = renderHook(() => useTimelineMarkers('image', 'screenshot', ' receipt '), { wrapper })
    await waitFor(() => expect(screenshotHook.result.current.isSuccess).toBe(true))
    screenshotHook.unmount()

    const documentHook = renderHook(() => useTimelineMarkers('image', 'document', ' receipt '), { wrapper })
    await waitFor(() => expect(documentHook.result.current.isSuccess).toBe(true))

    expect(mocks.getTimelineMarkers.mock.calls).toEqual([
      ['image', 'screenshot', 'receipt'],
      ['image', 'document', 'receipt'],
    ])
    expect(queryClient.getQueryCache().find({ queryKey: ['timeline', 'markers', 'image', 'screenshot', 'receipt'] })).toBeTruthy()
    expect(queryClient.getQueryCache().find({ queryKey: ['timeline', 'markers', 'image', 'document', 'receipt'] })).toBeTruthy()
  })

  it('isolates page cache contexts and propagates classification to each request', async () => {
    const marker = { label: '2026-08', anchorDate: '2026-08-01' }
    const { rerender } = renderHook(
      ({ classification }: { classification: 'screenshot' | 'document' }) => useTimelineWindow({
        groupBy: 'day',
        search: ' receipt ',
        mediaType: 'image',
        classification,
        marker,
        preloadKey: 0,
        refreshKey: 1,
      }),
      { initialProps: { classification: 'screenshot' as const } },
    )
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(1))

    rerender({ classification: 'document' })
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(2))
    rerender({ classification: 'screenshot' })
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(3))

    expect(mocks.listTimeline.mock.calls.map(([request]) => request)).toEqual([
      { groupBy: 'day', search: 'receipt', mediaType: 'image', classification: 'screenshot', direction: 'older', anchorDate: '2026-08-01' },
      { groupBy: 'day', search: 'receipt', mediaType: 'image', classification: 'document', direction: 'older', anchorDate: '2026-08-01' },
      { groupBy: 'day', search: 'receipt', mediaType: 'image', classification: 'screenshot', direction: 'older', anchorDate: '2026-08-01' },
    ])
  })
})
