import { useCallback, useEffect, useRef, useState, type PointerEvent, type WheelEvent } from 'react'
import { Image as ImageIcon, Loader2 } from 'lucide-react'
import type { GroupBy, MediaTypeFilter, TimelineMarker } from '../../api/media'
import type { Media } from '../../api/types'
import { useTimelineMarkers, useTimelineWindow } from '../../hooks/useTimeline'
import DateHeader from './DateHeader'
import PhotoGrid from './PhotoGrid'

const EMPTY_MARKERS: TimelineMarker[] = []

interface TimelineViewProps {
  onPhotoClick: (media: Media, allMedia: Media[]) => void
  onAddToAlbum?: (media: Media) => void
  onDelete?: (media: Media) => void
  groupBy: GroupBy
  search: string
  mediaType: MediaTypeFilter | null
}

interface TimelineScrubberProps {
  markers: TimelineMarker[]
  activeMarkerIndex: number
  onMarkerSelect: (marker: TimelineMarker) => void
  onWheel: (event: WheelEvent<HTMLElement>) => void
}

function formatMarkerLabel(marker: TimelineMarker): string {
  const [year, month] = marker.label.split('-')
  if (!year || !month) return marker.label
  const monthName = new Intl.DateTimeFormat('en-US', { month: 'short' }).format(new Date(Number(year), Number(month) - 1, 1))
  return `${monthName} ${year}`
}

function TimelineScrubber({ markers, activeMarkerIndex, onMarkerSelect, onWheel }: TimelineScrubberProps) {
  const railRef = useRef<HTMLDivElement>(null)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [dragging, setDragging] = useState(false)

  const markerAtPosition = useCallback((clientY: number) => {
    const rail = railRef.current
    if (!rail || markers.length === 0) return 0
    const bounds = rail.getBoundingClientRect()
    const fraction = Math.min(Math.max((clientY - bounds.top) / bounds.height, 0), 1)
    return Math.round(fraction * (markers.length - 1))
  }, [markers.length])

  const selectAtPosition = useCallback((clientY: number) => {
    const marker = markers[markerAtPosition(clientY)]
    if (marker) onMarkerSelect(marker)
  }, [markerAtPosition, markers, onMarkerSelect])

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    setDragging(true)
    event.currentTarget.setPointerCapture(event.pointerId)
    selectAtPosition(event.clientY)
  }

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    setHoveredIndex(markerAtPosition(event.clientY))
    if (dragging) selectAtPosition(event.clientY)
  }

  const stopDragging = (event: PointerEvent<HTMLDivElement>) => {
    setDragging(false)
    setHoveredIndex(null)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
  }

  const visibleIndex = hoveredIndex ?? activeMarkerIndex
  const visibleMarker = markers[visibleIndex]

  return (
    <aside className="group/scrubber absolute inset-y-0 right-0 z-20 hidden w-28 md:block" onWheel={onWheel}>
      <div
        ref={railRef}
        role="scrollbar"
        aria-label="Timeline index"
        aria-orientation="vertical"
        aria-valuemin={0}
        aria-valuemax={Math.max(markers.length - 1, 0)}
        aria-valuenow={activeMarkerIndex}
        tabIndex={0}
        className="absolute inset-y-8 right-3 left-3 cursor-pointer opacity-0 outline-none transition-opacity duration-200 group-hover/scrubber:opacity-100 focus-within:opacity-100 focus-visible:ring-2 focus-visible:ring-primary/40 motion-reduce:transition-none"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerLeave={() => setHoveredIndex(null)}
        onPointerUp={stopDragging}
        onPointerCancel={stopDragging}
        onKeyDown={(event) => {
          const delta = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
          const target = event.key === 'Home' ? 0 : event.key === 'End' ? markers.length - 1 : activeMarkerIndex + delta
          if (delta === 0 && event.key !== 'Home' && event.key !== 'End') return
          event.preventDefault()
          const marker = markers[Math.min(Math.max(target, 0), markers.length - 1)]
          if (marker) onMarkerSelect(marker)
        }}
      >
        <div className="absolute inset-y-0 right-2 w-px bg-border" />
        {markers.map((marker, index) => {
          const isYearStart = index === 0 || marker.label.slice(0, 4) !== markers[index - 1]?.label.slice(0, 4)
          const distance = hoveredIndex === null ? 0 : Math.abs(hoveredIndex - index)
          const scale = distance === 0 ? 1.75 : distance === 1 ? 1.3 : 1
          return (
            <button
              key={marker.label}
              type="button"
              aria-current={index === activeMarkerIndex ? 'true' : undefined}
              aria-label={`Jump to ${formatMarkerLabel(marker)}`}
              className="absolute right-0 flex -translate-y-1/2 origin-right items-center gap-1.5 transition-transform duration-200 ease-out motion-reduce:transition-none"
              style={{
                top: `${(index / Math.max(markers.length - 1, 1)) * 100}%`,
                transform: `translateY(-50%) scale(${scale})`,
              }}
              onClick={(event) => {
                event.stopPropagation()
                onMarkerSelect(marker)
              }}
            >
              {isYearStart && <span className="rounded bg-foreground px-1.5 py-0.5 text-[10px] font-semibold text-background">{marker.label.slice(0, 4)}</span>}
              <span className={index === activeMarkerIndex ? 'h-1 w-7 rounded-full bg-primary' : 'h-px w-4 bg-muted-foreground/70'} />
            </button>
          )
        })}
        {visibleMarker && (
          <div
            className="absolute right-7 -translate-y-1/2 whitespace-nowrap rounded-md bg-primary px-2 py-1 text-[10px] font-semibold text-primary-foreground shadow-md"
            style={{ top: `${(visibleIndex / Math.max(markers.length - 1, 1)) * 100}%` }}
          >
            {formatMarkerLabel(visibleMarker)}
          </div>
        )}
      </div>
    </aside>
  )
}

export default function TimelineView({ onPhotoClick, onAddToAlbum, onDelete, groupBy, search, mediaType }: TimelineViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const userInteractedRef = useRef(false)
  const pendingNewerRef = useRef(false)
  const loadingNewerRef = useRef(false)
  const [selectedMarker, setSelectedMarker] = useState<TimelineMarker | null>(null)
  const [activeMarkerIndex, setActiveMarkerIndex] = useState(0)
  const [markerJumpKey, setMarkerJumpKey] = useState(0)
  const markerQuery = useTimelineMarkers(mediaType, search)
  const { data: markerData, isLoading: isLoadingMarkers, error: markerError } = markerQuery
  const markers = markerData?.markers ?? EMPTY_MARKERS

  useEffect(() => {
    setSelectedMarker(markers[0] ?? null)
    setActiveMarkerIndex(0)
    setMarkerJumpKey(0)
    userInteractedRef.current = false
    pendingNewerRef.current = false
    scrollRef.current?.scrollTo({ top: 0 })
  }, [groupBy, markerData, markers])

  const timeline = useTimelineWindow({
    groupBy,
    search,
    mediaType,
    marker: selectedMarker,
    preloadKey: markerJumpKey,
    refreshKey: markerQuery.dataUpdatedAt,
  })

  const {
    groups: timelineGroups,
    hasNextPage,
    hasPreviousPage,
    isFetching,
    isLoadingOlder,
    isLoadingNewer,
    isLoading,
    error,
    loadOlder: requestOlder,
    loadNewer: requestNewer,
  } = timeline

  const updateActiveMarker = useCallback(() => {
    const container = scrollRef.current
    if (!container || markers.length === 0) return
    const viewportTop = container.getBoundingClientRect().top
    const firstSection = Array.from(container.querySelectorAll<HTMLElement>('[data-timeline-group]'))
      .find((section) => section.getBoundingClientRect().bottom > viewportTop + 1)
    const firstGroupDate = firstSection?.dataset.timelineGroup
    const firstGroup = timelineGroups.find((group) => group.date === firstGroupDate)
    const firstMediaDate = firstGroup?.media[0]?.dateTaken
    if (!firstMediaDate) return
    const markerIndex = markers.findIndex((marker) => firstMediaDate.slice(0, 7) === marker.label)
    if (markerIndex >= 0) {
      setActiveMarkerIndex((current) => current === markerIndex ? current : markerIndex)
    }
  }, [markers, timelineGroups])

  useEffect(() => {
    const frame = requestAnimationFrame(updateActiveMarker)
    return () => cancelAnimationFrame(frame)
  }, [timelineGroups, updateActiveMarker])

  const loadNewer = useCallback(async () => {
    if (!hasPreviousPage || isLoadingNewer || loadingNewerRef.current) return
    pendingNewerRef.current = false
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
  }, [hasPreviousPage, isLoadingNewer, requestNewer, updateActiveMarker])

  useEffect(() => {
    if (!pendingNewerRef.current || isFetching || !hasPreviousPage) return
    void loadNewer()
  }, [hasPreviousPage, isFetching, loadNewer])

  const loadOlder = useCallback(() => {
    if (!hasNextPage || isLoadingOlder) return
    void requestOlder()
  }, [hasNextPage, isLoadingOlder, requestOlder])

  const handleScroll = useCallback(() => {
    const container = scrollRef.current
    if (!container) return
    if (container.scrollTop > 0) userInteractedRef.current = true
    updateActiveMarker()
    if (userInteractedRef.current && container.scrollTop <= 100) {
      if (hasPreviousPage || isFetching) pendingNewerRef.current = true
      if (!isFetching) void loadNewer()
    }
    if (!userInteractedRef.current || isFetching) return
    if (container.scrollTop + container.clientHeight >= container.scrollHeight - 240) loadOlder()
  }, [hasPreviousPage, isFetching, loadNewer, loadOlder, updateActiveMarker])

  const handleWheel = useCallback((event: WheelEvent<HTMLElement>) => {
    userInteractedRef.current = true
    const container = scrollRef.current
    if (!container) return
    if (event.currentTarget !== container) {
      event.preventDefault()
      container.scrollBy({ top: event.deltaY, behavior: 'auto' })
    }
    if (event.deltaY < 0 && container.scrollTop <= 100) {
      if (hasPreviousPage || isFetching) pendingNewerRef.current = true
      if (!isFetching) void loadNewer()
    }
    if (isFetching) return
    if (event.deltaY > 0 && container.scrollTop + container.clientHeight >= container.scrollHeight - 240) loadOlder()
  }, [hasPreviousPage, isFetching, loadNewer, loadOlder])

  const handleMarkerSelect = useCallback((marker: TimelineMarker) => {
    const markerIndex = markers.findIndex((item) => item.label === marker.label)
    setSelectedMarker(marker)
    if (markerIndex >= 0) setActiveMarkerIndex(markerIndex)
    setMarkerJumpKey((key) => key + 1)
    userInteractedRef.current = false
    pendingNewerRef.current = false
    scrollRef.current?.scrollTo({ top: 0 })
  }, [markers])

  if (isLoadingMarkers || (selectedMarker !== null && isLoading)) {
    return <div className="flex h-[50vh] flex-col items-center justify-center gap-3 text-muted-foreground"><Loader2 className="h-8 w-8 animate-spin text-primary" /><p className="text-sm font-medium">Loading your memories...</p></div>
  }

  if (markerError || error) {
    return <div className="flex h-[50vh] flex-col items-center justify-center gap-3 text-destructive"><p className="text-lg font-semibold">Unable to load photos</p><p className="text-sm text-muted-foreground">Please try again later</p></div>
  }

  if (markers.length === 0) {
    return <div className="flex h-[50vh] flex-col items-center justify-center gap-6 text-muted-foreground"><ImageIcon className="h-12 w-12 opacity-40" /><div className="text-center"><h3 className="text-xl font-medium text-foreground">{search ? 'No matching media' : 'No media yet'}</h3><p className="mt-2 text-sm">{search ? `No media matched "${search}".` : 'Import some photos or videos to get started.'}</p></div></div>
  }

  const allMedia = timelineGroups.flatMap((group) => group.media)

  return (
    <div className="relative h-full min-h-0">
      {isFetching && <div className="absolute right-4 top-4 z-10 flex items-center gap-2 rounded-full bg-background/90 px-3 py-1.5 text-xs font-medium text-muted-foreground shadow-sm"><Loader2 className="h-3.5 w-3.5 animate-spin" /> Updating timeline</div>}
      {isLoadingNewer && <div className="pointer-events-none absolute left-1/2 top-4 z-10 -translate-x-1/2"><Loader2 className="h-5 w-5 animate-spin text-muted-foreground" /></div>}
      <TimelineScrubber markers={markers} activeMarkerIndex={activeMarkerIndex} onMarkerSelect={handleMarkerSelect} onWheel={handleWheel} />
      <div
        ref={scrollRef}
        className="h-full overflow-y-auto overscroll-contain pr-4 md:pr-20"
        onScroll={handleScroll}
        onWheel={handleWheel}
        onTouchMove={() => { userInteractedRef.current = true }}
        onPointerDown={() => { userInteractedRef.current = true }}
        onKeyDown={() => { userInteractedRef.current = true }}
      >
        {timelineGroups.map((group) => (
          <section key={group.date} data-timeline-group={group.date} className="mb-2">
            <DateHeader date={group.date} count={group.media.length} groupBy={groupBy} />
            <PhotoGrid media={group.media} onPhotoClick={(media) => onPhotoClick(media, allMedia)} onAddToAlbum={onAddToAlbum} onDelete={onDelete} />
          </section>
        ))}
        {isLoadingOlder && <div className="flex justify-center py-8"><Loader2 className="h-6 w-6 animate-spin text-muted-foreground" /></div>}
        {!hasNextPage && <div className="py-10 text-center text-xs uppercase tracking-[0.2em] text-muted-foreground">End of timeline</div>}
      </div>
    </div>
  )
}
