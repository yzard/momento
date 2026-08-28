import { useState, useEffect, useRef } from 'react'
import { useAlbum, useRemoveAlbumMedia, useReorderAlbum } from '../../hooks/useAlbums'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { ArrowLeft, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'
import { batchLoader } from '../../utils/batcher'
import { useMediaSelection } from '../../hooks/useMediaSelection'
import ConfirmationDialog from '../common/ConfirmationDialog'
import MediaSelectionToolbar, { MediaSelectButton } from '../media/MediaSelectionToolbar'
import MediaSelectionOverlay from '../media/MediaSelectionOverlay'
import { useLazyImage } from '../../hooks/useLazyImage'

interface AlbumViewProps {
  albumId: number
  onBack: () => void
  onPhotoClick: (media: Media, allMedia: Media[]) => void
}

function useAlbumOrdering(
  albumId: number,
  albumMedia: Media[] | undefined,
  selectionMode: boolean
) {
  const reorderAlbum = useReorderAlbum()
  const [items, setItems] = useState<Media[]>([])
  const [draggedId, setDraggedId] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const itemsRef = useRef<Media[]>([])
  const confirmedItemsRef = useRef<Media[]>([])
  const inFlightRef = useRef(false)

  useEffect(() => {
    if (!albumMedia || draggedId !== null || inFlightRef.current) return
    itemsRef.current = albumMedia
    confirmedItemsRef.current = albumMedia
    setItems(albumMedia)
  }, [albumMedia, draggedId])

  const dragStart = (event: React.DragEvent, mediaId: number) => {
    if (selectionMode) return
    setDraggedId(mediaId)
    event.dataTransfer.effectAllowed = 'move'
  }
  const dragOver = (event: React.DragEvent, targetId: number) => {
    event.preventDefault()
    if (selectionMode || draggedId === null || draggedId === targetId) return
    const nextItems = moveAlbumMedia(itemsRef.current, draggedId, targetId)
    itemsRef.current = nextItems
    setItems(nextItems)
  }
  const drop = (event: React.DragEvent) => {
    event.preventDefault()
    if (selectionMode || draggedId === null || itemsRef.current.length === 0 || inFlightRef.current)
      return
    const desiredItems = itemsRef.current
    const confirmedItems = confirmedItemsRef.current
    setDraggedId(null)
    if (desiredItems.every((item, index) => item.id === confirmedItems[index]?.id)) return
    inFlightRef.current = true
    setError(null)
    void reorderAlbum
      .mutateAsync({ albumId, mediaIds: desiredItems.map((item) => item.id) })
      .then(() => {
        confirmedItemsRef.current = desiredItems
      })
      .catch(() => {
        itemsRef.current = confirmedItems
        setItems(confirmedItems)
        setError('Could not save the album order.')
      })
      .finally(() => {
        inFlightRef.current = false
      })
  }
  const removeItems = (mediaIds: ReadonlySet<number>) => {
    const remainingItems = itemsRef.current.filter((item) => !mediaIds.has(item.id))
    itemsRef.current = remainingItems
    confirmedItemsRef.current = remainingItems
    setItems(remainingItems)
  }
  return { items, draggedId, error, dragStart, dragOver, drop, removeItems }
}

interface AlbumHeaderProps {
  name: string
  description: string | null
  itemCount: number
  selectionMode: boolean
  selectedCount: number
  isRemoving: boolean
  onBack: () => void
  onStartSelection: () => void
  onClearSelection: () => void
  onCancelSelection: () => void
  onRemoveRequest: () => void
}

function AlbumHeader(props: AlbumHeaderProps) {
  return (
    <div className="mb-8 flex flex-col gap-6">
      <button
        onClick={props.onBack}
        className="group flex w-fit items-center gap-2 text-muted-foreground transition-colors hover:text-foreground"
      >
        <span className="rounded-full bg-muted/50 p-2 transition-colors group-hover:bg-muted">
          <ArrowLeft className="h-4 w-4" />
        </span>
        <span className="text-sm font-medium">Back to Albums</span>
      </button>
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="font-display text-4xl font-bold tracking-tight text-foreground">
            {props.name}
          </h2>
          {!props.selectionMode && <MediaSelectButton onClick={props.onStartSelection} />}
        </div>
        <div className="flex items-center gap-4 text-sm text-muted-foreground">
          <span className="rounded-md bg-secondary px-2.5 py-0.5 font-medium text-secondary-foreground">
            {props.itemCount} items
          </span>
          {props.description && <span>{props.description}</span>}
        </div>
        {props.selectionMode && (
          <MediaSelectionToolbar
            selectedCount={props.selectedCount}
            isProcessing={props.isRemoving}
            onClear={props.onClearSelection}
            onDone={props.onCancelSelection}
            onAddToAlbum={null}
            onRemoveFromAlbum={props.onRemoveRequest}
            onMoveToTrash={null}
          />
        )}
      </div>
    </div>
  )
}

interface AlbumMediaGridProps {
  items: Media[]
  draggedId: number | null
  selectionMode: boolean
  selectedMediaIds: ReadonlySet<number>
  onToggleSelection: (mediaId: number) => void
  onPhotoClick: (media: Media, allMedia: Media[]) => void
  onDragStart: (event: React.DragEvent, mediaId: number) => void
  onDragOver: (event: React.DragEvent, mediaId: number) => void
  onDrop: (event: React.DragEvent) => void
}

function AlbumMediaGrid(props: AlbumMediaGridProps) {
  if (props.items.length === 0) {
    return (
      <div className="rounded-2xl border-2 border-dashed border-border bg-muted/20 py-20 text-center text-muted-foreground">
        <p>No photos in this album yet.</p>
      </div>
    )
  }

  return (
    <div className="grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8">
      {props.items.map((item) => (
        <AlbumMediaItem
          key={item.id}
          item={item}
          isDragged={props.draggedId === item.id}
          selectionMode={props.selectionMode}
          selected={props.selectedMediaIds.has(item.id)}
          onDragStart={(event) => props.onDragStart(event, item.id)}
          onDragOver={(event) => props.onDragOver(event, item.id)}
          onDrop={props.onDrop}
          onClick={() => {
            if (props.selectionMode) {
              props.onToggleSelection(item.id)
              return
            }
            props.onPhotoClick(item, props.items)
          }}
        />
      ))}
    </div>
  )
}

export default function AlbumView({ albumId, onBack, onPhotoClick }: AlbumViewProps) {
  const { data: album, isLoading, error } = useAlbum(albumId)
  const removeAlbumMedia = useRemoveAlbumMedia()
  const [removeError, setRemoveError] = useState<string | null>(null)
  const [showRemoveConfirmation, setShowRemoveConfirmation] = useState(false)
  const {
    selectionMode,
    selectedMediaIds,
    startSelection,
    clearSelection,
    cancelSelection,
    toggleSelection,
  } = useMediaSelection()
  const ordering = useAlbumOrdering(albumId, album?.media, selectionMode)

  useEffect(() => {
    cancelSelection()
    setShowRemoveConfirmation(false)
    setRemoveError(null)
  }, [albumId, cancelSelection])

  const handleRemoveSelected = () => {
    if (selectedMediaIds.size === 0 || removeAlbumMedia.isPending) return
    const mediaIds = Array.from(selectedMediaIds)
    const removedMediaIds = new Set(selectedMediaIds)
    setRemoveError(null)
    removeAlbumMedia.mutate(
      { albumId, mediaIds },
      {
        onSuccess: () => {
          ordering.removeItems(removedMediaIds)
          setShowRemoveConfirmation(false)
          cancelSelection()
        },
        onError: () => {
          setShowRemoveConfirmation(false)
          setRemoveError(
            'Could not remove the selected media from this album. Nothing was removed.'
          )
        },
      }
    )
  }

  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-muted-foreground gap-3">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading album...</p>
      </div>
    )
  }

  if (error || !album) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-destructive gap-3">
        <p className="text-lg font-semibold">Unable to load album</p>
        <button onClick={onBack} className="text-sm underline hover:text-destructive/80">
          Go back
        </button>
      </div>
    )
  }

  return (
    <div className="animate-fade-in">
      <AlbumHeader
        name={album.name}
        description={album.description}
        itemCount={ordering.items.length}
        selectionMode={selectionMode}
        selectedCount={selectedMediaIds.size}
        isRemoving={removeAlbumMedia.isPending}
        onBack={onBack}
        onStartSelection={startSelection}
        onClearSelection={clearSelection}
        onCancelSelection={cancelSelection}
        onRemoveRequest={() => setShowRemoveConfirmation(true)}
      />

      <AlbumMediaGrid
        items={ordering.items}
        draggedId={ordering.draggedId}
        selectionMode={selectionMode}
        selectedMediaIds={selectedMediaIds}
        onToggleSelection={toggleSelection}
        onPhotoClick={onPhotoClick}
        onDragStart={ordering.dragStart}
        onDragOver={ordering.dragOver}
        onDrop={ordering.drop}
      />
      {ordering.error && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {ordering.error}
        </p>
      )}
      {removeError && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {removeError}
        </p>
      )}
      {showRemoveConfirmation && (
        <ConfirmationDialog
          title={`Remove ${selectedMediaIds.size} selected item${selectedMediaIds.size === 1 ? '' : 's'}?`}
          description="The selected media will remain in your library and other albums."
          confirmLabel="Remove from album"
          isProcessing={removeAlbumMedia.isPending}
          destructive={false}
          onConfirm={handleRemoveSelected}
          onCancel={() => setShowRemoveConfirmation(false)}
        />
      )}
    </div>
  )
}

function moveAlbumMedia(items: Media[], draggedId: number, targetId: number): Media[] {
  const draggedIndex = items.findIndex((item) => item.id === draggedId)
  const targetIndex = items.findIndex((item) => item.id === targetId)
  if (draggedIndex === -1 || targetIndex === -1 || draggedIndex === targetIndex) return items

  const nextItems = [...items]
  const [draggedItem] = nextItems.splice(draggedIndex, 1)
  if (!draggedItem) return items
  nextItems.splice(targetIndex, 0, draggedItem)
  return nextItems
}

interface AlbumMediaItemProps {
  item: Media
  isDragged: boolean
  selectionMode: boolean
  selected: boolean
  onDragStart: (e: React.DragEvent) => void
  onDragOver: (e: React.DragEvent) => void
  onDrop: (e: React.DragEvent) => void
  onClick: () => void
}

function AlbumMediaItem({
  item,
  isDragged,
  selectionMode,
  selected,
  onDragStart,
  onDragOver,
  onDrop,
  onClick,
}: AlbumMediaItemProps) {
  const { targetRef: containerRef, imageUrl: thumbnailUrl } = useLazyImage<HTMLDivElement, number>({
    resourceId: item.id,
    loader: batchLoader,
    getCachedUrl: (mediaId) => mediaApi.getCachedThumbnailURL(mediaId, 'normal'),
    rootMargin: '400px',
  })

  return (
    <div
      ref={containerRef}
      draggable={!selectionMode}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      className={cn(
        'aspect-square relative cursor-pointer group overflow-hidden rounded-xl bg-muted shadow-sm transition-[box-shadow,opacity,transform] duration-200 motion-reduce:transition-none',
        isDragged && 'cursor-move opacity-25 ring-2 ring-primary',
        !isDragged &&
          selected &&
          'ring-4 ring-primary ring-offset-2 ring-offset-background shadow-lg',
        !isDragged &&
          !selected &&
          'opacity-100 hover:shadow-md hover:ring-2 hover:ring-primary/30 active:scale-[0.98]'
      )}
      role="button"
      tabIndex={0}
      aria-pressed={selectionMode ? selected : undefined}
      aria-label={
        selectionMode
          ? `${selected ? 'Deselect' : 'Select'} ${item.originalFilename}`
          : `Open ${item.originalFilename}`
      }
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key !== 'Enter' && event.key !== ' ') return
        event.preventDefault()
        onClick()
      }}
    >
      {thumbnailUrl ? (
        <img
          src={thumbnailUrl}
          alt={item.originalFilename}
          className="w-full h-full object-cover pointer-events-none select-none"
        />
      ) : (
        <div className="w-full h-full animate-pulse" />
      )}
      {selectionMode && <MediaSelectionOverlay selected={selected} />}
    </div>
  )
}
