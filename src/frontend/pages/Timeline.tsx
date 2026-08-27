import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useSearchParams } from 'react-router-dom'
import TimelineView from '../components/timeline/TimelineView'
import Lightbox from '../components/viewer/Lightbox'
import AddToAlbumModal from '../components/albums/AddToAlbumModal'
import ConfirmationDialog from '../components/common/ConfirmationDialog'
import MediaSelectionToolbar, { MediaSelectButton } from '../components/media/MediaSelectionToolbar'
import { mediaApi, type GroupBy, type MediaTypeFilter, type TimelineClassification } from '../api/media'
import type { Media } from '../api/types'
import { Calendar, ChevronDown, Search } from 'lucide-react'
import { useMediaSelection } from '../hooks/useMediaSelection'

const groupByOptions: { value: GroupBy; label: string }[] = [
  { value: 'day', label: 'Day' },
  { value: 'week', label: 'Week' },
  { value: 'month', label: 'Month' },
  { value: 'year', label: 'Year' },
]

interface TimelineProps {
  mediaType: MediaTypeFilter | null
  classification: TimelineClassification | null
}

export default function Timeline({ mediaType, classification }: TimelineProps) {
  const queryClient = useQueryClient()
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [initialIndex, setInitialIndex] = useState(0)
  const [mediaIds, setMediaIds] = useState<number[]>([])
  const [showAlbumPicker, setShowAlbumPicker] = useState(false)
  const [showTrashConfirmation, setShowTrashConfirmation] = useState(false)
  const [selectionError, setSelectionError] = useState<string | null>(null)
  const [groupBy, setGroupBy] = useState<GroupBy>('day')
  const [showGroupByMenu, setShowGroupByMenu] = useState(false)
  const [searchParams, setSearchParams] = useSearchParams()
  const searchParameter = searchParams.get('search') ?? ''
  const [searchInput, setSearchInput] = useState(searchParameter)
  const [search, setSearch] = useState(searchParameter)
  const {
    selectionMode,
    selectedMediaIds,
    startSelection,
    clearSelection,
    cancelSelection,
    toggleSelection,
  } = useMediaSelection()

  useEffect(() => {
    setSearchInput(searchParameter)
    setSearch(searchParameter)
  }, [searchParameter])

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      const normalizedSearch = searchInput.trim()
      setSearch(normalizedSearch)
      setSearchParams((currentParams) => {
        const nextParams = new URLSearchParams(currentParams)
        if (normalizedSearch) {
          nextParams.set('search', normalizedSearch)
        } else {
          nextParams.delete('search')
        }
        return nextParams
      }, { replace: true })
    }, 250)

    return () => window.clearTimeout(timeoutId)
  }, [searchInput, setSearchParams])

  useEffect(() => {
    cancelSelection()
    setShowAlbumPicker(false)
    setShowTrashConfirmation(false)
    setSelectionError(null)
  }, [cancelSelection, classification, groupBy, mediaType, search])

  const deleteMutation = useMutation({
    mutationFn: mediaApi.delete,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['timeline'] })
      void queryClient.invalidateQueries({ queryKey: ['media'] })
      void queryClient.invalidateQueries({ queryKey: ['trash'] })
      void queryClient.invalidateQueries({ queryKey: ['deduplicate', 'groups'] })
      setShowTrashConfirmation(false)
      cancelSelection()
    },
    onError: () => {
      setShowTrashConfirmation(false)
      setSelectionError('Could not move the selected media to Trash. Nothing was removed from this view.')
    },
  })

  const handlePhotoClick = (media: Media, allMedia: Media[]) => {
    const index = allMedia.findIndex((m) => m.id === media.id)
    setMediaIds(allMedia.map((item) => item.id))
    setInitialIndex(index >= 0 ? index : 0)
    setLightboxOpen(true)
  }

  const currentGroupByLabel = groupByOptions.find((o) => o.value === groupBy)?.label || 'Day'
  const title = classification === 'screenshot' ? 'Screenshots' : classification === 'document' ? 'Documents' : mediaType === 'image' ? 'Photos' : mediaType === 'video' ? 'Videos' : 'Timeline'
  const searchPlaceholder = classification === 'screenshot' ? 'Search screenshots...' : classification === 'document' ? 'Search documents...' : mediaType === 'image' ? 'Search photos...' : mediaType === 'video' ? 'Search videos...' : 'Search media...'

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="container max-w-[1800px] mx-auto px-6 md:px-10 pt-6 md:pt-10">
        <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex items-center justify-between gap-3 sm:justify-start">
            <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">{title}</h1>
            {!selectionMode && <MediaSelectButton onClick={startSelection} />}
          </div>
          <div className="flex flex-col sm:flex-row gap-3 sm:items-center">
            <label className="relative block sm:w-72 lg:w-96">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
               <span className="sr-only">Search media</span>
              <input
                type="search"
                value={searchInput}
                onChange={(event) => setSearchInput(event.target.value)}
                 placeholder={searchPlaceholder}
                 aria-label="Search media"
                className="w-full rounded-lg border border-border bg-background py-2 pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus:border-primary focus:ring-2 focus:ring-primary/20"
              />
            </label>
            <div className="relative self-end sm:self-auto">
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
        {selectionMode && (
          <div className="mb-4">
            <MediaSelectionToolbar
              selectedCount={selectedMediaIds.size}
              isProcessing={deleteMutation.isPending}
              onClear={clearSelection}
              onDone={cancelSelection}
              onAddToAlbum={() => setShowAlbumPicker(true)}
              onRemoveFromAlbum={null}
              onMoveToTrash={() => setShowTrashConfirmation(true)}
            />
          </div>
        )}
        {selectionError && (
          <p role="alert" className="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">
            {selectionError}
          </p>
        )}
      </div>
      <div className="flex-1 min-h-0">
        <TimelineView
          onPhotoClick={handlePhotoClick}
          selection={selectionMode ? { selectedMediaIds, toggleSelection } : null}
           groupBy={groupBy}
           search={search}
           mediaType={mediaType}
           classification={classification}
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
      {showAlbumPicker && selectedMediaIds.size > 0 && (
        <AddToAlbumModal
          mediaIds={Array.from(selectedMediaIds)}
          onClose={() => {
            setShowAlbumPicker(false)
            cancelSelection()
          }}
        />
      )}
      {showTrashConfirmation && (
        <ConfirmationDialog
          title={`Move ${selectedMediaIds.size} selected item${selectedMediaIds.size === 1 ? '' : 's'} to Trash?`}
          description="The selected media will leave your timelines and albums. You can restore it from Trash."
          confirmLabel="Move to Trash"
          isProcessing={deleteMutation.isPending}
          destructive
          onConfirm={() => deleteMutation.mutate(Array.from(selectedMediaIds))}
          onCancel={() => setShowTrashConfirmation(false)}
        />
      )}
    </div>
  )
}
