import { useCallback, useEffect, useRef, useState } from 'react'
import { Image as ImageIcon, Loader2 } from 'lucide-react'
import type {
  GroupBy,
  MediaTypeFilter,
  TimelineClassification,
  TimelineMarker,
} from '../../api/media'
import type { Media } from '../../api/types'
import { useActiveTimelineMarker, useTimelinePaging } from '../../hooks/useTimelineNavigation'
import { useTimelineMarkers, useTimelineWindow } from '../../hooks/useTimeline'
import { timelinePresentation } from '../../lib/timelinePresentation'
import DateHeader from './DateHeader'
import PhotoGrid, { type PhotoGridSelection } from './PhotoGrid'
import TimelineScrubber from './TimelineScrubber'

const EMPTY_MARKERS: TimelineMarker[] = []

interface TimelineViewProps {
  onPhotoClick: (media: Media, allMedia: Media[]) => void
  selection: PhotoGridSelection | null
  groupBy: GroupBy
  search: string
  mediaType: MediaTypeFilter | null
  classification: TimelineClassification | null
}

function TimelineEmptyState({
  classification,
  mediaType,
  search,
}: Pick<TimelineViewProps, 'classification' | 'mediaType' | 'search'>) {
  const { mediaLabel, emptyDescription } = timelinePresentation(mediaType, classification)
  return (
    <div className="flex h-[50vh] flex-col items-center justify-center gap-6 text-muted-foreground">
      <ImageIcon className="h-12 w-12 opacity-40" />
      <div className="text-center">
        <h3 className="text-xl font-medium text-foreground">
          {search ? `No matching ${mediaLabel}` : `No ${mediaLabel} yet`}
        </h3>
        <p className="mt-2 text-sm">
          {search ? `No ${mediaLabel} matched "${search}".` : emptyDescription}
        </p>
      </div>
    </div>
  )
}

function timelineStatus(isLoading: boolean, error: unknown) {
  if (isLoading) {
    return (
      <div className="flex h-[50vh] flex-col items-center justify-center gap-3 text-muted-foreground">
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading your memories...</p>
      </div>
    )
  }
  if (error) {
    return (
      <div className="flex h-[50vh] flex-col items-center justify-center gap-3 text-destructive">
        <p className="text-lg font-semibold">Unable to load photos</p>
        <p className="text-sm text-muted-foreground">Please try again later</p>
      </div>
    )
  }
  return null
}

function TimelineActivityIndicators({
  isFetching,
  isLoadingNewer,
}: {
  isFetching: boolean
  isLoadingNewer: boolean
}) {
  return (
    <>
      {isFetching && (
        <div className="absolute right-4 top-4 z-10 flex items-center gap-2 rounded-full bg-background/90 px-3 py-1.5 text-xs font-medium text-muted-foreground shadow-sm">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> Updating timeline
        </div>
      )}
      {isLoadingNewer && (
        <div className="pointer-events-none absolute left-1/2 top-4 z-10 -translate-x-1/2">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      )}
    </>
  )
}

export default function TimelineView({
  onPhotoClick,
  selection,
  groupBy,
  search,
  mediaType,
  classification,
}: TimelineViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [selectedMarker, setSelectedMarker] = useState<TimelineMarker | null>(null)
  const markerQuery = useTimelineMarkers(mediaType, classification, search)
  const { data: markerData, isLoading: isLoadingMarkers, error: markerError } = markerQuery
  const markers = markerData?.markers ?? EMPTY_MARKERS

  const timeline = useTimelineWindow({
    groupBy,
    search,
    mediaType,
    classification,
    marker: selectedMarker,
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

  const { activeMarkerIndex, setActiveMarkerIndex, updateActiveMarker } = useActiveTimelineMarker(
    scrollRef,
    markers,
    timelineGroups
  )
  const paging = useTimelinePaging({
    scrollRef,
    hasOlder: hasNextPage,
    hasNewer: hasPreviousPage,
    isLoadingOlder,
    isLoadingNewer,
    requestOlder,
    requestNewer,
    updateActiveMarker,
  })
  const { handleScroll, handleWheel, reset: resetPaging } = paging

  useEffect(() => {
    setSelectedMarker(markers[0] ?? null)
    setActiveMarkerIndex(0)
    resetPaging()
  }, [groupBy, markerData, markers, resetPaging, setActiveMarkerIndex])

  const handleMarkerSelect = useCallback(
    (marker: TimelineMarker) => {
      const markerIndex = markers.findIndex((item) => item.label === marker.label)
      setSelectedMarker(marker)
      if (markerIndex >= 0) setActiveMarkerIndex(markerIndex)
      resetPaging()
    },
    [markers, resetPaging, setActiveMarkerIndex]
  )

  const status = timelineStatus(
    isLoadingMarkers || (selectedMarker !== null && isLoading),
    markerError || error
  )
  if (status) return status

  if (markers.length === 0) {
    return (
      <TimelineEmptyState classification={classification} mediaType={mediaType} search={search} />
    )
  }

  const allMedia = timelineGroups.flatMap((group) => group.media)

  return (
    <div className="relative h-full min-h-0">
      <TimelineActivityIndicators isFetching={isFetching} isLoadingNewer={isLoadingNewer} />
      <TimelineScrubber
        markers={markers}
        activeMarkerIndex={activeMarkerIndex}
        onMarkerSelect={handleMarkerSelect}
        onWheel={handleWheel}
      />
      <div
        ref={scrollRef}
        className="h-full overflow-y-auto overscroll-contain pr-4 md:pr-20"
        onScroll={handleScroll}
        onWheel={handleWheel}
      >
        {timelineGroups.map((group) => (
          <section key={group.date} data-timeline-group={group.date} className="mb-2">
            <DateHeader date={group.date} count={group.media.length} groupBy={groupBy} />
            <PhotoGrid
              media={group.media}
              onPhotoClick={(media) => onPhotoClick(media, allMedia)}
              selection={selection}
            />
          </section>
        ))}
        {isLoadingOlder && (
          <div className="flex justify-center py-8">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        )}
        {!hasNextPage && (
          <div className="py-10 text-center text-xs uppercase tracking-[0.2em] text-muted-foreground">
            End of timeline
          </div>
        )}
      </div>
    </div>
  )
}
