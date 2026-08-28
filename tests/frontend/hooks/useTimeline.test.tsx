import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
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
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const wrapper = createWrapper(queryClient)
    const screenshotHook = renderHook(
      () => useTimelineMarkers('image', 'screenshot', ' receipt '),
      { wrapper }
    )
    await waitFor(() => expect(screenshotHook.result.current.isSuccess).toBe(true))
    screenshotHook.unmount()

    const documentHook = renderHook(() => useTimelineMarkers('image', 'document', ' receipt '), {
      wrapper,
    })
    await waitFor(() => expect(documentHook.result.current.isSuccess).toBe(true))

    expect(mocks.getTimelineMarkers.mock.calls).toEqual([
      ['image', 'screenshot', 'receipt'],
      ['image', 'document', 'receipt'],
    ])
    expect(
      queryClient.getQueryCache().find({
        queryKey: ['timeline', 'markers', 'image', 'screenshot', 'receipt'],
      })
    ).toBeTruthy()
    expect(
      queryClient.getQueryCache().find({
        queryKey: ['timeline', 'markers', 'image', 'document', 'receipt'],
      })
    ).toBeTruthy()
  })

  it('isolates page cache contexts and propagates classification to each request', async () => {
    const marker = { label: '2026-08', anchorDate: '2026-08-01' }
    const { rerender } = renderHook(
      ({ classification }: { classification: 'screenshot' | 'document' }) =>
        useTimelineWindow({
          groupBy: 'day',
          search: ' receipt ',
          mediaType: 'image',
          classification,
          marker,
          refreshKey: 1,
        }),
      { initialProps: { classification: 'screenshot' as const } }
    )
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(1))

    rerender({ classification: 'document' })
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(2))
    rerender({ classification: 'screenshot' })
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(3))

    expect(mocks.listTimeline.mock.calls.map(([request]) => request)).toEqual([
      {
        limit: 100,
        groupBy: 'day',
        search: 'receipt',
        mediaType: 'image',
        classification: 'screenshot',
        direction: 'older',
        anchorDate: '2026-08-01',
      },
      {
        limit: 100,
        groupBy: 'day',
        search: 'receipt',
        mediaType: 'image',
        classification: 'document',
        direction: 'older',
        anchorDate: '2026-08-01',
      },
      {
        limit: 100,
        groupBy: 'day',
        search: 'receipt',
        mediaType: 'image',
        classification: 'screenshot',
        direction: 'older',
        anchorDate: '2026-08-01',
      },
    ])
  })

  it('ignores an older-page response after the timeline context changes', async () => {
    let resolveOlder!: (response: ReturnType<typeof timelineResponse>) => void
    const olderPage = new Promise<ReturnType<typeof timelineResponse>>((resolve) => {
      resolveOlder = resolve
    })
    mocks.listTimeline.mockImplementation(
      (request: { classification: string; cursor?: string }) => {
        if (request.cursor) return olderPage
        if (request.classification === 'document') {
          return Promise.resolve(timelineResponse(2, false))
        }
        return Promise.resolve(timelineResponse(1, true))
      }
    )
    const marker = { label: '2026-08', anchorDate: '2026-08-01' }
    const hook = renderHook(
      ({ classification }: { classification: 'screenshot' | 'document' }) =>
        useTimelineWindow({
          groupBy: 'day',
          search: '',
          mediaType: 'image',
          classification,
          marker,
          refreshKey: 1,
        }),
      { initialProps: { classification: 'screenshot' as const } }
    )
    await waitFor(() => expect(hook.result.current.groups[0]?.media[0]?.id).toBe(1))

    act(() => {
      void hook.result.current.loadOlder()
    })
    await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(2))
    hook.rerender({ classification: 'document' })
    await waitFor(() => expect(hook.result.current.groups[0]?.media[0]?.id).toBe(2))

    await act(async () => resolveOlder(timelineResponse(3, false)))
    expect(
      hook.result.current.groups.flatMap((group) => group.media.map((media) => media.id))
    ).toEqual([2])
  })

  it('keeps a bounded six-page render window while scrolling older', async () => {
    mocks.listTimeline.mockImplementation((request: { cursor?: string }) => {
      const page = request.cursor ? Number(request.cursor.replace('cursor-', '')) : 0
      return Promise.resolve(boundedPage(page))
    })
    const marker = { label: '2026-08', anchorDate: '2026-08-31' }
    const hook = renderHook(() =>
      useTimelineWindow({
        groupBy: 'day',
        search: '',
        mediaType: null,
        classification: null,
        marker,
        refreshKey: 1,
      })
    )
    await waitFor(() => expect(hook.result.current.groups).toHaveLength(1))

    for (let page = 1; page <= 8; page += 1) {
      await act(async () => hook.result.current.loadOlder())
      await waitFor(() => expect(mocks.listTimeline).toHaveBeenCalledTimes(page + 1))
    }

    expect(hook.result.current.groups).toHaveLength(6)
    expect(
      hook.result.current.groups.flatMap((group) => group.media.map((media) => media.id))
    ).toEqual([4, 5, 6, 7, 8, 9])
  })
})

function timelineResponse(mediaId: number, hasOlder: boolean) {
  return {
    groups: [
      {
        date: '2026-08-01',
        media: [{ id: mediaId, dateTaken: '2026-08-01T12:00:00' }],
      },
    ],
    nextCursor: hasOlder ? 'older-cursor' : null,
    previousCursor: null,
    hasOlder,
    hasNewer: false,
  }
}

function boundedPage(page: number) {
  const date = `2026-08-${String(31 - page).padStart(2, '0')}`
  return {
    groups: [
      {
        date,
        media: [{ id: page + 1, dateTaken: `${date}T12:00:00` }],
      },
    ],
    nextCursor: `cursor-${page + 1}`,
    previousCursor: page > 0 ? `cursor-${page - 1}` : null,
    hasOlder: true,
    hasNewer: page > 0,
  }
}
