import { useEffect, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useSearchParams } from 'react-router-dom'
import TimelineView from '../components/timeline/TimelineView'
import ManagedLightbox from '../components/viewer/ManagedLightbox'
import AddToAlbumModal from '../components/albums/AddToAlbumModal'
import ConfirmationDialog from '../components/common/ConfirmationDialog'
import MediaSelectionToolbar, { MediaSelectButton } from '../components/media/MediaSelectionToolbar'
import {
  mediaApi,
  type GroupBy,
  type MediaTypeFilter,
  type TimelineClassification,
} from '../api/media'
import type { Media } from '../api/types'
import { Calendar, ChevronDown, Search } from 'lucide-react'
import { useMediaSelection } from '../hooks/useMediaSelection'
import { invalidateMediaConsumers } from '../lib/queryKeys'
import { timelinePresentation } from '../lib/timelinePresentation'
import { useLightbox } from '../hooks/useLightbox'

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

function useTimelineSearch() {
  const [searchParams, setSearchParams] = useSearchParams()
  const searchParameter = searchParams.get('search') ?? ''
  const [searchInput, setSearchInput] = useState(searchParameter)
  const [search, setSearch] = useState(searchParameter)

  useEffect(() => {
    setSearchInput(searchParameter)
    setSearch(searchParameter)
  }, [searchParameter])

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      const normalizedSearch = searchInput.trim()
      setSearch(normalizedSearch)
      setSearchParams(
        (currentParams) => {
          const nextParams = new URLSearchParams(currentParams)
          if (normalizedSearch) nextParams.set('search', normalizedSearch)
          else nextParams.delete('search')
          return nextParams
        },
        { replace: true }
      )
    }, 250)
    return () => window.clearTimeout(timeoutId)
  }, [searchInput, setSearchParams])

  return { searchInput, setSearchInput, search }
}

interface TimelineHeaderProps {
  title: string
  searchPlaceholder: string
  searchInput: string
  groupBy: GroupBy
  selectionMode: boolean
  onSearchInputChange: (value: string) => void
  onGroupByChange: (groupBy: GroupBy) => void
  onStartSelection: () => void
}

function TimelineHeader(props: TimelineHeaderProps) {
  const [showGroupByMenu, setShowGroupByMenu] = useState(false)
  const currentGroupByLabel =
    groupByOptions.find((option) => option.value === props.groupBy)?.label ?? 'Day'

  return (
    <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex items-center justify-between gap-3 sm:justify-start">
        <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">
          {props.title}
        </h1>
        {!props.selectionMode && <MediaSelectButton onClick={props.onStartSelection} />}
      </div>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <label className="relative block sm:w-72 lg:w-96">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <span className="sr-only">Search media</span>
          <input
            type="search"
            value={props.searchInput}
            onChange={(event) => props.onSearchInputChange(event.target.value)}
            placeholder={props.searchPlaceholder}
            aria-label="Search media"
            className="w-full rounded-lg border border-border bg-background py-2 pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus:border-primary focus:ring-2 focus:ring-primary/20"
          />
        </label>
        <div className="relative self-end sm:self-auto">
          <button
            type="button"
            onClick={() => setShowGroupByMenu((visible) => !visible)}
            className="flex items-center gap-2 rounded-lg bg-muted px-4 py-2 text-sm font-medium transition-colors hover:bg-muted/80"
          >
            <Calendar className="h-4 w-4" />
            {currentGroupByLabel}
            <ChevronDown className="h-4 w-4" />
          </button>
          {showGroupByMenu && (
            <>
              <button
                type="button"
                aria-label="Close grouping menu"
                className="fixed inset-0 z-40"
                onClick={() => setShowGroupByMenu(false)}
              />
              <div className="absolute right-0 top-full z-50 mt-2 min-w-[120px] rounded-lg border border-border bg-background py-1 shadow-lg">
                {groupByOptions.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => {
                      props.onGroupByChange(option.value)
                      setShowGroupByMenu(false)
                    }}
                    className={`w-full px-4 py-2 text-left text-sm transition-colors hover:bg-muted ${props.groupBy === option.value ? 'font-medium text-primary' : ''}`}
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
  )
}

function useTimelineTrash(
  cancelSelection: () => void,
  closeConfirmation: () => void,
  showError: (message: string) => void
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: mediaApi.delete,
    onSuccess: () => {
      void invalidateMediaConsumers(queryClient)
      closeConfirmation()
      cancelSelection()
    },
    onError: () => {
      closeConfirmation()
      showError('Could not move the selected media to Trash. Nothing was removed from this view.')
    },
  })
}

export default function Timeline({ mediaType, classification }: TimelineProps) {
  const lightbox = useLightbox()
  const [showAlbumPicker, setShowAlbumPicker] = useState(false)
  const [showTrashConfirmation, setShowTrashConfirmation] = useState(false)
  const [selectionError, setSelectionError] = useState<string | null>(null)
  const [groupBy, setGroupBy] = useState<GroupBy>('day')
  const { searchInput, setSearchInput, search } = useTimelineSearch()
  const {
    selectionMode,
    selectedMediaIds,
    startSelection,
    clearSelection,
    cancelSelection,
    toggleSelection,
  } = useMediaSelection()

  useEffect(() => {
    cancelSelection()
    setShowAlbumPicker(false)
    setShowTrashConfirmation(false)
    setSelectionError(null)
  }, [cancelSelection, classification, groupBy, mediaType, search])

  const deleteMutation = useTimelineTrash(
    cancelSelection,
    () => setShowTrashConfirmation(false),
    setSelectionError
  )

  const handlePhotoClick = (media: Media, allMedia: Media[]) => {
    lightbox.open(
      media.id,
      allMedia.map((item) => item.id)
    )
  }

  const { title, searchPlaceholder } = timelinePresentation(mediaType, classification)

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="container max-w-[1800px] mx-auto px-6 md:px-10 pt-6 md:pt-10">
        <TimelineHeader
          title={title}
          searchPlaceholder={searchPlaceholder}
          searchInput={searchInput}
          groupBy={groupBy}
          selectionMode={selectionMode}
          onSearchInputChange={setSearchInput}
          onGroupByChange={setGroupBy}
          onStartSelection={startSelection}
        />
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
          <p
            role="alert"
            className="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive"
          >
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
      <ManagedLightbox controller={lightbox} />
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
