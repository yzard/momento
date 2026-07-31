import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import TimelineView from '../components/timeline/TimelineView'
import Lightbox from '../components/viewer/Lightbox'
import AddToAlbumModal from '../components/albums/AddToAlbumModal'
import { mediaApi, type GroupBy } from '../api/media'
import type { Media } from '../api/types'
import { Calendar, ChevronDown } from 'lucide-react'

const groupByOptions: { value: GroupBy; label: string }[] = [
  { value: 'day', label: 'Day' },
  { value: 'week', label: 'Week' },
  { value: 'month', label: 'Month' },
  { value: 'year', label: 'Year' },
]

export default function Timeline() {
  const queryClient = useQueryClient()
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [initialIndex, setInitialIndex] = useState(0)
  const [mediaIds, setMediaIds] = useState<number[]>([])
  const [addToAlbumMedia, setAddToAlbumMedia] = useState<Media | null>(null)
  const [groupBy, setGroupBy] = useState<GroupBy>('day')
  const [showGroupByMenu, setShowGroupByMenu] = useState(false)

  const deleteMutation = useMutation({
    mutationFn: mediaApi.delete,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['timeline'] })
      queryClient.invalidateQueries({ queryKey: ['media'] })
      queryClient.invalidateQueries({ queryKey: ['trash'] })
    },
  })

  const handlePhotoClick = (media: Media, allMedia: Media[]) => {
    const index = allMedia.findIndex((m) => m.id === media.id)
    setMediaIds(allMedia.map((item) => item.id))
    setInitialIndex(index >= 0 ? index : 0)
    setLightboxOpen(true)
  }

  const handleAddToAlbum = (media: Media) => {
    setAddToAlbumMedia(media)
  }

  const handleDelete = (media: Media) => {
    deleteMutation.mutate(media.id)
  }

  const currentGroupByLabel = groupByOptions.find((o) => o.value === groupBy)?.label || 'Day'

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="container max-w-[1800px] mx-auto px-6 md:px-10 pt-6 md:pt-10">
        <div className="flex items-center justify-between mb-4">
          <h1 className="text-2xl font-semibold">Timeline</h1>
          <div className="relative">
            <button
              onClick={() => setShowGroupByMenu(!showGroupByMenu)}
              className="flex items-center gap-2 px-4 py-2 rounded-lg bg-muted hover:bg-muted/80 transition-colors text-sm font-medium"
            >
              <Calendar className="w-4 h-4" />
              {currentGroupByLabel}
              <ChevronDown className="w-4 h-4" />
            </button>
            {showGroupByMenu && (
              <>
                <div
                  className="fixed inset-0 z-40"
                  onClick={() => setShowGroupByMenu(false)}
                />
                <div className="absolute right-0 top-full mt-2 bg-background border border-border rounded-lg shadow-lg py-1 z-50 min-w-[120px]">
                  {groupByOptions.map((option) => (
                    <button
                      key={option.value}
                      onClick={() => {
                        setGroupBy(option.value)
                        setShowGroupByMenu(false)
                      }}
                      className={`w-full px-4 py-2 text-left text-sm hover:bg-muted transition-colors ${
                        groupBy === option.value ? 'text-primary font-medium' : ''
                      }`}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </>
            )}
          </div>
        </div>
      </div>
      <div className="flex-1 min-h-0">
        <TimelineView
          onPhotoClick={handlePhotoClick}
          onAddToAlbum={handleAddToAlbum}
          onDelete={handleDelete}
          groupBy={groupBy}
        />
      </div>
      {lightboxOpen && (
        <Lightbox
          mediaIds={mediaIds}
          currentIndex={initialIndex}
          onClose={() => setLightboxOpen(false)}
          onIndexChange={setInitialIndex}
        />
      )}
      {addToAlbumMedia && (
        <AddToAlbumModal
          mediaId={addToAlbumMedia.id}
          onClose={() => setAddToAlbumMedia(null)}
        />
      )}
    </div>
  )
}
