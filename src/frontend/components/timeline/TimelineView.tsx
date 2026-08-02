import { useCallback, useEffect, useRef } from 'react'
import { Virtuoso, type VirtuosoHandle, type ListRange } from 'react-virtuoso'
import { useTimeline } from '../../hooks/useMedia'
import DateHeader from './DateHeader'
import PhotoGrid from './PhotoGrid'
import type { Media } from '../../api/types'
import type { GroupBy } from '../../api/media'
import { Loader2, Image as ImageIcon } from 'lucide-react'

interface TimelineViewProps {
  onPhotoClick: (media: Media, allMedia: Media[]) => void
  onAddToAlbum?: (media: Media) => void
  onDelete?: (media: Media) => void
  groupBy?: GroupBy
  search: string
}

const SCROLL_STORAGE_KEY = 'timeline_scroll_index'

export default function TimelineView({ onPhotoClick, onAddToAlbum, onDelete, groupBy = 'day', search }: TimelineViewProps) {
  const { data, fetchNextPage, hasNextPage, isFetching, isFetchingNextPage, isLoading, error } =
    useTimeline(groupBy, 100, search)
  const virtuosoRef = useRef<VirtuosoHandle>(null)
  const lastGroupByRef = useRef(groupBy)
  const lastSearchRef = useRef(search)

  const groups = (() => {
    const allGroups = data?.pages.flatMap((page) => page.groups ?? []) ?? []
    const merged = new Map<string, Media[]>()
    for (const group of allGroups) {
      const existing = merged.get(group.date)
      if (existing) {
        existing.push(...group.media)
      } else {
        merged.set(group.date, [...group.media])
      }
    }
    return Array.from(merged.entries()).map(([date, media]) => ({ date, media }))
  })()
  const allMedia = groups.flatMap((g) => g.media)

  const savedIndex = sessionStorage.getItem(SCROLL_STORAGE_KEY)
  const initialIndex = savedIndex ? parseInt(savedIndex, 10) : 0

  useEffect(() => {
    if (lastGroupByRef.current !== groupBy || lastSearchRef.current !== search) {
      sessionStorage.removeItem(SCROLL_STORAGE_KEY)
      virtuosoRef.current?.scrollToIndex({ index: 0 })
      lastGroupByRef.current = groupBy
      lastSearchRef.current = search
    }
  }, [groupBy, search])

  const handleRangeChanged = useCallback((range: ListRange) => {
    sessionStorage.setItem(SCROLL_STORAGE_KEY, String(range.startIndex))
  }, [])

  const loadMore = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) {
      fetchNextPage()
    }
  }, [hasNextPage, isFetchingNextPage, fetchNextPage])

  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-muted-foreground gap-3">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading your memories...</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-destructive gap-3">
        <p className="text-lg font-semibold">Unable to load photos</p>
        <p className="text-sm text-muted-foreground">Please try again later</p>
      </div>
    )
  }

  if (groups.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-muted-foreground gap-6">
        <div className="w-20 h-20 bg-muted/20 flex items-center justify-center rounded-full border border-border/50 shadow-lg">
          <ImageIcon className="w-8 h-8 opacity-50 text-primary" strokeWidth={1.5} />
        </div>
        <div className="text-center">
          <h3 className="text-xl font-medium text-foreground font-display tracking-tight">
            {search ? 'No matching photos' : 'No photos yet'}
          </h3>
          <p className="text-sm mt-2 font-medium">
            {search ? `No photos matched "${search}".` : 'Import some photos to get started.'}
          </p>
        </div>
      </div>
    )
  }

  return (
    <div className="relative h-full">
      {isFetching && !isFetchingNextPage && (
        <div className="absolute right-4 top-4 z-10 flex items-center gap-2 rounded-full bg-background/90 px-3 py-1.5 text-xs font-medium text-muted-foreground shadow-sm">
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
          Searching...
        </div>
      )}
      <Virtuoso
        ref={virtuosoRef}
        style={{ height: '100%' }}
        data={groups}
        initialTopMostItemIndex={initialIndex}
        rangeChanged={handleRangeChanged}
        endReached={loadMore}
        overscan={500}
        itemContent={(_, group) => (
          <div key={group.date} className="mb-2">
            <DateHeader date={group.date} count={group.media.length} groupBy={groupBy} />
            <PhotoGrid
              media={group.media}
              onPhotoClick={(media) => onPhotoClick(media, allMedia)}
              onAddToAlbum={onAddToAlbum}
              onDelete={onDelete}
            />
          </div>
        )}
        components={{
          Footer: () =>
            isFetchingNextPage ? (
              <div className="py-8 flex justify-center text-muted-foreground">
                <Loader2 className="w-6 h-6 animate-spin" />
              </div>
            ) : <div className="py-8" />,
        }}
      />
    </div>
  )
}
