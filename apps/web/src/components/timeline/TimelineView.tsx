import { useCallback } from 'react'
import { Virtuoso } from 'react-virtuoso'
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
}

export default function TimelineView({ onPhotoClick, onAddToAlbum, onDelete, groupBy = 'day' }: TimelineViewProps) {
  const { data, fetchNextPage, hasNextPage, isFetchingNextPage, isLoading, error } = useTimeline(groupBy)

  const groups = data?.pages.flatMap((page) => page.groups) ?? []
  const allMedia = groups.flatMap((g) => g.media)

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
          <h3 className="text-xl font-medium text-foreground font-display tracking-tight">No photos yet</h3>
          <p className="text-sm mt-2 font-medium">Import some photos to get started.</p>
        </div>
      </div>
    )
  }

  return (
    <Virtuoso
      style={{ height: '100%' }}
      data={groups}
      endReached={loadMore}
      overscan={200}
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
  )
}

