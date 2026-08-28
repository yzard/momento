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
  isFetching: boolean
  isLoadingOlder: boolean
  isLoadingNewer: boolean
  requestOlder: () => Promise<void>
  requestNewer: () => Promise<void>
  updateActiveMarker: () => void
}

export function useTimelinePaging(options: TimelinePagingOptions) {
  const {
    scrollRef,
    hasOlder,
    hasNewer,
    isFetching,
    isLoadingOlder,
    isLoadingNewer,
    requestOlder,
    requestNewer,
    updateActiveMarker,
  } = options
  const userInteractedRef = useRef(false)
  const pendingNewerRef = useRef(false)
  const loadingNewerRef = useRef(false)

  const loadNewer = useCallback(async () => {
    if (!hasNewer || isLoadingNewer || loadingNewerRef.current) return
    const container = scrollRef.current
    if (!container) return
    pendingNewerRef.current = false
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
    if (!pendingNewerRef.current || isFetching || !hasNewer) return
    void loadNewer()
  }, [hasNewer, isFetching, loadNewer])

  const checkBoundaries = useCallback(
    (deltaY: number) => {
      const container = scrollRef.current
      if (!container) return
      if (deltaY < 0 && container.scrollTop <= 100) {
        if (hasNewer || isFetching) pendingNewerRef.current = true
        if (!isFetching) void loadNewer()
      }
      if (
        !isFetching &&
        deltaY > 0 &&
        container.scrollTop + container.clientHeight >= container.scrollHeight - 240
      ) {
        loadOlder()
      }
    },
    [hasNewer, isFetching, loadNewer, loadOlder, scrollRef]
  )

  const handleScroll = useCallback(() => {
    const container = scrollRef.current
    if (!container) return
    if (container.scrollTop > 0) userInteractedRef.current = true
    updateActiveMarker()
    if (userInteractedRef.current) checkBoundaries(container.scrollTop <= 100 ? -1 : 1)
  }, [checkBoundaries, scrollRef, updateActiveMarker])

  const handleWheel = useCallback(
    (event: WheelEvent<HTMLElement>) => {
      userInteractedRef.current = true
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

  const markInteracted = useCallback(() => {
    userInteractedRef.current = true
  }, [])
  const reset = useCallback(() => {
    userInteractedRef.current = false
    pendingNewerRef.current = false
    scrollRef.current?.scrollTo({ top: 0 })
  }, [scrollRef])

  return { handleScroll, handleWheel, markInteracted, reset }
}
