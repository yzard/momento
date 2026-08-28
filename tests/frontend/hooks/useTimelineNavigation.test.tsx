import { act, renderHook } from '@testing-library/react'
import type { RefObject } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { TimelineGroup } from '../../../src/frontend/api/types'
import {
  useActiveTimelineMarker,
  useTimelinePaging,
} from '../../../src/frontend/hooks/useTimelineNavigation'

describe('timeline navigation hooks', () => {
  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0)
      return 1
    })
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('tracks the marker belonging to the first visible timeline group', () => {
    const container = document.createElement('div')
    const hiddenSection = document.createElement('section')
    hiddenSection.dataset.timelineGroup = '2026-08-01'
    hiddenSection.getBoundingClientRect = () => ({ bottom: 0 }) as DOMRect
    const visibleSection = document.createElement('section')
    visibleSection.dataset.timelineGroup = '2026-07-01'
    visibleSection.getBoundingClientRect = () => ({ bottom: 100 }) as DOMRect
    container.append(hiddenSection, visibleSection)
    container.getBoundingClientRect = () => ({ top: 0 }) as DOMRect
    const scrollRef: RefObject<HTMLDivElement | null> = { current: container }
    const groups = [
      { date: '2026-08-01', media: [{ dateTaken: '2026-08-10' }] },
      { date: '2026-07-01', media: [{ dateTaken: '2026-07-10' }] },
    ] as TimelineGroup[]

    const hook = renderHook(() =>
      useActiveTimelineMarker(
        scrollRef,
        [
          { label: '2026-08', anchorDate: '2026-08-01' },
          { label: '2026-07', anchorDate: '2026-07-01' },
        ],
        groups
      )
    )

    expect(hook.result.current.activeMarkerIndex).toBe(1)
  })

  it('requests an older page after user interaction reaches the lower boundary', () => {
    const container = document.createElement('div')
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { value: 1000 },
    })
    container.scrollTop = 500
    const scrollRef: RefObject<HTMLDivElement | null> = { current: container }
    const requestOlder = vi.fn().mockResolvedValue(undefined)
    const hook = renderHook(() =>
      useTimelinePaging({
        scrollRef,
        hasOlder: true,
        hasNewer: false,
        isFetching: false,
        isLoadingOlder: false,
        isLoadingNewer: false,
        requestOlder,
        requestNewer: vi.fn().mockResolvedValue(undefined),
        updateActiveMarker: vi.fn(),
      })
    )

    act(() => hook.result.current.markInteracted())
    act(() => hook.result.current.handleScroll())

    expect(requestOlder).toHaveBeenCalledTimes(1)
  })
})
