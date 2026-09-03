import { useCallback, useEffect, useRef, useState, type RefObject, type WheelEvent } from 'react'
import type { TimelineMarker } from '../api/media'
import type { TimelineGroup } from '../api/types'

export function useActiveTimelineMarker(
  scrollRef: RefObject<HTMLDivElement | null>,
  markers: TimelineMarker[],
  groups: TimelineGroup[]
) {
  const [activeMarkerIndex, setActiveMarkerIndex] = useState(0)
  const updateActiveMarker = useCallback(() => {
    const container = scrollRef.current
    if (!container || markers.length === 0) return
    const viewportTop = container.getBoundingClientRect().top
    const firstSection = Array.from(
      container.querySelectorAll<HTMLElement>('[data-timeline-group]')
    ).find((section) => section.getBoundingClientRect().bottom > viewportTop + 1)
    const firstGroup = groups.find((group) => group.date === firstSection?.dataset.timelineGroup)
    const firstMediaDate = firstGroup?.media[0]?.dateTaken
    if (!firstMediaDate) return
    const markerIndex = markers.findIndex((marker) => firstMediaDate.slice(0, 7) === marker.label)
    if (markerIndex >= 0) {
      setActiveMarkerIndex((current) => (current === markerIndex ? current : markerIndex))
    }
  }, [groups, markers, scrollRef])

  useEffect(() => {
    const frame = requestAnimationFrame(updateActiveMarker)
    return () => cancelAnimationFrame(frame)
  }, [groups, updateActiveMarker])

  return { activeMarkerIndex, setActiveMarkerIndex, updateActiveMarker }
}

interface TimelinePagingOptions {
  scrollRef: RefObject<HTMLDivElement | null>
  hasOlder: boolean
  hasNewer: boolean
  isLoadingOlder: boolean
  isLoadingNewer: boolean
  requestOlder: () => Promise<void>
  requestNewer: () => Promise<void>
  updateActiveMarker: () => void
}

const TIMELINE_PREFETCH_VIEWPORTS = 3

export function timelinePrefetchDistance(viewportHeight: number): number {
  if (!Number.isFinite(viewportHeight) || viewportHeight <= 0) return 0
  return viewportHeight * TIMELINE_PREFETCH_VIEWPORTS
}

export function useTimelinePaging(options: TimelinePagingOptions) {
  const {
    scrollRef,
    hasOlder,
    hasNewer,
    isLoadingOlder,
    isLoadingNewer,
    requestOlder,
    requestNewer,
    updateActiveMarker,
  } = options
  const loadingNewerRef = useRef(false)
  const previousScrollTopRef = useRef(0)

  const loadNewer = useCallback(async () => {
    if (!hasNewer || isLoadingNewer || loadingNewerRef.current) return
    const container = scrollRef.current
    if (!container) return
    loadingNewerRef.current = true
    const previousHeight = container.scrollHeight
    try {
      await requestNewer()
      requestAnimationFrame(() => {
        container.scrollTop += container.scrollHeight - previousHeight
        updateActiveMarker()
      })
    } finally {
      loadingNewerRef.current = false
    }
  }, [hasNewer, isLoadingNewer, requestNewer, scrollRef, updateActiveMarker])

  const loadOlder = useCallback(() => {
    if (!hasOlder || isLoadingOlder) return
    void requestOlder()
  }, [hasOlder, isLoadingOlder, requestOlder])

  useEffect(() => {
    if (!hasOlder || isLoadingOlder) return

    const frame = requestAnimationFrame(() => {
      const container = scrollRef.current
      if (!container || container.clientHeight <= 0) return
      const remaining = container.scrollHeight - container.scrollTop - container.clientHeight
      if (remaining <= timelinePrefetchDistance(container.clientHeight)) loadOlder()
    })
    return () => cancelAnimationFrame(frame)
  }, [hasOlder, isLoadingOlder, loadOlder, scrollRef])

  useEffect(() => {
    if (!hasNewer || isLoadingNewer) return

    const frame = requestAnimationFrame(() => {
      const container = scrollRef.current
      if (!container || container.clientHeight <= 0) return
      if (container.scrollTop <= timelinePrefetchDistance(container.clientHeight)) {
        void loadNewer()
      }
    })
    return () => cancelAnimationFrame(frame)
  }, [hasNewer, isLoadingNewer, loadNewer, scrollRef])

  const checkBoundaries = useCallback(
    (deltaY: number) => {
      const container = scrollRef.current
      if (!container || container.clientHeight <= 0) return
      const prefetchDistance = timelinePrefetchDistance(container.clientHeight)
      if (deltaY < 0 && container.scrollTop <= prefetchDistance) {
        void loadNewer()
      }
      const remaining = container.scrollHeight - container.scrollTop - container.clientHeight
      if (deltaY > 0 && remaining <= prefetchDistance) {
        loadOlder()
      }
    },
    [loadNewer, loadOlder, scrollRef]
  )

  const handleScroll = useCallback(() => {
    const container = scrollRef.current
    if (!container) return
    const scrollDelta = container.scrollTop - previousScrollTopRef.current
    previousScrollTopRef.current = container.scrollTop
    updateActiveMarker()
    if (scrollDelta !== 0) checkBoundaries(scrollDelta)
  }, [checkBoundaries, scrollRef, updateActiveMarker])

  const handleWheel = useCallback(
    (event: WheelEvent<HTMLElement>) => {
      const container = scrollRef.current
      if (!container) return
      if (event.currentTarget !== container) {
        event.preventDefault()
        container.scrollBy({ top: event.deltaY, behavior: 'auto' })
      }
      checkBoundaries(event.deltaY)
    },
    [checkBoundaries, scrollRef]
  )

  const reset = useCallback(() => {
    previousScrollTopRef.current = 0
    scrollRef.current?.scrollTo({ top: 0 })
  }, [scrollRef])

  return { handleScroll, handleWheel, reset }
}
