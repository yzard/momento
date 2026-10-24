import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
  type WheelEvent,
} from 'react'
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
  contentKey: string
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
    contentKey,
    hasOlder,
    hasNewer,
    isLoadingOlder,
    isLoadingNewer,
    requestOlder,
    requestNewer,
    updateActiveMarker,
  } = options
  const generationRef = useRef(0)
  const loadingNewerRef = useRef(false)
  const previousScrollTopRef = useRef(0)

  const visibleAnchorRef = useRef<{ id: string; top: number } | null>(null)
  const captureAnchor = useCallback(() => {
    const container = scrollRef.current
    if (!container) return
    const viewportTop = container.getBoundingClientRect().top
    const anchor = Array.from(container.querySelectorAll<HTMLElement>('[data-media-id]')).find(
      (item) => item.getBoundingClientRect().bottom > viewportTop
    )
    visibleAnchorRef.current = anchor?.dataset.mediaId
      ? { id: anchor.dataset.mediaId, top: anchor.getBoundingClientRect().top }
      : null
  }, [scrollRef])

  useLayoutEffect(() => {
    const container = scrollRef.current
    if (!container) return
    const saved = visibleAnchorRef.current
    if (saved) {
      const anchor = Array.from(container.querySelectorAll<HTMLElement>('[data-media-id]')).find(
        (item) => item.dataset.mediaId === saved.id
      )
      if (anchor) container.scrollTop += anchor.getBoundingClientRect().top - saved.top
    }
    previousScrollTopRef.current = container.scrollTop
    captureAnchor()
  }, [contentKey, scrollRef, captureAnchor])

  const preservePosition = useCallback(
    async (request: () => Promise<void>) => {
      captureAnchor()
      await request()
    },
    [captureAnchor]
  )

  const loadNewer = useCallback(async () => {
    if (!hasNewer || isLoadingNewer || loadingNewerRef.current) return
    const generation = generationRef.current
    loadingNewerRef.current = true
    try {
      await preservePosition(requestNewer)
    } finally {
      if (generation === generationRef.current) loadingNewerRef.current = false
    }
  }, [hasNewer, isLoadingNewer, requestNewer, preservePosition])

  const loadOlder = useCallback(() => {
    if (!hasOlder || isLoadingOlder) return
    void preservePosition(requestOlder)
  }, [hasOlder, isLoadingOlder, requestOlder, preservePosition])

  useEffect(() => {
    if (!hasOlder || isLoadingOlder) return

    const frame = requestAnimationFrame(() => {
      const container = scrollRef.current
      if (!container || container.clientHeight <= 0) return
      const remaining = container.scrollHeight - container.scrollTop - container.clientHeight
      if (remaining <= timelinePrefetchDistance(container.clientHeight)) loadOlder()
    })
    return () => cancelAnimationFrame(frame)
  }, [hasOlder, isLoadingOlder, loadOlder, scrollRef, contentKey])

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
  }, [hasNewer, isLoadingNewer, loadNewer, scrollRef, contentKey])

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
    captureAnchor()
    updateActiveMarker()
    if (scrollDelta !== 0) checkBoundaries(scrollDelta)
  }, [checkBoundaries, scrollRef, updateActiveMarker, captureAnchor])

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
    generationRef.current += 1
    visibleAnchorRef.current = null
    loadingNewerRef.current = false
    previousScrollTopRef.current = 0
    scrollRef.current?.scrollTo({ top: 0 })
  }, [scrollRef])

  return { handleScroll, handleWheel, reset }
}
