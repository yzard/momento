import { act, renderHook } from '@testing-library/react'
import type { RefObject } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { TimelineGroup } from '../../../src/frontend/api/types'
import {
  timelinePrefetchDistance,
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

  it('prefetches three measurable viewports', () => {
    expect(timelinePrefetchDistance(500)).toBe(1500)
    expect(timelinePrefetchDistance(0)).toBe(0)
    expect(timelinePrefetchDistance(Number.NaN)).toBe(0)
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

  it('requests an older page before scrolling consumes the buffered viewports', () => {
    const container = document.createElement('div')
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { value: 3000 },
    })
    container.scrollTop = 0
    const scrollRef: RefObject<HTMLDivElement | null> = { current: container }
    const requestOlder = vi.fn().mockResolvedValue(undefined)
    const hook = renderHook(() =>
      useTimelinePaging({
        contentKey: 'initial',
        scrollRef,
        hasOlder: true,
        hasNewer: false,
        isLoadingOlder: false,
        isLoadingNewer: false,
        requestOlder,
        requestNewer: vi.fn().mockResolvedValue(undefined),
        updateActiveMarker: vi.fn(),
      })
    )

    container.scrollTop = 1001
    act(() => hook.result.current.handleScroll())

    expect(requestOlder).toHaveBeenCalledTimes(1)
  })

  it('keeps requesting older pages until the initial viewport is filled', () => {
    const container = document.createElement('div')
    let scrollHeight = 500
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { configurable: true, get: () => scrollHeight },
    })
    const scrollRef: RefObject<HTMLDivElement | null> = { current: container }
    const requestOlder = vi.fn().mockResolvedValue(undefined)
    const { rerender } = renderHook(
      ({ isLoadingOlder }: { isLoadingOlder: boolean }) =>
        useTimelinePaging({
          contentKey: 'initial',
          scrollRef,
          hasOlder: true,
          hasNewer: false,
          isLoadingOlder,
          isLoadingNewer: false,
          requestOlder,
          requestNewer: vi.fn().mockResolvedValue(undefined),
          updateActiveMarker: vi.fn(),
        }),
      { initialProps: { isLoadingOlder: false } }
    )

    expect(requestOlder).toHaveBeenCalledTimes(1)

    rerender({ isLoadingOlder: true })
    scrollHeight = 1800
    rerender({ isLoadingOlder: false })
    expect(requestOlder).toHaveBeenCalledTimes(2)

    rerender({ isLoadingOlder: true })
    scrollHeight = 2200
    rerender({ isLoadingOlder: false })
    expect(requestOlder).toHaveBeenCalledTimes(2)
  })

  it('continues cached pages when content changes without a loading-state render', () => {
    const container = document.createElement('div')
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { value: 500 },
    })
    const options = {
      scrollRef: { current: container },
      hasOlder: true,
      hasNewer: false,
      isLoadingOlder: false,
      isLoadingNewer: false,
      requestOlder: vi.fn().mockResolvedValue(undefined),
      requestNewer: vi.fn().mockResolvedValue(undefined),
      updateActiveMarker: vi.fn(),
    }
    const hook = renderHook(({ contentKey }) => useTimelinePaging({ ...options, contentKey }), {
      initialProps: { contentKey: 'first-page' },
    })
    expect(options.requestOlder).toHaveBeenCalledTimes(1)
    hook.rerender({ contentKey: 'first-and-second-page' })
    expect(options.requestOlder).toHaveBeenCalledTimes(2)
  })

  it('prefetches newer pages after jumping to a historical marker', () => {
    const container = document.createElement('div')
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { value: 2500 },
    })
    container.scrollTop = 0
    const requestNewer = vi.fn().mockResolvedValue(undefined)

    renderHook(() =>
      useTimelinePaging({
        contentKey: 'initial',
        scrollRef: { current: container },
        hasOlder: false,
        hasNewer: true,
        isLoadingOlder: false,
        isLoadingNewer: false,
        requestOlder: vi.fn().mockResolvedValue(undefined),
        requestNewer,
        updateActiveMarker: vi.fn(),
      })
    )

    expect(requestNewer).toHaveBeenCalledTimes(1)
  })

  it('loads the older direction while newer media is still loading', () => {
    const container = document.createElement('div')
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { value: 500 },
    })
    const requestOlder = vi.fn().mockResolvedValue(undefined)

    renderHook(() =>
      useTimelinePaging({
        contentKey: 'initial',
        scrollRef: { current: container },
        hasOlder: true,
        hasNewer: true,
        isLoadingOlder: false,
        isLoadingNewer: true,
        requestOlder,
        requestNewer: vi.fn().mockResolvedValue(undefined),
        updateActiveMarker: vi.fn(),
      })
    )

    expect(requestOlder).toHaveBeenCalledTimes(1)
  })

  it('does not request initial pages before the viewport has a measurable height', () => {
    const container = document.createElement('div')
    const scrollRef: RefObject<HTMLDivElement | null> = { current: container }
    const requestOlder = vi.fn().mockResolvedValue(undefined)

    renderHook(() =>
      useTimelinePaging({
        contentKey: 'initial',
        scrollRef,
        hasOlder: true,
        hasNewer: false,
        isLoadingOlder: false,
        isLoadingNewer: false,
        requestOlder,
        requestNewer: vi.fn().mockResolvedValue(undefined),
        updateActiveMarker: vi.fn(),
      })
    )

    expect(requestOlder).not.toHaveBeenCalled()
  })
  it('does not move a new marker when the previous marker request finishes', async () => {
    const container = document.createElement('div')
    container.scrollTo = vi.fn()
    let height = 2500
    Object.defineProperties(container, {
      clientHeight: { value: 500 },
      scrollHeight: { get: () => height },
    })
    let finish!: () => void
    const pending = new Promise<void>((resolve) => {
      finish = resolve
    })
    const hook = renderHook(() =>
      useTimelinePaging({
        contentKey: 'initial',
        scrollRef: { current: container },
        hasOlder: false,
        hasNewer: true,
        isLoadingOlder: false,
        isLoadingNewer: false,
        requestOlder: vi.fn().mockResolvedValue(undefined),
        requestNewer: () => pending,
        updateActiveMarker: vi.fn(),
      })
    )
    act(() => hook.result.current.reset())
    height = 5000
    await act(async () => finish())
    expect(container.scrollTop).toBe(0)
  })

  it('anchors the visible image instead of counting appended older-page height', async () => {
    const container = document.createElement('div')
    Object.defineProperty(container, 'clientHeight', { value: 500 })
    const image = document.createElement('div')
    image.dataset.mediaId = '42'
    let top = 0
    image.getBoundingClientRect = () => ({ top, bottom: top + 100 }) as DOMRect
    container.append(image)
    let finish!: () => void
    const pending = new Promise<void>((resolve) => {
      finish = resolve
    })
    const scrollRef = { current: container }
    const hook = renderHook(
      ({ contentKey }) =>
        useTimelinePaging({
          contentKey,
          scrollRef,
          hasOlder: false,
          hasNewer: true,
          isLoadingOlder: false,
          isLoadingNewer: false,
          requestOlder: vi.fn().mockResolvedValue(undefined),
          requestNewer: () => pending,
          updateActiveMarker: vi.fn(),
        }),
      { initialProps: { contentKey: 'initial' } }
    )
    top = 300
    Object.defineProperty(container, 'scrollHeight', { value: 6000 })
    await act(async () => finish())
    hook.rerender({ contentKey: 'newer-and-older-pages' })
    expect(container.scrollTop).toBe(300)
  })
})
