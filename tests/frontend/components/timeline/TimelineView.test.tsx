import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  useTimelineMarkers: vi.fn(),
  useTimelineWindow: vi.fn(),
}))

vi.mock('../../../../src/frontend/hooks/useTimeline', () => ({
  useTimelineMarkers: mocks.useTimelineMarkers,
  useTimelineWindow: mocks.useTimelineWindow,
}))

import TimelineView from '../../../../src/frontend/components/timeline/TimelineView'

describe('TimelineView classification empty state', () => {
  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0)
      return 1
    })
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
    HTMLElement.prototype.scrollTo = vi.fn()
    mocks.useTimelineMarkers.mockReturnValue({ data: { markers: [] }, dataUpdatedAt: 1, isLoading: false, error: null })
    mocks.useTimelineWindow.mockReturnValue({
      groups: [],
      hasNextPage: false,
      hasPreviousPage: false,
      isFetching: false,
      isLoadingOlder: false,
      isLoadingNewer: false,
      isLoading: false,
      error: null,
      loadOlder: vi.fn(),
      loadNewer: vi.fn(),
    })
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
    vi.unstubAllGlobals()
  })

  it('shows screenshot-specific empty copy and propagates classification', () => {
    render(<TimelineView onPhotoClick={vi.fn()} groupBy="day" search="" mediaType="image" classification="screenshot" />)

    expect(screen.getByText('No screenshots yet')).toBeTruthy()
    expect(screen.getByText('Screenshots identified by Screenshot Detection will appear here.')).toBeTruthy()
    expect(mocks.useTimelineMarkers).toHaveBeenCalledWith('image', 'screenshot', '')
    expect(mocks.useTimelineWindow).toHaveBeenCalledWith(expect.objectContaining({ classification: 'screenshot' }))
  })

  it('shows document-specific search empty copy', () => {
    render(<TimelineView onPhotoClick={vi.fn()} groupBy="day" search="invoice" mediaType="image" classification="document" />)

    expect(screen.getByText('No matching documents')).toBeTruthy()
    expect(screen.getByText('No documents matched "invoice".')).toBeTruthy()
  })
})
